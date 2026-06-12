//! Shared HTTP cache + fetcher abstraction for the widget worker.
//!
//! The cache is owned by the worker task (no locks): responses are keyed
//! by URL and considered fresh for the *requesting widget's* effective
//! refresh interval (the TTL is passed per lookup, so two widgets with
//! different refresh floors can share one entry). Concurrent requests
//! for the same URL dedup through a pending-waiters map: the first
//! requester starts the fetch, everyone else just queues up for the
//! result.
//!
//! [`HttpFetcher`] exists so tests can count upstream hits with a
//! double; production uses [`ReqwestFetcher`]. Transport failures are
//! reported as status 0 (and never cached) — widgets see them exactly
//! like a permission denial, as `HttpResponse { status: 0, body: reason }`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

/// Abstract GET. Returns `(status, body)`; transport errors are status 0
/// with the error text as the body.
pub trait HttpFetcher: Send + Sync {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = (u16, Vec<u8>)> + Send + 'a>>;
}

/// Production fetcher over a shared reqwest client (rustls).
pub struct ReqwestFetcher {
    client: reqwest::Client,
}

impl ReqwestFetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("oxidemx-widget-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("static reqwest client config is valid");
        ReqwestFetcher { client }
    }
}

impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpFetcher for ReqwestFetcher {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = (u16, Vec<u8>)> + Send + 'a>> {
        Box::pin(async move {
            match self.client.get(url).send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
                    (status, body)
                }
                Err(e) => (0, format!("request failed: {e}").into_bytes()),
            }
        })
    }
}

/// One lookup against the cache.
#[derive(Debug, PartialEq)]
pub enum CacheResult {
    /// Fresh entry — use it, no fetch needed.
    Hit { status: u16, body: Vec<u8> },
    /// A fetch for this URL is already in flight — register a waiter.
    Pending,
    /// Nothing usable — register a waiter AND start a fetch.
    Miss,
}

struct CacheEntry {
    fetched_at: Instant,
    status: u16,
    body: Vec<u8>,
}

/// URL-keyed response cache with in-flight dedup. `W` is the caller's
/// waiter handle (the worker uses `(InstanceId, request-id)`).
pub struct HttpCache<W> {
    entries: HashMap<String, CacheEntry>,
    pending: HashMap<String, Vec<W>>,
}

impl<W> Default for HttpCache<W> {
    fn default() -> Self {
        HttpCache { entries: HashMap::new(), pending: HashMap::new() }
    }
}

impl<W> HttpCache<W> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up `url`, treating entries older than `ttl` as gone.
    pub fn get(&mut self, url: &str, ttl: Duration) -> CacheResult {
        if let Some(entry) = self.entries.get(url) {
            if entry.fetched_at.elapsed() <= ttl {
                return CacheResult::Hit { status: entry.status, body: entry.body.clone() };
            }
            self.entries.remove(url);
        }
        if self.pending.contains_key(url) {
            CacheResult::Pending
        } else {
            CacheResult::Miss
        }
    }

    /// Queue a waiter for an in-flight (or about-to-start) fetch of `url`.
    pub fn add_waiter(&mut self, url: &str, waiter: W) {
        self.pending.entry(url.to_string()).or_default().push(waiter);
    }

    /// A fetch finished: store the response (status 0 = transport failure,
    /// not cached) and hand back every queued waiter for delivery.
    pub fn complete(&mut self, url: &str, status: u16, body: &[u8]) -> Vec<W> {
        if status != 0 {
            self.entries.insert(
                url.to_string(),
                CacheEntry { fetched_at: Instant::now(), status, body: body.to_vec() },
            );
        }
        self.pending.remove(url).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miss_then_pending_then_hit() {
        let mut cache: HttpCache<u32> = HttpCache::new();
        let ttl = Duration::from_secs(60);
        assert_eq!(cache.get("https://a/x", ttl), CacheResult::Miss);
        cache.add_waiter("https://a/x", 1);
        assert_eq!(cache.get("https://a/x", ttl), CacheResult::Pending);
        cache.add_waiter("https://a/x", 2);

        let waiters = cache.complete("https://a/x", 200, b"ok");
        assert_eq!(waiters, vec![1, 2]);
        assert_eq!(
            cache.get("https://a/x", ttl),
            CacheResult::Hit { status: 200, body: b"ok".to_vec() }
        );
        // Zero TTL: the entry is stale immediately.
        assert_eq!(cache.get("https://a/x", Duration::ZERO), CacheResult::Miss);
    }

    #[test]
    fn transport_failures_are_not_cached() {
        let mut cache: HttpCache<u32> = HttpCache::new();
        cache.add_waiter("https://a/x", 7);
        let waiters = cache.complete("https://a/x", 0, b"request failed: dns");
        assert_eq!(waiters, vec![7]);
        assert_eq!(cache.get("https://a/x", Duration::from_secs(60)), CacheResult::Miss);
    }
}

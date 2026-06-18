//! Per-conversation session substrate (Phase 2).
//!
//! A chat/project session is the unit of isolation: it owns **one** LLM
//! provider instance (built once and reused across turns) and its own
//! cancellation token. Holding one provider per conversation is the hard
//! rule from the `building-llm-agents-in-rust` skill — a server-side-session
//! backend (Gemini Interactions, or a future session-bearing local model)
//! would interleave conversations if the instance were shared. For the
//! stateless backends in use today the same ownership keeps the reqwest
//! connection pool warm between turns.
//!
//! The [`SessionStore`] trait is the seam: the in-memory [`SessionManager`]
//! implements it today; a Ractor-supervised store (AutoAgents'
//! `autoagents-core::runtime`, `ractor` is already in the tree) can replace
//! it later without touching call sites. We deliberately did **not** adopt
//! the actor runtime now — ownership, not actors, is what guarantees the
//! one-provider-per-conversation invariant; the actor layer earns its place
//! only once concurrent background/project sessions need supervision.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use autoagents::llm::error::LLMError;
use autoagents::llm::LLMProvider;
use oxidemx_shared::config::AiProvider;
use tokio_util::sync::CancellationToken;

use crate::factory;

/// Stable identifier for a conversation. The overlay stores it on the chat
/// thread (`ChatThread.session_id`) so it survives the thread list's
/// index-shifting deletes; other consumers can mint their own.
pub type SessionId = String;

/// The configuration a cached provider was built from. Any change — provider
/// swap, per-thread model switch, edited key, or a new local endpoint —
/// changes the fingerprint and forces a one-time rebuild on the next turn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProviderFingerprint {
    pub provider: AiProvider,
    pub model: String,
    pub key: String,
    pub endpoint: String,
}

struct Cached {
    fp: ProviderFingerprint,
    llm: Arc<dyn LLMProvider>,
}

/// One isolated conversation.
pub struct Session {
    id: SessionId,
    cached: Mutex<Option<Cached>>,
    cancel: Mutex<CancellationToken>,
}

impl Session {
    fn new(id: SessionId) -> Self {
        Self {
            id,
            cached: Mutex::new(None),
            cancel: Mutex::new(CancellationToken::new()),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The provider for `fp`, built once and reused while the fingerprint
    /// holds. Never shared across sessions.
    pub fn provider(&self, fp: ProviderFingerprint) -> Result<Arc<dyn LLMProvider>, LLMError> {
        let mut slot = self.cached.lock().unwrap();
        if let Some(c) = slot.as_ref() {
            if c.fp == fp {
                return Ok(c.llm.clone());
            }
        }
        let llm = factory::provider_from_config_with_endpoint(
            fp.provider,
            &fp.model,
            &fp.key,
            Some(&fp.endpoint),
        )?;
        *slot = Some(Cached { fp, llm: llm.clone() });
        Ok(llm)
    }

    /// Open a turn: install a fresh cancellation token (so [`Session::cancel`]
    /// targets *this* turn, not a stale one) and hand back a clone to
    /// `select!` the turn's future against.
    pub fn begin_turn(&self) -> CancellationToken {
        let tok = CancellationToken::new();
        *self.cancel.lock().unwrap() = tok.clone();
        tok
    }

    /// Cancel the in-flight turn. Idempotent and safe when idle — the next
    /// [`Session::begin_turn`] replaces the (now-cancelled) token. This is the
    /// canonical cancel path for non-UI consumers (agentd, conductor); the
    /// overlay also drops the turn's `Task` via its abort handle, so both
    /// converge. It additionally covers the case the skill warns about — a
    /// turn parked on an approval `await`, where dropping an HTTP request
    /// alone would not unblock.
    pub fn cancel(&self) {
        self.cancel.lock().unwrap().cancel();
    }
}

/// Session lookup + lifecycle. The seam a Ractor-backed supervisor can
/// implement later in place of [`SessionManager`].
pub trait SessionStore: Send + Sync {
    /// Get the session for `id`, creating it on first use.
    fn session(&self, id: &str) -> Arc<Session>;
    /// Cancel `id`'s in-flight turn (no-op if unknown).
    fn cancel(&self, id: &str);
    /// Drop `id` (e.g. the chat thread was deleted). Aborts any in-flight
    /// turn first so a parked future can't outlive its session.
    fn end(&self, id: &str);
}

/// In-memory session map. Internally synchronized — share by `&`.
#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Live session count (diagnostics).
    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SessionStore for SessionManager {
    fn session(&self, id: &str) -> Arc<Session> {
        self.sessions
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Session::new(id.to_string())))
            .clone()
    }

    fn cancel(&self, id: &str) {
        if let Some(s) = self.sessions.lock().unwrap().get(id) {
            s.cancel();
        }
    }

    fn end(&self, id: &str) {
        if let Some(s) = self.sessions.lock().unwrap().remove(id) {
            s.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(model: &str) -> ProviderFingerprint {
        ProviderFingerprint {
            provider: AiProvider::MistralRs,
            model: model.to_string(),
            key: String::new(),
            endpoint: "http://localhost:1234/v1/".to_string(),
        }
    }

    #[test]
    fn manager_reuses_session_by_id() {
        let m = SessionManager::new();
        let a = m.session("t1");
        let b = m.session("t1");
        assert!(Arc::ptr_eq(&a, &b), "same id must yield the same session");
        assert_eq!(m.len(), 1);
        m.session("t2");
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn provider_cached_until_fingerprint_changes() {
        let s = Session::new("t1".into());
        let p1 = s.provider(fp("default")).unwrap();
        let p2 = s.provider(fp("default")).unwrap();
        assert!(Arc::ptr_eq(&p1, &p2), "same fingerprint reuses the instance");
        // A model switch invalidates the cache → a fresh instance.
        let p3 = s.provider(fp("Qwen/Qwen3-4B")).unwrap();
        assert!(!Arc::ptr_eq(&p1, &p3), "changed fingerprint rebuilds");
    }

    #[test]
    fn end_removes_and_cancels() {
        let m = SessionManager::new();
        let s = m.session("t1");
        let tok = s.begin_turn();
        assert!(!tok.is_cancelled());
        m.end("t1");
        assert!(tok.is_cancelled(), "end() must cancel the in-flight turn");
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn begin_turn_then_cancel_fires_current_token() {
        let s = Session::new("t1".into());
        let tok = s.begin_turn();
        s.cancel();
        assert!(tok.is_cancelled());
    }
}

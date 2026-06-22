//! Tailscale bind-address resolution behind a mockable source.
#![forbid(unsafe_code)]

use std::net::{IpAddr, SocketAddr};

/// Source of the host's current tailnet IP. Seam so resolution is testable
/// without a live tailnet.
pub trait TailnetSource: Send + Sync {
    fn tailnet_ip(&self) -> Option<IpAddr>;
}

/// Production source: shells `tailscale status --json`.
pub struct CliTailnetSource;

impl TailnetSource for CliTailnetSource {
    fn tailnet_ip(&self) -> Option<IpAddr> {
        let out = std::process::Command::new("tailscale")
            .args(["status", "--json"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        // `Self.TailscaleIPs` is an array; take the first IPv4.
        let ips = v.get("Self")?.get("TailscaleIPs")?.as_array()?;
        ips.iter()
            .filter_map(|s| s.as_str())
            .filter_map(|s| s.parse::<IpAddr>().ok())
            .find(|ip| ip.is_ipv4())
    }
}

/// Resolve the TCP bind address. `Ok(None)` ⇒ no TCP listener (UDS still runs).
/// `Err` ⇒ a configured override is invalid/unspecified (caller logs + skips TCP).
pub fn resolve_bind_addr(
    source: &dyn TailnetSource,
    bind_override: &str,
    port: u16,
) -> Result<Option<SocketAddr>, String> {
    if !bind_override.trim().is_empty() {
        let ip: IpAddr = bind_override
            .trim()
            .parse()
            .map_err(|_| format!("invalid http.bind_override: {bind_override:?}"))?;
        if ip.is_unspecified() {
            return Err(format!("refusing to bind unspecified address {ip} (never 0.0.0.0)"));
        }
        return Ok(Some(SocketAddr::new(ip, port)));
    }
    Ok(source.tailnet_ip().map(|ip| SocketAddr::new(ip, port)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSource(Option<IpAddr>);
    impl TailnetSource for MockSource {
        fn tailnet_ip(&self) -> Option<IpAddr> { self.0 }
    }

    fn ip(s: &str) -> IpAddr { s.parse().unwrap() }

    #[test]
    fn uses_tailnet_ip_when_present_and_no_override() {
        let src = MockSource(Some(ip("100.101.102.103")));
        let addr = resolve_bind_addr(&src, "", 8765).unwrap();
        assert_eq!(addr, Some(SocketAddr::new(ip("100.101.102.103"), 8765)));
    }

    #[test]
    fn none_when_no_tailnet_and_no_override() {
        let src = MockSource(None);
        assert_eq!(resolve_bind_addr(&src, "", 8765).unwrap(), None);
    }

    #[test]
    fn override_wins_over_tailnet() {
        let src = MockSource(Some(ip("100.1.1.1")));
        let addr = resolve_bind_addr(&src, "100.9.9.9", 8765).unwrap();
        assert_eq!(addr, Some(SocketAddr::new(ip("100.9.9.9"), 8765)));
    }

    #[test]
    fn rejects_unspecified_override() {
        let src = MockSource(None);
        assert!(resolve_bind_addr(&src, "0.0.0.0", 8765).is_err());
        assert!(resolve_bind_addr(&src, "::", 8765).is_err());
    }

    #[test]
    fn errors_on_garbage_override() {
        let src = MockSource(None);
        assert!(resolve_bind_addr(&src, "not-an-ip", 8765).is_err());
    }
}

//! Bearer-token provenance for the tailnet listener (the UDS listener needs none).
#![forbid(unsafe_code)]

use std::io;
use std::path::Path;

/// Load `<dir>/agentd-token`, or generate + persist a fresh 32-byte (64 hex) token.
pub fn load_or_create_token(dir: &Path) -> io::Result<String> {
    let path = dir.join("agentd-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    // Generate 32 random bytes from the OS CSPRNG.
    let mut buf = [0u8; 32];
    read_random(&mut buf)?;
    let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();

    // Ensure parent dir exists (0700) and write the token file (0600).
    std::fs::create_dir_all(dir)?;
    set_dir_mode(dir, 0o700)?;
    std::fs::write(&path, &token)?;
    set_file_mode(&path, 0o600)?;
    Ok(token)
}

/// Constant-time check of an `Authorization: Bearer <token>` header.
pub fn verify_bearer(header_value: Option<&str>, token: &str) -> bool {
    let Some(h) = header_value else { return false };
    let Some(presented) = h.strip_prefix("Bearer ") else { return false };
    constant_time_eq(presented.as_bytes(), token.as_bytes())
}

/// XOR-accumulate constant-time byte comparison (avoids a `subtle` dep).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn read_random(buf: &mut [u8]) -> io::Result<()> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(buf)
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> io::Result<()> { Ok(()) }
#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) -> io::Result<()> { Ok(()) }

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_persist_and_reload_roundtrip() {
        let dir = tempdir().unwrap();
        let t1 = load_or_create_token(dir.path()).unwrap();
        assert_eq!(t1.len(), 64);
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
        // Second call loads the SAME token (no regeneration).
        let t2 = load_or_create_token(dir.path()).unwrap();
        assert_eq!(t1, t2);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        load_or_create_token(dir.path()).unwrap();
        let mode = std::fs::metadata(dir.path().join("agentd-token"))
            .unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn verify_bearer_accepts_correct_rejects_wrong_and_missing() {
        let token = "abc123";
        assert!(verify_bearer(Some("Bearer abc123"), token));
        assert!(!verify_bearer(Some("Bearer wrong"), token));
        assert!(!verify_bearer(Some("abc123"), token));      // missing scheme
        assert!(!verify_bearer(None, token));
        assert!(!verify_bearer(Some("Bearer "), token));
    }
}

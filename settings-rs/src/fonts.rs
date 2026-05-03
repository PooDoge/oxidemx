//! Resolve a runtime font-family name into an `iced::Font`.
//!
//! `iced::Font::with_name` requires `&'static str`, so we intern
//! each unique family string once via `Box::leak`. A small static
//! HashSet keeps the dedup, so saving a slider 200 times doesn't
//! leak 200 copies of "Inter". Total expected memory: tens of
//! bytes per family the user ever picks — fine.

use iced::Font;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Resolve `family` to an `iced::Font`. Empty string returns the
/// default sans-serif font.
pub fn resolve(family: &str) -> Font {
    let f = family.trim();
    if f.is_empty() {
        return Font::DEFAULT;
    }
    Font::with_name(intern(f))
}

fn intern(s: &str) -> &'static str {
    let cache = INTERNED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = cache.lock().expect("font intern mutex");
    if let Some(existing) = guard.get(s) {
        return existing;
    }
    // Leak a single owned copy and store the static reference back.
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    guard.insert(leaked);
    leaked
}

static INTERNED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_default() {
        let f = resolve("");
        assert_eq!(f, Font::DEFAULT);
        let f = resolve("   ");
        assert_eq!(f, Font::DEFAULT);
    }

    #[test]
    fn dedup_returns_same_static_pointer() {
        let a = intern("Inter");
        let b = intern("Inter");
        // Same allocation — no double-leak.
        assert!(std::ptr::eq(a, b));
    }
}

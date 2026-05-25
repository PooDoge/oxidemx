//! Resolve a runtime font-family name into an `iced::Font`.
//!
//! `iced::Font::with_name` requires `&'static str`, so we intern
//! each unique family string once via `Box::leak`. A small static
//! HashSet keeps the dedup, so saving a slider 200 times doesn't
//! leak 200 copies of "Inter". Total expected memory: tens of
//! bytes per family the user ever picks — fine.
//!
//! Also enumerates installed system font families via `fc-list`
//! for the settings UI's font picker. Result is cached behind a
//! `OnceLock` so the dropdown isn't shelling out on every render.

use iced::Font;
use std::collections::{BTreeSet, HashSet};
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

/// Enumerate installed font families on the system. Linux: shells
/// out to `fc-list :family` once and caches the (deduped, sorted)
/// result. Empty fallback when `fc-list` isn't available — the
/// UI then renders the picker with no options and the user can
/// still type a family name into the text input fallback.
pub fn system_families() -> &'static [String] {
    SYSTEM_FAMILIES.get_or_init(load_system_families)
}

fn load_system_families() -> Vec<String> {
    let output = match std::process::Command::new("fc-list")
        .arg(":family")
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let s = String::from_utf8_lossy(&output);
    let mut set: BTreeSet<String> = BTreeSet::new();
    for line in s.lines() {
        // fc-list :family prints comma-separated localised aliases
        // per line, e.g. "Noto Sans CJK JP,Noto Sans CJK JP Bold".
        // Take the first alias — that's the canonical English name
        // most users will recognise.
        let first = line.split(',').next().unwrap_or("").trim();
        if first.is_empty() {
            continue;
        }
        // Skip families starting with a dot (vendor-private builds
        // like ".SF NS Mono" that aren't supposed to surface in
        // pickers).
        if first.starts_with('.') {
            continue;
        }
        set.insert(first.to_string());
    }
    set.into_iter().collect()
}

static SYSTEM_FAMILIES: OnceLock<Vec<String>> = OnceLock::new();

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

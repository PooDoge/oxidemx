//! Resolve a runtime font-family name into an `iced::Font`. Same
//! intern-cache trick as settings-rs/src/fonts.rs (kept duplicated
//! intentionally — both crates only need a few lines and pulling
//! it into a shared crate would require lifting iced out of
//! oxidemx-shared).

use iced::Font;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

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
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    guard.insert(leaked);
    leaked
}

static INTERNED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();

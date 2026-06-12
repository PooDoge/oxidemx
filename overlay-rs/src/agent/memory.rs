//! Backend for the `memory` agent tool: a small, local, plain-JSON
//! store at `~/.local/share/oxidemx/memories.json`.
//!
//! Design notes:
//! - Plain JSON file, no database — the store is expected to hold
//!   tens of entries, and a flat file the user can read/edit/delete
//!   matches the rest of OxideMX's config philosophy.
//! - Retention: unpinned entries expire 90 days after their last
//!   *use* (injection into a prompt counts as use), pinned entries
//!   live until explicitly deleted. The sweep runs once per process
//!   on first access; [`sweep_expired`] stays public and takes `now`
//!   so tests (and a future settings page) can drive it
//!   deterministically.
//! - IDs are short hex digests of `(text, created_at, pid, counter)`
//!   via the std hasher — unique enough for a personal store without
//!   pulling in uuid/rand.
//! - Every function is a thin wrapper over a `*_at(path, …)`
//!   internal so tests run against a temp file, never the real
//!   store.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

/// Unpinned entries unused for this long are swept.
const RETENTION_SECS: u64 = 90 * 24 * 3600;

/// How many recently-used unpinned entries ride along with the
/// pinned ones in the system-prompt injection block.
const INJECT_RECENT_CAP: usize = 10;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntry {
    /// Short hex id, stable for the entry's lifetime.
    pub id: String,
    pub text: String,
    /// Free-form grouping label ("preferences", "projects", …) shown
    /// as the `[scope]` prefix in the injection block.
    pub scope: String,
    /// Pinned entries never expire and always inject.
    pub pinned: bool,
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds; refreshed whenever the entry is injected into a
    /// prompt. Drives the 90-day retention window.
    pub last_used_at: u64,
}

// =============================================================================
// PATH-PARAMETERISED INTERNALS (unit-testable)
// =============================================================================

/// Read the whole store. Missing or corrupt file → empty (a corrupt
/// store should degrade to "no memories", not break the agent).
fn load_from(path: &Path) -> Vec<MemoryEntry> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Persist the whole store, creating parent dirs as needed. Write
/// failures are logged, not propagated — memory persistence must
/// never take down an agent turn.
fn save_to(path: &Path, entries: &[MemoryEntry]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(entries) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                tracing::error!("failed to write memory store {}: {e}", path.display());
            }
        }
        Err(e) => tracing::error!("failed to serialize memory store: {e}"),
    }
}

/// Per-process counter mixed into ids so two saves in the same
/// second with identical text still get distinct ids.
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Short hex id from std hashing — no uuid/rand dependency.
fn make_id(text: &str, created_at: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    created_at.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    ID_COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

fn save_entry_at(path: &Path, text: &str, scope: &str, now: u64) -> MemoryEntry {
    let mut entries = load_from(path);
    let entry = MemoryEntry {
        id: make_id(text, now),
        text: text.to_string(),
        scope: scope.to_string(),
        pinned: false,
        created_at: now,
        last_used_at: now,
    };
    entries.push(entry.clone());
    save_to(path, &entries);
    entry
}

fn delete_at(path: &Path, id: &str) -> bool {
    let mut entries = load_from(path);
    let before = entries.len();
    entries.retain(|e| e.id != id);
    let removed = entries.len() != before;
    if removed {
        save_to(path, &entries);
    }
    removed
}

fn set_pinned_at(path: &Path, id: &str, pinned: bool) -> bool {
    let mut entries = load_from(path);
    let mut found = false;
    for e in &mut entries {
        if e.id == id {
            e.pinned = pinned;
            found = true;
        }
    }
    if found {
        save_to(path, &entries);
    }
    found
}

/// Drop unpinned entries whose `last_used_at` is more than 90 days
/// before `now`. Returns how many were removed.
fn sweep_expired_at(path: &Path, now: u64) -> usize {
    let mut entries = load_from(path);
    let before = entries.len();
    entries.retain(|e| e.pinned || now.saturating_sub(e.last_used_at) <= RETENTION_SECS);
    let removed = before - entries.len();
    if removed > 0 {
        save_to(path, &entries);
    }
    removed
}

/// Build the system-prompt block: all pinned entries first, then the
/// 10 most-recently-used unpinned ones. Marks every included entry
/// as used (`last_used_at = now`) and persists, so injection itself
/// keeps memories alive. `None` when the store is empty.
fn injection_block_at(path: &Path, now: u64) -> Option<String> {
    let mut entries = load_from(path);
    if entries.is_empty() {
        return None;
    }

    // Collect ids in render order: pinned (save order), then
    // unpinned by recency.
    let pinned: Vec<&MemoryEntry> = entries.iter().filter(|e| e.pinned).collect();
    let mut unpinned: Vec<&MemoryEntry> = entries.iter().filter(|e| !e.pinned).collect();
    unpinned.sort_by_key(|e| std::cmp::Reverse(e.last_used_at));
    unpinned.truncate(INJECT_RECENT_CAP);

    let mut block = String::from("The user's saved memories:");
    let mut injected_ids = Vec::new();
    for e in pinned.iter().chain(unpinned.iter()) {
        block.push_str(&format!("\n- [{}] {}", e.scope, e.text));
        injected_ids.push(e.id.clone());
    }

    // Touch the injected entries so use, not just creation, drives
    // the retention window.
    for e in &mut entries {
        if injected_ids.contains(&e.id) {
            e.last_used_at = now;
        }
    }
    save_to(path, &entries);

    Some(block)
}

// =============================================================================
// PUBLIC API (real store path)
// =============================================================================

/// `~/.local/share/oxidemx/memories.json`.
fn store_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    Path::new(&home).join(".local/share/oxidemx/memories.json")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Run the retention sweep at most once per process, lazily on the
/// first store access. Keeps stale entries from accumulating without
/// needing a background job.
fn sweep_once() {
    static SWEEP: Once = Once::new();
    SWEEP.call_once(|| {
        let removed = sweep_expired_at(&store_path(), unix_now());
        if removed > 0 {
            tracing::info!("memory sweep removed {removed} expired entries");
        }
    });
}

/// Every saved memory, oldest first (save order).
pub fn load_all() -> Vec<MemoryEntry> {
    sweep_once();
    load_from(&store_path())
}

/// Append a new (unpinned) memory and return it.
pub fn save_entry(text: &str, scope: &str) -> MemoryEntry {
    save_entry_at(&store_path(), text, scope, unix_now())
}

/// Remove an entry by id; `false` when no such id exists.
pub fn delete(id: &str) -> bool {
    delete_at(&store_path(), id)
}

/// Pin (never expires) or unpin (90-day retention) an entry;
/// `false` when no such id exists.
pub fn set_pinned(id: &str, pinned: bool) -> bool {
    set_pinned_at(&store_path(), id, pinned)
}

/// On-disk size of the store, for the settings page's "memory usage"
/// readout. 0 when the store doesn't exist yet.
#[allow(dead_code)] // consumed by the memory-management settings UI
pub fn store_size_bytes() -> u64 {
    std::fs::metadata(store_path())
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Remove unpinned entries unused for 90 days, judged against `now`
/// (unix secs). Public + deterministic for tests and explicit
/// "clean up now" UI actions; also runs automatically once per
/// process via [`load_all`]/[`injection_block`].
#[allow(dead_code)] // consumed by the memory-management settings UI
pub fn sweep_expired(now: u64) -> usize {
    sweep_expired_at(&store_path(), now)
}

/// The memory block appended to the agent's system instruction, or
/// `None` when there's nothing saved. Touches `last_used_at` on the
/// injected entries.
pub fn injection_block() -> Option<String> {
    sweep_once();
    injection_block_at(&store_path(), unix_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh store file in a unique temp dir per test.
    fn temp_store(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxidemx-memory-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("memories.json")
    }

    #[test]
    fn save_load_round_trip() {
        let path = temp_store("roundtrip");
        let a = save_entry_at(&path, "likes dark themes", "preferences", 100);
        let b = save_entry_at(&path, "works on oxidemx", "projects", 200);
        assert_ne!(a.id, b.id);

        let loaded = load_from(&path);
        assert_eq!(loaded, vec![a, b]);
        assert!(!loaded[0].pinned);
        assert_eq!(loaded[0].created_at, 100);
        assert_eq!(loaded[0].last_used_at, 100);
    }

    #[test]
    fn missing_or_corrupt_store_loads_empty() {
        let path = temp_store("corrupt");
        assert!(load_from(&path).is_empty());
        std::fs::write(&path, "not json {").unwrap();
        assert!(load_from(&path).is_empty());
    }

    #[test]
    fn sweep_removes_only_stale_unpinned() {
        let path = temp_store("sweep");
        let now = 100 * 24 * 3600u64; // day 100
        let stale = save_entry_at(&path, "stale", "s", 0); // unused for 100d
        let fresh = save_entry_at(&path, "fresh", "s", now - 3600);
        let ancient_pinned = save_entry_at(&path, "ancient but pinned", "s", 0);
        assert!(set_pinned_at(&path, &ancient_pinned.id, true));

        assert_eq!(sweep_expired_at(&path, now), 1);
        let ids: Vec<String> = load_from(&path).into_iter().map(|e| e.id).collect();
        assert!(!ids.contains(&stale.id));
        assert!(ids.contains(&fresh.id));
        assert!(ids.contains(&ancient_pinned.id));
        // Idempotent: nothing more to remove.
        assert_eq!(sweep_expired_at(&path, now), 0);
    }

    #[test]
    fn pin_toggle_and_delete() {
        let path = temp_store("pin");
        let e = save_entry_at(&path, "x", "s", 1);
        assert!(set_pinned_at(&path, &e.id, true));
        assert!(load_from(&path)[0].pinned);
        assert!(set_pinned_at(&path, &e.id, false));
        assert!(!load_from(&path)[0].pinned);
        assert!(!set_pinned_at(&path, "nope", true));

        assert!(delete_at(&path, &e.id));
        assert!(!delete_at(&path, &e.id));
        assert!(load_from(&path).is_empty());
    }

    #[test]
    fn injection_block_pinned_first_and_touches_last_used() {
        let path = temp_store("inject");
        let old = save_entry_at(&path, "old unpinned", "a", 10);
        let new = save_entry_at(&path, "new unpinned", "b", 20);
        let pinned = save_entry_at(&path, "pinned one", "c", 5);
        assert!(set_pinned_at(&path, &pinned.id, true));

        let block = injection_block_at(&path, 1000).unwrap();
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines[0], "The user's saved memories:");
        // Pinned first, then unpinned by recency (new before old).
        assert_eq!(lines[1], "- [c] pinned one");
        assert_eq!(lines[2], "- [b] new unpinned");
        assert_eq!(lines[3], "- [a] old unpinned");

        // All injected entries were touched.
        for e in load_from(&path) {
            assert_eq!(e.last_used_at, 1000, "entry {} not touched", e.text);
        }
        let _ = (old, new);
    }

    #[test]
    fn injection_block_caps_unpinned_at_ten() {
        let path = temp_store("cap");
        for i in 0..12u64 {
            save_entry_at(&path, &format!("entry {i}"), "s", i);
        }
        let block = injection_block_at(&path, 100).unwrap();
        // Header + 10 bullets; the two oldest (0, 1) fall off.
        assert_eq!(block.lines().count(), 11);
        assert!(!block.contains("entry 0\n") && !block.ends_with("entry 0"));
        assert!(block.contains("entry 11"));
    }

    #[test]
    fn injection_block_empty_store_is_none() {
        let path = temp_store("empty");
        assert_eq!(injection_block_at(&path, 1), None);
    }
}

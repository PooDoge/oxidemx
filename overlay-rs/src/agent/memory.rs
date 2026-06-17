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
    /// How many prompts this entry has been injected into — a small
    /// usage boost in recall scoring (capped so favourites can't
    /// permanently crowd out new facts).
    #[serde(default)]
    pub times_injected: u32,
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

/// Lowercased word tokens minus trivial stopwords — shared by the
/// write-time dedupe and the recall scorer.
fn tokens(text: &str) -> std::collections::HashSet<String> {
    const STOP: &[&str] = &[
        "the", "a", "an", "is", "are", "was", "were", "to", "of", "in", "on", "and", "or", "for",
        "with", "that", "this", "it", "as", "at", "be", "by", "has", "have", "his", "her", "their",
        "my", "your", "user", "users",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1 && !STOP.contains(t))
        .map(str::to_string)
        .collect()
}

/// Jaccard similarity of the two texts' token sets.
fn jaccard(a: &str, b: &str) -> f32 {
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f32;
    let union = (ta.len() + tb.len()) as f32 - inter;
    inter / union.max(1.0)
}

/// Write-time triage (mem0's ADD/UPDATE collapsed to two outcomes):
/// a save whose token set substantially overlaps an existing entry
/// in the same scope UPDATES that entry (newest wording wins, id and
/// `created_at` survive) instead of appending a near-duplicate.
const DEDUPE_THRESHOLD: f32 = 0.7;

fn save_entry_at(path: &Path, text: &str, scope: &str, now: u64) -> MemoryEntry {
    let mut entries = load_from(path);

    // UPDATE path: replace the most-similar same-scope entry above
    // the threshold. Corrections kill stale facts immediately —
    // staleness, not volume, is what users notice.
    let best = entries
        .iter_mut()
        .filter(|e| e.scope == scope)
        .map(|e| (jaccard(&e.text, text), e))
        .filter(|(sim, _)| *sim >= DEDUPE_THRESHOLD)
        .max_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if let Some((_, existing)) = best {
        existing.text = text.to_string();
        existing.last_used_at = now;
        let updated = existing.clone();
        save_to(path, &entries);
        return updated;
    }

    let entry = MemoryEntry {
        id: make_id(text, now),
        text: text.to_string(),
        scope: scope.to_string(),
        pinned: false,
        created_at: now,
        last_used_at: now,
        times_injected: 0,
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

/// Recall score (Generative-Agents shape): relevance to the query,
/// recency with a 30-day half-life, and a capped usage boost.
fn score(entry: &MemoryEntry, query_tokens: &std::collections::HashSet<String>, now: u64) -> f32 {
    score_blended(entry, query_tokens, now, None)
}

/// Recall score: `0.6 relevance + 0.25 recency + 0.15 usage`.
///
/// `relevance` is the stronger of the lexical (token-overlap) and the
/// semantic (cosine) signals — so a meaning-match surfaces at zero
/// word overlap, while keeping relevance the dominant term and
/// recency/usage as tiebreakers (weights unchanged from the original
/// lexical-only scorer; `sem = None` ⇒ identical behaviour).
///
/// The cosine is rescaled before use: `gemini-embedding-001` cosines
/// sit in a high, compressed band (~0.45 for unrelated text, ~0.70+
/// for related), so the raw value barely moves the score. Mapping
/// `[0.40, 0.85] → [0, 1]` (empirical for this model at 768-dim)
/// restores a usable dynamic range. Lexical relevance is unbounded-ish
/// (`inter / sqrt(len)`); `max` over the two is fine since both land
/// in roughly the same 0–1 range for real queries.
fn score_blended(
    entry: &MemoryEntry,
    query_tokens: &std::collections::HashSet<String>,
    now: u64,
    sem: Option<f32>,
) -> f32 {
    let etoks = tokens(&entry.text);
    let lexical = if query_tokens.is_empty() || etoks.is_empty() {
        0.0
    } else {
        let inter = etoks.intersection(query_tokens).count() as f32;
        // Normalise by entry length so short atomic facts aren't
        // drowned out by long rambly ones.
        inter / (etoks.len() as f32).sqrt()
    };
    let relevance = match sem {
        Some(cos) => {
            let rescaled = ((cos - 0.40) / 0.45).clamp(0.0, 1.0);
            lexical.max(rescaled)
        }
        None => lexical,
    };
    let age_days = now.saturating_sub(entry.last_used_at.max(entry.created_at)) as f32 / 86_400.0;
    let recency = 0.5_f32.powf(age_days / 30.0);
    let usage = (entry.times_injected.min(5)) as f32 / 5.0;
    0.6 * relevance + 0.25 * recency + 0.15 * usage
}

/// Approximate character budget for the unpinned tier of the
/// injection block (~900 tokens). Pinned entries always inject and
/// don't count against this.
const INJECT_CHAR_BUDGET: usize = 3600;

/// `YYYY-MM-DD` from unix seconds (Howard Hinnant's civil-from-days;
/// no chrono dependency for one date stamp).
fn unix_to_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64 + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Build the system-prompt block. Tier 0: every pinned entry, always
/// (pins are sacred). Tier 1: unpinned entries ranked against the
/// user's prompt by `score`, filling a ~900-token budget. Each line
/// carries its date so the model can reason about staleness. Marks
/// included entries as used and bumps `times_injected`. `None` when
/// the store is empty.
fn injection_block_at(
    path: &Path,
    query: &str,
    now: u64,
    sem: Option<&std::collections::HashMap<String, f32>>,
) -> Option<String> {
    let mut entries = load_from(path);
    if entries.is_empty() {
        return None;
    }

    let qtoks = tokens(query);
    let pinned: Vec<&MemoryEntry> = entries.iter().filter(|e| e.pinned).collect();
    let mut unpinned: Vec<(&MemoryEntry, f32)> = entries
        .iter()
        .filter(|e| !e.pinned)
        .map(|e| {
            let s = sem.and_then(|m| m.get(&e.id).copied());
            (e, score_blended(e, &qtoks, now, s))
        })
        .collect();
    unpinned.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let mut block = String::from("The user's saved memories (dated; older facts may be stale):");
    let mut injected_ids = Vec::new();
    for e in &pinned {
        block.push_str(&format!(
            "\n- [{} · {}] {}",
            e.scope,
            unix_to_date(e.created_at),
            e.text
        ));
        injected_ids.push(e.id.clone());
    }
    let mut budget = INJECT_CHAR_BUDGET;
    for (taken, (e, _)) in unpinned.iter().enumerate() {
        if taken >= INJECT_RECENT_CAP || e.text.len() + 24 > budget {
            break;
        }
        budget -= e.text.len() + 24;
        block.push_str(&format!(
            "\n- [{} · {}] {}",
            e.scope,
            unix_to_date(e.created_at),
            e.text
        ));
        injected_ids.push(e.id.clone());
    }

    // Touch the injected entries so use, not just creation, drives
    // the retention window and the usage boost.
    for e in &mut entries {
        if injected_ids.contains(&e.id) {
            e.last_used_at = now;
            e.times_injected = e.times_injected.saturating_add(1);
        }
    }
    save_to(path, &entries);

    Some(block)
}

/// Top-`k` entries for a free-text query — the `search` action's
/// backend and the long-tail escape hatch beyond the injected block.
/// Does NOT touch usage stats (search ≠ injection).
fn search_at(path: &Path, query: &str, now: u64, k: usize) -> Vec<MemoryEntry> {
    let entries = load_from(path);
    let qtoks = tokens(query);
    let mut scored: Vec<(f32, MemoryEntry)> = entries
        .into_iter()
        .map(|e| (score(&e, &qtoks, now), e))
        .collect();
    scored.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(k).map(|(_, e)| e).collect()
}

// =============================================================================
// CONSOLIDATION ("dreaming") — apply side. The LLM call lives in
// ai_client::tools; this module owns the rails: pinned entries never
// enter the plan, every input id must be accounted for, results that
// shrink the store too aggressively are rejected, and retired
// entries are tombstoned to an archive file instead of deleted.
// =============================================================================

/// Trigger thresholds: a store with this many unpinned entries (or
/// this much time since the last pass, given a non-trivial store)
/// is due for consolidation.
const CONSOLIDATE_COUNT_TRIGGER: usize = 40;
const CONSOLIDATE_INTERVAL_SECS: u64 = 14 * 24 * 3600;
const CONSOLIDATE_MIN_ENTRIES: usize = 12;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct MemoryMeta {
    #[serde(default)]
    last_consolidated_at: u64,
}

fn load_meta(path: &Path) -> MemoryMeta {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_meta(path: &Path, meta: &MemoryMeta) {
    if let Ok(json) = serde_json::to_string(meta) {
        let _ = std::fs::write(path, json);
    }
}

fn consolidation_due_at(store: &Path, meta: &Path, now: u64) -> bool {
    let unpinned = load_from(store).iter().filter(|e| !e.pinned).count();
    if unpinned >= CONSOLIDATE_COUNT_TRIGGER {
        return true;
    }
    unpinned >= CONSOLIDATE_MIN_ENTRIES
        && now.saturating_sub(load_meta(meta).last_consolidated_at) >= CONSOLIDATE_INTERVAL_SECS
}

/// Apply a consolidation plan produced by the LLM. Plan shape:
/// `{"ledger": {"<id>": "keep|merged|superseded|expired"},
///   "entries": [{"text": "...", "scope": "..."}]}` where `entries`
/// are the NEW merged/rewritten facts. All-or-nothing: any
/// validation failure leaves the store untouched.
fn apply_consolidation_at(
    store: &Path,
    archive: &Path,
    plan: &serde_json::Value,
    now: u64,
) -> Result<String, String> {
    let entries = load_from(store);
    let (pinned, unpinned): (Vec<MemoryEntry>, Vec<MemoryEntry>) =
        entries.into_iter().partition(|e| e.pinned);

    let ledger = plan["ledger"]
        .as_object()
        .ok_or("plan missing ledger object")?;
    // Rail 1: every unpinned id must be accounted for.
    for e in &unpinned {
        if !ledger.contains_key(&e.id) {
            return Err(format!("ledger missing id {}", e.id));
        }
    }

    let mut kept: Vec<MemoryEntry> = Vec::new();
    let mut retired: Vec<MemoryEntry> = Vec::new();
    for e in unpinned.iter() {
        match ledger[&e.id].as_str().unwrap_or("keep") {
            "keep" => kept.push(e.clone()),
            _ => retired.push(e.clone()),
        }
    }
    let mut new_entries: Vec<MemoryEntry> = Vec::new();
    for ne in plan["entries"].as_array().into_iter().flatten() {
        let Some(text) = ne["text"].as_str().filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        let scope = ne["scope"].as_str().unwrap_or("general");
        new_entries.push(MemoryEntry {
            id: make_id(text, now),
            text: text.to_string(),
            scope: scope.to_string(),
            pinned: false,
            created_at: now,
            last_used_at: now,
            times_injected: 0,
        });
    }

    // Rail 2: shrink-only with a floor — a hallucinating
    // consolidator nukes everything; a sane one trims 10–30%.
    let before = unpinned.len();
    let after = kept.len() + new_entries.len();
    if before >= 6 && after * 2 < before {
        return Err(format!(
            "plan shrinks store too aggressively ({before} -> {after}); rejected"
        ));
    }

    // Rail 3: tombstone, don't delete. Retired originals go to the
    // archive file with their retirement time.
    let mut archived: Vec<serde_json::Value> = std::fs::read_to_string(archive)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for e in &retired {
        archived.push(serde_json::json!({
            "retired_at": now,
            "entry": e,
        }));
    }
    if let Ok(json) = serde_json::to_string_pretty(&archived) {
        let _ = std::fs::write(archive, json);
    }

    // One-deep backup of the pre-consolidation store, then swap.
    if let Ok(orig) = std::fs::read_to_string(store) {
        let _ = std::fs::write(store.with_extension("json.bak"), orig);
    }
    let mut result = pinned;
    result.extend(kept);
    result.extend(new_entries);
    let summary = format!(
        "Consolidated: {before} unpinned entries -> {after} ({} retired to archive).",
        retired.len()
    );
    save_to(store, &result);
    Ok(summary)
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

/// Passive, high-precision capture of durable facts from a user message.
/// Only fires on UNAMBIGUOUS cues ("remember …", "my name is …") so it
/// never spams the store with conversational noise — the agent's
/// `memory` tool still handles judgment calls. Deduped via `save_entry`.
/// Returns the number captured.
pub fn capture_from_user(text: &str) -> usize {
    let facts = extract_user_captures(text);
    for f in &facts {
        save_entry(f, "user");
    }
    facts.len()
}

/// Pure extraction of durable facts from an unambiguous user message
/// (no I/O — `capture_from_user` saves the results). High precision by
/// design: only explicit directives + identity cues.
fn extract_user_captures(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();

    const CUES: &[&str] = &[
        "remember that ",
        "remember to ",
        "remember ",
        "note that ",
        "keep in mind that ",
        "don't forget that ",
        "for future reference, ",
        "for future reference ",
    ];
    for cue in CUES {
        if let Some(pos) = lower.find(cue) {
            let fact = text[pos + cue.len()..]
                .trim()
                .trim_end_matches(['.', '!'])
                .to_string();
            if (4..=240).contains(&fact.chars().count()) {
                out.push(fact);
            }
            break; // one directive per message
        }
    }

    for cue in ["my name is ", "call me "] {
        if let Some(pos) = lower.find(cue) {
            let name: String = text[pos + cue.len()..]
                .trim()
                .split([' ', ',', '.', '!'])
                .next()
                .unwrap_or("")
                .to_string();
            if (1..=40).contains(&name.chars().count()) {
                out.push(format!("The user's name is {name}"));
            }
            break;
        }
    }

    out
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

/// The memory block appended to the agent's system instruction —
/// pinned entries plus the unpinned entries most relevant to
/// `query` — or `None` when there's nothing saved. Touches usage
/// stats on the injected entries.
pub fn injection_block_for(query: &str) -> Option<String> {
    sweep_once();
    injection_block_at(&store_path(), query, unix_now(), None)
}

/// Hybrid lexical + semantic recall: embeds the query and the saved
/// memories (cached) and blends cosine similarity into the ranking,
/// so a fact with zero word-overlap but matching meaning ("prefers
/// dark mode" for "what theme should I use?") can surface. Best-
/// effort with a hard time budget: any embedding failure, missing
/// key, or a slow round falls back to the lexical [`injection_block_for`]
/// — the chat turn is never blocked waiting on embeddings.
pub async fn injection_block_for_async(query: &str) -> Option<String> {
    sweep_once();
    let path = store_path();
    let now = unix_now();

    // Candidate (id, text) for every entry — semantic recall ranks
    // the whole store, not a lexical pre-filter (which would exclude
    // exactly the low-word-overlap matches we want).
    let candidates: Vec<(String, String)> = load_from(&path)
        .into_iter()
        .map(|e| (e.id, e.text))
        .collect();
    if candidates.is_empty() {
        return None;
    }

    let sem = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        crate::agent::memory_semantic::semantic_scores(query, &candidates),
    )
    .await
    .ok()
    .flatten();

    injection_block_at(&path, query, now, sem.as_ref())
}

/// Top-5 entries for a free-text query (the memory tool's `search`
/// action).
pub fn search(query: &str) -> Vec<MemoryEntry> {
    sweep_once();
    search_at(&store_path(), query, unix_now(), 5)
}

fn meta_path() -> PathBuf {
    store_path().with_file_name("memories_meta.json")
}

fn archive_path() -> PathBuf {
    store_path().with_file_name("memories_archive.json")
}

/// Whether the store is due for a consolidation pass (size or age
/// trigger). Checked at app start — never mid-conversation.
pub fn consolidation_due() -> bool {
    consolidation_due_at(&store_path(), &meta_path(), unix_now())
}

/// The unpinned entries a consolidation plan may operate on. Pinned
/// entries are physically excluded — user-controlled, immutable by
/// machinery.
pub fn consolidation_input() -> Vec<MemoryEntry> {
    load_from(&store_path())
        .into_iter()
        .filter(|e| !e.pinned)
        .collect()
}

/// Validate + apply an LLM-produced consolidation plan and stamp the
/// meta timestamp. All-or-nothing; see `apply_consolidation_at`.
pub fn apply_consolidation(plan: &serde_json::Value) -> Result<String, String> {
    let now = unix_now();
    let summary = apply_consolidation_at(&store_path(), &archive_path(), plan, now)?;
    save_meta(
        &meta_path(),
        &MemoryMeta {
            last_consolidated_at: now,
        },
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_extraction() {
        let c = extract_user_captures("Please remember that I deploy on Fridays.");
        assert_eq!(c, vec!["I deploy on Fridays"]);
        let c = extract_user_captures("Hi, my name is Jim and I use Bazzite.");
        assert_eq!(c, vec!["The user's name is Jim"]);
        // No cue → nothing captured (high precision).
        assert!(extract_user_captures("what's the weather today?").is_empty());
        assert!(extract_user_captures("I think this is great").is_empty());
    }

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

        let block = injection_block_at(&path, "", 1000, None).unwrap();
        let lines: Vec<&str> = block.lines().collect();
        assert!(lines[0].starts_with("The user's saved memories"));
        // Pinned first; unpinned by score (recency dominates an
        // empty query, so new before old).
        assert!(lines[1].contains("pinned one") && lines[1].starts_with("- [c"));
        assert!(lines[2].contains("new unpinned"));
        assert!(lines[3].contains("old unpinned"));

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
            // Distinct token sets per entry — texts this similar
            // would otherwise (correctly) collapse via the
            // write-time dedupe.
            save_entry_at(&path, &format!("entry topic{i} detail{i}"), "s", i);
        }
        let block = injection_block_at(&path, "", 100, None).unwrap();
        // Header + 10 bullets; the two oldest (0, 1) fall off.
        assert_eq!(block.lines().count(), 11);
        assert!(!block.contains("topic0 "));
        assert!(block.contains("topic11"));
    }

    #[test]
    fn injection_block_empty_store_is_none() {
        let path = temp_store("empty");
        assert_eq!(injection_block_at(&path, "", 1, None), None);
    }
}

#[cfg(test)]
mod recall_tests {
    use super::*;

    fn temp_store(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxidemx-memory-recall-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("memories.json")
    }

    #[test]
    fn near_duplicate_save_updates_instead_of_appending() {
        let path = temp_store("dedupe");
        let a = save_entry_at(
            &path,
            "Jim prefers dark color themes everywhere",
            "prefs",
            100,
        );
        let b = save_entry_at(
            &path,
            "Jim prefers dark color themes everywhere, especially teal",
            "prefs",
            200,
        );
        assert_eq!(a.id, b.id, "near-duplicate should update in place");
        let all = load_from(&path);
        assert_eq!(all.len(), 1);
        assert!(all[0].text.contains("teal"));
        assert_eq!(all[0].created_at, 100, "created_at survives updates");

        // A genuinely different fact still appends.
        save_entry_at(
            &path,
            "Jim's mouse is an MX Master 4 on Bazzite",
            "prefs",
            300,
        );
        assert_eq!(load_from(&path).len(), 2);
    }

    #[test]
    fn injection_ranks_relevant_entries_first() {
        let path = temp_store("rank");
        // Old but relevant vs new but irrelevant.
        save_entry_at(
            &path,
            "Jim's weather widget shows Huntington Station",
            "w",
            100,
        );
        save_entry_at(&path, "Jim plays guitar on weekends", "hobby", 9_000_000);
        let block = injection_block_at(
            &path,
            "change the weather widget location",
            10_000_000,
            None,
        )
        .unwrap();
        let weather_pos = block.find("weather widget").unwrap();
        let guitar_pos = block.find("guitar").unwrap();
        assert!(
            weather_pos < guitar_pos,
            "query-relevant entry should rank above newer irrelevant one"
        );
    }

    #[test]
    fn search_returns_relevant_top_k() {
        let path = temp_store("search");
        for i in 0..8 {
            save_entry_at(
                &path,
                &format!("filler subject{i} detail{i} extra{i}"),
                "s",
                i,
            );
        }
        save_entry_at(
            &path,
            "the daemon battery fix uses try_initial_connect",
            "dev",
            50,
        );
        let hits = search_at(&path, "battery daemon connect", 100, 5);
        assert_eq!(hits.len(), 5);
        assert!(hits[0].text.contains("battery"));
    }

    #[test]
    fn consolidation_rails_hold() {
        let path = temp_store("consolidate");
        let archive = path.with_file_name("memories_archive.json");
        let mut ids = Vec::new();
        for i in 0..8 {
            ids.push(save_entry_at(&path, &format!("unique fact {i} alpha{i}"), "s", i).id);
        }
        let pinned = save_entry_at(&path, "sacred pinned fact", "s", 0);
        assert!(set_pinned_at(&path, &pinned.id, true));

        // Rail 1: missing id in ledger -> rejected, store untouched.
        let bad = serde_json::json!({"ledger": {ids[0].clone(): "keep"}, "entries": []});
        assert!(apply_consolidation_at(&path, &archive, &bad, 1000).is_err());
        assert_eq!(load_from(&path).len(), 9);

        // Rail 2: nuking everything -> rejected.
        let mut nuke_ledger = serde_json::Map::new();
        for id in &ids {
            nuke_ledger.insert(id.clone(), serde_json::json!("expired"));
        }
        let nuke = serde_json::json!({"ledger": nuke_ledger, "entries": []});
        assert!(apply_consolidation_at(&path, &archive, &nuke, 1000).is_err());

        // Valid plan: merge two entries into one, keep the rest.
        let mut ledger = serde_json::Map::new();
        for (i, id) in ids.iter().enumerate() {
            ledger.insert(
                id.clone(),
                serde_json::json!(if i < 2 { "merged" } else { "keep" }),
            );
        }
        let plan = serde_json::json!({
            "ledger": ledger,
            "entries": [{"text": "facts 0 and 1, merged", "scope": "s"}],
        });
        let summary = apply_consolidation_at(&path, &archive, &plan, 1000).unwrap();
        assert!(summary.contains("8 unpinned entries -> 7"));
        let after = load_from(&path);
        // 1 pinned + 6 kept + 1 merged = 8; pinned untouched.
        assert_eq!(after.len(), 8);
        assert!(after
            .iter()
            .any(|e| e.pinned && e.text == "sacred pinned fact"));
        assert!(after.iter().any(|e| e.text == "facts 0 and 1, merged"));
        // Tombstones in the archive, originals gone from the store.
        let archived: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(&archive).unwrap()).unwrap();
        assert_eq!(archived.len(), 2);
        assert!(!after.iter().any(|e| e.text == "unique fact 0 alpha0"));
    }

    #[test]
    fn unix_to_date_known_values() {
        assert_eq!(unix_to_date(0), "1970-01-01");
        assert_eq!(unix_to_date(1_781_222_400), "2026-06-12");
    }
}

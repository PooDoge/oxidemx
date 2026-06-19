//! Per-project transcript persistence + session hosting.
//!
//! [`TranscriptStore`] appends/reads JSONL transcript files, one per
//! `thread_id`, stored under a configurable root directory.
//!
//! [`Sessions`] is the agentd-level host: it wraps a [`SessionManager`] (an
//! in-process seam for per-conversation LLM provider caching) and a per-project
//! [`TranscriptStore`] cache. The `SessionManager` here is a local stub because
//! the canonical implementation lives in `oxidemx-agent` (not
//! `oxidemx-agent-core`), and pulling that crate into agentd would drag in the
//! entire autoagents + reqwest stack. The seam is documented in
//! `sdd/task-3-report.md`; a future task can replace [`SessionManager`] with
//! the real one via a trait object once the dep boundary is resolved.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::AgentdError;
use crate::projects::ProjectKey;

// ── TranscriptTurn ────────────────────────────────────────────────────────────

/// One turn in a conversation transcript, serialised as a single JSON line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptTurn {
    /// `"user"` or `"assistant"` (open-ended for tool turns etc.).
    pub role: String,
    /// The turn's text content.
    pub text: String,
    /// Unix timestamp (milliseconds since epoch) when the turn was appended.
    pub ts: u64,
}

// ── TranscriptStore ───────────────────────────────────────────────────────────

/// Append-only JSONL transcript store for a single project.
///
/// Each thread gets its own file: `<root>/<thread_id>.jsonl`.
/// The directory is created on first use.
pub struct TranscriptStore {
    root: PathBuf,
}

impl TranscriptStore {
    /// Create a store rooted at `root`. The directory is created lazily on the
    /// first write, so constructing a store is always cheap and infallible.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn ensure_dir(&self) -> Result<(), AgentdError> {
        fs::create_dir_all(&self.root).map_err(AgentdError::from)
    }

    fn thread_path(&self, thread: &str) -> PathBuf {
        self.root.join(format!("{thread}.jsonl"))
    }

    /// Append a single turn to `<root>/<thread>.jsonl`, creating the file if
    /// it does not yet exist.
    pub fn append(&self, thread: &str, turn: &TranscriptTurn) -> Result<(), AgentdError> {
        self.ensure_dir()?;
        let path = self.thread_path(thread);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(AgentdError::from)?;
        let line = serde_json::to_string(turn).map_err(|e| AgentdError::Io(e.to_string()))?;
        writeln!(file, "{line}").map_err(AgentdError::from)
    }

    /// Read all turns for `thread` in append order. Returns an empty `Vec` if
    /// the file does not exist.
    pub fn read(&self, thread: &str) -> Result<Vec<TranscriptTurn>, AgentdError> {
        let path = self.thread_path(thread);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path).map_err(AgentdError::from)?;
        let reader = BufReader::new(file);
        let mut turns = Vec::new();
        for (i, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(AgentdError::from)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let turn: TranscriptTurn = serde_json::from_str(trimmed).map_err(|e| {
                AgentdError::Io(format!(
                    "transcript {thread}: parse error at line {i}: {e}"
                ))
            })?;
            turns.push(turn);
        }
        Ok(turns)
    }

    /// List all thread IDs (file stems of `*.jsonl` files in `root`).
    /// Returns an empty `Vec` if the directory does not yet exist.
    pub fn list_threads(&self) -> Result<Vec<String>, AgentdError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&self.root).map_err(AgentdError::from)?;
        let mut threads = Vec::new();
        for entry in entries {
            let entry = entry.map_err(AgentdError::from)?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    threads.push(stem.to_string());
                }
            }
        }
        Ok(threads)
    }
}

// ── SessionManager (local stub) ───────────────────────────────────────────────
//
// The canonical `SessionManager` lives in `oxidemx-agent` (crate path
// `oxidemx_agent::session::SessionManager`). That crate is not a dependency
// of `agentd` because it pulls in autoagents + reqwest — a heavy stack that
// agentd doesn't need for its core duties.
//
// This minimal stub satisfies the `Sessions` wrapper's API surface. A future
// task can swap it for the real implementation once the dep boundary question
// (should agentd depend on oxidemx-agent, or should session logic migrate to
// oxidemx-agent-core?) is resolved. The key type and composition pattern
// (`"{project}:{thread}"`) are preserved so the seam is zero-change.

/// A minimal session tracker: ensures each `(project, thread)` conversation
/// gets a stable, isolated entry. Replace with the real `SessionManager` from
/// `oxidemx-agent` once the dep boundary is resolved (see task-3-report.md).
#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Arc<SessionEntry>>>,
}

/// Lightweight per-conversation entry (no LLM provider here — that stays in
/// the real `oxidemx-agent::session::Session`).
#[allow(dead_code)]
pub struct SessionEntry {
    pub id: String,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create the session entry for `(project_key, thread_id)`.
    pub fn session(&self, project_key: &ProjectKey, thread_id: &str) -> Arc<SessionEntry> {
        let id = compose_session_id(project_key, thread_id);
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id.clone())
            .or_insert_with(|| Arc::new(SessionEntry { id }))
            .clone()
    }

    /// Drop the entry for `(project_key, thread_id)`, e.g. when a thread is
    /// deleted.
    pub fn end(&self, project_key: &ProjectKey, thread_id: &str) {
        let id = compose_session_id(project_key, thread_id);
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    /// Number of live session entries (diagnostics).
    pub fn len(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Compose the canonical session ID used as the `SessionManager` key.
/// Per-spec: `"{project}:{thread}"`.
pub fn compose_session_id(project_key: &ProjectKey, thread_id: &str) -> String {
    format!("{}:{}", project_key.as_str(), thread_id)
}

// ── Sessions ──────────────────────────────────────────────────────────────────

/// agentd-level host: owns a [`SessionManager`] and a per-project
/// [`TranscriptStore`] cache.
pub struct Sessions {
    sm: SessionManager,
    /// Transcript stores, keyed by project key string.
    transcripts_by_project: Mutex<HashMap<String, Arc<TranscriptStore>>>,
}

impl Sessions {
    pub fn new() -> Self {
        Self {
            sm: SessionManager::new(),
            transcripts_by_project: Mutex::new(HashMap::new()),
        }
    }

    /// Get (or create) the [`TranscriptStore`] for `project_key`, rooted at
    /// `transcripts_root`.
    ///
    /// `transcripts_root` is typically obtained from
    /// `ProjectPaths::transcripts_dir()`.
    pub fn transcripts(
        &self,
        project_key: &ProjectKey,
        transcripts_root: PathBuf,
    ) -> Arc<TranscriptStore> {
        self.transcripts_by_project
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(project_key.as_str().to_string())
            .or_insert_with(|| Arc::new(TranscriptStore::new(transcripts_root)))
            .clone()
    }

    /// Borrow the [`SessionManager`].
    pub fn session_manager(&self) -> &SessionManager {
        &self.sm
    }
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_composed_correctly() {
        let d = tempfile::tempdir().unwrap();
        let key = ProjectKey::from_cwd(d.path());
        let id = compose_session_id(&key, "thread-42");
        assert!(id.contains(':'), "must use colon separator");
        assert!(id.ends_with(":thread-42"));
        assert!(id.starts_with(key.as_str()));
    }

    #[test]
    fn session_manager_returns_same_entry_per_key() {
        let d = tempfile::tempdir().unwrap();
        let key = ProjectKey::from_cwd(d.path());
        let sm = SessionManager::new();
        let a = sm.session(&key, "t1");
        let b = sm.session(&key, "t1");
        assert!(Arc::ptr_eq(&a, &b), "same key returns same entry");
        sm.session(&key, "t2");
        assert_eq!(sm.len(), 2);
        sm.end(&key, "t1");
        assert_eq!(sm.len(), 1);
    }

    #[test]
    fn sessions_caches_transcript_store_per_project() {
        let d = tempfile::tempdir().unwrap();
        let key = ProjectKey::from_cwd(d.path());
        let sessions = Sessions::new();
        let root = d.path().join("transcripts");
        let ts1 = sessions.transcripts(&key, root.clone());
        let ts2 = sessions.transcripts(&key, root.clone());
        assert!(
            Arc::ptr_eq(&ts1, &ts2),
            "same project returns cached store"
        );
    }
}

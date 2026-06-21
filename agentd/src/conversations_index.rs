//! Per-project index of `Conversation` metadata — backed by `<project_store_dir>/conversations.json`.
//!
//! Each public method re-reads and re-writes the file on every call (files are tiny;
//! stateless design keeps the index safe for multi-process access and mirrors
//! `ProjectStore`'s philosophy).  Interior mutability is intentionally absent: all
//! mutation paths load → mutate vec → save.
use std::path::PathBuf;

use tracing::warn;

use crate::model::{Conversation, ConversationId};

pub struct ConversationIndex {
    project_store_dir: PathBuf,
}

impl ConversationIndex {
    pub fn new(project_store_dir: PathBuf) -> Self {
        Self { project_store_dir }
    }

    // ------------------------------------------------------------------ load/save

    fn path(&self) -> PathBuf {
        self.project_store_dir.join("conversations.json")
    }

    fn load(&self) -> Vec<Conversation> {
        let p = self.path();
        match std::fs::read_to_string(&p) {
            Ok(txt) => serde_json::from_str(&txt).unwrap_or_else(|e| {
                warn!("ConversationIndex: failed to parse {}: {e}", p.display());
                vec![]
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => {
                warn!("ConversationIndex: failed to read {}: {e}", p.display());
                vec![]
            }
        }
    }

    fn save(&self, conversations: &[Conversation]) {
        let p = self.path();
        let tmp = self.project_store_dir.join("conversations.json.tmp");
        let json = match serde_json::to_string_pretty(conversations) {
            Ok(j) => j,
            Err(e) => { warn!("ConversationIndex: serialize failed: {e}"); return; }
        };
        if let Err(e) = std::fs::create_dir_all(&self.project_store_dir) {
            warn!("ConversationIndex: mkdir failed: {e}");
            return;
        }
        if let Err(e) = std::fs::write(&tmp, &json) {
            warn!("ConversationIndex: write tmp failed: {e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &p) {
            warn!("ConversationIndex: rename failed: {e}");
        }
    }

    // ------------------------------------------------------------------ public API

    /// Returns all conversations, reading from disk.
    pub fn list(&self) -> Vec<Conversation> {
        self.load()
    }

    /// Returns the conversation with the given id, or `None` if not found.
    pub fn get(&self, id: &ConversationId) -> Option<Conversation> {
        self.load().into_iter().find(|c| &c.id == id)
    }

    /// Replaces the conversation with the same id if present, otherwise appends it.
    pub fn upsert(&self, conversation: Conversation) {
        let mut conversations = self.load();
        if let Some(slot) = conversations.iter_mut().find(|c| c.id == conversation.id) {
            *slot = conversation;
        } else {
            conversations.push(conversation);
        }
        self.save(&conversations);
    }

    /// Sets the title of the conversation with the given id.
    pub fn rename(&self, id: &ConversationId, title: &str) {
        let mut conversations = self.load();
        if let Some(c) = conversations.iter_mut().find(|c| &c.id == id) {
            c.title = title.to_string();
        }
        self.save(&conversations);
    }

    /// Removes the conversation with the given id.
    pub fn delete(&self, id: &ConversationId) {
        let mut conversations = self.load();
        conversations.retain(|c| &c.id != id);
        self.save(&conversations);
    }

    /// Sets the working directory of the conversation with the given id.
    pub fn set_working_dir(&self, id: &ConversationId, working_dir: PathBuf) {
        let mut conversations = self.load();
        if let Some(c) = conversations.iter_mut().find(|c| &c.id == id) {
            c.working_dir = working_dir;
        }
        self.save(&conversations);
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConversationId, ProjectId};

    fn mk(id: &str, title: &str) -> Conversation {
        Conversation {
            id: ConversationId::from(id),
            project_id: ProjectId::from("personal"),
            title: title.to_string(),
            working_dir: std::path::PathBuf::from("/tmp"),
            model: "gemini-2.5-flash".to_string(),
            summary: String::new(),
            summary_upto: 0,
            tokens_prompt: 0,
            tokens_completion: 0,
            created_at: 1,
            updated_at: 2,
            worktree: None,
        }
    }

    #[test]
    fn upsert_list_rename_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = ConversationIndex::new(tmp.path().to_path_buf());
        let c = mk("chat-1", "first");
        idx.upsert(c.clone());
        assert_eq!(idx.list().len(), 1);
        idx.rename(&c.id, "renamed");
        assert_eq!(idx.get(&c.id).unwrap().title, "renamed");
        // reload from disk
        let idx2 = ConversationIndex::new(tmp.path().to_path_buf());
        assert_eq!(idx2.list().len(), 1);
        idx2.delete(&c.id);
        assert!(idx2.get(&c.id).is_none());
        assert!(ConversationIndex::new(tmp.path().to_path_buf()).list().is_empty());
    }
}

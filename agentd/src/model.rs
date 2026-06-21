//! Project / Conversation / Worktree value types — the agentd-owned model
//! (single source of truth; clients are thin readers).
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name { pub fn as_str(&self) -> &str { &self.0 } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
        }
        impl From<&str> for $name { fn from(s: &str) -> Self { Self(s.to_string()) } }
        impl From<String> for $name { fn from(s: String) -> Self { Self(s) } }
    };
}
id_newtype!(ProjectId);
id_newtype!(ConversationId);

pub const PERSONAL_PROJECT_ID: &str = "personal";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    /// Empty for the Personal project (no specific repo).
    pub default_working_dir: PathBuf,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub project_id: ProjectId,
    pub title: String,
    pub working_dir: PathBuf,
    pub model: String,
    #[serde(default)] pub summary: String,
    #[serde(default)] pub summary_upto: usize,
    #[serde(default)] pub tokens_prompt: u64,
    #[serde(default)] pub tokens_completion: u64,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Worktree>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Worktree { pub path: PathBuf, pub branch: String, pub base_ref: String }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_and_conversation_roundtrip() {
        let p = Project { id: ProjectId::from("personal"), name: "Personal".into(),
            default_working_dir: std::path::PathBuf::new(), created_at: 1 };
        let j = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id.as_str(), "personal");
        let c = Conversation { id: ConversationId::from("chat-1"),
            project_id: ProjectId::from("personal"), title: "hi".into(),
            working_dir: std::path::PathBuf::from("/tmp"), model: "gemini-2.5-flash".into(),
            summary: String::new(), summary_upto: 0, tokens_prompt: 0, tokens_completion: 0,
            created_at: 1, updated_at: 2, worktree: None };
        let back: Conversation = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.id.as_str(), "chat-1");
        assert!(back.worktree.is_none());
    }
}

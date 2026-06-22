//! Wire DTOs mirroring agentd's agent-protocol JSON (loose coupling; unknown
//! fields are tolerated, never denied).
use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name {
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
        }
        impl From<&str> for $name { fn from(s: &str) -> Self { Self(s.to_string()) } }
        impl From<String> for $name { fn from(s: String) -> Self { Self(s) } }
    };
}
id_newtype!(ProjectId);
id_newtype!(ConversationId);
id_newtype!(MessageId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    #[serde(default)]
    pub default_working_dir: String,
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub project_id: ProjectId,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub ts: u64,
}

/// A normalized SSE event. `seq` from the frame `id:`, `kind` from `event:`
/// (falls back to `payload.kind`), `payload` is the frame `data:` JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentEvent {
    pub seq: u64,
    pub kind: String,
    pub payload: serde_json::Value,
}

impl AgentEvent {
    /// The `text` field if present (delta/final/activity carry it).
    pub fn text(&self) -> Option<&str> {
        self.payload.get("text").and_then(|v| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_decodes_from_agentd_json() {
        let j = r#"{"id":"personal","name":"Personal","default_working_dir":"","created_at":17}"#;
        let p: Project = serde_json::from_str(j).unwrap();
        assert_eq!(p.id.as_str(), "personal");
        assert_eq!(p.name, "Personal");
    }

    #[test]
    fn conversation_ignores_unknown_fields() {
        let j = r#"{"id":"c1","project_id":"personal","title":"hi","working_dir":"/tmp",
                    "model":"gemini-2.5-flash","created_at":1,"updated_at":2,
                    "summary":"x","tokens_prompt":9,"worktree":{"path":"/w","branch":"b","base_ref":"r"}}"#;
        let c: Conversation = serde_json::from_str(j).unwrap();
        assert_eq!(c.id.as_str(), "c1");
        assert_eq!(c.model, "gemini-2.5-flash");
    }

    #[test]
    fn turn_decodes() {
        let j = r#"{"role":"assistant","text":"hello","ts":42}"#;
        let t: Turn = serde_json::from_str(j).unwrap();
        assert_eq!(t.role, "assistant");
        assert_eq!(t.text, "hello");
    }
}

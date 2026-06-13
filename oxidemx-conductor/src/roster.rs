//! Agent roster — reusable agent definitions (spec §8, end).
//!
//! `~/.config/oxidemx/agents/<id>.md` shares flow.md's shape: TOML
//! frontmatter (`id, name, model, executor, tools, approval,
//! memory_scope, capabilities`) + a markdown body that is the agent's
//! system-prompt addendum (the persona). Steps reference roster
//! agents by id (`agent = "web-researcher"`); `capabilities` feed the
//! ranked registry used by route-steps and the NL conductor (P5).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::flowdoc::{split_frontmatter_pub, FlowDocError};

/// A parsed agent definition: declaration + persona body.
#[derive(Debug, Clone)]
pub struct AgentDef {
    pub decl: AgentDecl,
    /// Markdown body = system-prompt addendum (the persona text).
    pub persona: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentDecl {
    pub id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub executor: Option<String>,
    /// Tool grants. A step's agent may only use tools it lists here;
    /// validation checks the grants exist in the tool registry.
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub approval: Option<String>,
    #[serde(default)]
    pub memory_scope: Option<String>,
    /// Capability tags for the ranked registry (exact > substring).
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl AgentDef {
    pub fn parse(src: &str) -> Result<Self, FlowDocError> {
        let (frontmatter, body) = split_frontmatter_pub(src)?;
        let decl: AgentDecl = toml::from_str(frontmatter)?;
        Ok(Self {
            decl,
            persona: body.to_string(),
        })
    }

    pub fn name(&self) -> &str {
        self.decl.name.as_deref().unwrap_or(&self.decl.id)
    }
}

/// An in-memory roster keyed by agent id.
#[derive(Debug, Clone, Default)]
pub struct Roster {
    agents: BTreeMap<String, AgentDef>,
}

impl Roster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, def: AgentDef) {
        self.agents.insert(def.decl.id.clone(), def);
    }

    pub fn get(&self, id: &str) -> Option<&AgentDef> {
        self.agents.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.agents.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Rank roster agents by how well they match a capability query,
    /// kowalski-style: exact tag match (10_000) beats substring
    /// (scored by overlap length) beats nothing; id is the tiebreak.
    /// Used by route-steps and the NL conductor's delegation tool.
    pub fn rank_by_capability(&self, query: &str) -> Vec<(&str, u32)> {
        let q = query.to_lowercase();
        let mut scored: Vec<(&str, u32)> = self
            .agents
            .values()
            .filter_map(|a| {
                let best = a
                    .decl
                    .capabilities
                    .iter()
                    .map(|cap| {
                        let cap = cap.to_lowercase();
                        if cap == q {
                            10_000
                        } else if cap.contains(&q) || q.contains(&cap) {
                            cap.len().min(q.len()) as u32
                        } else {
                            0
                        }
                    })
                    .max()
                    .unwrap_or(0);
                (best > 0).then_some((a.decl.id.as_str(), best))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB_RESEARCHER: &str = r#"---
id = "web-researcher"
name = "Web Researcher"
model = "gemini-2.5-flash"
executor = "react"
tools = ["execute_command"]
approval = "allowlist"
capabilities = ["fetch", "web", "ingest"]
---

You fetch web pages and normalize them into clean markdown.
"#;

    #[test]
    fn parses_agent_decl_and_persona() {
        let def = AgentDef::parse(WEB_RESEARCHER).unwrap();
        assert_eq!(def.decl.id, "web-researcher");
        assert_eq!(def.name(), "Web Researcher");
        assert_eq!(def.decl.tools, vec!["execute_command"]);
        assert!(def.persona.contains("normalize them into clean markdown"));
    }

    #[test]
    fn roster_capability_ranking_prefers_exact() {
        let mut r = Roster::new();
        r.insert(AgentDef::parse(WEB_RESEARCHER).unwrap());
        r.insert(
            AgentDef::parse(
                "---\nid = \"summarizer\"\ncapabilities = [\"summarize\", \"digest\"]\n---\nbody",
            )
            .unwrap(),
        );
        let ranked = r.rank_by_capability("fetch");
        assert_eq!(ranked[0].0, "web-researcher");
        assert_eq!(ranked[0].1, 10_000);
        // No match → not present.
        assert!(r.rank_by_capability("kubernetes").is_empty());
    }
}

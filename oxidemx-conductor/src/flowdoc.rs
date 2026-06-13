//! The flow document — the one canonical artifact (spec §8).
//!
//! A `flow.md` is TOML frontmatter inside `---` fences followed by a
//! markdown body. The frontmatter is the machine-readable manifest
//! parsed here; the body is the flow's self-description (fed to
//! semantic memory so the NL conductor's `find_flow` works — P5).
//!
//! This is a deliberate superset of kowalski's horde format, fixing
//! each documented limitation: sequential-only → DAG via `needs`;
//! no retry → per-step `retry`; no parallelism → free from the DAG
//! (ready steps run concurrently); no conditionals → `kind="route"`.
//!
//! Parsing is read-only here. The Tier-B wizard (P5) will need a
//! round-trip editor (`toml_edit`) that preserves unknown fields +
//! body; this module deliberately does NOT throw unknown fields away
//! at the type level beyond what serde drops, so that swap is local.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Parse / validation errors for a flow document.
#[derive(Debug, thiserror::Error)]
pub enum FlowDocError {
    #[error("flow.md has no `---` TOML frontmatter fence")]
    MissingFrontmatter,
    #[error("flow.md frontmatter is not closed by a second `---`")]
    UnclosedFrontmatter,
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

/// A fully-parsed flow document: manifest + markdown body.
#[derive(Debug, Clone)]
pub struct FlowDoc {
    pub manifest: Manifest,
    /// Markdown body after the frontmatter fence (the NL discovery
    /// text). Empty string if the file is frontmatter-only.
    pub body: String,
}

/// The TOML manifest (everything inside the `---` fences).
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub flow: FlowMeta,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputSpec>,
    #[serde(default)]
    pub defaults: Defaults,
    /// `[[step]]` tables, in file order. The DAG comes from `needs`,
    /// not from order, but order is preserved for deterministic
    /// scheduling of otherwise-equal ready steps.
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub triggers: Triggers,
    #[serde(default)]
    pub delivery: Option<Delivery>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlowMeta {
    pub id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

/// A typed run input, templated as `{{input.<name>}}`.
#[derive(Debug, Clone, Deserialize)]
pub struct InputSpec {
    #[serde(default = "default_input_type", rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

fn default_input_type() -> String {
    "string".to_string()
}

/// Per-run defaults, overridable per step.
#[derive(Debug, Clone, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_executor")]
    pub executor: String,
    #[serde(default = "default_approval")]
    pub approval: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            model: default_model(),
            executor: default_executor(),
            approval: default_approval(),
            memory: default_memory(),
            max_turns: default_max_turns(),
        }
    }
}

fn default_model() -> String {
    "gemini-2.5-flash".to_string()
}
fn default_executor() -> String {
    "react".to_string()
}
fn default_approval() -> String {
    "allowlist".to_string()
}
fn default_memory() -> String {
    "run".to_string()
}
fn default_max_turns() -> usize {
    10
}

/// One `[[step]]`. The `kind` discriminates the node type; the
/// fields relevant to other kinds are simply `None` for a plain agent
/// step (validation in `plan.rs` enforces kind-specific requirements).
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub id: String,
    /// `"agent"` (default), `"reflect"`, or `"route"`.
    #[serde(default = "default_step_kind")]
    pub kind: String,

    // ── agent-step fields ────────────────────────────────────────
    /// Roster id or `./agents/<file>.md`. Required for agent steps.
    #[serde(default)]
    pub agent: Option<String>,
    /// DAG edges. `[]` (or omitted) ⇒ entry point. Accepts a single
    /// string or an array (`needs = "x"` or `needs = ["x", "y"]`).
    #[serde(default, deserialize_with = "string_or_seq")]
    pub needs: Vec<String>,
    /// The concrete instruction, templated with `{{input.*}}`.
    #[serde(default)]
    pub task: Option<String>,
    /// Context tokens injected before the task (`@artifact@`,
    /// `@step:<id>@`). Resolved by `template.rs`. Accepts a single
    /// string or an array.
    #[serde(default, deserialize_with = "string_or_seq")]
    pub context: Vec<String>,
    /// Artifact path under the run workdir to write this step's
    /// output to (e.g. `debug/raw.md`, `ANSWER.md`).
    #[serde(default)]
    pub output: Option<String>,
    /// Per-step model override.
    #[serde(default)]
    pub model: Option<String>,
    /// Per-step approval policy override.
    #[serde(default)]
    pub approval: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub retry: Option<Retry>,
    /// Output normalization (kowalski post-1.2.0 semantics).
    #[serde(default)]
    pub normalize: Option<Normalize>,

    // ── reflect-step fields ──────────────────────────────────────
    /// Step whose output is critiqued + revised.
    #[serde(default)]
    pub target: Option<String>,
    /// Roster agent that plays critic.
    #[serde(default)]
    pub critic: Option<String>,
    #[serde(default)]
    pub max_rounds: Option<u32>,
    /// Acceptance predicate (`"no_blocking_findings"` for v1).
    #[serde(default)]
    pub accept_when: Option<String>,

    // ── route-step fields ────────────────────────────────────────
    /// `{ <choice_label> = <next_step_id> }`.
    #[serde(default)]
    pub choices: BTreeMap<String, String>,
    /// Classification prompt for the router.
    #[serde(default)]
    pub prompt: Option<String>,
}

fn default_step_kind() -> String {
    "agent".to_string()
}

/// Deserialize a `Vec<String>` from either a single string or an
/// array — so `needs = "x"` and `needs = ["x"]` both parse. Forgiving
/// ergonomics for hand-authored and LLM-composed flows alike.
fn string_or_seq<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Retry {
    #[serde(default)]
    pub max: u32,
    #[serde(default)]
    pub backoff_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Normalize {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub sections: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Triggers {
    #[serde(default)]
    pub slice: bool,
    #[serde(default)]
    pub schedule: Vec<String>,
    #[serde(default)]
    pub on_dbus: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Delivery {
    pub root: Option<String>,
    pub title: Option<String>,
    pub note: Option<String>,
}

impl FlowDoc {
    /// Parse a `flow.md`: TOML frontmatter between the first two `---`
    /// fences, then the markdown body.
    pub fn parse(src: &str) -> Result<Self, FlowDocError> {
        let (frontmatter, body) = split_frontmatter(src)?;
        let manifest: Manifest = toml::from_str(frontmatter)?;
        Ok(Self {
            manifest,
            body: body.to_string(),
        })
    }

    /// The display name (falls back to the id).
    pub fn name(&self) -> &str {
        self.manifest
            .flow
            .name
            .as_deref()
            .unwrap_or(&self.manifest.flow.id)
    }
}

/// Shared frontmatter splitter, reused by the roster parser (agent
/// `.md` files use the same TOML-frontmatter-+-markdown-body shape).
pub fn split_frontmatter_pub(src: &str) -> Result<(&str, &str), FlowDocError> {
    split_frontmatter(src)
}

/// Split `---\n<toml>\n---\n<body>`. Tolerates leading whitespace /
/// a BOM before the opening fence and CRLF line endings.
fn split_frontmatter(src: &str) -> Result<(&str, &str), FlowDocError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let trimmed = src.trim_start_matches([' ', '\t', '\n', '\r']);
    // The opening fence must be a line that is exactly `---`.
    let after_open = trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
        .ok_or(FlowDocError::MissingFrontmatter)?;

    // Find the closing fence: a line that is exactly `---`.
    let mut search_from = 0usize;
    loop {
        let rel = after_open[search_from..]
            .find("---")
            .ok_or(FlowDocError::UnclosedFrontmatter)?;
        let abs = search_from + rel;
        // Must be at a line start.
        let at_line_start = abs == 0 || after_open.as_bytes()[abs - 1] == b'\n';
        // Must be followed by end-of-string or a newline (so `---` is
        // the whole line, not e.g. a `---` inside the TOML).
        let after = &after_open[abs + 3..];
        let line_end = after.is_empty()
            || after.starts_with('\n')
            || after.starts_with("\r\n")
            || after.starts_with('\r');
        if at_line_start && line_end {
            let frontmatter = &after_open[..abs];
            let body = after
                .strip_prefix("\r\n")
                .or_else(|| after.strip_prefix('\n'))
                .unwrap_or(after);
            return Ok((frontmatter, body.trim_start_matches('\n')));
        }
        search_from = abs + 3;
    }
}

impl Step {
    /// Effective model: per-step override, else the flow default.
    pub fn model<'a>(&'a self, defaults: &'a Defaults) -> &'a str {
        self.model.as_deref().unwrap_or(&defaults.model)
    }

    /// Effective approval policy: per-step override, else default.
    pub fn approval<'a>(&'a self, defaults: &'a Defaults) -> &'a str {
        self.approval.as_deref().unwrap_or(&defaults.approval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESEARCH_DIGEST: &str = r#"---
[flow]
id = "research-digest"
name = "Research Digest"
description = "Ingest a URL, digest it, answer."
version = 1

[inputs]
url = { type = "string", required = true }
question = { type = "string", default = "Summarize the key changes." }

[defaults]
model = "gemini-2.5-flash"
executor = "react"
approval = "allowlist"
max_turns = 8

[[step]]
id = "ingest"
agent = "web-researcher"
needs = []
task = "Fetch and normalize {{input.url}} into markdown."
output = "debug/raw.md"
timeout_secs = 300
retry = { max = 2, backoff_secs = 10 }

[[step]]
id = "digest"
agent = "summarizer"
needs = ["ingest"]
task = "Digest the source. Question: {{input.question}}"
context = ["@artifact@"]
output = "debug/digest.md"
normalize = { title = "Source Digest", sections = ["Summary", "Claims"] }

[[step]]
id = "answer"
agent = "writer"
needs = ["digest"]
context = ["@step:digest@"]
output = "ANSWER.md"

[triggers]
slice = true
schedule = ["daily 09:00"]

[delivery]
root = "ANSWER.md"
title = "Research digest"
---

# Research Digest

Use this flow when the user wants a source fetched, digested and
interrogated.
"#;

    #[test]
    fn parses_full_manifest_and_body() {
        let doc = FlowDoc::parse(RESEARCH_DIGEST).expect("parse");
        assert_eq!(doc.manifest.flow.id, "research-digest");
        assert_eq!(doc.name(), "Research Digest");
        assert_eq!(doc.manifest.flow.version, 1);
        assert_eq!(doc.manifest.steps.len(), 3);
        assert!(doc.body.starts_with("# Research Digest"));
    }

    #[test]
    fn input_defaults_and_required_flags() {
        let doc = FlowDoc::parse(RESEARCH_DIGEST).unwrap();
        let url = &doc.manifest.inputs["url"];
        assert!(url.required);
        assert_eq!(url.ty, "string");
        let q = &doc.manifest.inputs["question"];
        assert!(!q.required);
        assert_eq!(q.default.as_deref(), Some("Summarize the key changes."));
    }

    #[test]
    fn step_kind_defaults_to_agent_and_retry_parses() {
        let doc = FlowDoc::parse(RESEARCH_DIGEST).unwrap();
        let ingest = &doc.manifest.steps[0];
        assert_eq!(ingest.kind, "agent");
        assert_eq!(ingest.needs, Vec::<String>::new());
        assert_eq!(ingest.timeout_secs, Some(300));
        let retry = ingest.retry.unwrap();
        assert_eq!(retry.max, 2);
        assert_eq!(retry.backoff_secs, 10);
    }

    #[test]
    fn per_step_model_falls_back_to_defaults() {
        let doc = FlowDoc::parse(RESEARCH_DIGEST).unwrap();
        let ingest = &doc.manifest.steps[0];
        assert_eq!(ingest.model(&doc.manifest.defaults), "gemini-2.5-flash");
        assert_eq!(ingest.approval(&doc.manifest.defaults), "allowlist");
    }

    #[test]
    fn defaults_fill_when_section_absent() {
        let src = "---\n[flow]\nid = \"bare\"\n---\nbody";
        let doc = FlowDoc::parse(src).unwrap();
        assert_eq!(doc.manifest.defaults.model, "gemini-2.5-flash");
        assert_eq!(doc.manifest.defaults.max_turns, 10);
        assert_eq!(doc.manifest.flow.version, 1);
        assert_eq!(doc.body, "body");
    }

    #[test]
    fn missing_frontmatter_is_an_error() {
        let err = FlowDoc::parse("# just markdown\n").unwrap_err();
        assert!(matches!(err, FlowDocError::MissingFrontmatter));
    }

    #[test]
    fn unclosed_frontmatter_is_an_error() {
        let err = FlowDoc::parse("---\n[flow]\nid=\"x\"\n").unwrap_err();
        assert!(matches!(err, FlowDocError::UnclosedFrontmatter));
    }

    #[test]
    fn needs_and_context_accept_string_or_array() {
        let src = r#"---
[flow]
id = "lenient"
[[step]]
id = "a"
agent = "x"
task = "t"
[[step]]
id = "b"
agent = "x"
task = "t"
needs = "a"
context = "@artifact@"
---
"#;
        let doc = FlowDoc::parse(src).unwrap();
        let b = doc.manifest.steps.iter().find(|s| s.id == "b").unwrap();
        assert_eq!(b.needs, vec!["a".to_string()]);
        assert_eq!(b.context, vec!["@artifact@".to_string()]);
    }

    #[test]
    fn route_and_reflect_fields_parse() {
        let src = r#"---
[flow]
id = "branchy"

[[step]]
id = "answer"
agent = "writer"
task = "write it"

[[step]]
id = "stress"
kind = "reflect"
target = "answer"
critic = "skeptic"
max_rounds = 2
accept_when = "no_blocking_findings"

[[step]]
id = "disposition"
kind = "route"
needs = ["answer"]
choices = { archive = "archive-note", notify = "send-notification" }
prompt = "Worth notifying?"
---
"#;
        let doc = FlowDoc::parse(src).unwrap();
        let stress = doc.manifest.steps.iter().find(|s| s.id == "stress").unwrap();
        assert_eq!(stress.kind, "reflect");
        assert_eq!(stress.target.as_deref(), Some("answer"));
        assert_eq!(stress.max_rounds, Some(2));
        let disp = doc.manifest.steps.iter().find(|s| s.id == "disposition").unwrap();
        assert_eq!(disp.kind, "route");
        assert_eq!(disp.choices["notify"], "send-notification");
    }
}

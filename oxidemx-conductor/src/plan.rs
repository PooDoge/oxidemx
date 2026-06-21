//! FlowDoc → validated `FlowPlan` (spec §8 validation list).
//!
//! Modeled on kowalski's `agent-app validate`
//! (`agent_app_ops.rs:127-167`) plus the DAG checks they don't need.
//! Collects ALL errors in one pass (not fail-first) so the wizard /
//! CLI can show every problem at once. A clean validation yields a
//! `FlowPlan`: the doc plus the precomputed topological order and
//! per-step transitive-ancestor sets the supervisor schedules from.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::flowdoc::{FlowDoc, Step};
use crate::roster::Roster;
use crate::template;

/// One validation problem, scoped to a step where applicable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Step id the error belongs to, or `None` for flow-level errors.
    pub step: Option<String>,
    pub message: String,
}

impl ValidationError {
    fn flow(message: impl Into<String>) -> Self {
        Self {
            step: None,
            message: message.into(),
        }
    }
    fn step(id: &str, message: impl Into<String>) -> Self {
        Self {
            step: Some(id.to_string()),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.step {
            Some(s) => write!(f, "[{s}] {}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// A validated, runnable flow.
#[derive(Debug, Clone)]
pub struct FlowPlan {
    pub doc: FlowDoc,
    /// Step ids in a topological order (deterministic: ready steps in
    /// file order). Entry points first.
    pub topo: Vec<String>,
    /// `ancestors[step]` = transitive closure of `needs`.
    ancestors: BTreeMap<String, BTreeSet<String>>,
}

impl FlowPlan {
    /// Look up a step by id.
    pub fn step(&self, id: &str) -> Option<&Step> {
        self.doc.manifest.steps.iter().find(|s| s.id == id)
    }

    /// Entry steps (no `needs`).
    pub fn entry_steps(&self) -> Vec<&Step> {
        self.doc
            .manifest
            .steps
            .iter()
            .filter(|s| s.needs.is_empty())
            .collect()
    }

    /// Steps that directly `need` `id` (its dependents).
    pub fn dependents(&self, id: &str) -> Vec<&Step> {
        self.doc
            .manifest
            .steps
            .iter()
            .filter(|s| s.needs.iter().any(|n| n == id))
            .collect()
    }

    /// Transitive ancestors of `id` (the steps it depends on).
    pub fn ancestors_of(&self, id: &str) -> Option<&BTreeSet<String>> {
        self.ancestors.get(id)
    }

    /// Steps grouped into the parallel execution stages the scheduler
    /// produces: stage 0 = entry nodes (no `needs`); stage N = steps whose
    /// `needs` all resolved in stages `0..N`. Order within a stage follows
    /// `topo` (deterministic). This mirrors the supervisor's readiness rule
    /// and is the concurrency contract the flow_stages tests assert.
    pub fn stages(&self) -> Vec<Vec<String>> {
        let mut placed: BTreeSet<String> = BTreeSet::new();
        let mut remaining: Vec<&str> = self.topo.iter().map(String::as_str).collect();
        let mut out: Vec<Vec<String>> = Vec::new();
        while !remaining.is_empty() {
            let ready: Vec<String> = remaining
                .iter()
                .filter(|id| {
                    self.step(id)
                        .map(|s| s.needs.iter().all(|n| placed.contains(n)))
                        .unwrap_or(false)
                })
                .map(|s| s.to_string())
                .collect();
            if ready.is_empty() {
                break; // defensive: a cycle can't reach here (validate rejects cycles)
            }
            for id in &ready {
                placed.insert(id.clone());
            }
            remaining.retain(|id| !placed.contains(*id));
            out.push(ready);
        }
        out
    }
}

/// Validate a flow document against a roster and the known-tool set.
/// Returns the plan, or every problem found.
pub fn validate(
    doc: &FlowDoc,
    roster: &Roster,
    known_tools: &[&str],
) -> Result<FlowPlan, Vec<ValidationError>> {
    let mut errors = Vec::new();
    let steps = &doc.manifest.steps;

    if doc.manifest.flow.id.trim().is_empty() {
        errors.push(ValidationError::flow("flow.id is empty"));
    }
    if steps.is_empty() {
        errors.push(ValidationError::flow("flow has no steps"));
    }

    // Unique, non-empty step ids.
    let mut seen = BTreeSet::new();
    for s in steps {
        if s.id.trim().is_empty() {
            errors.push(ValidationError::flow("a step has an empty id"));
        } else if !seen.insert(s.id.clone()) {
            errors.push(ValidationError::step(&s.id, "duplicate step id"));
        }
    }
    let ids: BTreeSet<&str> = steps.iter().map(|s| s.id.as_str()).collect();

    // Every `needs` resolves; no self-loop.
    for s in steps {
        for n in &s.needs {
            if n == &s.id {
                errors.push(ValidationError::step(&s.id, "step needs itself"));
            } else if !ids.contains(n.as_str()) {
                errors.push(ValidationError::step(
                    &s.id,
                    format!("needs unknown step `{n}`"),
                ));
            }
        }
    }

    // Input templating is closed against declared inputs.
    let declared: BTreeSet<&str> = doc.manifest.inputs.keys().map(String::as_str).collect();
    for s in steps {
        if let Some(task) = &s.task {
            for key in template::referenced_inputs(task) {
                if !declared.contains(key.as_str()) {
                    errors.push(ValidationError::step(
                        &s.id,
                        format!("task references undeclared input `{key}`"),
                    ));
                }
            }
        }
    }

    // Kind-specific structure + agent/tool/context resolution.
    let ancestors = transitive_ancestors(steps);
    for s in steps {
        match s.kind.as_str() {
            "agent" => validate_agent_step(s, roster, known_tools, &ancestors, &mut errors),
            "reflect" => validate_reflect_step(s, &ids, roster, &ancestors, &mut errors),
            "route" => validate_route_step(s, &ids, &mut errors),
            other => errors.push(ValidationError::step(
                &s.id,
                format!("unknown step kind `{other}` (expected agent|reflect|route)"),
            )),
        }
    }

    // Acyclicity via Kahn's algorithm; topo order falls out.
    let topo = match topological_order(steps) {
        Ok(order) => order,
        Err(cycle) => {
            errors.push(ValidationError::flow(format!(
                "flow has a dependency cycle involving: {}",
                cycle.join(" → ")
            )));
            Vec::new()
        }
    };

    if errors.is_empty() {
        Ok(FlowPlan {
            doc: doc.clone(),
            topo,
            ancestors,
        })
    } else {
        Err(errors)
    }
}

fn validate_agent_step(
    s: &Step,
    roster: &Roster,
    known_tools: &[&str],
    ancestors: &BTreeMap<String, BTreeSet<String>>,
    errors: &mut Vec<ValidationError>,
) {
    if s.task.as_deref().map(str::trim).unwrap_or("").is_empty() {
        errors.push(ValidationError::step(&s.id, "agent step has no `task`"));
    }
    let Some(agent_ref) = &s.agent else {
        errors.push(ValidationError::step(&s.id, "agent step has no `agent`"));
        return;
    };
    // Path refs (`./agents/x.md`) are resolved by the loader; only
    // roster-id refs are checked here.
    let is_path = agent_ref.starts_with("./") || agent_ref.ends_with(".md");
    if !is_path {
        match roster.get(agent_ref) {
            None => errors.push(ValidationError::step(
                &s.id,
                format!("agent `{agent_ref}` not in roster"),
            )),
            Some(def) => {
                for tool in &def.decl.tools {
                    if !known_tools.contains(&tool.as_str()) {
                        errors.push(ValidationError::step(
                            &s.id,
                            format!("agent `{agent_ref}` grants unknown tool `{tool}`"),
                        ));
                    }
                }
            }
        }
    }
    validate_context_tokens(s, ancestors, errors);
}

/// `@step:<id>@` must reference a transitive ancestor; `@artifact@` is
/// only well-defined when the step has exactly one `needs` entry.
fn validate_context_tokens(
    s: &Step,
    ancestors: &BTreeMap<String, BTreeSet<String>>,
    errors: &mut Vec<ValidationError>,
) {
    let empty = BTreeSet::new();
    let anc = ancestors.get(&s.id).unwrap_or(&empty);
    for tok in &s.context {
        let tok = tok.trim();
        if tok == "@artifact@" {
            if s.needs.len() != 1 {
                errors.push(ValidationError::step(
                    &s.id,
                    format!(
                        "`@artifact@` is ambiguous with {} needs; use `@step:<id>@`",
                        s.needs.len()
                    ),
                ));
            }
        } else if let Some(id) = tok.strip_prefix("@step:").and_then(|t| t.strip_suffix('@')) {
            if !anc.contains(id) {
                errors.push(ValidationError::step(
                    &s.id,
                    format!("`@step:{id}@` is not an ancestor of this step"),
                ));
            }
        } else {
            errors.push(ValidationError::step(
                &s.id,
                format!("unknown context token `{tok}`"),
            ));
        }
    }
}

fn validate_reflect_step(
    s: &Step,
    ids: &BTreeSet<&str>,
    roster: &Roster,
    ancestors: &BTreeMap<String, BTreeSet<String>>,
    errors: &mut Vec<ValidationError>,
) {
    match &s.target {
        None => errors.push(ValidationError::step(&s.id, "reflect step has no `target`")),
        Some(t) if !ids.contains(t.as_str()) => {
            errors.push(ValidationError::step(
                &s.id,
                format!("reflect target `{t}` is not a step"),
            ));
        }
        Some(t) => {
            // The target must be a real dependency, not just a runtime gate —
            // otherwise Kahn's sort treats this reflect step as an entry node
            // and it goes "ready" too early (the foot-gun).
            let empty = BTreeSet::new();
            let anc = ancestors.get(&s.id).unwrap_or(&empty);
            if !s.needs.iter().any(|n| n == t) && !anc.contains(t.as_str()) {
                errors.push(ValidationError::step(
                    &s.id,
                    format!(
                        "reflect target `{t}` must be listed in `needs` (directly or transitively)"
                    ),
                ));
            }
        }
    }
    match &s.critic {
        None => errors.push(ValidationError::step(&s.id, "reflect step has no `critic`")),
        Some(c) if roster.get(c).is_none() && !(c.starts_with("./") || c.ends_with(".md")) => {
            errors.push(ValidationError::step(
                &s.id,
                format!("critic `{c}` not in roster"),
            ));
        }
        _ => {}
    }
}

fn validate_route_step(s: &Step, ids: &BTreeSet<&str>, errors: &mut Vec<ValidationError>) {
    if s.prompt.as_deref().map(str::trim).unwrap_or("").is_empty() {
        errors.push(ValidationError::step(&s.id, "route step has no `prompt`"));
    }
    if s.choices.is_empty() {
        errors.push(ValidationError::step(&s.id, "route step has no `choices`"));
    }
    for (label, target) in &s.choices {
        if !ids.contains(target.as_str()) {
            errors.push(ValidationError::step(
                &s.id,
                format!("route choice `{label}` → unknown step `{target}`"),
            ));
        }
    }
}

/// Transitive `needs` closure per step (BFS up the edges).
fn transitive_ancestors(steps: &[Step]) -> BTreeMap<String, BTreeSet<String>> {
    let direct: BTreeMap<&str, &Vec<String>> =
        steps.iter().map(|s| (s.id.as_str(), &s.needs)).collect();
    let mut out = BTreeMap::new();
    for s in steps {
        let mut acc = BTreeSet::new();
        let mut queue: VecDeque<&str> = s.needs.iter().map(String::as_str).collect();
        while let Some(n) = queue.pop_front() {
            if acc.insert(n.to_string()) {
                if let Some(ups) = direct.get(n) {
                    for u in ups.iter() {
                        queue.push_back(u.as_str());
                    }
                }
            }
        }
        out.insert(s.id.clone(), acc);
    }
    out
}

/// Kahn topological sort. `Err` carries the steps still cyclic.
fn topological_order(steps: &[Step]) -> Result<Vec<String>, Vec<String>> {
    let order_index: BTreeMap<&str, usize> =
        steps.iter().enumerate().map(|(i, s)| (s.id.as_str(), i)).collect();
    let mut indegree: BTreeMap<&str, usize> = steps.iter().map(|s| (s.id.as_str(), 0)).collect();
    for s in steps {
        for n in &s.needs {
            if indegree.contains_key(n.as_str()) {
                *indegree.get_mut(s.id.as_str()).unwrap() += 1;
            }
        }
    }
    // Ready set, drained in file order for determinism.
    let mut ready: Vec<&str> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    ready.sort_by_key(|id| order_index[id]);
    let mut ready: VecDeque<&str> = ready.into();

    let mut out = Vec::with_capacity(steps.len());
    while let Some(id) = ready.pop_front() {
        out.push(id.to_string());
        // Decrement dependents.
        let mut newly: Vec<&str> = Vec::new();
        for s in steps {
            if s.needs.iter().any(|n| n == id) {
                let d = indegree.get_mut(s.id.as_str()).unwrap();
                *d -= 1;
                if *d == 0 {
                    newly.push(s.id.as_str());
                }
            }
        }
        newly.sort_by_key(|id| order_index[id]);
        for n in newly {
            ready.push_back(n);
        }
    }

    if out.len() == steps.len() {
        Ok(out)
    } else {
        let cyclic: Vec<String> = steps
            .iter()
            .map(|s| s.id.clone())
            .filter(|id| !out.contains(id))
            .collect();
        Err(cyclic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::AgentDef;

    fn roster() -> Roster {
        let mut r = Roster::new();
        for (id, tools) in [
            ("web-researcher", "[\"execute_command\"]"),
            ("summarizer", "[]"),
            ("extractor", "[]"),
            ("writer", "[]"),
            ("skeptic", "[]"),
        ] {
            r.insert(
                AgentDef::parse(&format!("---\nid = \"{id}\"\ntools = {tools}\n---\npersona"))
                    .unwrap(),
            );
        }
        r
    }

    fn doc(src: &str) -> FlowDoc {
        FlowDoc::parse(src).unwrap()
    }

    const GOOD: &str = r#"---
[flow]
id = "research-digest"

[inputs]
url = { type = "string", required = true }

[[step]]
id = "ingest"
agent = "web-researcher"
task = "Fetch {{input.url}}."
output = "debug/raw.md"

[[step]]
id = "digest"
agent = "summarizer"
needs = ["ingest"]
task = "Digest it."
context = ["@artifact@"]

[[step]]
id = "answer"
agent = "writer"
needs = ["digest"]
task = "Answer."
context = ["@step:digest@"]
---
"#;

    #[test]
    fn good_flow_validates_with_topo_order() {
        let plan = validate(&doc(GOOD), &roster(), &["execute_command"]).expect("valid");
        assert_eq!(plan.topo, vec!["ingest", "digest", "answer"]);
        assert_eq!(plan.entry_steps().len(), 1);
        assert_eq!(plan.dependents("ingest")[0].id, "digest");
        assert!(plan.ancestors_of("answer").unwrap().contains("ingest"));
    }

    #[test]
    fn cycle_is_rejected() {
        let src = r#"---
[flow]
id = "cyclic"
[[step]]
id = "a"
agent = "writer"
task = "t"
needs = ["b"]
[[step]]
id = "b"
agent = "writer"
task = "t"
needs = ["a"]
---
"#;
        let errs = validate(&doc(src), &roster(), &[]).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("cycle")));
    }

    #[test]
    fn unknown_needs_and_agent_and_input_are_caught() {
        let src = r#"---
[flow]
id = "broken"
[[step]]
id = "x"
agent = "ghost"
task = "Use {{input.nope}}."
needs = ["missing"]
---
"#;
        let errs = validate(&doc(src), &roster(), &[]).unwrap_err();
        let msgs: Vec<_> = errs.iter().map(|e| e.message.clone()).collect();
        assert!(msgs.iter().any(|m| m.contains("unknown step `missing`")));
        assert!(msgs.iter().any(|m| m.contains("not in roster")));
        assert!(msgs.iter().any(|m| m.contains("undeclared input `nope`")));
    }

    #[test]
    fn artifact_token_ambiguous_on_join() {
        let src = r#"---
[flow]
id = "joiny"
[[step]]
id = "a"
agent = "writer"
task = "t"
[[step]]
id = "b"
agent = "writer"
task = "t"
[[step]]
id = "j"
agent = "writer"
task = "t"
needs = ["a", "b"]
context = ["@artifact@"]
---
"#;
        let errs = validate(&doc(src), &roster(), &[]).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("@artifact@` is ambiguous")));
    }

    #[test]
    fn step_token_must_be_ancestor() {
        let src = r#"---
[flow]
id = "nonanc"
[[step]]
id = "a"
agent = "writer"
task = "t"
[[step]]
id = "b"
agent = "writer"
task = "t"
context = ["@step:a@"]
---
"#;
        // b does not `need` a ⇒ a is not an ancestor.
        let errs = validate(&doc(src), &roster(), &[]).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("not an ancestor")));
    }

    #[test]
    fn unknown_tool_grant_is_caught() {
        let mut r = Roster::new();
        r.insert(
            AgentDef::parse("---\nid = \"w\"\ntools = [\"rm_rf\"]\n---\np").unwrap(),
        );
        let src = "---\n[flow]\nid=\"x\"\n[[step]]\nid=\"s\"\nagent=\"w\"\ntask=\"t\"\n---\n";
        let errs = validate(&doc(src), &r, &["execute_command"]).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("unknown tool `rm_rf`")));
    }

    const DIAMOND: &str = r#"---
[flow]
id = "diamond"
description = "x"
[[step]]
id = "a"
task = "t"
agent = "web-researcher"
[[step]]
id = "b"
needs = ["a"]
task = "t"
agent = "web-researcher"
[[step]]
id = "c"
needs = ["a"]
task = "t"
agent = "web-researcher"
[[step]]
id = "d"
needs = ["b", "c"]
task = "t"
agent = "web-researcher"
---
body"#;

    #[test]
    fn stages_groups_parallel_steps() {
        let plan = validate(&doc(DIAMOND), &roster(), &["execute_command"]).expect("valid");
        assert_eq!(
            plan.stages(),
            vec![
                vec!["a".to_string()],
                vec!["b".to_string(), "c".to_string()],
                vec!["d".to_string()],
            ]
        );
    }

    #[test]
    fn stages_of_linear_chain() {
        // GOOD is the existing linear fixture (ingest → digest → answer).
        let plan = validate(&doc(GOOD), &roster(), &["execute_command"]).expect("valid");
        assert_eq!(
            plan.stages(),
            vec![
                vec!["ingest".to_string()],
                vec!["digest".to_string()],
                vec!["answer".to_string()],
            ]
        );
    }

    const REFLECT_NO_NEEDS: &str = r#"---
[flow]
id = "rn"
description = "x"
[[step]]
id = "make"
task = "t"
agent = "web-researcher"
[[step]]
id = "check"
kind = "reflect"
target = "make"
critic = "skeptic"
---
body"#;

    #[test]
    fn reflect_target_must_be_in_needs() {
        let errs = validate(&doc(REFLECT_NO_NEEDS), &roster(), &["execute_command"]).unwrap_err();
        assert!(
            errs.iter().any(|e| e.step.as_deref() == Some("check")
                && e.message.contains("needs")),
            "expected a reflect-target-in-needs error, got {errs:?}"
        );
    }

    #[test]
    fn route_and_reflect_structure_checked() {
        let src = r#"---
[flow]
id = "branchy"
[[step]]
id = "answer"
agent = "writer"
task = "t"
[[step]]
id = "stress"
kind = "reflect"
target = "ghost"
critic = "skeptic"
[[step]]
id = "disp"
kind = "route"
needs = ["answer"]
choices = { go = "nowhere" }
---
"#;
        let errs = validate(&doc(src), &roster(), &[]).unwrap_err();
        let msgs: Vec<_> = errs.iter().map(|e| e.message.clone()).collect();
        assert!(msgs.iter().any(|m| m.contains("reflect target `ghost` is not a step")));
        assert!(msgs.iter().any(|m| m.contains("route step has no `prompt`")));
        assert!(msgs.iter().any(|m| m.contains("unknown step `nowhere`")));
    }
}

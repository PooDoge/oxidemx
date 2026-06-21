//! Regression guard: each shipped flow's parallel-stage structure is locked.
//! Reads the real flow.md files under oxidemx-conductor/flows/ so any future
//! edit to a flow that breaks the intended stage shape is caught immediately.
//!
//! Seam resolution: Roster is built inline using the public API
//! (Roster::new + Roster::insert + AgentDef::parse), mirroring the private
//! `roster()` helper in src/plan.rs.  No loader is needed; the test only
//! needs agent ids to be known — tool grants and capabilities are intentionally
//! left minimal (empty tools list) because the flows under test don't grant
//! tools from their step definitions.

use oxidemx_conductor::{
    flowdoc::FlowDoc,
    plan::validate,
    roster::{AgentDef, Roster},
};
use std::path::PathBuf;

fn flows_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("flows")
}

/// In-memory roster covering every agent the shipped flows reference:
/// web-researcher / summarizer / extractor / writer / skeptic (research-digest)
/// sysadmin / skeptic / writer                                 (system-doctor)
/// parser / summarizer / extractor / writer                    (doc-digest)
fn full_roster() -> Roster {
    let mut r = Roster::new();
    for id in &[
        "web-researcher",
        "summarizer",
        "extractor",
        "writer",
        "skeptic",
        "sysadmin",
        "parser",
    ] {
        r.insert(
            AgentDef::parse(&format!("---\nid = \"{id}\"\ntools = []\n---\npersona"))
                .unwrap_or_else(|e| panic!("AgentDef::parse({id}): {e}")),
        );
    }
    r
}

fn stages_of(flow_id: &str) -> Vec<Vec<String>> {
    let path = flows_dir().join(flow_id).join("flow.md");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc = FlowDoc::parse(&src)
        .unwrap_or_else(|e| panic!("{flow_id} flow.md parse error: {e}"));
    let plan = validate(&doc, &full_roster(), &["execute_command"])
        .unwrap_or_else(|e| panic!("{flow_id} invalid: {e:?}"));
    plan.stages()
}

#[test]
fn research_digest_stages() {
    // Expected: ingest first, then digest+claims in parallel, then
    // stress-test (reflect on digest), then answer (joins stress-test+claims).
    assert_eq!(
        stages_of("research-digest"),
        vec![
            vec!["ingest".to_string()],
            vec!["digest".to_string(), "claims".to_string()],
            vec!["stress-test".to_string()],
            vec!["answer".to_string()],
        ]
    );
}

#[test]
fn doc_digest_stages() {
    // Expected: parse first, then digest+claims in parallel, then answer.
    // (doc-digest has no reflect step — this should pass before any fixes.)
    assert_eq!(
        stages_of("doc-digest"),
        vec![
            vec!["parse".to_string()],
            vec!["digest".to_string(), "claims".to_string()],
            vec!["answer".to_string()],
        ]
    );
}

#[test]
fn system_doctor_stages() {
    // Expected: diagnose first, then review (reflect on diagnose), then report.
    assert_eq!(
        stages_of("system-doctor"),
        vec![
            vec!["diagnose".to_string()],
            vec!["review".to_string()],
            vec!["report".to_string()],
        ]
    );
}

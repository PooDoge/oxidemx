//! Filesystem loading of flows + the agent roster (Tier A, spec §9.1).
//!
//! Layout (kowalski convention, §8):
//!   <flows_root>/<id>/flow.md          the manifest
//!   <flows_root>/<id>/agents/*.md      optional flow-local agents
//!   <agents_root>/*.md                 the global roster
//!
//! Flow-local agents override the global roster (a flow can pin its
//! own variant of an agent). Defaults: `~/.config/oxidemx/flows` and
//! `~/.config/oxidemx/agents`, overridable by the CLI.

use std::path::{Path, PathBuf};

use crate::flowdoc::FlowDoc;
use crate::roster::{AgentDef, Roster};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("flow `{0}` not found at {1}")]
    FlowNotFound(String, PathBuf),
    #[error("reading {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("parsing {0}: {1}")]
    Parse(PathBuf, String),
}

/// Default flows root (`$OXIDEMX_FLOWS_DIR` or `~/.config/oxidemx/flows`).
pub fn default_flows_root() -> PathBuf {
    std::env::var_os("OXIDEMX_FLOWS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir().join("flows"))
}

/// Default global roster dir (`~/.config/oxidemx/agents`).
pub fn default_agents_root() -> PathBuf {
    std::env::var_os("OXIDEMX_AGENTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir().join("agents"))
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oxidemx")
}

/// Load the global roster from `<agents_root>/*.md` (missing dir ⇒
/// empty roster, not an error).
pub fn load_roster(agents_root: &Path) -> Result<Roster, LoadError> {
    let mut roster = Roster::new();
    load_agents_into(agents_root, &mut roster)?;
    Ok(roster)
}

fn load_agents_into(dir: &Path, roster: &mut Roster) -> Result<(), LoadError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // absent dir is fine
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
        .collect();
    paths.sort();
    for path in paths {
        let src = std::fs::read_to_string(&path).map_err(|e| LoadError::Io(path.clone(), e))?;
        let def = AgentDef::parse(&src).map_err(|e| LoadError::Parse(path.clone(), e.to_string()))?;
        roster.insert(def);
    }
    Ok(())
}

/// Load a flow by id plus the roster it resolves against (global
/// agents, then flow-local `agents/` overriding).
pub fn load_flow(
    flows_root: &Path,
    agents_root: &Path,
    id: &str,
) -> Result<(FlowDoc, Roster), LoadError> {
    let flow_md = flows_root.join(id).join("flow.md");
    if !flow_md.exists() {
        return Err(LoadError::FlowNotFound(id.to_string(), flow_md));
    }
    let src = std::fs::read_to_string(&flow_md).map_err(|e| LoadError::Io(flow_md.clone(), e))?;
    let doc = FlowDoc::parse(&src).map_err(|e| LoadError::Parse(flow_md.clone(), e.to_string()))?;

    let mut roster = load_roster(agents_root)?;
    load_agents_into(&flows_root.join(id).join("agents"), &mut roster)?;
    Ok((doc, roster))
}

/// List available flow ids under `flows_root` (dirs containing a
/// `flow.md`).
pub fn list_flows(flows_root: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(entries) = std::fs::read_dir(flows_root) {
        for e in entries.flatten() {
            if e.path().join("flow.md").exists() {
                if let Some(name) = e.file_name().to_str() {
                    ids.push(name.to_string());
                }
            }
        }
    }
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_flow_with_local_and_global_agents() {
        let root = std::env::temp_dir().join("conductor-loader-test");
        let _ = std::fs::remove_dir_all(&root);
        let flows = root.join("flows");
        let agents = root.join("agents");
        std::fs::create_dir_all(flows.join("demo").join("agents")).unwrap();
        std::fs::create_dir_all(&agents).unwrap();

        std::fs::write(
            agents.join("writer.md"),
            "---\nid = \"writer\"\n---\nglobal writer",
        )
        .unwrap();
        std::fs::write(
            flows.join("demo").join("agents").join("writer.md"),
            "---\nid = \"writer\"\n---\nlocal writer override",
        )
        .unwrap();
        std::fs::write(
            flows.join("demo").join("flow.md"),
            "---\n[flow]\nid = \"demo\"\n[[step]]\nid = \"s\"\nagent = \"writer\"\ntask = \"t\"\n---\n",
        )
        .unwrap();

        let (doc, roster) = load_flow(&flows, &agents, "demo").unwrap();
        assert_eq!(doc.manifest.flow.id, "demo");
        // Flow-local override won.
        assert_eq!(roster.get("writer").unwrap().persona, "local writer override");
        assert_eq!(list_flows(&flows), vec!["demo".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_flow_is_an_error() {
        let err = load_flow(Path::new("/nonexistent"), Path::new("/nonexistent"), "ghost").unwrap_err();
        assert!(matches!(err, LoadError::FlowNotFound(_, _)));
    }
}

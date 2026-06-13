//! The tool registry — every tool the agent runtime can hand a model.
//!
//! Our own `execute_command` (allowlist-gated) plus the **AutoAgents
//! Toolkit** (git-vendored — it isn't on crates.io): filesystem
//! (read/write/list/search/copy/move/delete/create, each with the
//! toolkit's own path-traversal guard), document parsing
//! (pdf/docx/xlsx/html/csv/md/xml/pptx), and web search (Brave). MCP
//! server tools load separately via [`mcp`].
//!
//! Tools are granted per-agent: a roster agent's `tools = [...]` list
//! is the gate (a flow author decides what each agent may touch). The
//! conductor validates grants against [`BUILTIN_TOOLS`]; for
//! interactive (non-flow) use, callers should additionally route
//! [`is_mutating`] tools through an approval prompt.

use autoagents::core::tool::ToolT;
use autoagents_toolkit::tools::document_parsing::DocumentParser;
use autoagents_toolkit::tools::filesystem::{
    CopyFile, CreateDir, DeleteFile, ListDir, MoveFile, ReadFile, SearchFile, WriteFile,
};
use autoagents_toolkit::tools::search::BraveSearch;

use crate::tools::ExecuteCommand;

/// Every built-in tool name the registry can build. `brave_search`
/// only materializes when a Brave API key is present (env
/// `BRAVE_SEARCH_API_KEY` / `BRAVE_API_KEY`).
pub const BUILTIN_TOOLS: &[&str] = &[
    "execute_command",
    "read_file",
    "write_file",
    "list_dir",
    "search_file",
    "create_dir",
    "move_file",
    "copy_file",
    "delete_file",
    "parse_document",
    "brave_search",
];

/// Tools that mutate the filesystem or run code. Interactive callers
/// (the overlay chat) should gate these behind a per-call approval;
/// trusted flows gate them via roster grants instead.
pub fn is_mutating(name: &str) -> bool {
    matches!(
        name,
        "execute_command" | "write_file" | "delete_file" | "move_file" | "copy_file" | "create_dir"
    )
}

/// Build one boxed tool by name. `None` for an unknown name, or a tool
/// whose prerequisite is missing (Brave key) — so an agent simply
/// doesn't get that tool rather than the run failing.
pub fn build_tool(name: &str) -> Option<Box<dyn ToolT>> {
    Some(match name {
        "execute_command" => Box::new(ExecuteCommand {}),
        "read_file" => Box::new(ReadFile::new()),
        "write_file" => Box::new(WriteFile::new()),
        "list_dir" => Box::new(ListDir::new()),
        // SearchFile bounds its own recursion; 10k entries is plenty.
        "search_file" => Box::new(SearchFile::new(10_000)),
        "create_dir" => Box::new(CreateDir::new()),
        "move_file" => Box::new(MoveFile::new()),
        "copy_file" => Box::new(CopyFile::new()),
        "delete_file" => Box::new(DeleteFile::new()),
        "parse_document" => Box::new(DocumentParser::new()),
        // BraveSearch::new() force-unwraps the key Lazy — only build it
        // when a key is actually configured.
        "brave_search" if brave_key_present() => Box::new(BraveSearch::new()),
        _ => return None,
    })
}

fn brave_key_present() -> bool {
    std::env::var("BRAVE_SEARCH_API_KEY")
        .or_else(|_| std::env::var("BRAVE_API_KEY"))
        .is_ok()
}

/// Build the granted tool set, silently dropping unknown/unavailable
/// names.
pub fn build_tools(names: &[String]) -> Vec<Box<dyn ToolT>> {
    names.iter().filter_map(|n| build_tool(n)).collect()
}

/// Execute a registry tool by name directly (for callers OUTSIDE the
/// AutoAgents executor — e.g. the overlay's `execute_local_tool`
/// dispatcher). Returns the tool's JSON result or an error string.
pub async fn execute_tool(
    name: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use autoagents::core::tool::ToolRuntime;
    let r = match name {
        "execute_command" => ExecuteCommand {}.execute(args).await,
        "read_file" => ReadFile::new().execute(args).await,
        "write_file" => WriteFile::new().execute(args).await,
        "list_dir" => ListDir::new().execute(args).await,
        "search_file" => SearchFile::new(10_000).execute(args).await,
        "create_dir" => CreateDir::new().execute(args).await,
        "move_file" => MoveFile::new().execute(args).await,
        "copy_file" => CopyFile::new().execute(args).await,
        "delete_file" => DeleteFile::new().execute(args).await,
        "parse_document" => DocumentParser::new().execute(args).await,
        "brave_search" if brave_key_present() => BraveSearch::new().execute(args).await,
        _ => return Err(format!("unknown or unavailable tool: {name}")),
    };
    r.map_err(|e| e.to_string())
}

/// MCP (Model Context Protocol) server tools, loaded from a config
/// file and exposed as native `ToolT`s by the toolkit's rmcp client.
pub mod mcp {
    use std::path::{Path, PathBuf};

    use autoagents::core::tool::ToolT;
    use autoagents_toolkit::mcp::McpTools;

    /// Default config path: `~/.config/oxidemx/mcp.toml`. The format is
    /// the toolkit's: `[[mcp.servers]]` tables with
    /// `name / protocol / command / args / env / cwd / timeout`.
    pub fn default_config_path() -> Option<PathBuf> {
        std::env::var_os("OXIDEMX_MCP_CONFIG")
            .map(PathBuf::from)
            .or_else(|| {
                let base = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
                Some(base.join("oxidemx").join("mcp.toml"))
            })
    }

    /// Connect to every server in `path` and return their tools (the
    /// toolkit namespaces them `server::tool`). An absent file yields
    /// an empty list (MCP is opt-in); a malformed file or a failed
    /// connection is an error the caller surfaces.
    pub async fn load_tools(path: &Path) -> Result<Vec<Box<dyn ToolT>>, String> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let tools = McpTools::from_config(path)
            .await
            .map_err(|e| format!("MCP config {}: {e}", path.display()))?;
        Ok(tools.to_boxed_tools().await)
    }

    /// Convenience: load from the default config path (empty if none).
    pub async fn load_default() -> Result<Vec<Box<dyn ToolT>>, String> {
        match default_config_path() {
            Some(p) => load_tools(&p).await,
            None => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_every_keyless_builtin() {
        for name in BUILTIN_TOOLS {
            if *name == "brave_search" {
                continue; // key-gated
            }
            assert!(build_tool(name).is_some(), "{name} failed to build");
            // The built tool reports the same name the model will call.
            assert_eq!(build_tool(name).unwrap().name(), *name);
        }
    }

    #[test]
    fn unknown_tool_is_none() {
        assert!(build_tool("nuke_the_site_from_orbit").is_none());
    }

    #[test]
    fn mutating_classification() {
        assert!(is_mutating("write_file"));
        assert!(is_mutating("execute_command"));
        assert!(!is_mutating("read_file"));
        assert!(!is_mutating("brave_search"));
    }

    #[test]
    fn build_tools_drops_unknowns() {
        let got = build_tools(&["read_file".into(), "bogus".into(), "list_dir".into()]);
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn mcp_default_path_is_under_config() {
        // Doesn't require the file to exist.
        if let Some(p) = mcp::default_config_path() {
            assert!(p.ends_with("oxidemx/mcp.toml"));
        }
    }
}

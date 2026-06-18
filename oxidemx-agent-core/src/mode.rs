//! `AgentMode` enum + system-instruction assembly.
//!
//! Everything the model needs to start a conversation:
//!  - Which mode we're in (`AgentMode`).
//!  - The full system instruction (persona + memory injection) via
//!    `system_instruction_async`.
//!  - The tool declarations sent to the provider via `tools`.
//!
//! Helper fns (`scan_flows`, `available_flows_block`,
//! `available_skills_block`, `agent_tool_declarations`) live here too
//! because they are all pure config-file readers (std::fs only) with
//! no UI dependency.

use serde_json::json;

// =============================================================================
// FLOW HELPERS
// =============================================================================

/// Scan `~/.config/oxidemx/flows/<id>/flow.md`, returning
/// `(id, name, description)` for each flow, sorted by id.
/// Lightweight frontmatter parse — no conductor dependency.
pub fn scan_flows() -> Vec<(String, String, String)> {
    let Ok(home) = std::env::var("HOME") else {
        return Vec::new();
    };
    let dir = std::path::Path::new(&home).join(".config/oxidemx/flows");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, String, String)> = Vec::new();
    for e in rd.flatten() {
        let md = e.path().join("flow.md");
        let Ok(src) = std::fs::read_to_string(&md) else {
            continue;
        };
        // Only scan the frontmatter (between the first two `---` fences).
        let front = src.split("---").nth(1).unwrap_or(&src);
        let field = |key: &str| -> Option<String> {
            front.lines().find_map(|l| {
                let l = l.trim();
                l.strip_prefix(key)
                    .and_then(|r| r.trim().strip_prefix('='))
                    .map(|v| v.trim().trim_matches('"').to_string())
                    .filter(|v| !v.is_empty())
            })
        };
        let id = field("id").unwrap_or_else(|| e.file_name().to_string_lossy().to_string());
        let name = field("name").unwrap_or_else(|| id.clone());
        let desc = field("description").unwrap_or_default();
        entries.push((id, name, desc));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// `(id, name)` pairs for the slash palette. Delegates to `scan_flows`.
pub fn list_flows() -> Vec<(String, String)> {
    scan_flows()
        .into_iter()
        .map(|(id, name, _)| (id, name))
        .collect()
}

fn available_flows_block() -> Option<String> {
    let entries = scan_flows();
    if entries.is_empty() {
        return None;
    }
    let mut block = String::from(
        "AVAILABLE FLOWS (the user's pre-authored pipelines — run by id with run_flow; \
         offer these when relevant, and list them if asked what you can do):\n",
    );
    for (id, name, desc) in entries {
        block.push_str(&format!("- `{id}` — {name}: {desc}\n"));
    }
    Some(block)
}

// =============================================================================
// SKILL HELPERS
// =============================================================================

/// A block listing the user's ENABLED skills (`name — description`) so
/// the agent knows its candidate pool. The full instructions for any one
/// skill are loaded on demand via the `use_skill` tool — progressive
/// disclosure, so a large skill library doesn't bloat every prompt.
/// `None` when no skills are enabled.
fn available_skills_block() -> Option<String> {
    let enabled = crate::skills::enabled_set();
    if enabled.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for s in crate::skills::discover() {
        if enabled.contains(&s.name) {
            lines.push(format!("- `{}` — {}", s.name, s.description));
        }
    }
    if lines.is_empty() {
        return None;
    }
    let mut block = String::from(
        "AVAILABLE SKILLS (enabled by the user). When one is clearly relevant to the \
         request, call use_skill with its exact name to load its full instructions, then \
         follow them. Apply skills silently — don't announce them unless asked:\n",
    );
    block.push_str(&lines.join("\n"));
    Some(block)
}

// =============================================================================
// TOOL DECLARATIONS
// =============================================================================

/// Declarations for the local agent tools sent to the model.
/// Pure JSON data — no overlay runtime dependency.
pub fn agent_tool_declarations() -> Vec<serde_json::Value> {
    vec![
        json!({
            "type": "function",
            "name": "compose_flow",
            "description": "Author a new multi-agent flow from the user's description and save it (then it's runnable via run_flow / the Agents tab / Mission Control). You write the COMPLETE flow.md content. Format: TOML frontmatter between `---` fences, then a markdown body.\n[flow] id, name, description, version=1\n[inputs] <name> = { type=\"string\", required=true|false, default=\"...\" }  (use in tasks as {{input.<name>}})\n[defaults] model=\"gemini-2.5-flash\", approval=\"allowlist\", max_turns=8\n[[step]] id, agent (a roster id — one of: web-researcher, summarizer, extractor, writer, skeptic), needs=[ids], task=\"...\", context=[\"@artifact@\" (sole predecessor) or \"@step:<id>@\" (a named ancestor)], output=\"debug/x.md\". Optional review step: kind=\"reflect\", target=\"<step>\", critic=\"skeptic\", max_rounds=2. Optional branch: kind=\"route\", needs=[...], choices={ label=\"<step-id>\" }, prompt=\"...\".\n[delivery] root=\"ANSWER.md\", title=\"...\"\nThe tool validates after writing and returns any errors — if invalid, call it again with the corrected flow.md.",
            "parameters": {
                "type": "object",
                "properties": {
                    "flow_id": {
                        "type": "string",
                        "description": "A short id for the flow (becomes a dir slug, e.g. 'release-watch')"
                    },
                    "flow_md": {
                        "type": "string",
                        "description": "The complete flow.md content (TOML frontmatter + markdown body)"
                    }
                },
                "required": ["flow_id", "flow_md"]
            }
        }),
        json!({
            "type": "function",
            "name": "read_file",
            "description": "Read a file from the local filesystem and return its contents. Use absolute paths (e.g. /home/jim/.local/share/oxidemx/runs/<run>/ANSWER.md).",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Absolute path of the file to read" }
                },
                "required": ["file_path"]
            }
        }),
        json!({
            "type": "function",
            "name": "list_dir",
            "description": "List the entries in a directory on the local filesystem.",
            "parameters": {
                "type": "object",
                "properties": {
                    "directory_path": { "type": "string", "description": "Absolute path of the directory to list" }
                },
                "required": ["directory_path"]
            }
        }),
        json!({
            "type": "function",
            "name": "search_file",
            "description": "Find files by name pattern (wildcards * and ?) under a directory, recursively.",
            "parameters": {
                "type": "object",
                "properties": {
                    "directory": { "type": "string", "description": "Absolute directory to search in" },
                    "pattern": { "type": "string", "description": "Filename pattern, e.g. *.md" }
                },
                "required": ["directory", "pattern"]
            }
        }),
        json!({
            "type": "function",
            "name": "parse_document",
            "description": "Extract clean text from a local document file (PDF, DOCX, XLSX, PPTX, HTML, CSV, Markdown, XML). Use for binary/rich formats that read_file can't show as text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Absolute path of the document file" }
                },
                "required": ["source"]
            }
        }),
        json!({
            "type": "function",
            "name": "use_skill",
            "description": "Load the full instructions for one of the AVAILABLE SKILLS (listed in your system context) by its exact name, then follow them. Call this when an enabled skill is clearly relevant to the user's request. Returns the skill's instruction body.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The exact skill name from AVAILABLE SKILLS" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "type": "function",
            "name": "run_flow",
            "description": "Run a multi-agent OxideMX flow (a named, pre-authored pipeline of agent steps) via the conductor. Use when the user asks to run a flow by name, or for a multi-step task a flow exists for (e.g. 'research-digest' to fetch+digest+answer a URL). List available flows is out of scope — the user knows the flow id. Streams live per-step progress and returns a summary; a card with a 'Watch' button to Mission Control appears in chat.",
            "parameters": {
                "type": "object",
                "properties": {
                    "flow_id": {
                        "type": "string",
                        "description": "The flow id to run (a directory under ~/.config/oxidemx/flows/)"
                    },
                    "inputs_json": {
                        "type": "string",
                        "description": "Optional JSON object of input values keyed by name, e.g. {\"url\": \"https://…\"}. Declared flow defaults apply for anything omitted."
                    },
                    "mock": {
                        "type": "boolean",
                        "description": "Run with the deterministic mock provider (no LLM calls) for a dry run. Default false (real run)."
                    }
                },
                "required": ["flow_id"]
            }
        }),
        json!({
            "type": "function",
            "name": "execute_command",
            "description": "Run a shell command on the user's machine via `sh -c`, with a 10 second timeout. Commands matching the user's allowlist run immediately; anything else asks the user for confirmation first. Returns the command's output and exit code.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command line to execute"
                    }
                },
                "required": ["command"]
            }
        }),
        json!({
            "type": "function",
            "name": "schedule_task",
            "description": "Manage recurring tasks backed by systemd user timers. `create` needs name, on_calendar and command; `enable`/`disable`/`delete`/`run_now` need unit; `list` returns all OxideMX tasks as JSON.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "enable", "disable", "delete", "run_now", "list"],
                        "description": "What to do"
                    },
                    "name": {
                        "type": "string",
                        "description": "Human-readable task name (create only)"
                    },
                    "on_calendar": {
                        "type": "string",
                        "description": "systemd OnCalendar expression, e.g. 'daily' or '*-*-* 03:00:00' (create only)"
                    },
                    "command": {
                        "type": "string",
                        "description": "Shell command the task runs (create only)"
                    },
                    "unit": {
                        "type": "string",
                        "description": "Task unit base name from list/create output, e.g. 'oxidemx-task-nightly-backup'"
                    }
                },
                "required": ["action"]
            }
        }),
        json!({
            "type": "function",
            "name": "memory",
            "description": "Persist and recall small facts about the user across conversations. `save` needs text (and optionally scope); `delete`/`pin`/`unpin` need id; `list` returns all entries as JSON; `search` needs query and returns the 5 most relevant entries; `consolidate` merges duplicates and distils stale entries (use when the user asks to tidy/optimise memories). Unpinned memories expire after 90 days of disuse; pinned ones are kept until deleted.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["save", "list", "delete", "pin", "unpin", "search", "consolidate"],
                        "description": "What to do"
                    },
                    "text": {
                        "type": "string",
                        "description": "The fact to remember (save only)"
                    },
                    "scope": {
                        "type": "string",
                        "description": "Grouping label like 'preferences' or 'projects' (save only, defaults to 'general')"
                    },
                    "id": {
                        "type": "string",
                        "description": "Memory id from list/save output (delete/pin/unpin)"
                    }
                },
                "required": ["action"]
            }
        }),
        json!({
            "type": "function",
            "name": "persona",
            "description": "Read or rewrite the user-editable persona files: soul.md (your identity, tone, values — first person) and user.md (durable facts about the user). `read` returns both files. `write_soul`/`write_user` REPLACE the whole file with `content` — include everything that should remain. Use sparingly: on the first-run ritual, or when the user asks to change your personality or correct their profile.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "write_soul", "write_user"]
                    },
                    "content": {
                        "type": "string",
                        "description": "Full replacement markdown for write_* actions"
                    }
                },
                "required": ["action"]
            }
        }),
    ]
}

// =============================================================================
// AGENT MODE
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    /// One unified agentic assistant: conversation + web search,
    /// memory/persona, shell, task scheduling, radial-menu config, and
    /// multi-agent flows. Replaces the old General / Menu Setup split
    /// (old persisted thread modes deserialize here via the aliases).
    #[serde(alias = "general_chat", alias = "settings_customizer")]
    Agentic,
}

impl AgentMode {
    /// Short label for the chat shell.
    pub fn label(&self) -> &'static str {
        "Agentic"
    }

    /// Number of tools armed for this mode — surfaced in the chat
    /// header's status line ("· N tools armed").
    pub fn tool_count(&self) -> usize {
        self.tools().len()
    }

    /// Persona half of the system instruction (base prompt + soul.md
    /// + first-run ritual + user.md), WITHOUT the memory block.
    fn system_instruction_base(&self) -> String {
        let base = "You are OxideMX-AI, the user's agentic desktop assistant. You hold a \
             natural conversation AND act on the machine through your tools — answer questions, \
             query the web for real-time facts, run shell commands, schedule recurring tasks, \
             remember durable facts, configure the OxideMX radial menu, and launch multi-agent \
             flows. Reach for a tool whenever it gets a better, grounded result; otherwise just \
             reply. Keep responses concise and format them in markdown.\n\n\
             RADIAL MENU CONFIG\n\
             You can read/modify the active layout via get_menu_config / set_menu_config. When \
             editing, slice `icon` fields MUST be icon names that actually exist: a standard \
             Adwaita/freedesktop symbolic name (e.g. utilities-terminal-symbolic, \
             system-run-symbolic, web-browser-symbolic, preferences-system-symbolic) or an Icon= \
             value from list_system_apps. NEVER invent icon names (a nonexistent name renders \
             blank); prefer list_system_apps for real exec + icon when binding launchers. Use \
             ask_multiple_choice_question when options need clarifying.\n\n\
             FILES\n\
             You can read the filesystem: read_file (text), list_dir, search_file (by pattern), and \
             parse_document (PDF/DOCX/XLSX/HTML — rich formats). Flow runs write their artifacts to \
             ~/.local/share/oxidemx/runs/<flow>-<timestamp>/ (the final answer is usually ANSWER.md, \
             intermediates under debug/) — read them there when the user asks about a flow's output.\n\n\
             FLOWS\n\
             For multi-step work a pre-authored flow covers, call run_flow with its id. When the \
             user wants a NEW repeatable pipeline ('every morning fetch X, digest it, …'), author \
             it with compose_flow (you write the flow.md; it validates and tells you any errors to \
             fix). run_flow streams live progress and returns a summary.\n\n\
             MEMORY RULES\n\
             You have a memory tool. Save a memory (action=save) ONLY when ALL of these hold:\n\
             1. DURABLE - the fact will still be true and useful in 2+ weeks (preferences, \
             hardware/setup facts, decisions, corrections, recurring projects, names). \
             Not today's task details, transient state, or anything trivially re-derivable.\n\
             2. ACTIONABLE - knowing it would change how you respond in a future, unrelated \
             conversation.\n\
             3. NOT ALREADY KNOWN - check the saved-memories block first. If a memory exists \
             on the topic, save the corrected/updated wording instead of a duplicate (the \
             store supersedes near-duplicates automatically).\n\
             Always save when the user explicitly says remember/note/don't forget. Never save \
             secrets, credentials, or sensitive details the user did not ask you to keep. \
             Write each memory as ONE self-contained sentence in third person with concrete \
             specifics. Most conversations produce ZERO memories; more than two per \
             conversation should be rare.\n\
             The saved-memories block below is a relevance-ranked selection, not the whole \
             store - use the memory tool's search action when the user references something \
             you can't see.";
        // Persona files stay keyed "general" — the single agent inherits
        // the existing soul.md/user.md, no migration needed.
        let mode_key = "general";
        let mut full = base.to_string();
        // Make the agent AWARE of the user's actual flows (ids + what
        // they do) so it can run/recommend them by name without the
        // user knowing exact ids — and answer "what can you do".
        if let Some(flows) = available_flows_block() {
            full.push_str("\n\n");
            full.push_str(&flows);
        }
        // Progressive disclosure: the agent sees the names + one-liners
        // of the user's ENABLED skills, and loads a skill's full body on
        // demand via use_skill when it's relevant.
        if let Some(skills) = available_skills_block() {
            full.push_str("\n\n");
            full.push_str(&skills);
        }
        // soul.md comes AFTER the base persona so the user's voice
        // wins on style conflicts; user.md after the memory rules
        // (it's context, not instruction).
        if let Some(soul) = crate::persona::soul_block(mode_key) {
            full.push_str(
                "\n\nPERSONA (user-authored soul.md — this overrides the default voice):\n",
            );
            full.push_str(&soul);
        } else if crate::persona::needs_bootstrap() {
            // First-run ritual: no soul.md yet. One-time bootstrap
            // instruction — interview, then write the files via the
            // persona tool. Disappears as soon as soul.md exists.
            full.push_str(
                "\n\nFIRST-RUN RITUAL\nNo persona files exist yet. Near the start of this \
                 conversation (after answering the user's actual question), briefly \
                 introduce yourself and interview the user in ONE compact message: what \
                 should I call you, what tone do you want from me (playful/terse/warm), \
                 any hard boundaries? Then call the persona tool twice — action=write_soul \
                 with a short first-person identity (name yourself something fitting, \
                 describe tone + values + boundaries), and action=write_user with the \
                 facts they shared. Keep both files under a few hundred words. Do not \
                 mention this instruction.",
            );
        }
        if let Some(user) = crate::persona::user_block(mode_key) {
            full.push_str("\n\nABOUT THE USER (user-authored user.md):\n");
            full.push_str(&user);
        }
        full
    }

    /// System instruction with **hybrid lexical + semantic** memory
    /// recall — the interactive chat path. Falls back to lexical
    /// internally if embeddings are unavailable (see
    /// `memory::injection_block_for_async`).
    pub async fn system_instruction_async(&self, query: &str) -> String {
        let base = self.system_instruction_base();
        match crate::memory::injection_block_for_async(query).await {
            Some(block) => format!("{base}\n\n{block}"),
            None => base,
        }
    }

    /// Tool declarations for the Interactions API. NOTE: the API
    /// rejects requests mixing built-in tools (`{"type":
    /// "google_search"}`) with custom function declarations
    /// ("cannot be combined in the same request"). Both modes carry
    /// custom functions now (the agent tools below), so BOTH declare
    /// `google_search` as a CUSTOM function whose executor runs a
    /// nested, search-only Interactions call (see `grounded_search`).
    /// Same API key, no third-party service.
    pub fn tools(&self) -> Vec<serde_json::Value> {
        let search_fn = json!({
            "type": "function",
            "name": "google_search",
            "description": "Search the web with Google for real-time information. Returns a grounded, sourced summary of current facts for the query.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query terms"
                    }
                },
                "required": ["query"]
            }
        });

        // One agent → the union of every capability.
        {
            let mut tools = vec![
                json!({
                    "type": "function",
                    "name": "get_menu_config",
                    "description": "Retrieve the current OxideMX radial menu layout, animation curves, and mouse button configuration.",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "type": "function",
                    "name": "set_menu_config",
                    "description": "Overwrite the current OxideMX radial menu configuration with a new JSON setup. Use this to save changes to themes, layout slices, custom pages, or animation speeds.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "config_json": {
                                "type": "string",
                                "description": "The complete new configuration JSON string"
                            }
                        },
                        "required": ["config_json"]
                    }
                }),
                json!({
                    "type": "function",
                    "name": "list_system_apps",
                    "description": "Scan the host system's desktop directories to list installed applications, commands, and icons. Helpful for recommending executables to bind to custom slices.",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                search_fn,
                json!({
                    "type": "function",
                    "name": "ask_multiple_choice_question",
                    "description": "Ask the user a clarifying multiple-choice question. Used when there are multiple valid options or parameters to clarify.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "The question text to present"
                            },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "string"
                                },
                                "description": "The list of choices/options the user can click"
                            }
                        },
                        "required": ["question", "options"]
                    }
                }),
            ];
            // + run_flow/execute_command/schedule_task/memory/persona
            // (search_fn is already in the vec above).
            tools.extend(agent_tool_declarations());
            tools
        }
    }
}

# Task 2 Report — AgentToolExecutor + native fs/shell/web tools

## ToolExecutor trait signature

From `oxidemx-agent-core/src/tool.rs`:

```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, args: Value, sink: &Option<StreamSink>) -> Result<String, String>;
}
```

`AgentToolExecutor` in `agentd/src/tools/mod.rs` implements this trait.
The `sink` parameter is accepted but not used (no streaming progress in these
sync/simple tools); the field is ready for Task 3 wiring.

## Oracle helpers: reused vs. ported inline

| Tool | Decision | Notes |
|---|---|---|
| `read_file`, `list_dir`, `search_file`, `parse_document` | Ported inline | No oracle counterpart in agentd; pattern derived from brief spec |
| `execute_command` | Ported inline from brief | `sh -c` + cwd + 10 s timeout + 2048-char cap |
| `list_system_apps` | Ported inline from oracle description | `.desktop` file scanner over 3 standard dirs |
| `google_search` | Ported inline | `reqwest` POST to Gemini generateContent with `google_search` tool; soft-fails on missing key |

The autoagents-toolkit document-parsing helpers are not available in agentd's
dependency graph; `parse_document` therefore reads the file as raw UTF-8 text,
which is sufficient for feeding model context.

## Escape-guarding

`resolve_and_guard(base, user_path)` in `tools/fs.rs`:

1. If `user_path` is absolute, use it directly; otherwise join onto `base` (the project `cwd`).
2. `std::fs::canonicalize` both the resolved path and `base` — this resolves
   all symlinks and `..` components to their real on-disk paths.
3. `canonical.starts_with(&canon_base)` — if the resolved path is not a
   descendant of the real cwd, return `Err("path escapes project directory: …")`.

Because canonicalization is used, tricks like `a/../../etc` or symlinks pointing
outside the tree are caught reliably.

## Per-tool test results

All 8 tests pass in a single `cargo test -p agentd tools::` run:

```
test tools::tests::read_file_reads_within_cwd    ... ok
test tools::tests::read_file_rejects_escape       ... ok
test tools::tests::list_dir_lists_within_cwd      ... ok
test tools::tests::list_dir_rejects_escape        ... ok
test tools::tests::search_file_finds_match        ... ok
test tools::tests::parse_document_reads_file      ... ok
test tools::tests::execute_command_runs_in_cwd    ... ok
test tools::tests::unknown_tool_returns_err       ... ok

test result: ok. 8 passed; 0 failed; 0 ignored
```

## Deviations from the brief

| Item | Brief says | Actual |
|---|---|---|
| `parse_document` | "document parsing via autoagents toolkit … return raw text" | Raw UTF-8 read — identical intent, no deviation |
| `search_file` escape guard | Not explicitly specified (only cwd-scoping mentioned) | Applied the same `resolve_and_guard` for consistency |
| `execute_command` stderr | Not specified | Combined with stdout into `combined`; stderr appended after stdout if non-empty |
| `google_search` reqwest dep | "add if needed" | Added `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }` to agentd/Cargo.toml |
| Report path | `juhradial-mx/.git/worktrees/oxidemx-phase1/sdd/` | Written to `oxidemx-phase1/sdd/` (the working tree, not git bookkeeping) |

## mistralrs absent confirmation

```
$ cargo tree -p agentd | grep -i mistralrs
(no output)
```

mistralrs is not in the agentd dependency tree. The `mistral` feature in
agentd/Cargo.toml gates `oxidemx-agent-local/mistral`, which is a separate crate;
the tools module has no path to it.

## Commit

SHA: `41cab50`
Subject: `feat(agentd): AgentToolExecutor + native fs/shell/web tools (cwd-scoped)`

Branch: `phase1-local-llm-gateway`

Files changed:
- `agentd/Cargo.toml` — added reqwest direct dep
- `agentd/src/lib.rs` — added `pub mod tools;`
- `agentd/src/tools/mod.rs` — `AgentToolExecutor` struct + `ToolExecutor` impl + dispatch + 8 tests
- `agentd/src/tools/fs.rs` — 7 tool body implementations

---

## T2 Review Fixes — commit `e79a0c1`

### Final arg keys per tool

| Tool | Declared key(s) | Fallback keys accepted |
|---|---|---|
| `read_file` | `file_path` | `path`, `source` |
| `list_dir` | `directory_path` | `path`, `directory` |
| `search_file` | `directory`, `pattern` | `directory_path`, `path` (for directory); none for pattern |
| `parse_document` | `source` | `file_path`, `path` |
| `execute_command` | `command` | — |
| `google_search` | `query` | — |

### Glob approach

No `glob` crate added (not in workspace Cargo.toml).  Implemented `glob_match` / `glob_match_inner` as a small inline recursive function supporting `*` (zero or more chars) and `?` (any single char).  Only the filename component is matched, not path separators.  Tested: `*.rs` matches `foo.rs`, rejects `foo.txt`.

### Output shapes

- `list_dir`: JSON array of `{"name":"…","path":"…","is_dir":bool,"size":u64}` objects, sorted by name.
- `search_file`: JSON array of `{"name":"…","path":"…","is_dir":false,"size":u64}` objects (relative path from cwd), sorted by path.

### Escape-guard rewrite

`resolve_and_guard` no longer calls `std::fs::canonicalize` on the *target* path.  Instead:

1. Join `base + user_path` (or use absolute path as-is).
2. Walk `Path::components()` manually, collapsing `..` (pop) and `.` (skip) — no I/O required.
3. Check `normalized.starts_with(canon_base)` — escape rejected here.
4. Only if the path *exists* and *is a symlink*, canonicalize the symlink target and re-check prefix.

Result: a missing but in-cwd path passes the guard and the subsequent `read_to_string` returns a clean OS "No such file or directory" error.

### Test command + pass count

```
cargo test -p agentd
test result: ok. 46 passed; 0 failed; 0 ignored
```

### Commit SHA

`e79a0c1`  branch `phase1-local-llm-gateway`

Files changed: `agentd/src/tools/fs.rs`, `agentd/src/tools/mod.rs`

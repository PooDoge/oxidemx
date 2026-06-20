# Claude Code System Prompt Patterns

Source: https://github.com/Piebald-AI/claude-code-system-prompts (v2.1.182, June 2026)
Research date: 2026-06-20

---

## 1. Truthfulness / No-Fabrication / Verification Discipline

**PRIORITY — these directly map to our rule "never assert run/task/file/test/command status without a tool result this turn."**

### 1.1 Action Safety and Truthful Reporting (system-prompt-action-safety-and-truthful-reporting.md)

The prompt imposes explicit truthfulness on *outcome reporting*:

> "Report outcomes accurately, including test failures, skipped steps, and successful completions."

And before destructive edits:

> "Before destructive edits, examine the target to verify it matches its description and that you created it."

Approval is scoped and non-transferable:

> "A user approving an action...once does NOT mean that they approve it in all contexts."

### 1.2 Executing Actions With Care — Fragment (system-prompt-executing-actions-with-care-fragment.md)

Verbatim:

> "Read, search, and investigate freely — looking is not acting. For actions that are hard to reverse, affect shared systems, or are otherwise risky (deleting data, force-pushing, sending messages, modifying shared infrastructure), confirm with the user before proceeding unless durably authorized. Approval in one context doesn't extend to the next."

### 1.3 Autonomous Operation Guidelines (system-prompt-autonomous-operation-guidelines.md)

Verbatim:

> "Before running a command that changes system state — restarts, deletes, config edits — check that the evidence actually supports that specific action. A signal that pattern-matches to a known failure may have a different cause."

And on scope:

> "Exception: when the user is describing a problem, asking a question, or thinking out loud rather than requesting a change, the deliverable is your assessment. Report your findings and stop. Don't apply a fix until they ask for one."

### 1.4 Act When Ready (system-prompt-act-when-ready.md)

Anti-confabulation via decisiveness rather than speculation:

> "When you have enough information to act, act. Do not re-derive facts already established in the conversation, re-litigate a decision the user has already made, or narrate options you will not pursue. If you are weighing a choice, give a recommendation, not an exhaustive survey."

### 1.5 Troubleshooting Confirmation Policy (system-prompt-troubleshooting-confirmation-policy.md)

Verbatim:

> "For each issue: briefly explain what the fix will do, then ask me to confirm before running any shell command that deletes files, modifies global config, or changes my installation. Safe read-only checks are fine without asking. If a suggested fix looks wrong for my setup, say so instead of running it."

**Pattern**: Truthfulness in Claude Code is enforced structurally — the agent must run tools to *produce* the evidence it reports. The system prompt never asks the agent to be "honest"; it instead mandates tool-based verification before any claim of completion/outcome is made.

---

## 2. Tool-Use Discipline

### 2.1 Parallel Tool Calls (system-prompt-parallel-tool-call-note-part-of-tool-usage-policy.md)

Verbatim:

> "If you intend to call multiple tools and there are no dependencies between them, make all independent tool calls in parallel."

### 2.2 Read Before Edit / Prefer Existing Files

Two separate files enforce this:

- `system-prompt-prefer-editing-existing-files.md`: "Prefer editing existing files to creating new ones."
- `tool-description-write-read-existing-file-first.md` (tool description): requires reading any existing file before overwriting it.

### 2.3 Permission Discipline (system-prompt-system-section.md)

Verbatim:

> "Tools are executed in a user-selected permission mode. When you attempt to call a tool that is not automatically allowed by the user's permission mode or permission settings, the user will be prompted so that they can approve or deny the execution. If the user denies a tool you call, do not re-attempt the exact same tool call. Instead, think about why the user has denied the tool call and adjust your approach."

### 2.4 No Over-calling / Bash Alternatives

Multiple tool descriptions enforce routing: bash alternatives exist for read, write, edit, search, grep. The agent is instructed to prefer the right-shaped tool over a shell workaround.

### 2.5 Tool Call Colon Avoidance

`system-prompt-tool-call-colon-avoidance.md` — a small but telling rule: do not write "Let me read the file:" before a tool call. The action speaks; narrating it is noise. (This maps directly to our conciseness rules.)

---

## 3. When to Ask vs. Act

### 3.1 Autonomous Session Default (system-prompt-autonomous-operation-guidelines.md)

> "For reversible actions that follow from the original request, proceed without asking. Stop only for destructive actions or genuine scope changes the user must decide. Offering follow-ups after the task is done is fine; asking permission before doing the work is not."

### 3.2 Exploratory / Ambiguous Questions

`system-prompt-exploratory-questions-analyze-before-implementing.md`:

> "respond in 2-3 sentences with a recommendation and the main tradeoff. Present it as something the user can redirect, not a decided plan. Don't implement until the user agrees."

### 3.3 Interactive/Blocking Distinction

The system distinguishes between "interactive" sessions (user present, can answer) and "autonomous" sessions (user not watching). The autonomous mode shifts the ask-vs-act threshold toward acting; clarifying questions that would block work are disallowed.

**Gate classification rule** (maps to our ApprovalClassifier):
- Reversible + in-scope → proceed
- Destructive, outward-facing (push/PR/message), or scope-expanding → confirm first
- User thinking aloud / asking a question → report findings only, do not apply fix

---

## 4. Conciseness / Output Format

### 4.1 Communication Style (system-prompt-communication-style.md)

- Before tool calls: one sentence stating next action.
- During work: one sentence per update at critical moments (discovery, direction change, obstacle).
- End-of-turn: one or two sentences — what changed, next steps.
- "Avoid narrating internal deliberation; focus on relevant user communication rather than thought processes."

### 4.2 Outcome-First Style (system-prompt-outcome-first-communication-style.md)

Lead with the direct answer. Match response complexity to question complexity. Code comments: "only constraints that code itself cannot express."

### 4.3 No Preamble/Postamble Signals

- `system-prompt-tool-call-colon-avoidance.md`: no filler before tool calls.
- `system-prompt-emoji-avoidance.md`: no emoji in responses.
- `system-prompt-tone-and-style-concise-output-short.md`: "Your responses should be short and concise."

---

## 5. Overall Prompt Structure (Section Ordering)

The 515+ files are *composable fragments* assembled per-session. The rough assembly order inferred from the naming convention and ccVersion sweep:

```
1. system-prompt-system-section           — permission mode / tool denial rule
2. system-prompt-claude-fable-5-model-identity — identity (who/what the agent is)
3. system-prompt-communication-style      — tone, brevity, update cadence
4. system-prompt-outcome-first-communication-style
5. system-prompt-doing-tasks-software-engineering-focus  — task domain framing
6. system-prompt-doing-tasks-ambitious-tasks             — scope/capability framing
7. system-prompt-doing-tasks-no-unnecessary-additions    — minimal-change discipline
8. system-prompt-doing-tasks-no-compatibility-hacks      — delete, don't shim
9. system-prompt-doing-tasks-security                    — OWASP/injection rules
10. system-prompt-comment-why-only-guidance              — code comment discipline
11. system-prompt-prefer-editing-existing-files          — file hygiene
12. system-prompt-executing-actions-with-care            — reversibility gate
13. system-prompt-action-safety-and-truthful-reporting   — outcome truthfulness
14. system-prompt-autonomous-operation-guidelines        — ask-vs-act in loop mode
15. system-prompt-act-when-ready                         — anti-dithering
16. [conditional] system-prompt-auto-mode                — auto-mode specifics
17. [conditional] system-prompt-plan-mode-*              — plan mode
18. [conditional] system-prompt-memory-instructions      — memory/CLAUDE.md
19. [volatile]    system-reminder-*                      — per-turn injections
```

**For prefix-cache design (stable → context → volatile):**
- Stable tier: identity + tone + domain framing + tool policies (1–11 above)
- Context tier: reversibility gate + truthfulness rules + memory index (12–18)
- Volatile tier: system-reminders (file modifications, token budget, hook feedback, plan state)

---

## 6. Coding-Specific Rules

### Follow conventions, never invent
`system-prompt-doing-tasks-software-engineering-focus.md`: interpret generic instructions in the context of the actual codebase; never produce abstract textual answers when code changes are meant.

### Comments: why only, default none
`system-prompt-comment-why-only-guidance.md` (verbatim):
> "Default to writing no comments. Only add one when the WHY is non-obvious: a hidden constraint, a subtle invariant, a workaround for a specific bug, behavior that would surprise a reader. If removing the comment wouldn't confuse a future reader, don't write it."

### No gold-plating
`system-prompt-doing-tasks-no-unnecessary-additions.md`:
> "Don't add features, refactor, or introduce abstractions beyond what the task requires."
> "Three similar code lines are preferable to premature abstraction."

### No compatibility shims
`system-prompt-doing-tasks-no-compatibility-hacks.md`:
> "Avoid backwards-compatibility hacks like renaming unused _vars, re-exporting types, adding // removed comments for removed code, etc. If you are certain that something is unused, you can delete it completely."

### Security always
`system-prompt-doing-tasks-security.md`:
> "Be careful not to introduce security vulnerabilities such as command injection, XSS, SQL injection, and other OWASP top 10 vulnerabilities."

### Git discipline (from tool descriptions)
`tool-description-bash-git-*` files collectively mandate: never skip hooks, prefer new commits over amend, avoid destructive ops (force-reset, branch -D, push --force) unless explicitly requested, never commit unless asked.

### Run tests / verify outcomes
The action-safety prompt requires reporting test failures, not just successes. The verify skill (`skill-verify-skill.md`) shows a pattern: run the app and *observe* rather than assume.

---

## 7. What to Adopt for OxideMX — Priority-Ranked Rules

These should fold into `oxidemx-agent-core`'s system-prompt assembly. Marked (P1) = highest priority for our coding-first agent.

### (P1) Truthfulness via structural mandate, not exhortation
**What Claude Code does**: the prompt never says "be honest." Instead it requires tool-call evidence before any status claim. The autonomous-loop rule "check that the evidence actually supports that specific action" is the anti-hallucination mechanism.

**Adopt verbatim for OxideMX**:
> "Report outcomes accurately, including test failures, skipped steps, and successful completions. Before asserting that a command succeeded, a test passed, or a file was written, verify via tool output from this turn — not from memory or prior turns."

### (P1) Approval-scope non-transferability
Verbatim adaptation:
> "Approval of an action in one context does not extend to the next. 'User approved X' is not authorization for Y, even if Y is similar."

### (P1) Reversibility gate for ask-vs-act
Adopt the three-tier gate directly:
- Read/investigate → always free
- Reversible in-scope change → proceed autonomously
- Destructive / outward-facing / scope-expanding → confirm before proceeding

The exact fragment to adapt:
> "Read, search, and investigate freely — looking is not acting. For actions that are hard to reverse, affect shared systems, or are otherwise risky, confirm with the user before proceeding unless durably authorized."

### (P2) Anti-dithering / act-when-ready rule
> "When you have enough information to act, act. Do not re-derive facts already established in the conversation, re-litigate a decided question, or narrate options you will not pursue. Give a recommendation, not an exhaustive survey."

### (P2) Pattern-match trap prevention
Verbatim:
> "Before running a command that changes system state — restarts, deletes, config edits — check that the evidence actually supports that specific action. A signal that pattern-matches to a known failure may have a different cause."

### (P2) Scope discipline (no gold-plating)
> "Don't add features, refactor, or introduce abstractions beyond what the task requires. Bug fixes stand alone. Three similar lines are preferable to premature abstraction."

### (P3) Code comment discipline
> "Default to writing no comments. Only add one when the WHY is non-obvious: a hidden constraint, a subtle invariant, a workaround for a specific bug, behavior that would surprise a reader."

### (P3) Parallel tool calls
> "If you intend to call multiple tools and there are no dependencies between them, make all independent tool calls in parallel."

### (P3) Conciseness / one sentence per update
Before each tool call: one sentence. Mid-task updates: one sentence. End-of-turn: one or two sentences (what changed, next steps). No narrating internal deliberation.

### (P3) Permission / tool-denial handling
> "If the user denies a tool call, do not re-attempt the exact same call. Think about why it was denied and adjust your approach."

---

## 8. What NOT to Copy (Claude Code–Specific)

- **Identity/model sections** (`system-prompt-claude-fable-5-model-identity.md`) — Claude-specific branding.
- **Memory/CLAUDE.md persistence machinery** — our memory system is already richer (hybrid semantic/lexical, scored recall). Don't import the simpler file-write memory model.
- **PowerShell / Windows tool variants** — irrelevant for OxideMX on Linux.
- **Auto-mode / plan-mode / loop-mode sections** — we have our own Conductor/mission-control topology; don't blindly import Claude Code's loop-tick/plan-mode machinery.
- **Managed Agents / Cowork plugin references** — Claude.ai product-specific.
- **`system-prompt-censoring-assistance-with-malicious-activities.md`** — the Anthropic safety-filter framing is built into the base model, not something we control via prompt. Our safety refusals should be simpler and scoped to `oxidemx-agent-core`'s actual tool surface.

---

## 9. Three-Tier Prompt Cache Architecture (Recommended for oxidemx-agent-core)

Based on the Claude Code fragment taxonomy:

```
┌─────────────────────────────────────────────────────────────────┐
│ STABLE  (cache this prefix — changes only on major refactor)    │
│  • Agent identity + capability statement                        │
│  • Coding-domain framing (software engineering focus)          │
│  • Conciseness / output-format rules                           │
│  • Tool-use policy (parallel calls, read-before-edit, no reattempt on deny) │
│  • Scope discipline (no gold-plating, no shims, security)      │
│  • Comment discipline (why-only, default none)                 │
│  • Reversibility gate (read freely; confirm destructive)       │
│  • Truthfulness mandate (evidence from this turn, not memory)  │
│  • Anti-dithering rule (act when ready)                        │
└─────────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────┐
│ CONTEXT (cache per-project / per-session start)                 │
│  • CLAUDE.md / project memory index                            │
│  • Active worktree / git branch state                          │
│  • Loaded skill summaries                                      │
│  • Conductor flow context (if task active)                     │
└─────────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────┐
│ VOLATILE (injected per-turn, never cached)                      │
│  • File modification notices                                   │
│  • Hook feedback                                               │
│  • Token/budget usage                                          │
│  • Plan approval state                                         │
│  • Deferred tools available reminder                           │
└─────────────────────────────────────────────────────────────────┘
```

---

*Source files examined: ~20 system-prompt-* and tool-description-* files from Piebald-AI/claude-code-system-prompts @ main (v2.1.182). Full file list available via `gh api repos/Piebald-AI/claude-code-system-prompts/contents/system-prompts`.*

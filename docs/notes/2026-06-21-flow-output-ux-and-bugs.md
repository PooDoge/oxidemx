# Open items from Jim (2026-06-21) — flow output UX + bugs + next brainstorm

## ⮕ RESUME ANCHOR (post-compaction entry point)
**State:** W1 (conductor correctness) is DONE + MERGED to `phase1-local-llm-gateway` (e76a136) +
**now installed** (agentd + oxidemx-conductor host-built + pkexec'd 2026-06-21; `use_agentd:true`
re-set; agentd restarted; one clean `oxidemx-chat` relaunched in agentd mode). Program memory:
[[project_flow_schema_v2]] (W1 ✅, Phase 2a next). Deploy rule reinforced: [[feedback_install_and_restart_after_updates]].
**Next action:** Jim will add more brainstorming points, then we brainstorm THIS batch (below) +
**Phase 2a (typed structured I/O)** — the next Flow Schema v2 slice. Use the superpowers
brainstorming skill. The items below are the agreed scope to fold in.

Captured before context compaction. These come AFTER W1 (shipped) and feed the next
brainstorming batch alongside Phase 2a (typed I/O).

## Bugs to fix
1. **Chat window X-close hangs → "OxideMX Chat Is Not Responding" → force quit.** The
   native-decoration standalone chat (`oxidemx-chat`) hangs when the WM close (X) is clicked.
   Investigate the close/quit path: single-instance D-Bus connection not releasing on close?
   A blocking `Drop`? iced window-close → daemon handler deadlock? (Related:
   [[project_standalone_chat_window]], [[project_wayland_focus_architecture]].)
2. **Bubbles don't appear for chat-triggered flow runs.** Screenshots show run-id
   `research-digest-<ts>` + `run_status` "Unknown tool" ⇒ the chat is on the IN-PROC agent
   (not agentd), so (a) the in-proc agent lacks run_status/the agentd flow tools, and (b) the
   activity dock is fed by the **agentd bus** so in-proc runs never reach it — only the footer
   status text updates ("Flow doc-digest · answer…"). The bubbles REQUIRE `use_agentd: true`
   (see [[project_agent_activity_bubbles]]). Decision needed: force agentd mode for flow
   features, OR also feed the dock from the in-proc run channel. `use_agentd` keeps reverting —
   check why (config write path? default?).
3. **research-digest fails to run in chat** (Jim suspects the flow def) — re-test after install;
   the W1 user-disk fix is applied but the in-proc launcher may differ.

## ★ HEADLINE DELIVERY BUG — flow result never auto-posts to chat (confirmed 2026-06-21)
When a flow is launched from chat, the agent's `run_flow` tool is **fire-and-background**: it
returns a `run_id` and the agent's turn ENDS ("launched run-N"). When the flow finishes,
`RunFinished{handoff_markdown, artifacts}` reaches the **activity bubble** (Transcript button shows
it on demand) but **nothing auto-posts the result into the chat thread**. So the user only sees the
final output by clicking the bubble's Transcript. **Fix:** on `RunFinished` for a chat-launched run,
auto-deliver the result (handoff/ANSWER.md, richly — see below) into the launching conversation as
an assistant message. Requires the run→thread linkage (the bubbles' deferred conversation-scoping
gap — currently `ActivityState` is window-global, runs aren't tied to a thread). This is the #1
delivery item; the terseness + artifact-card items below are how that delivered message should look.

## Flow final-delivery is too terse (investigation + design)
Every successfully-run flow ends with a terse final response, e.g. *"The doc-digest flow ran;
the points are in ANSWER.md at ~/.local/share/oxidemx/runs/doc-digest-1782034089084/ANSWER.md."*
Far less informative than Claude Code's end-of-run summary.
- **Analyze:** is this the flow's `[delivery]` section, or a broader final-delivery-step issue
  across all flows? (Likely the agent summarizing the run result tersely, not reading the
  artifact.)
- **Idea:** apply Claude Code's conversation-summarization system prompt:
  `https://raw.githubusercontent.com/Piebald-AI/claude-code-system-prompts/refs/heads/main/system-prompts/agent-prompt-conversation-summarization.md`
- **Idea:** the chat should inline the delivery artifact (ANSWER.md) contents, not just its path.

## NEW FEATURE — rich flow output in chat (a cohesive UI slice)
When a chat response lists output file path(s), render an **inline artifact card** below the
paragraph:
- Title = filename; right-side **icon buttons**: open file, open containing folder in the file
  manager, copy path to clipboard.
- Body = the file contents, **truncated** (iced built-in `text` Ellipsis —
  https://docs.iced.rs/iced/advanced/text/enum.Ellipsis.html); **click the card body to
  expand/collapse** full contents.
- **Density-aware:** many artifacts ⇒ truncate more aggressively; high density ⇒ default
  collapsed (title + buttons only).
- **Markdown** files render properly; **code** uses iced's syntax highlighter crate
  `iced_highlighter` (https://docs.iced.rs/iced_highlighter/index.html), which supports
  STREAMING — use streaming highlight for inline streaming code.
- artifact paths are detected from the assistant text (e.g. the `runs/<id>/ANSWER.md` paths).

## Mechanical (do now, pre-compaction)
- Rebuild + install latest agentd + oxidemx-conductor (host-side); the W1 conductor changes
  aren't installed. The chat/overlay binary is current (yesterday's bubbles build) EXCEPT it
  needs the X-close fix (deferred). Kill old/duplicate processes; ensure single live chat;
  ensure `use_agentd: true`.

## Then: brainstorm batch (post-compaction)
Phase 2a (typed I/O) + the above (artifact cards + delivery richness + X-close bug + bubbles
in-proc-vs-agentd decision). Jim has more brainstorming points to add.

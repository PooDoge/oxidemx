# Chat-Shell Completion — Design

**Date:** 2026-06-25
**Branch / worktree:** `2b-collapsible-panels` / `oxidemx-2b`
**Status:** approved (brainstorm), pending implementation plan

## Goal

Complete the OxideMX Freya chat-shell chrome so the three-panel "Collapsible
Panels" layout is fully interactive: a working **project switcher**, a
**collapse mechanism** whose state survives re-render with a real **icon rail**,
the **right panel** expanded into its four status **directions**, and the
**animations** that tie it together. Built from the committed prose specs +
established component/token patterns (the `.freya.json` export is not in the
repo); each piece verified via dark-theme headless snapshots before any relaunch.

## Scope boundary (what this slice is NOT)

The right panel's four directions get their **framework** (rail icons, segmented
expanded panel, transitions) plus the **"now" content drawn from data we already
have**. Their **rich 2d content** — spec-doc viewer, mission/todo tracker,
workbench diff/worktree inspector, ambient process monitor — needs new agentd
endpoints + reducers and is explicitly deferred to follow-on slices (one per
direction). Each direction body is its own file so that later content drops into
one place. No new agentd endpoints are added here.

## Global constraints

- Freya v0.4.0-rc.23 builder API (`rect().child()`, never `rsx!`); reuse Freya
  built-ins (`Select`, `MenuButton`, `use_animation`) over hand-rolling (Rule 4).
- Match surrounding hand-formatting; no repo-wide `cargo fmt`. `cargo clippy`
  clean. No gold-plating (Rule 2).
- Every headless snapshot uses `use_init_theme(dark_theme)` + a `bg_deep()` root,
  or near-white text/panels render invisible (snapshot-dark-theme rule).
- Build the overlay in the `claude_development` distrobox with
  `LIBRARY_PATH=/tmp/oxidemx-lib-links`; disk-based `oxide-app/target`.
- Name the directions enum `StatusDirection` — Freya already exports `Direction`
  (layout axis); a second `Direction` would collide.

## Architecture

### 1. State model (`oxide-freya/src/state.rs`)

`AppState` gains four signals + one method; `PartialEq` updated to include them.

```rust
pub current_project:   State<Option<ProjectId>>,
pub sidebar_collapsed: State<bool>,   // default false
pub context_collapsed: State<bool>,   // default true
pub active_direction:  State<StatusDirection>, // default Spec
```

`StatusDirection` (new, in `state.rs` or a small `directions` module):

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusDirection { Spec, Mission, Workbench, Ambient }

impl StatusDirection {
    pub const ALL: [StatusDirection; 4] =
        [Self::Spec, Self::Mission, Self::Workbench, Self::Ambient];
    pub fn label(self) -> &'static str { /* "Spec" | "Mission" | "Workbench" | "Ambient" */ }
    pub fn icon(self) -> &'static str  { /* "📋" | "🎯" | "🔧" | "〰️" */ }
}
```

New method (replaces the hardcoded `"personal"` load):

```rust
/// Select a project: record it, load its conversations, clear the active one.
pub fn open_project(&self, id: ProjectId) {
    let t = self.transport.clone();
    let mut current = self.current_project;
    let mut conversations = self.conversations;
    let mut active = self.active;
    let pid = id.clone();
    current.set(Some(id));
    active.set(None);
    spawn(async move {
        if let Ok(cs) = t.list_conversations(pid.as_str()).await {
            conversations.set(cs);
        }
    });
}
```

`bootstrap()` change: after `list_projects()`, seed the current project from the
first project (fallback id `"personal"`) and load via the same path
`open_project` uses, instead of the hardcoded `list_conversations("personal")`.
(Bootstrap stays one spawn; it sets `current_project` then loads conversations
for that id.)

The active conversation is derived where needed:
`conversations.iter().find(|c| Some(&c.id) == active.as_ref())`.

### 2. Project switcher (`oxide-ui/src/components/sidebar_header.rs`)

Replace the static pill with a Freya `Select`:

- `.selected_item(pill)` where `pill` is the existing accent-dot + project-name +
  chevron `rect` (current name resolved from `current_project` → matching
  `Project.name`, fallback to the first project / `"oxidemx-phase1"`).
- Option children: one `MenuButton` per `state.projects`, label = `project.name`,
  `.on_press` → `state.open_project(project.id.clone())`.
- Theme via `SelectThemePartial`: `surface` / `hairline_strong` at rest; Select's
  built-in open animation + accent-on-open handle the active state.
- `SidebarHeader` takes `state: AppState` (instead of a `project: String` prop) so
  it can read `projects` / `current_project` and call `open_project`.

### 3. Sidebar collapse + icon rail (`sidebar.rs`, `collapsible_panel.rs`)

- `Sidebar` reads/writes `state.sidebar_collapsed` (remove the local `use_state`);
  the collapse `«` button sets it `true`, the rail `»` sets it `false`.
- `CollapsiblePanel.collapsed` is driven by that signal (already its API).
- The collapsed **rail** (`SIDEBAR_RAIL_W = 60`) becomes an icon column:
  - project dot at top (opens the switcher on press → also expands),
  - one icon button per `state.conversations` (a `StatusDot`-tinted glyph), press
    = `state.open_conversation(id)` **and** `sidebar_collapsed.set(false)`,
  - the `»` expand button at the bottom.
- Width animates (see §5); `collapsible_panel.rs` takes an animated width input
  rather than the hard `if collapsed { RAIL } else { FULL }` swap.

### 4. Right panel — four-direction framework

`regions/context.rs` rewritten; new `regions/directions/` module:

```
regions/directions/mod.rs       // DirectionPanel dispatcher + StatusDirection match
regions/directions/spec.rs      // SpecDirection
regions/directions/mission.rs   // MissionDirection
regions/directions/workbench.rs // WorkbenchDirection
regions/directions/ambient.rs   // AmbientDirection
```

- **Collapsed rail (60px):** vertical column of the 4 `StatusDirection::icon()`
  buttons + the expand toggle. Pressing a direction icon sets `active_direction`
  **and** `context_collapsed.set(false)`. The `active_direction` icon is
  `accent`-tinted; others `faint()`.
- **Expanded (~300px):** a segmented header (4 buttons, active = accent fill) +
  the active direction's body via `DirectionPanel { state, direction }` which
  matches `StatusDirection` to the right sub-component.
- **"Now" content** (each body, from existing DTOs; each ends with a faint
  "more coming" line marking the deferred 2d content):
  - **Spec:** current project name + `default_working_dir`.
  - **Mission:** active conversation `title` + `model` (or "No conversation").
  - **Workbench:** active conversation `working_dir` + `worktree` (path/branch)
    if present + attachment count for the active conversation.
  - **Ambient:** `ConnState` + active conversation id/model. If the agentd
    activity/run feed is reachable from `AppState` without new wiring, show it
    read-only; otherwise show connection + a faint "activity feed coming" line
    (the live feed is then a follow-on, not a blocker for this slice).
- The collapse `«`/expand `»` on the context panel toggle `context_collapsed`.

### 5. Animations (`use_animation`, the hook `Select` itself uses)

- **Sidebar + context width:** an `AnimNum` between `FULL_W`↔`RAIL_W`
  (~180ms, `Ease::Out`), re-run on the collapsed flag (`OnChange::Rerun`), feeding
  `CollapsiblePanel`'s width. Same shape as `Select`'s internal animation.
- **Direction body cross-fade:** an opacity `AnimNum` (~120ms) keyed on
  `active_direction` changing.
- **Select dropdown:** built in (125ms scale/opacity/offset) — no work.

Animations are additive: if an `AnimNum` width proves fiddly in a panel, the
fallback is the existing instant swap (the slice's correctness does not depend on
the tween). Snapshots assert the end states, not mid-tween frames.

## Data flow

```
bootstrap ──health/list_projects──▶ projects, current_project
   └─open_project(first)──list_conversations──▶ conversations
Select option press ──open_project(id)──▶ current_project, conversations, active:=None
Rail/segment press  ──set active_direction / collapsed flags──▶ shell re-renders
DirectionPanel ──reads state (conversations/active/connection/projects)──▶ body
```

All four new signals live on the single `AppState` created in `shell()`; every
region already receives `state.clone()`, so no prop-threading changes beyond
`SidebarHeader` (project string → `AppState`) and `ContextRegion`
(`collapsed: bool` → `state: AppState`).

## Error / edge handling

- Empty `projects`: Select shows the fallback pill, no options; switcher is inert
  (no panic).
- `open_project` transport error: conversations left unchanged (same tolerant
  pattern as `bootstrap`); connection banner already covers unreachable agentd.
- No active conversation: Mission/Workbench bodies render an explicit
  "No conversation selected" state, never an `unwrap`.
- Re-render safety: state on `AppState` (not local), so collapse/direction
  survive parent re-renders (fixes the current defect).

## Testing

- **Unit (`state.rs`):** `open_project` sets `current_project`, clears `active`;
  `StatusDirection::ALL` length/labels/icons; default flags
  (`context_collapsed == true`, `active_direction == Spec`).
- **Snapshots (dark theme, `bg_deep` root):** switcher closed + open (options
  visible); sidebar expanded + collapsed icon rail; context collapsed rail (4
  icons) + expanded for **each** of the 4 directions. Read each PNG back before
  any relaunch (snapshot-read discipline).
- **Live (after green snapshots):** pick a project → conversations swap; collapse
  both panels → rails + animation; click each direction → correct body.

## File structure

| File | Responsibility |
|------|----------------|
| `oxide-freya/src/state.rs` | +4 signals, `StatusDirection`, `open_project`, bootstrap rewrite, `PartialEq` |
| `oxide-ui/src/components/sidebar_header.rs` | `Select` switcher; takes `AppState` |
| `oxide-freya/src/regions/sidebar.rs` | drive collapse from state; icon rail |
| `oxide-ui/src/components/collapsible_panel.rs` | animated width input |
| `oxide-freya/src/regions/context.rs` | rewrite: rail + segmented expanded host |
| `oxide-freya/src/regions/directions/{mod,spec,mission,workbench,ambient}.rs` | per-direction bodies |
| `oxide-freya/src/app.rs` | wire the new shell state into the three regions |

## Out of scope (follow-ons)

- Rich per-direction 2d content (new agentd endpoints + reducers).
- `.oxide` settings nav, MCP fork flow, draggable/pinned popovers.
- Right-panel drag-to-resize.

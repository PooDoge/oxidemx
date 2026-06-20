# Agent Activity Bubbles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface background conductor runs in the `oxidemx-chat` window as a floating corner cluster of per-step agent bubbles (live animation, unread badge, click-to-peek log tail + progress + actions), driven by the run-kind events already flowing from agentd.

**Architecture:** One backend fix (stamp `run_id` on every run-kind event in the one-per-run `RunEventBridge`). Then a new `src/activity/` module in the `oxidemx-overlay` crate: a pure, unit-tested event reducer (`apply_run_event`) folding `RunEventView`s into a `RunCluster`/`AgentBubble` model, plus view code (`dock.rs`) that overlays a floating cluster onto `chat_window_view` via a `Stack`, consuming `oxidemx-widgets::Kit`/`Palette`. Animations reuse the existing `Tween` + gated 60fps tick. Everything frontend is gated on `chat_window_mode`.

**Tech Stack:** Rust, iced 0.14, zbus 5 (agentd), oxidemx-conductor, oxidemx-widgets (Palette/Kit), tokio.

## Global Constraints

- **No hardcoded hex.** Every color is an `oxidemx-widgets::Palette`/`Kit` field (TOKENS.md). The mockup's cyan is a mockup value — ignore it.
- **Chat-window-only.** All new frontend state/views/messages no-op when `!state.chat_window_mode`.
- **Visual contract:** `agent-bubbles-app.jsx` in Claude Design project `686a723e-0412-4e94-870e-b4e32ae465f2` — fetch the exact sizes/states/animations via the `DesignSync` MCP (`get_file`, path `agent-bubbles-app.jsx`). Sizes in this plan are from it; treat it as the styling reference.
- **Spec:** `docs/superpowers/specs/2026-06-20-agent-activity-bubbles-design.md` is authoritative; §N references point into it.
- **Scope OUT (do not build):** inline per-step approval (`ApprovalRequested` is ignored), the "+" spawn button, the Tweaks panel.
- **Builds (Rule 3):** `agentd` + `oxidemx-widgets` build **host-side** with `CARGO_TARGET_DIR=/tmp/oxidemx-host-target` (no `-devel` libs). The `oxidemx-overlay` crate needs the `claude_development` **distrobox** (GTK4 `-devel`). **Never** mix host + distrobox cargo over the same `target/`.
- **Quality (Rule 2):** `cargo clippy` clean (warnings = defects); borrow over clone; `?` over unwrap in non-test code; no gold-plating; hand-formatted (do NOT run repo-wide `cargo fmt`).
- **Truthfulness (Rule 1):** a bubble's state comes only from a real event / `run_statuses`. Action chips that depend on a missing bus capability are disabled with a tooltip, never faked.
- **Commit after every task** (Jim wants frequent revert points). `git add` only the files you changed — never `git add -A` (Jim edits the checkout concurrently).
- **Repo root:** `/run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1` (call it `$REPO`). Branch: `phase1-local-llm-gateway`.

### Build/test command reference (use the right one per task)

```bash
# host-side (agentd, oxidemx-widgets):
cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p <crate> <filter>
cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p <crate> -- -D warnings

# distrobox (oxidemx-overlay):
distrobox enter claude_development -- bash -lc 'cd $REPO && cargo test -p oxidemx-overlay <filter>'
distrobox enter claude_development -- bash -lc 'cd $REPO && cargo clippy -p oxidemx-overlay -- -D warnings'
```
(`$REPO` must be expanded — the distrobox shell won't inherit it; write the absolute path.)

---

## File Structure

| File | Responsibility |
|---|---|
| `agentd/src/run_bridge.rs` (modify) | Add `run_id` field to `RunEventBridge`; stamp it on every step event. |
| `agentd/src/run_launcher.rs` (modify ~216) | Pass `run_id` to `RunEventBridge::new`. |
| `oxidemx-widgets/src/kit.rs` (modify) | Add slice colors (`peach`/`teal`/`blue`/`sapphire`/`pink`/`lavender`) to `Kit` + `from_palette`. |
| `overlay-rs/src/activity/mod.rs` (create) | `ActivityState`, `RunEventView`, `apply_run_event` reducer (pure, tested). |
| `overlay-rs/src/activity/model.rs` (create) | `RunCluster`, `AgentBubble`, `BubbleState`, `AgentTone` mapping, helpers. |
| `overlay-rs/src/activity/dock.rs` (create) | `dock_view` / `bubble_view` / `peek_view` (consume `Kit`). |
| `overlay-rs/src/activity/anim.rs` (create) | Per-bubble `Tween` clocks + advance + reduce-motion gate. |
| `overlay-rs/src/app/agent_events.rs` (modify ~192) | Replace flat "run" mapping with a `RunEventView` parse → `Message::RunEvent`. |
| `overlay-rs/src/app/mod.rs` (modify) | New `Message` variants. |
| `overlay-rs/src/radial/state.rs` (modify) | `activity: ActivityState` field; advance bubble tweens. |
| `overlay-rs/src/app/update.rs` (modify) | Handle new messages (gated). |
| `overlay-rs/src/app/subscriptions.rs` (modify ~40) | Extend `needs_tick` for live runs. |
| `overlay-rs/src/app/view.rs` (modify ~757) | Layer `dock_view` into `chat_window_view`'s `Stack`. |
| `overlay-rs/src/lib.rs` (modify) | `mod activity;`. |

---

## Task 1: Stamp `run_id` on every run-kind event (backend)

**Files:**
- Modify: `agentd/src/run_bridge.rs` (struct ~47-54, `new` ~57-67, `do_emit` call sites ~107-153)
- Modify: `agentd/src/run_launcher.rs:216-220` (the `RunEventBridge::new` call)
- Test: `agentd/src/run_bridge.rs` (in-file `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: every emitted `payload.kind=="run"` event now carries a non-empty `payload.run_id` (the owning run). Frontend Task 4 relies on this for grouping.

**Context:** The bridge is constructed **once per run** inside the spawned supervisor task (`run_launcher.rs:216`), so a single `run_id` field is correct. Today step events call `self.do_emit("", …)` (empty run_id). The fix: store the run_id and use it.

- [ ] **Step 1: Add a failing test** — append to the `tests` mod in `agentd/src/run_bridge.rs`:

```rust
    #[tokio::test]
    async fn step_events_carry_run_id() {
        let (bridge, emitter, _statuses) = make_bridge();
        bridge.emit(RunEvent::TaskStarted { step: "s1".into() }).await;
        bridge.emit(RunEvent::AgentMessage { step: "s1".into(), message: "hi".into() }).await;
        bridge.emit(RunEvent::TaskFinished {
            step: "s1".into(), success: true, artifact: None, summary: "done".into(),
        }).await;
        for ev in emitter.events() {
            assert_eq!(
                ev.payload["run_id"], "run-77",
                "every step event must carry the owning run_id, got {:?}", ev.payload
            );
        }
    }
```

- [ ] **Step 2: Update the test helper** so the bridge has a run_id. In the same `tests` mod, change `make_bridge` to pass a run_id:

```rust
    fn make_bridge() -> (RunEventBridge, Arc<RecordingEmitter>, Arc<Mutex<HashMap<String, String>>>) {
        let emitter = Arc::new(RecordingEmitter::default());
        let statuses: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let bridge = RunEventBridge::new("test-project", "run-77", emitter.clone(), statuses.clone());
        (bridge, emitter, statuses)
    }
```

- [ ] **Step 3: Run the test — verify it fails to compile** (`new` takes 3 args, and step events emit `""`).

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd step_events_carry_run_id`
Expected: FAIL — compile error on `RunEventBridge::new` arity.

- [ ] **Step 4: Add the `run_id` field + constructor arg.** In `run_bridge.rs`, add to the struct (after `pub project: String,`):

```rust
    /// The run this bridge belongs to (one bridge per run). Stamped on every
    /// emitted event so the UI can group step events under their run.
    pub run_id: String,
```

Update `new`:

```rust
    pub fn new(
        project: impl Into<String>,
        run_id: impl Into<String>,
        emitter: Arc<dyn EventEmitter>,
        statuses: Arc<Mutex<HashMap<String, String>>>,
    ) -> Self {
        Self {
            project: project.into(),
            run_id: run_id.into(),
            emitter,
            statuses,
        }
    }
```

- [ ] **Step 5: Use `self.run_id` for every step event.** In the `EventSink::emit` match, replace each `self.do_emit("", …)` with `self.do_emit(self.run_id.clone(), …)` for: `TaskAssigned`, `TaskStarted`, `AgentMessage`, `TaskFinished`, `TaskError`, `StepRetrying`, `StepSkipped`, `ApprovalRequested`. (Terminal events `RunStarted`/`RunFinished`/`RunFailed`/`RunCancelled` already pass their own `run_id` — leave them; they equal `self.run_id`.)

- [ ] **Step 6: Update the existing `non_terminal_events_emit_without_status_update` test.** It asserts no status is written (still true) — but its events now carry a run_id, which is fine. No change needed unless it asserts run_id emptiness; it doesn't. Leave it.

- [ ] **Step 7: Wire the launcher.** In `run_launcher.rs:216`, change:

```rust
    let bridge = Arc::new(RunEventBridge::new(
        project,
        run_id.clone(),
        self.emitter.clone(),
        self.run_statuses.clone(),
    ));
```

(`run_id` is the `String` generated at ~158 and already cloned for the handle.)

- [ ] **Step 8: Run tests — verify pass.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_bridge`
Expected: PASS (all bridge tests, including `step_events_carry_run_id`).

- [ ] **Step 9: Clippy + commit.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd -- -D warnings` → clean.

```bash
cd $REPO && git add agentd/src/run_bridge.rs agentd/src/run_launcher.rs
git commit -m "fix(agentd): stamp run_id on every run-kind event (per-run bridge)"
```

---

## Task 2: Activity model — clusters, bubbles, tone mapping

**Files:**
- Create: `overlay-rs/src/activity/model.rs`
- Create: `overlay-rs/src/activity/mod.rs` (stub now; reducer in Task 3)
- Modify: `overlay-rs/src/lib.rs` (add `mod activity;`)

**Interfaces:**
- Produces (consumed by Tasks 3, 7, 8):
  - `pub struct RunCluster { pub run_id: String, pub flow_id: String, pub status: ClusterStatus, pub bubbles: Vec<AgentBubble>, pub artifacts: Vec<String>, pub handoff: String, pub finished_at: Option<std::time::Instant> }`
  - `pub struct AgentBubble { pub step: String, pub agent: String, pub state: BubbleState, pub tone: AgentTone, pub logs: Vec<String>, pub unread: u32, pub artifact: Option<String>, pub summary: String, pub started_at: Option<std::time::Instant> }`
  - `pub enum BubbleState { Pending, Working, Done, Failed, Skipped }`
  - `pub enum ClusterStatus { Running, Finished, Failed, Cancelled }`
  - `pub struct AgentTone(pub ToneKey);` with `pub enum ToneKey { Blue, Peach, Mauve, Teal, Green, Accent }`, `AgentTone::for_agent(name: &str) -> AgentTone`, `AgentTone::icon(&self) -> &'static str`, `AgentTone::color(&self, kit: &oxidemx_widgets::Kit) -> iced::Color`.
  - `RunCluster::progress(&self) -> f32` (0.0–1.0), `RunCluster::running_count(&self) -> usize`.

- [ ] **Step 1: Create `overlay-rs/src/activity/model.rs` with a failing tone test first.** Put the test at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_maps_known_archetypes() {
        assert!(matches!(AgentTone::for_agent("web-researcher").0, ToneKey::Blue));
        assert!(matches!(AgentTone::for_agent("shell-op").0, ToneKey::Peach));
        assert!(matches!(AgentTone::for_agent("writer").0, ToneKey::Mauve));
        assert!(matches!(AgentTone::for_agent("summarizer").0, ToneKey::Teal));
        assert!(matches!(AgentTone::for_agent("browser").0, ToneKey::Green));
        assert!(matches!(AgentTone::for_agent("coordinator").0, ToneKey::Accent));
        // unknown → Accent (default)
        assert!(matches!(AgentTone::for_agent("totally-unknown").0, ToneKey::Accent));
    }

    #[test]
    fn progress_counts_terminal_bubbles() {
        let mut c = RunCluster::new("run-1", "flow-x", vec!["a".into(), "b".into()]);
        assert_eq!(c.progress(), 0.0);
        c.bubbles[0].state = BubbleState::Done;
        assert_eq!(c.progress(), 0.5);
    }
}
```

- [ ] **Step 2: Implement the model above the tests.** Match the agent→tone mapping in spec §1 (substring match so `web-researcher`/`fact-check` → researcher→Blue, `shell-op`/`indexer` → Peach, etc.):

```rust
//! Activity-bubble data model: a run is a cluster, each step is a bubble.
//! Pure data — no iced widgets here (those live in `dock.rs`). Colors are
//! resolved from the live `Kit` at render time; never hardcode hex.

use std::time::Instant;
use oxidemx_widgets::Kit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleState { Pending, Working, Done, Failed, Skipped }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterStatus { Running, Finished, Failed, Cancelled }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneKey { Blue, Peach, Mauve, Teal, Green, Accent }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTone(pub ToneKey);

impl AgentTone {
    /// Map an agent/archetype name to a slice tone (substring match, lowercased).
    pub fn for_agent(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        let key = if n.contains("research") || n.contains("fact") || n.contains("source") {
            ToneKey::Blue
        } else if n.contains("shell") || n.contains("index") || n.contains("repo") || n.contains("ops") {
            ToneKey::Peach
        } else if n.contains("writ") || n.contains("draft") || n.contains("author") {
            ToneKey::Mauve
        } else if n.contains("summ") || n.contains("digest") || n.contains("condense") {
            ToneKey::Teal
        } else if n.contains("brows") || n.contains("web") || n.contains("fetch") {
            ToneKey::Green
        } else {
            ToneKey::Accent
        };
        AgentTone(key)
    }

    pub fn icon(&self) -> &'static str {
        match self.0 {
            ToneKey::Blue => "search",
            ToneKey::Peach => "terminal",
            ToneKey::Mauve => "pencil",
            ToneKey::Teal => "clipboard",
            ToneKey::Green => "globe",
            ToneKey::Accent => "sparkle",
        }
    }

    pub fn color(&self, kit: &Kit) -> iced::Color {
        match self.0 {
            ToneKey::Blue => kit.blue,
            ToneKey::Peach => kit.peach,
            ToneKey::Mauve => kit.mauve,
            ToneKey::Teal => kit.teal,
            ToneKey::Green => kit.green,
            ToneKey::Accent => kit.accent,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentBubble {
    pub step: String,
    pub agent: String,
    pub state: BubbleState,
    pub tone: AgentTone,
    pub logs: Vec<String>,
    pub unread: u32,
    pub artifact: Option<String>,
    pub summary: String,
    pub started_at: Option<Instant>,
}

impl AgentBubble {
    pub fn new(step: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            agent: String::new(),
            state: BubbleState::Pending,
            tone: AgentTone(ToneKey::Accent),
            logs: Vec::new(),
            unread: 0,
            artifact: None,
            summary: String::new(),
            started_at: None,
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self.state, BubbleState::Done | BubbleState::Failed | BubbleState::Skipped)
    }
}

#[derive(Debug, Clone)]
pub struct RunCluster {
    pub run_id: String,
    pub flow_id: String,
    pub status: ClusterStatus,
    pub bubbles: Vec<AgentBubble>,
    pub artifacts: Vec<String>,
    pub handoff: String,
    pub finished_at: Option<Instant>,
}

impl RunCluster {
    pub fn new(run_id: impl Into<String>, flow_id: impl Into<String>, steps: Vec<String>) -> Self {
        Self {
            run_id: run_id.into(),
            flow_id: flow_id.into(),
            status: ClusterStatus::Running,
            bubbles: steps.into_iter().map(AgentBubble::new).collect(),
            artifacts: Vec::new(),
            handoff: String::new(),
            finished_at: None,
        }
    }
    pub fn bubble_mut(&mut self, step: &str) -> Option<&mut AgentBubble> {
        self.bubbles.iter_mut().find(|b| b.step == step)
    }
    pub fn progress(&self) -> f32 {
        if self.bubbles.is_empty() { return 0.0; }
        let done = self.bubbles.iter().filter(|b| b.is_terminal()).count();
        done as f32 / self.bubbles.len() as f32
    }
    pub fn running_count(&self) -> usize {
        self.bubbles.iter().filter(|b| matches!(b.state, BubbleState::Working)).count()
    }
}
```

- [ ] **Step 3: Create `overlay-rs/src/activity/mod.rs` (stub).**

```rust
//! Background-run activity bubbles for the standalone chat window (spec
//! `2026-06-20-agent-activity-bubbles-design.md`). Gated on `chat_window_mode`.

pub mod model;

pub use model::{AgentBubble, AgentTone, BubbleState, ClusterStatus, RunCluster, ToneKey};
```

- [ ] **Step 4: Register the module.** In `overlay-rs/src/lib.rs`, add `pub mod activity;` near the other `pub mod` lines.

- [ ] **Step 5: Build (the test needs `kit.blue`/`kit.peach`/`kit.teal` which Task 6 adds — so this task will not compile until Kit is extended).**

> **Ordering note:** Do **Task 6 (Kit slice colors) before Task 2's build.** Reorder if executing strictly: the tone test references `kit.blue`/`kit.peach`/`kit.teal`. If running in number order, comment out `AgentTone::color` + its usage until Task 6, then re-enable. Recommended: execute Task 6 first, then Task 2.

Run (after Task 6): `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo test -p oxidemx-overlay activity::model'`
Expected: PASS.

- [ ] **Step 6: Clippy + commit.**

```bash
cd $REPO && git add overlay-rs/src/activity/model.rs overlay-rs/src/activity/mod.rs overlay-rs/src/lib.rs
git commit -m "feat(activity): run-cluster + agent-bubble model + tone mapping"
```

---

## Task 6 (execute before Task 2's build): Extend `Kit` with slice colors

**Files:**
- Modify: `oxidemx-widgets/src/kit.rs` (struct ~15-52, `from_palette` constructor)
- Test: `oxidemx-widgets/src/kit.rs` (in-file test)

**Interfaces:**
- Produces: `Kit` gains `pub peach: Color, pub teal: Color, pub blue: Color, pub sapphire: Color, pub pink: Color, pub lavender: Color`, populated from the `Palette`'s same-named fields. Consumed by `AgentTone::color` (Task 2) and `dock.rs` (Task 7).

**Context:** `Kit` currently carries `accent/green/yellow/mauve/red/danger` but not the other slice colors. The bubble tones need `peach/teal/blue`. Add all six slice colors `Palette` already has, for symmetry.

- [ ] **Step 1: Add a failing test** at the bottom of `kit.rs`:

```rust
#[cfg(test)]
mod kit_slice_tests {
    use super::*;
    use crate::palette::Palette;

    #[test]
    fn kit_carries_slice_colors() {
        let p = Palette::hardcoded_mocha();
        let kit = Kit::from_palette(&p, 1.0, 0.0);
        assert_eq!(kit.peach, p.peach);
        assert_eq!(kit.teal, p.teal);
        assert_eq!(kit.blue, p.blue);
        assert_eq!(kit.sapphire, p.sapphire);
        assert_eq!(kit.pink, p.pink);
        assert_eq!(kit.lavender, p.lavender);
    }
}
```

(If the mocha fallback constructor has a different name, grep `hardcoded_mocha`/`fn mocha` in `palette.rs` and use the real one.)

- [ ] **Step 2: Run — verify fail.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-widgets kit_carries_slice_colors`
Expected: FAIL — no field `peach` on `Kit`.

- [ ] **Step 3: Add the fields.** In the `Kit` struct (kit.rs), after the existing `pub mauve: Color,` add:

```rust
    pub peach: Color,
    pub teal: Color,
    pub blue: Color,
    pub sapphire: Color,
    pub pink: Color,
    pub lavender: Color,
```

- [ ] **Step 4: Populate them in `from_palette`.** In the `Kit::from_palette(p: &Palette, alpha: f32, pulse: f32)` constructor body, add to the struct literal:

```rust
            peach: p.peach,
            teal: p.teal,
            blue: p.blue,
            sapphire: p.sapphire,
            pink: p.pink,
            lavender: p.lavender,
```

- [ ] **Step 5: Run — verify pass.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-widgets kit_carries_slice_colors`
Expected: PASS.

- [ ] **Step 6: Clippy + commit.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p oxidemx-widgets -- -D warnings` → clean.

```bash
cd $REPO && git add oxidemx-widgets/src/kit.rs
git commit -m "feat(widgets): expose slice colors (peach/teal/blue/…) on Kit"
```

---

## Task 3: The event reducer — `apply_run_event`

**Files:**
- Modify: `overlay-rs/src/activity/mod.rs` (add `ActivityState`, `RunEventView`, `apply_run_event`)
- Test: `overlay-rs/src/activity/mod.rs` (in-file tests)

**Interfaces:**
- Produces (consumed by Tasks 4, 5, 7, 8):
  - `pub struct RunEventView { pub run_id: String, pub variant: String, pub flow_id: String, pub steps: Vec<String>, pub step: String, pub agent: String, pub message: String, pub success: bool, pub artifact: Option<String>, pub summary: String, pub artifacts: Vec<String>, pub handoff: String }` — derives `Debug, Clone`.
  - `pub struct ActivityState { pub clusters: Vec<RunCluster>, pub expanded: Option<String>, pub peek: Option<(String, String)> /* (run_id, step) */ }` with `Default`.
  - `impl ActivityState { pub fn apply_run_event(&mut self, ev: &RunEventView); pub fn cluster(&self, run_id: &str) -> Option<&RunCluster>; }`
- Consumes: `model::*` from Task 2.

- [ ] **Step 1: Write the reducer tests first** (append to `mod.rs`):

```rust
#[cfg(test)]
mod reducer_tests {
    use super::*;

    fn ev(variant: &str) -> RunEventView {
        RunEventView { run_id: "run-1".into(), variant: variant.into(), ..Default::default() }
    }

    #[test]
    fn full_lifecycle_builds_cluster() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView {
            steps: vec!["a".into(), "b".into()], flow_id: "research".into(), ..ev("RunStarted")
        });
        assert_eq!(s.clusters.len(), 1);
        assert_eq!(s.cluster("run-1").unwrap().bubbles.len(), 2);

        s.apply_run_event(&RunEventView { step: "a".into(), agent: "web-researcher".into(), ..ev("TaskAssigned") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].tone.0, ToneKey::Blue));

        s.apply_run_event(&RunEventView { step: "a".into(), ..ev("TaskStarted") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Working));

        s.apply_run_event(&RunEventView { step: "a".into(), message: "google_search · x".into(), ..ev("AgentMessage") });
        assert_eq!(s.cluster("run-1").unwrap().bubbles[0].logs.len(), 1);
        assert_eq!(s.cluster("run-1").unwrap().bubbles[0].unread, 1);

        s.apply_run_event(&RunEventView {
            step: "a".into(), success: true, artifact: Some("/tmp/out.md".into()), summary: "ok".into(), ..ev("TaskFinished")
        });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Done));
        assert_eq!(s.cluster("run-1").unwrap().progress(), 0.5);

        s.apply_run_event(&RunEventView {
            artifacts: vec!["/tmp/out.md".into()], handoff: "# done".into(), ..ev("RunFinished")
        });
        assert!(matches!(s.cluster("run-1").unwrap().status, ClusterStatus::Finished));
    }

    #[test]
    fn unknown_step_and_unknown_run_are_ignored() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { step: "ghost".into(), ..ev("TaskStarted") }); // no cluster yet
        assert!(s.clusters.is_empty());
        s.apply_run_event(&RunEventView { steps: vec!["a".into()], ..ev("RunStarted") });
        s.apply_run_event(&RunEventView { step: "ghost".into(), ..ev("TaskStarted") }); // unknown step
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Pending));
    }

    #[test]
    fn failure_and_skip_and_cancel() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { steps: vec!["a".into(), "b".into()], ..ev("RunStarted") });
        s.apply_run_event(&RunEventView { step: "a".into(), message: "boom".into(), ..ev("TaskError") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Failed));
        s.apply_run_event(&RunEventView { step: "b".into(), ..ev("StepSkipped") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[1].state, BubbleState::Skipped));
        s.apply_run_event(&ev("RunCancelled"));
        assert!(matches!(s.cluster("run-1").unwrap().status, ClusterStatus::Cancelled));
    }

    #[test]
    fn unread_does_not_bump_while_peek_open() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { steps: vec!["a".into()], ..ev("RunStarted") });
        s.peek = Some(("run-1".into(), "a".into()));
        s.apply_run_event(&RunEventView { step: "a".into(), message: "m".into(), ..ev("AgentMessage") });
        assert_eq!(s.cluster("run-1").unwrap().bubbles[0].unread, 0);
    }

    #[test]
    fn approval_requested_is_ignored() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { steps: vec!["a".into()], ..ev("RunStarted") });
        let before = format!("{:?}", s.cluster("run-1").unwrap().bubbles[0].state);
        s.apply_run_event(&RunEventView { step: "a".into(), ..ev("ApprovalRequested") });
        let after = format!("{:?}", s.cluster("run-1").unwrap().bubbles[0].state);
        assert_eq!(before, after); // no state change (v1 ignores approvals)
    }
}
```

- [ ] **Step 2: Run — verify fail to compile** (`RunEventView`/`ActivityState` undefined).

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo test -p oxidemx-overlay activity::mod_'` (or `activity::reducer_tests`)
Expected: FAIL — unresolved types.

- [ ] **Step 3: Implement `RunEventView`, `ActivityState`, and the reducer** (add to `mod.rs`, above the tests). `RunEventView` needs `Default` for the `..ev(...)` test ergonomics:

```rust
use std::time::Instant;

/// A flattened, connector-agnostic view of one conductor run event, parsed
/// from the agentd `run`-kind payload (`variant` + `details`). Defaulted
/// fields are simply absent for variants that don't carry them.
#[derive(Debug, Clone, Default)]
pub struct RunEventView {
    pub run_id: String,
    pub variant: String,
    pub flow_id: String,
    pub steps: Vec<String>,
    pub step: String,
    pub agent: String,
    pub message: String,
    pub success: bool,
    pub artifact: Option<String>,
    pub summary: String,
    pub artifacts: Vec<String>,
    pub handoff: String,
}

#[derive(Debug, Default)]
pub struct ActivityState {
    pub clusters: Vec<RunCluster>,
    /// run_id of the currently-expanded cluster (None = all collapsed).
    pub expanded: Option<String>,
    /// (run_id, step) of the open peek popover, if any.
    pub peek: Option<(String, String)>,
}

impl ActivityState {
    pub fn cluster(&self, run_id: &str) -> Option<&RunCluster> {
        self.clusters.iter().find(|c| c.run_id == run_id)
    }
    fn cluster_mut(&mut self, run_id: &str) -> Option<&mut RunCluster> {
        self.clusters.iter_mut().find(|c| c.run_id == run_id)
    }
    fn peek_is(&self, run_id: &str, step: &str) -> bool {
        self.peek.as_ref().is_some_and(|(r, s)| r == run_id && s == step)
    }

    pub fn apply_run_event(&mut self, ev: &RunEventView) {
        match ev.variant.as_str() {
            "RunStarted" => {
                if self.cluster(&ev.run_id).is_none() {
                    self.clusters.push(RunCluster::new(
                        ev.run_id.clone(), ev.flow_id.clone(), ev.steps.clone(),
                    ));
                }
            }
            "TaskAssigned" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.agent = ev.agent.clone();
                        b.tone = AgentTone::for_agent(&ev.agent);
                    }
                }
            }
            "TaskStarted" => {
                let now = Instant::now();
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Working;
                        b.started_at.get_or_insert(now);
                    }
                }
            }
            "AgentMessage" => {
                let open = self.peek_is(&ev.run_id, &ev.step);
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.logs.push(ev.message.clone());
                        if b.logs.len() > 40 { b.logs.remove(0); }
                        if !open { b.unread = b.unread.saturating_add(1); }
                    }
                }
            }
            "TaskFinished" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = if ev.success { BubbleState::Done } else { BubbleState::Failed };
                        b.artifact = ev.artifact.clone();
                        b.summary = ev.summary.clone();
                    }
                }
            }
            "TaskError" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Failed;
                        b.logs.push(format!("error: {}", ev.message));
                    }
                }
            }
            "StepRetrying" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Working;
                        b.logs.push("retrying…".into());
                    }
                }
            }
            "StepSkipped" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Skipped;
                    }
                }
            }
            "RunFinished" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    c.status = ClusterStatus::Finished;
                    c.artifacts = ev.artifacts.clone();
                    c.handoff = ev.handoff.clone();
                    c.finished_at = Some(Instant::now());
                }
            }
            "RunFailed" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    c.status = ClusterStatus::Failed;
                    c.finished_at = Some(Instant::now());
                }
            }
            "RunCancelled" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    c.status = ClusterStatus::Cancelled;
                    c.finished_at = Some(Instant::now());
                }
            }
            // v1 ignores per-step approvals (see spec §10).
            "ApprovalRequested" | _ => {}
        }
    }
}
```

- [ ] **Step 4: Re-export the new types.** Update the `pub use` in `mod.rs`:

```rust
pub use model::{AgentBubble, AgentTone, BubbleState, ClusterStatus, RunCluster, ToneKey};
// (ActivityState + RunEventView are defined in this module — already public.)
```

- [ ] **Step 5: Run — verify pass.**

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo test -p oxidemx-overlay activity::'`
Expected: PASS (model + reducer tests).

- [ ] **Step 6: Clippy + commit.** (Clippy may flag `"ApprovalRequested" | _` — if so, drop the explicit arm and keep just `_ => {}` with a comment.)

```bash
cd $REPO && git add overlay-rs/src/activity/mod.rs
git commit -m "feat(activity): apply_run_event reducer + RunEventView + ActivityState"
```

---

## Task 4: Parse run events into `RunEventView` (demux)

**Files:**
- Modify: `overlay-rs/src/app/agent_events.rs:192-211` (the `"run"` arm) + `192` region
- Modify: `overlay-rs/src/app/mod.rs` (add `Message::RunEvent`)
- Test: `overlay-rs/src/app/agent_events.rs` (in-file test of the parse)

**Interfaces:**
- Produces: `Message::RunEvent(crate::activity::RunEventView)` (consumed by Task 5's `update`). The `"run"` arm now parses `payload.run_id` + `payload.variant` + `payload.details.{…}` into a `RunEventView`.
- Consumes: `RunEventView` from Task 3.

- [ ] **Step 1: Add the `Message` variant.** In `overlay-rs/src/app/mod.rs` near `AgentdEvent`, add:

```rust
    /// A conductor run-kind event, parsed into a flat view for the activity dock.
    RunEvent(crate::activity::RunEventView),
```

- [ ] **Step 2: Write a failing parse test** at the bottom of `agent_events.rs`:

```rust
#[cfg(test)]
mod run_parse_tests {
    use super::*;

    #[test]
    fn parses_run_started_into_view() {
        let payload = r#"{"kind":"run","variant":"RunStarted","run_id":"run-9",
            "details":{"flow_id":"research","steps":["a","b"]}}"#;
        match demux_event("run-9", payload) {
            Message::RunEvent(v) => {
                assert_eq!(v.run_id, "run-9");
                assert_eq!(v.variant, "RunStarted");
                assert_eq!(v.flow_id, "research");
                assert_eq!(v.steps, vec!["a".to_string(), "b".to_string()]);
            }
            other => panic!("expected RunEvent, got {other:?}"),
        }
    }

    #[test]
    fn parses_task_finished_details() {
        let payload = r#"{"kind":"run","variant":"TaskFinished","run_id":"run-9",
            "details":{"step":"a","success":true,"artifact":"/tmp/o.md","summary":"ok"}}"#;
        match demux_event("run-9", payload) {
            Message::RunEvent(v) => {
                assert_eq!(v.step, "a");
                assert!(v.success);
                assert_eq!(v.artifact.as_deref(), Some("/tmp/o.md"));
                assert_eq!(v.summary, "ok");
            }
            other => panic!("expected RunEvent, got {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Run — verify fail.**

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo test -p oxidemx-overlay run_parse_tests'`
Expected: FAIL — the `"run"` arm returns `AgentdEvent`, not `RunEvent`.

- [ ] **Step 4: Replace the `"run"` arm** (agent_events.rs ~192-211) with a full parse:

```rust
        "run" => {
            let d = val.get("details").cloned().unwrap_or(serde_json::Value::Null);
            let s = |v: &serde_json::Value, k: &str| {
                v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string()
            };
            let strs = |v: &serde_json::Value, k: &str| {
                v.get(k).and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            };
            let view = crate::activity::RunEventView {
                run_id: val.get("run_id").and_then(|x| x.as_str()).unwrap_or(thread_or_run).to_string(),
                variant: val.get("variant").and_then(|x| x.as_str()).unwrap_or("run").to_string(),
                flow_id: s(&d, "flow_id"),
                steps: strs(&d, "steps"),
                step: s(&d, "step"),
                agent: s(&d, "agent"),
                message: s(&d, "message"),
                success: d.get("success").and_then(|x| x.as_bool()).unwrap_or(false),
                artifact: d.get("artifact").and_then(|x| x.as_str()).map(String::from),
                summary: s(&d, "summary"),
                artifacts: strs(&d, "artifacts"),
                handoff: s(&d, "handoff_markdown"),
            };
            Message::RunEvent(view)
        }
```

- [ ] **Step 5: Run — verify pass.**

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo test -p oxidemx-overlay run_parse_tests'`
Expected: PASS.

- [ ] **Step 6: Clippy + commit.** (`update` doesn't handle `Message::RunEvent` yet — add a temporary `Message::RunEvent(_) => {}` arm in update's match if clippy/compile demands exhaustiveness; Task 5 replaces it.)

```bash
cd $REPO && git add overlay-rs/src/app/agent_events.rs overlay-rs/src/app/mod.rs
git commit -m "feat(activity): parse run-kind events into RunEventView"
```

---

## Task 5: Wire state + messages + update (gated)

**Files:**
- Modify: `overlay-rs/src/radial/state.rs` (add `activity` field + init)
- Modify: `overlay-rs/src/app/mod.rs` (add action `Message` variants)
- Modify: `overlay-rs/src/app/update.rs` (handle messages, gated)
- Modify: `overlay-rs/src/app/subscriptions.rs:40` (extend `needs_tick`)

**Interfaces:**
- Consumes: `ActivityState` (Task 3), `Message::RunEvent` (Task 4), agentd `cancel_run`/`run_flow` D-Bus methods (existing).
- Produces (consumed by Tasks 7, 8): `state.activity: ActivityState`; messages `ActivityExpand(String)`, `ActivityCollapse`, `BubblePeekToggle(String, String)`, `BubbleDismiss(String, String)`, `RunCancel(String)`, `RunRetry(String, String) /* run_id, flow_id */`, `RunOpenArtifact(String)`, `RunTranscript(String)`.

- [ ] **Step 1: Add the field to `RadialState`.** In `radial/state.rs`, near `chat_window_mode`, add `pub activity: crate::activity::ActivityState,` and in the constructor add `activity: crate::activity::ActivityState::default(),`.

- [ ] **Step 2: Add the action messages** in `app/mod.rs`:

```rust
    ActivityExpand(String),                 // run_id — expand its cluster
    ActivityCollapse,                       // collapse all + close peek
    BubblePeekToggle(String, String),       // (run_id, step) — open/close peek
    BubbleDismiss(String, String),          // (run_id, step) — remove a finished bubble/cluster
    RunCancel(String),                      // run_id — cancel_run over D-Bus
    RunRetry(String, String),               // (run_id, flow_id) — re-launch the flow
    RunOpenArtifact(String),                // path — xdg-open
    RunTranscript(String),                  // run_id — post handoff/log into the chat thread
```

- [ ] **Step 3: Handle them in `update`** (in the big match in `app/update.rs`). Replace the temporary `Message::RunEvent(_)` arm and add the rest. All gated on `chat_window_mode`:

```rust
        Message::RunEvent(view) => {
            if state.chat_window_mode {
                state.activity.apply_run_event(&view);
            }
        }
        Message::ActivityExpand(run_id) => {
            if state.chat_window_mode {
                state.activity.expanded = Some(run_id);
                state.activity.peek = None;
            }
        }
        Message::ActivityCollapse => {
            if state.chat_window_mode {
                state.activity.expanded = None;
                state.activity.peek = None;
            }
        }
        Message::BubblePeekToggle(run_id, step) => {
            if state.chat_window_mode {
                let open = state.activity.peek.as_ref()
                    .is_some_and(|(r, s)| *r == run_id && *s == step);
                state.activity.peek = if open { None } else {
                    // opening clears that bubble's unread
                    if let Some(c) = state.activity.clusters.iter_mut().find(|c| c.run_id == run_id) {
                        if let Some(b) = c.bubbles.iter_mut().find(|b| b.step == step) {
                            b.unread = 0;
                        }
                    }
                    Some((run_id, step))
                };
            }
        }
        Message::BubbleDismiss(run_id, _step) => {
            if state.chat_window_mode {
                state.activity.clusters.retain(|c| c.run_id != run_id);
                if state.activity.expanded.as_deref() == Some(run_id.as_str()) {
                    state.activity.expanded = None;
                }
                state.activity.peek = None;
            }
        }
        Message::RunOpenArtifact(path) => {
            // xdg-open is fire-and-forget; failure is non-fatal.
            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
        }
        Message::RunCancel(run_id) => {
            // Reuse the existing agentd cancel_run D-Bus call. Follow the codebase's
            // existing pattern for issuing a Task that calls the AgentService proxy
            // (grep an existing `cancel_turn`/`run_flow` call site in update.rs/app
            // and mirror it). On success the conductor emits RunCancelled → the
            // reducer flips the cluster. Do NOT optimistically mark cancelled here
            // (Rule 1: wait for the event).
            return crate::app::agentd_calls::cancel_run(state, run_id);
        }
        Message::RunRetry(_run_id, flow_id) => {
            return crate::app::agentd_calls::run_flow(state, flow_id);
        }
        Message::RunTranscript(run_id) => {
            if state.chat_window_mode {
                if let Some(c) = state.activity.cluster(&run_id) {
                    let body = if c.handoff.is_empty() {
                        c.bubbles.iter().flat_map(|b| b.logs.iter().cloned()).collect::<Vec<_>>().join("\n")
                    } else { c.handoff.clone() };
                    // Append as an assistant message using the codebase's existing
                    // "insert assistant text into the active thread" helper (grep how
                    // AgentdFinal text is appended — mirror that).
                    crate::app::chat_insert::assistant_note(state, &body);
                }
            }
        }
```

> **Implementer note:** `agentd_calls::cancel_run` / `run_flow` and `chat_insert::assistant_note` are *named intentions* — the exact call mechanism already exists in this codebase. Before writing, grep for the existing `cancel_turn`, `run_flow` (the chat already launches flows), and the path that appends `AgentdFinal`/assistant text to a thread; reuse those exact functions. If `run_flow` from the UI isn't yet wired, make **RunRetry** a disabled chip (Rule 1) and note it. If appending assistant text needs a thread index, use the active thread.

- [ ] **Step 4: Extend `needs_tick`** in `subscriptions.rs:40`:

```rust
    let runs_live = state.chat_window_mode
        && state.activity.clusters.iter().any(|c| {
            matches!(c.status, crate::activity::ClusterStatus::Running)
                || c.finished_at.is_some_and(|t| t.elapsed().as_millis() < 7000)
        });
    let needs_tick = !state.chat_window_mode
        || state.ai_loading
        || state.ai_pending_question.is_some()
        || state.ai_scroll_active
        || runs_live;
```

(The `finished_at < 7000ms` keeps the tick alive through the burst + recent-tray slide from Task 9.)

- [ ] **Step 5: Build + clippy.** No new unit test (integration wiring; behavior is covered by the reducer tests + user visual check).

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo build -p oxidemx-overlay && cargo clippy -p oxidemx-overlay -- -D warnings'`
Expected: builds clean. (Resolve any `agentd_calls`/`chat_insert` naming against the real helpers found by grep.)

- [ ] **Step 6: Commit.**

```bash
cd $REPO && git add overlay-rs/src/radial/state.rs overlay-rs/src/app/mod.rs overlay-rs/src/app/update.rs overlay-rs/src/app/subscriptions.rs
git commit -m "feat(activity): state + action messages + update wiring (chat_window gated)"
```

---

## Task 7: The dock, bubble, and peek views

**Files:**
- Create: `overlay-rs/src/activity/dock.rs`
- Modify: `overlay-rs/src/activity/mod.rs` (`pub mod dock;`)
- Modify: `overlay-rs/src/app/view.rs:757` (`chat_window_view` — layer the dock into the `Stack`)

**Interfaces:**
- Consumes: `ActivityState`, `RunCluster`, `AgentBubble`, `AgentTone`, `BubbleState`, `ClusterStatus` (Tasks 2-3), `Kit` (Task 6), the `Message` actions (Task 5).
- Produces: `pub fn dock_view<'a>(act: &'a ActivityState, kit: &Kit) -> Option<Element<'a, Message>>` (None when no clusters). Internal `bubble_view`, `peek_view`, `cluster_orb`.

> **Visual contract:** fetch `agent-bubbles-app.jsx` via `DesignSync get_file` (project `686a723e-0412-4e94-870e-b4e32ae465f2`). Match: orb 52px, peek panel 286px, the state colors (working=tone, done=`kit.green`, failed=`kit.red`), the badge, the log tail (mono, newest highlighted), the action chips per state (spec §1). This is GUI code — verified by build/clippy + Jim's visual check, not headless tests.

- [ ] **Step 1: Create `dock.rs` with the bubble view.** Use iced `container`/`button`/`stack` + `Kit` colors. Build the orb as a `button` with a circular `container` background (gradient approximated by a solid tone fill at low alpha over `kit.crust` — iced has no radial-gradient; use `kit.fade(tone, 0.32)` over a `kit.crust` base, `1.5px` tone border, `border::radius(26)`), the icon via the existing icon helper (grep how `chat_ui` renders line icons — reuse that `Icon`/svg helper; the icon name comes from `bubble.tone.icon()`), a badge `container` (top-right) when `unread>0`/done/failed, and a dismiss affordance for terminal bubbles. Skeleton:

```rust
//! Activity dock: floating corner cluster of agent bubbles for the chat window.
use iced::widget::{button, column, container, row, text, Space, Stack};
use iced::{Alignment, Color, Element, Length};
use oxidemx_widgets::Kit;

use crate::app::Message;
use super::{ActivityState, AgentBubble, BubbleState, ClusterStatus, RunCluster};

const ORB: f32 = 52.0;

fn bubble_color(b: &AgentBubble, kit: &Kit) -> Color {
    match b.state {
        BubbleState::Done => kit.green,
        BubbleState::Failed => kit.red,
        BubbleState::Skipped => kit.overlay0,
        _ => b.tone.color(kit),
    }
}

fn bubble_view<'a>(run_id: &str, b: &'a AgentBubble, kit: &Kit, peek_open: bool) -> Element<'a, Message> {
    let c = bubble_color(b, kit);
    let icon = super::super::chat_ui::icons::svg(b.tone.icon(), 20.0, c); // reuse existing icon helper (grep real path)
    let orb = container(icon)
        .width(Length::Fixed(ORB)).height(Length::Fixed(ORB))
        .align_x(Alignment::Center).align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(kit.fade(c, 0.20))),
            border: iced::border::Border { color: c, width: 1.5, radius: (ORB / 2.0).into() },
            ..Default::default()
        });
    let (rid, step) = (run_id.to_string(), b.step.clone());
    let mut layers: Vec<Element<'a, Message>> = vec![
        button(orb).padding(0).style(|_, _| button::Style::default())
            .on_press(Message::BubblePeekToggle(rid.clone(), step.clone())).into(),
    ];
    // badge: unread while working, check when done, ! when failed
    if let Some(b_el) = badge(b, kit) { layers.push(b_el); }
    let _ = peek_open; // peek rendered by the dock layer (see peek_view), not here
    Stack::with_children(layers).width(Length::Fixed(ORB)).height(Length::Fixed(ORB)).into()
}
```

(Fill in `badge(...)` — a small `container` with `kit.crust` text on `c`/`kit.red`/`kit.green` background, `border::radius(10)`, positioned via a `Stack` offset; mirror the jsx badge. Grep the real icon helper path before writing `super::super::chat_ui::icons::svg` — replace with the actual one.)

- [ ] **Step 2: Add `peek_view`.** A 286px-wide `column` in a styled `container` (`kit.fade(kit.crust, 0.97)` bg, left border `2.5px` tone, `border::radius(16)`): header row (icon orb + name `Font::MONOSPACE` + state label + elapsed `m:ss` from `started_at.elapsed()`), task/progress (a 3px `container` track + a tone-filled inner sized to `cluster.progress()`), the log tail (`column` of the last 6 `b.logs`, `Font::MONOSPACE` size 10.5, newest = `kit.text` others `kit.subtext0`), and the action chips row per `b.state`:
  - Working → `Cancel` button → `Message::RunCancel(run_id)`.
  - Done → `Transcript` → `Message::RunTranscript(run_id)`, `Open artifact` (if `b.artifact`) → `Message::RunOpenArtifact(path)`, `Dismiss` → `Message::BubbleDismiss`.
  - Failed → `Retry` → `Message::RunRetry(run_id, flow_id)`, `Dismiss`.
  Chips are small `button`s styled with `kit.surface2` border / `kit.fade(kit.accent,0.16)` hover (mirror the existing slash-palette button style at `chat_ui/palette.rs`).

- [ ] **Step 3: Add `cluster_orb` + `dock_view`.** `dock_view` returns the corner stack:
  - If `act.expanded == Some(run_id)`: a header pill (`row`: status dot + `"{n} running"`/`"all done"` + flow name + a collapse `×` → `Message::ActivityCollapse`) above a `column` of that run's `bubble_view`s; if `act.peek == Some((run_id, step))`, render `peek_view` beside the column. Behind it all, a full-window transparent `button`/`mouse_area` emitting `Message::ActivityCollapse` (click-away).
  - Else (collapsed): a `column` of one `cluster_orb` per cluster — a 52px orb (icon `agents`, color = `kit.accent` if `status==Running` else `kit.green`) with a count badge = `bubbles.len()`; `on_press → Message::ActivityExpand(run_id)`. A single-bubble run may render its `bubble_view` directly (optional; cluster_orb is fine for v1).
  Return `None` if `act.clusters.is_empty()`.

```rust
pub fn dock_view<'a>(act: &'a ActivityState, kit: &Kit) -> Option<Element<'a, Message>> {
    if act.clusters.is_empty() { return None; }
    // ... build collapsed vs expanded per above ...
    Some(content.into())
}
```

- [ ] **Step 4: Register + layer into `chat_window_view`.** In `mod.rs` add `pub mod dock;`. In `view.rs` `chat_window_view` (~757), after building the chat layer, build the dock and push it as the top `Stack` child, aligned bottom-right with padding clear of the composer:

```rust
    // ... existing: let mut children = vec![body, shader?, chat]; ...
    let kit = oxidemx_widgets::Kit::from_palette(
        &oxidemx_widgets::palette::Palette::from_theme(&state.theme.theme), 1.0, state.pulse(), // use the same pulse the chat uses
    );
    if let Some(dock) = crate::activity::dock::dock_view(&state.activity, &kit) {
        let corner = iced::widget::container(dock)
            .width(Length::Fill).height(Length::Fill)
            .align_x(iced::Alignment::End).align_y(iced::Alignment::End)
            .padding([0, 18, 96, 0]); // clear of composer (bottom 96)
        children.push(corner.into());
    }
    iced::widget::Stack::with_children(children).into()
```

(Grep how `chat_ui::view` computes its `pulse` arg and reuse the same source so the dock breathes in sync.)

- [ ] **Step 5: Build + clippy.**

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo build -p oxidemx-overlay && cargo clippy -p oxidemx-overlay -- -D warnings'`
Expected: builds clean.

- [ ] **Step 6: Commit.**

```bash
cd $REPO && git add overlay-rs/src/activity/dock.rs overlay-rs/src/activity/mod.rs overlay-rs/src/app/view.rs
git commit -m "feat(activity): dock + bubble + peek views layered into chat window"
```

---

## Task 8: Animations (Tween + gated tick)

**Files:**
- Create: `overlay-rs/src/activity/anim.rs`
- Modify: `overlay-rs/src/activity/model.rs` (add per-bubble `Tween` clocks)
- Modify: `overlay-rs/src/radial/state.rs` (advance activity tweens in `advance_animations`)
- Modify: `overlay-rs/src/activity/dock.rs` (use the animated values)

**Interfaces:**
- Consumes: `crate::anim::Tween` (`Tween::at`, `step(dt_ms)`, `.current`), `state.advance_animations()` (update.rs:94), `state.reduce_motion` (a bool — add to config/state if absent; default false).
- Produces: a continuous `pulse`/`bob` value per working bubble + a one-shot entrance, read by `dock.rs`.

- [ ] **Step 1: Add animation clocks to `AgentBubble`.** In `model.rs`, add fields `pub pulse: crate::anim::Tween,` (continuous 0→1→0) and `pub appear: crate::anim::Tween,` (one-shot 0→1 on mount), initialized in `AgentBubble::new` to `Tween::at(0.0)` / `Tween::at(0.0)`. (Keep them out of the `Debug`-sensitive tests — `Tween` derives `Debug`.)

- [ ] **Step 2: Create `anim.rs`** with the advance helper + a phase reader:

```rust
//! Per-bubble animation clocks. Continuous pulse/bob driven by the shared
//! 60fps tick (gated to live runs in subscriptions.rs); reduce-motion freezes them.
use crate::activity::ActivityState;

/// Advance every bubble's clocks by `dt_ms`. Continuous clocks ping-pong 0↔1.
pub fn advance(act: &mut ActivityState, dt_ms: f32, reduce_motion: bool) {
    if reduce_motion { return; }
    for c in &mut act.clusters {
        for b in &mut c.bubbles {
            // appear: settle to 1.0 (one-shot)
            if b.appear.target < 1.0 { /* set once on creation in the reducer */ }
            b.appear.step(dt_ms);
            // pulse: bounce target between 0 and 1 when idle
            if b.pulse.is_idle() {
                let next = if b.pulse.current > 0.5 { 0.0 } else { 1.0 };
                b.pulse.set_target(next, &crate::anim::TransitionConfig::linear(900.0));
            }
            b.pulse.step(dt_ms);
        }
    }
}
```

(Grep the real `TransitionConfig` constructor/API — the explorer found `set_target(target, &TransitionConfig)`; use the existing easing/config the codebase already uses for breathing, e.g. how `ai_scroll_tween`/`menu` are configured. If there's a simpler sine-from-`show_time` pattern used elsewhere for breathing, prefer that over ping-ponging a Tween.)

- [ ] **Step 3: Set `appear.set_target(1.0, …)` when a bubble is created** — in the reducer's `RunStarted` arm (Task 3), after building the cluster, leave appear at 0 and start it here, OR start it in `advance` on first sight. Simplest: in `AgentBubble::new`, call `appear.set_target(1.0, &cfg)` so it animates in on creation.

- [ ] **Step 4: Call `anim::advance` from `advance_animations`.** In `radial/state.rs`'s `advance_animations(&mut self, dt_ms)`, add (gated to chat window):

```rust
        if self.chat_window_mode {
            crate::activity::anim::advance(&mut self.activity, dt_ms, self.reduce_motion);
        }
```

(If `self.reduce_motion` doesn't exist, add `pub reduce_motion: bool` to `RadialState` defaulting false; wire to config later. Don't block on a config UI.)

- [ ] **Step 5: Use the values in `dock.rs`.** In `bubble_view`, when `b.state == Working`, modulate the orb's glow/border alpha by `b.pulse.current` (e.g. border color `kit.fade(c, 0.6 + 0.4 * b.pulse.current)`), and scale the whole orb's opacity by `b.appear.current` (multiply the fades). Keep it subtle. (Pixel-exact `spin`/`burst` are optional per spec §6 — a pulsing border + appear fade satisfies "match the intent".)

- [ ] **Step 6: Build + clippy + visual.**

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo build -p oxidemx-overlay && cargo clippy -p oxidemx-overlay -- -D warnings'`
Expected: builds clean. (Idle CPU stays low because the tick only runs while a run is live — Task 5's `needs_tick`.)

- [ ] **Step 7: Commit.**

```bash
cd $REPO && git add overlay-rs/src/activity/anim.rs overlay-rs/src/activity/model.rs overlay-rs/src/radial/state.rs overlay-rs/src/activity/dock.rs
git commit -m "feat(activity): bubble pulse + appear animations on the gated tick"
```

---

## Task 9: Recent-tray lifecycle

**Files:**
- Modify: `overlay-rs/src/activity/dock.rs` (`dock_view` — render finished clusters as a recent tray)
- Modify: `overlay-rs/src/activity/mod.rs` (a helper to partition active vs recent + cap)

**Interfaces:**
- Consumes: `RunCluster::finished_at`, `ClusterStatus`.
- Produces: finished/failed/cancelled clusters older than ~6s collapse into a compact "recent" affordance (still expandable); cap the recent list at 5 (drop oldest, `log::debug!` what dropped — no silent truncation).

- [ ] **Step 1: Add a partition helper** in `mod.rs`:

```rust
impl ActivityState {
    /// (active_or_just_finished, recent) split. A cluster is "recent" once it
    /// finished/failed/cancelled more than 6s ago. Recent is capped at 5.
    pub fn partition(&self) -> (Vec<&RunCluster>, Vec<&RunCluster>) {
        let mut active = Vec::new();
        let mut recent = Vec::new();
        for c in &self.clusters {
            let is_recent = c.finished_at
                .is_some_and(|t| t.elapsed().as_secs() >= 6)
                && !matches!(c.status, ClusterStatus::Running);
            if is_recent { recent.push(c); } else { active.push(c); }
        }
        if recent.len() > 5 {
            let dropped = recent.len() - 5;
            log::debug!("activity: dropping {dropped} old recent run(s) past the cap of 5");
            recent.truncate(5);
        }
        (active, recent)
    }
}
```

- [ ] **Step 2: Render the recent tray in `dock_view`.** Below the active cluster-orbs (when collapsed), render a compact tray: a small pill `"recent ▸ {n}"` that, when expanded (reuse `expanded` keyed on a sentinel like `"__recent__"` or a separate `bool`), lists the recent clusters' final bubbles with Transcript/Open-artifact/Dismiss. Keep it minimal — a row of small done/failed dots that each `ActivityExpand(run_id)`.

- [ ] **Step 3: Build + clippy + visual.**

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo build -p oxidemx-overlay && cargo clippy -p oxidemx-overlay -- -D warnings'`
Expected: builds clean.

- [ ] **Step 4: Commit.**

```bash
cd $REPO && git add overlay-rs/src/activity/dock.rs overlay-rs/src/activity/mod.rs
git commit -m "feat(activity): recent-run tray (collapse finished clusters after 6s)"
```

---

## Task 10: Install + live verification

**Files:** none (build + install + user check)

- [ ] **Step 1: Full build (overlay, distrobox).**

Run: `distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1 && cargo build --release -p oxidemx-overlay'`
Expected: release build succeeds.

- [ ] **Step 2: Build agentd (host) + install.** Mirror the project's existing install flow (`install.sh` / the chat-window slice's install steps): install the new `agentd` + `oxidemx-chat` binaries, restart the agentd systemd user service, kill old chat instances, relaunch via the `.desktop`. (Follow `feedback_install_and_restart_after_updates`.)

- [ ] **Step 3: Hand to Jim for visual verification.** Provide a one-line manual test: launch `oxidemx-chat`, send a prompt that triggers a flow run (or run a flow), confirm: a cluster-orb appears bottom-right, expands to step bubbles, a bubble peek shows a live log tail + progress, done bubbles flip green with Open-artifact/Transcript, the dock collapses on click-away, and idle CPU stays low when no run is active. Compare against the Claude-design reference.

---

## Self-Review (filled in)

**1. Spec coverage:**
- §1 bubble/peek/cluster visuals → Tasks 7, 8, 9 ✅
- §2 data model + reducer → Tasks 2, 3 ✅; §2 run_id backend fix → Task 1 ✅
- §3 module layout + wiring + tick gate + recent tray → Tasks 2-5, 9 ✅
- §4 backend → Task 1 ✅; cancel via existing `cancel_run` → Task 5 ✅
- §5 Palette theming → Task 6 (Kit slice colors) + all views use `Kit` ✅
- §6 animations → Task 8 ✅
- §7 scope (approval/spawn/tweaks OUT) → reducer ignores ApprovalRequested (Task 3), no spawn/tweaks anywhere ✅
- §8 testing → reducer/tone/parse unit-tested (Tasks 2,3,4); views build-verified + user-visual ✅

**2. Placeholder scan:** View tasks (7,8,9) intentionally reference the jsx for exact pixel styling + name codebase helpers (`icons::svg`, `agentd_calls::*`, `chat_insert::*`, `pulse()`) the implementer must grep-resolve — each is flagged inline with "grep the real path", not left as silent TODOs. These are real seams in an existing codebase, not deferrals.

**3. Type consistency:** `RunCluster`/`AgentBubble`/`BubbleState`/`ClusterStatus`/`AgentTone`/`ToneKey`/`RunEventView`/`ActivityState` names are consistent across Tasks 2-9. `Kit` slice fields (Task 6) match `AgentTone::color` usage (Task 2). `Message` variants (Tasks 4-5) match `update`/`dock` usage (Tasks 5,7). Build ordering: **Task 6 before Task 2's build** (noted in Task 2 Step 5).

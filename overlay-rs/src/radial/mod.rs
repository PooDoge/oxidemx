//! Application state + canvas Painter for the radial menu.
//!
//! `RadialState` is the model `iced::application(boot, update, view)`
//! drives. `Painter` is a per-frame snapshot that implements
//! `canvas::Program` and renders the wedges / icons / centre puck
//! into `iced::widget::canvas::Frame` (cairo-equivalent calls in
//! pure Rust).

use oxidemx_shared::{AnimationConfig, RadialPage, Slice, VisualSettings};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::anim::Tween;
use crate::render::icons::IconCache;
use crate::theme::ActiveTheme;

mod chat_threads;
mod pages;
mod painter;
mod state;
mod submenu;

pub use chat_threads::{load_chat_threads, now_secs, save_chat_threads, ChatMessage, ChatThread};
pub use pages::AI_PAGE_NAME;
pub use painter::Painter;
pub use submenu::{
    subitem_at, SubmenuState, SUBITEM_RENDER_RADIUS, SUBITEM_RENDER_SPREAD_DEG, SUBMENU_RADIUS,
};

/// Maximum press-to-release duration that still counts as a "tap" —
/// mirrors the Python overlay's TAP_THRESHOLD_MS = 250.
const TAP_THRESHOLD: Duration = Duration::from_millis(250);

#[allow(dead_code)]
const SLICE_DEGREES: f32 = 45.0;

/// Top-level model for the iced app. Owns everything view() needs.
pub struct RadialState {
    pub theme: ActiveTheme,
    /// Slices of the currently active page. Mirror of
    /// `pages[active_page].slices`, refreshed via
    /// [`Self::refresh_active_slices`] whenever `active_page`
    /// changes — keeps the painter / hit-test path unchanged from
    /// the single-page era.
    pub slices: Vec<Slice>,
    /// All pages (default page + optional app-context / scroll-cycle
    /// pages). After every reload this is guaranteed to contain at
    /// least one page (the loader's `normalize_pages` ensures it).
    pub pages: Vec<RadialPage>,
    /// Index into `pages` for the page currently being rendered.
    /// Driven by the scroll-wheel cycle (toggle mode) and by
    /// app-focus matching at `show()` time.
    pub active_page: usize,
    /// Cached list of page indices that participate in the
    /// scroll-wheel cycle (global pages + app-context pages with
    /// `include_in_scroll = true`). Rebuilt on every config reload
    /// so we don't recompute it per scroll event.
    pub cycle_pages_cache: Vec<usize>,
    /// Window class of the most recently focused window (or None).
    /// Set by the daemon's focus-tracking signal in Phase 2; today
    /// it stays None and `show()` falls back to the last-active
    /// page index.
    pub focused_class: Option<String>,

    // AI Assistant state fields
    /// Multi-line prompt editor contents (runtime-only).
    pub ai_editor: iced::widget::text_editor::Content,
    pub ai_loading: bool,
    /// All chat threads (always ≥ 1); `ai_active` indexes the one
    /// the conversation view shows. Loaded from / persisted to
    /// `~/.config/oxidemx/ai-chats.json`.
    pub ai_threads: Vec<ChatThread>,
    pub ai_active: usize,
    /// True while the chat shell shows the thread list instead of
    /// the active conversation.
    pub ai_show_threads: bool,
    pub ai_pending_question: Option<crate::ai_client::PendingQuestion>,
    /// In-flight streamed reply: `(thread idx, text so far)`. Only
    /// one request can be in flight (`ai_loading` gates submit).
    pub ai_stream: Option<(usize, String)>,
    /// The in-flight reply parsed as markdown, re-derived on each delta
    /// so the streaming bubble renders formatted live (instead of plain
    /// text that only reflows into markdown at completion). `markdown::
    /// view` borrows these items, so they must live in state.
    pub ai_stream_md: Vec<iced::widget::markdown::Item>,
    /// What the agent is doing right now, for the loading row.
    pub ai_activity: Option<String>,
    /// Abort handle for the in-flight request (Stop button).
    pub ai_abort: Option<iced::task::Handle>,
    /// Index (within the active thread's history) of the bubble the
    /// pointer is over — reveals its copy button.
    pub ai_hover_msg: Option<usize>,
    /// Bubble index whose right-click context menu is open (`None` =
    /// closed). The menu renders attached to that bubble.
    pub ai_context_menu: Option<usize>,
    /// A bubble switched into text-selection mode: `(bubble idx,
    /// editable content)`. While set, that bubble renders as a
    /// read-only `text_editor` so the pointer can select + Ctrl+C
    /// (markdown::view can't be selected). `None` = no bubble selecting.
    pub ai_select: Option<(usize, iced::widget::text_editor::Content)>,
    /// Whether the conversation is scrolled to (near) the bottom. When
    /// false, a "jump to latest" affordance appears and auto-scroll on
    /// new deltas is suppressed so reading earlier messages isn't
    /// yanked away.
    pub ai_chat_at_bottom: bool,
    /// Transient confirmation toast (e.g. "Copied"): `(label, shown
    /// at)`. A timer clears it ~1.6 s after the most recent trigger.
    pub ai_toast: Option<(String, std::time::Instant)>,
    /// Skills management panel open (mutually exclusive with the
    /// memories / tasks / threads panels).
    pub ai_show_skills: bool,
    /// Discovered skills (refreshed when the panel opens / palette opens).
    pub ai_skills: Vec<crate::agent::skills::Skill>,
    /// Names of currently-enabled skills (the agent's candidate pool).
    pub ai_skills_enabled: std::collections::HashSet<String>,
    /// Search filter for the skills panel.
    pub ai_skills_query: String,
    /// Open slash-command palette: `(selected index, filtered items)`.
    /// `None` when the input doesn't start with `/`.
    pub ai_palette: Option<(usize, Vec<crate::chat_ui::palette::PaletteItem>)>,
    /// Cached `(id, name)` of the user's flows, for the palette.
    pub ai_flow_cache: Vec<(String, String)>,
    /// Thread-rename in progress: `(thread idx, draft title)`.
    pub ai_renaming: Option<(usize, String)>,
    /// User-tweakable animation parameters for menu / submenu /
    /// slice highlight. Reloaded by the inotify watcher so the
    /// user can iterate on feel without restarting.
    pub anim_config: AnimationConfig,
    /// Static visual knobs (background opacity, slice highlight
    /// peak alpha) the user controls from the settings UI.
    pub visuals: VisualSettings,
    /// Per-slice hover tween in [0, 1]. Index `i` is slot `i`
    /// clockwise from the top.
    pub(crate) highlights: [Tween; 8],
    /// Currently-targeted slice (or `None` for centre/outside).
    /// Drives `highlights` via `set_target()` on transitions.
    target_slice: Option<usize>,
    /// Whole-menu open/close tween. 1.0 = fully visible, 0.0 =
    /// hidden. Replaces the old `visible: bool` so the painter
    /// can keep drawing during an exit fade. `is_drawable()` is
    /// the renderer's "render anything?" gate.
    pub(crate) menu: Tween,
    /// Per-overlay icon cache shared with the painter via `Rc`.
    icons: Rc<IconCache>,
    /// When the daemon's most recent `Show` arrived. Compared
    /// against the matching `Hide` to detect a quick tap (tap →
    /// toggle mode; otherwise drag-select fires the highlighted
    /// slice and closes).
    pub(crate) show_time: Option<Instant>,
    /// Toggle mode is on after a quick tap. The menu stays visible
    /// after the daemon's `Hide`, listens for OS mouse events
    /// (cursor move + clicks via `canvas::Program::update`), and
    /// only closes on click or escape.
    toggle_mode: bool,
    /// Open submenu state, or `None` when no Submenu slice is
    /// currently hovered. The painter reads this to render the
    /// pop-out arc; `dispatch_and_close` consults it before
    /// dispatching the parent so a hovered sub-item wins over the
    /// parent slice.
    pub(crate) submenu: Option<SubmenuState>,
    /// Wall-clock timestamp of the most recent `Tick`. The Tween
    /// runtime is duration-based so we need real-time deltas
    /// rather than assuming 16.67 ms per tick — keeps animations
    /// honest if the compositor stalls.
    last_tick: Option<Instant>,

    /// Page-cycle transition tween — drives the spin/crossfade
    /// animation when the user scrolls between pages. Sits at 0
    /// when no transition is in flight; ramps 0 → 1 over the
    /// configured duration each `cycle_page` invocation.
    pub(crate) page_transition: Tween,

    /// Snapshot of the previous page's slices, kept around for the
    /// duration of a page-cycle transition so the renderer can draw
    /// the outgoing ring fading/rotating away while the new ring
    /// fades/rotates in. Dropped the moment `page_transition`
    /// settles back to idle.
    pub(crate) previous_slices: Option<Vec<Slice>>,

    /// Sign of the most recent page-cycle direction (+1 = next, -1
    /// = previous). Drives the rotation direction of the spin
    /// animation so it matches the scroll-wheel motion. Carries no
    /// meaning when `previous_slices.is_none()`.
    pub(crate) page_transition_dir: f32,

    /// Wall-clock timestamp of the most recent successful
    /// `cycle_page` invocation. Used to debounce hi-res scroll
    /// wheel ticks — without this, a single light flick on the MX
    /// Master 4 advances several pages because the wheel emits
    /// many tick events per physical click.
    pub(crate) last_cycle: Option<Instant>,

    /// When the current `target_slice` was first hovered. Drives
    /// the tooltip dwell timer — the arced description text only
    /// renders once the user has dwelled on a slice for
    /// `visuals.tooltip_delay_ms`. Reset to `Some(now)` on every
    /// `target_slice` change, cleared to `None` when the user
    /// leaves all slices.
    pub(crate) target_slice_since: Option<Instant>,

    /// Last known pointer position relative to the menu centre, in
    /// the canvas coordinate space (+X right, +Y down, units = px
    /// of the 484×484 widget). Drag mode accumulates daemon
    /// `CursorMoved` deltas; toggle mode stamps OS cursor coords
    /// translated to centre-relative. Reset to `(0,0)` on menu
    /// open. Read by shaders that need cursor-aware lighting
    /// (`hover_tilt`).
    pub(crate) pointer_dx: f64,
    pub(crate) pointer_dy: f64,

    /// Timestamp of the most-recent haptic-ripple trigger.
    /// `Some(now)` → a ripple shader pass animates outward from
    /// the menu centre for `RIPPLE_DURATION_MS`. `None` once it
    /// settles. The advance loop clears it; haptic-fire callers
    /// stamp it.
    pub(crate) ripple_started: Option<Instant>,

    /// Active page-name transition. `Some` while the centre
    /// puck is showing the page name (with optional slide-in
    /// from a previous name). `None` once the full timeline has
    /// elapsed. Stamped on:
    ///   * page cycle (scroll wheel) — direction = ±1
    ///   * show() when target page != 0 — direction = 0
    ///   * apply_focused_class on mid-open swap — direction = 0
    ///
    /// Hover labels still take precedence — the transition only
    /// renders when the user isn't pointing at a slice.
    pub(crate) page_name_flash: Option<PageNameTransition>,

    /// Timestamp of the most-recent dispatch-burst trigger. Set
    /// alongside `dispatch_origin` whenever a slice fires. The
    /// advance loop clears the pair after `BURST_DURATION_MS`.
    pub(crate) dispatch_started: Option<Instant>,

    /// Slice index (0..slot_count) that fired the most-recent
    /// burst. The shader anchors its effect at this slice's
    /// centre angle so Sparks/Shockwave/Glow all read as
    /// "originating from the slice you just clicked".
    pub(crate) dispatch_origin: Option<usize>,
    pub window_id: Option<iced::window::Id>,

    /// Disc ↔ AI-chat morph tween. 0.0 = round menu, 1.0 = the
    /// arc-shell chat layout. Retargeted by `sync_ai_morph()`
    /// whenever the active page changes; geometry + crossfade
    /// ramps live in `crate::chat_shell`.
    pub(crate) ai_morph: Tween,

    /// Set when entering the AI page; the app layer's Tick consumes
    /// it once the chat widgets are mounted in the tree and issues a
    /// `focus_next()` so the text input is focused without a click
    /// (iced 0.14 can't focus a widget that isn't in the tree yet,
    /// and at page-change time the chat content hasn't faded in).
    pub chat_focus_pending: bool,

    /// Center-puck handoff phase. Armed when the page cycle lands
    /// on the AI page (wheel keeps cycling, chat is render-only);
    /// disarmed to `ChatActive` by a deliberate click / mouse-out
    /// gesture. Transition rules + tests live in `crate::handoff`.
    pub ai_handoff: crate::handoff::AiHandoff,

    /// Current window size in logical px. Created from the persisted
    /// `overlay.chat_size` (clamped by
    /// `chat_shell::effective_window_size`) and updated when a
    /// resize-grip drag commits or the compositor reports a resize.
    pub win_size: (f32, f32),

    /// Set on every compositor configure during a native grip
    /// resize; Tick persists `overlay.chat_size` once the stream
    /// has been quiet for a beat. Also drives the live `W × H`
    /// badge while Some.
    pub chat_size_pending_save: Option<(Instant, (f32, f32))>,

    /// Memories management view open (header brain button).
    pub ai_show_memories: bool,
    /// Live search filter for the memories view.
    pub ai_memories_query: String,
    /// Cached memory entries, refreshed when the view opens or an
    /// entry is pinned/deleted (the store is a small local JSON).
    pub ai_memories: Vec<crate::agent::memory::MemoryEntry>,
    /// Store size at last refresh, for the "12 KB" label.
    pub ai_memories_bytes: u64,

    /// Scheduled-tasks view open (header clock button).
    pub ai_show_tasks: bool,
    /// Cached task list, loaded async when the view opens / changes
    /// (systemctl round trips don't belong on the render path).
    pub ai_tasks: Vec<crate::agent::tasks::TaskInfo>,

    /// Latest widget sample + sparkline ring buffers, fed by the
    /// 1 s sampler subscription while the menu is drawable.
    pub widgets: WidgetData,

    /// Custom-widget replay store: the last validated `Scene` (+
    /// revision) per placed plugin instance, filed by
    /// `Message::WidgetHost`. The painter replays these on every
    /// frame — the frame path never calls wasm (spec §8).
    pub widget_scenes: std::collections::HashMap<
        oxidemx_widget_host::InstanceId,
        (oxidemx_widget_proto::Scene, u64),
    >,

    /// Instances the worker reported dead (3 strikes / load
    /// failure) with their last error. These render the dimmed
    /// fallback wedge (spec §9) until a Scene arrives again
    /// (rescan / config edit reloads them).
    pub widget_failed: std::collections::HashMap<oxidemx_widget_host::InstanceId, String>,

    /// Installed-widget digests from the worker's last registry
    /// scan, keyed by widget id. Source of the fallback wedge's
    /// icon; Plan 3's picker reads it too.
    pub widget_registry: std::collections::HashMap<String, oxidemx_widget_host::WidgetSummary>,

    /// Name of the page whose slices are snapshotted in
    /// `previous_slices` — keeps derived `<page-slug>.slotN`
    /// instance keys correct for the outgoing ring during a
    /// page-cycle transition. Lifetime mirrors `previous_slices`.
    pub(crate) previous_page_name: Option<String>,

    /// Vision-loop dev hook: set once the OXIDEMX_VISION_SHOT
    /// capture has been scheduled so it fires exactly once.
    pub vision_shot_taken: bool,
}

/// Live widget data for the Splice Widgets page. Sparkline ring
/// buffers cap at [`SPARK_LEN`] samples (~30 s of history at the
/// 1 s tick).
#[derive(Debug, Clone, Default)]
pub struct WidgetData {
    pub snap: crate::sampler::WidgetSnapshot,
    pub cpu_history: std::collections::VecDeque<f32>,
    pub net_history: std::collections::VecDeque<f32>,
}

/// Sparkline sample count.
pub const SPARK_LEN: usize = 30;

impl WidgetData {
    /// Fold a fresh snapshot in, advancing the sparkline buffers.
    pub fn apply(&mut self, snap: crate::sampler::WidgetSnapshot) {
        if let Some(cpu) = snap.cpu_percent {
            self.cpu_history.push_back(cpu);
            while self.cpu_history.len() > SPARK_LEN {
                self.cpu_history.pop_front();
            }
        }
        if let Some(down) = snap.net_down_mbps {
            self.net_history.push_back(down);
            while self.net_history.len() > SPARK_LEN {
                self.net_history.pop_front();
            }
        }
        self.snap = snap;
    }
}

/// How long a haptic-ripple shader pass animates from trigger to
/// fully-faded. ~400 ms feels physical without lingering past
/// the next likely haptic event (slice_change debounce ~50 ms,
/// menu_appear once per open).
pub const RIPPLE_DURATION_MS: u64 = 400;

/// One page-name swap animation in flight. Carries the previous
/// name (so the renderer can cross-slide it out), the current
/// name, and the cycle direction. `direction = 0` means a
/// "no-prev" flash (show()/apply_focused_class) — incoming name
/// just fades in without a directional slide.
#[derive(Debug, Clone)]
pub struct PageNameTransition {
    pub started_at: Instant,
    pub current_name: String,
    pub previous_name: Option<String>,
    /// +1 = forward (next page); -1 = backward (previous page);
    /// 0 = no slide (used for app-context swaps where there's
    /// no meaningful "from" page to slide out).
    pub direction: i32,
}

/// How long a dispatch-burst shader pass animates from trigger
/// to fully-faded. Slightly longer than the haptic ripple so the
/// burst lingers visibly even after the menu has begun fading
/// away — the user's last visual impression should be the
/// confirmation of their action.
pub const BURST_DURATION_MS: u64 = 600;

impl std::fmt::Debug for RadialState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadialState")
            .field("theme", &self.theme)
            .field("slices.len", &self.slices.len())
            .field("target_slice", &self.target_slice)
            .field("menu", &self.menu)
            .field("toggle_mode", &self.toggle_mode)
            .field("submenu", &self.submenu)
            .finish()
    }
}

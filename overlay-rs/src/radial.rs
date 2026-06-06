//! Application state + canvas Painter for the radial menu.
//!
//! `RadialState` is the model `iced::application(boot, update, view)`
//! drives. `Painter` is a per-frame snapshot that implements
//! `canvas::Program` and renders the wedges / icons / centre puck
//! into `iced::widget::canvas::Frame` (cairo-equivalent calls in
//! pure Rust).

use iced::widget::canvas::{self, Action, Frame, Geometry, Path};
use iced::{mouse, Color, Event, Point, Rectangle, Renderer, Theme};
use oxidemx_shared::{
    theme::parse_hex_rgba, ActionKind, AnimationConfig, AppConfig, ElementAnimation, RadialPage,
    Slice, VisualSettings,
};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::anim::Tween;
use crate::geometry::{Geometry as RadialGeometry, CENTER_ZONE_RADIUS, MENU_RADIUS, WINDOW_SIZE};
use crate::render::icons::IconCache;
use crate::theme::ActiveTheme;

/// Distance from the menu centre to a submenu sub-item's centre,
/// in logical pixels. Has to clear the outer ring so the popped-out
/// items sit beyond the main wedges. Mirrors the Python overlay's
/// `SUBMENU_RADIUS = MENU_RADIUS + 45`.
pub const SUBMENU_RADIUS: f32 = (MENU_RADIUS as f32) + 45.0;
/// Visible radius of each sub-item disc.
pub const SUBITEM_RENDER_RADIUS: f32 = 24.0;
/// Generous hit-test radius — slightly larger than render so the
/// items don't feel "spiky" when crossing between them.
pub const SUBITEM_HIT_RADIUS: f32 = 32.0;
/// Degrees between adjacent sub-items, measured from the menu centre.
/// Render uses 18°, hit-test uses 15° (matches Python overlay).
pub const SUBITEM_RENDER_SPREAD_DEG: f32 = 18.0;
pub const SUBITEM_HIT_SPREAD_DEG: f32 = 15.0;

/// Maximum press-to-release duration that still counts as a "tap" —
/// mirrors the Python overlay's TAP_THRESHOLD_MS = 250.
const TAP_THRESHOLD: Duration = Duration::from_millis(250);

#[allow(dead_code)]
const SLICE_DEGREES: f32 = 45.0;
const RING_OUTER_INSET: f32 = 6.0;
const RING_INNER_INSET: f32 = 6.0;
const ICON_BG_RADIUS: f32 = 26.0;

/// Open submenu state. `None` when the user isn't hovering a
/// Submenu-kind slice (the common case). Created when hover lands
/// on a slice with `kind == Submenu` and a non-empty `submenu` vec;
/// dropped (after an exit fade settles) when the cursor leaves both
/// the parent slice and the sub-item arc.
#[derive(Debug, Clone)]
pub struct SubmenuState {
    /// Index of the parent slice (0..7) that opened this submenu.
    pub parent: usize,
    /// 0.0 → 1.0 grow-out tween. Drives the per-item radius / scale
    /// / opacity in the renderer via `crate::anim::evaluate`. Speed
    /// + curve come from `AnimationConfig::submenu`.
    ///
    /// The tween's `duration_ms` is **extended** to fit the full
    /// chain (`duration + (item_count − 1) × stagger`) so that the
    /// last sub-item gets to finish its per-item animation before
    /// `is_idle()` short-circuits the clock. Per-item progress eval
    /// in `anim::evaluate_chain_item` still uses the configured
    /// per-item duration. See `anim::extended_chain_duration_ms`.
    pub progress: Tween,
    /// Currently-hovered sub-item index, or `None` for "between
    /// items, mouse over the parent wedge".
    pub highlighted: Option<usize>,
    /// Cached count of sub-items at open time. Stored so
    /// `begin_exit` can recompute the chain duration without
    /// re-borrowing the parent slice list.
    pub item_count: usize,
}

impl SubmenuState {
    fn new(parent: usize, item_count: usize, anim: &ElementAnimation) -> Self {
        let mut progress = Tween::at(0.0);
        let stagger = anim
            .chain
            .as_ref()
            .map(|c| c.stagger_ms as f32)
            .unwrap_or(0.0);
        let mut enter = anim.enter.clone();
        enter.duration_ms = crate::anim::extended_chain_duration_ms(
            enter.duration_ms,
            item_count,
            stagger,
        );
        progress.set_target(1.0, &enter);
        Self {
            parent,
            progress,
            highlighted: None,
            item_count,
        }
    }

    /// Trigger the exit tween with chain-extended duration so
    /// every sub-item gets to finish its individual exit
    /// animation. Use this instead of calling
    /// `progress.set_target(0.0, &anim.exit)` directly — the
    /// raw call would only run for `anim.exit.duration_ms`,
    /// freezing the late items mid-flight.
    pub fn begin_exit(&mut self, anim: &ElementAnimation) {
        let stagger = anim
            .chain
            .as_ref()
            .map(|c| c.stagger_ms as f32)
            .unwrap_or(0.0);
        let mut exit = anim.exit.clone();
        exit.duration_ms = crate::anim::extended_chain_duration_ms(
            exit.duration_ms,
            self.item_count,
            stagger,
        );
        self.progress.set_target(0.0, &exit);
    }
}

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
    cycle_pages_cache: Vec<usize>,
    /// Window class of the most recently focused window (or None).
    /// Set by the daemon's focus-tracking signal in Phase 2; today
    /// it stays None and `show()` falls back to the last-active
    /// page index.
    pub focused_class: Option<String>,
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

impl RadialState {
    pub fn new(config: &AppConfig) -> Self {
        let theme = ActiveTheme::resolve(&config.theme);
        let pages = pages_from_config(config);
        let cycle_pages_cache = config.radial_menu.cycle_pages();
        let slices = pages.first().map(|p| p.slices.clone()).unwrap_or_default();
        let anim_config = config.radial_menu.animation.clone();
        let visuals = config.radial_menu.visuals.clone();
        RadialState {
            theme,
            slices,
            pages,
            active_page: 0,
            cycle_pages_cache,
            focused_class: None,
            anim_config,
            visuals,
            highlights: [Tween::at(0.0); 8],
            target_slice: None,
            menu: Tween::at(0.0),
            icons: Rc::new(IconCache::new()),
            show_time: None,
            toggle_mode: false,
            submenu: None,
            last_tick: None,
            page_transition: Tween::at(1.0),
            previous_slices: None,
            page_transition_dir: 1.0,
            last_cycle: None,
            target_slice_since: None,
            pointer_dx: 0.0,
            pointer_dy: 0.0,
            ripple_started: None,
            dispatch_started: None,
            dispatch_origin: None,
            page_name_flash: None,
            window_id: None,
        }
    }

    /// Trigger a page-name transition. `previous_name` and
    /// `direction` describe the cycle that just happened:
    ///   * direction = ±1: incoming slides in from `±slide`,
    ///     outgoing slides out to `∓slide`.
    ///   * direction = 0: no slide; incoming just fades in.
    /// Reads the current page's name from `self.pages`.
    fn flash_page_name(&mut self, previous_name: Option<String>, direction: i32) {
        let current_name = self
            .pages
            .get(self.active_page)
            .map(|p| p.name.clone())
            .filter(|n| !n.trim().is_empty());
        let Some(current_name) = current_name else {
            // Page has no name → nothing to flash.
            self.page_name_flash = None;
            return;
        };
        let previous_name = previous_name.filter(|n| !n.trim().is_empty());
        self.page_name_flash = Some(PageNameTransition {
            started_at: Instant::now(),
            current_name,
            previous_name,
            direction,
        });
    }

    /// Kick off the haptic-ripple shader pass. Called from the
    /// app layer whenever a haptic event is dispatched (any of
    /// menu_appear, slice_change, page_change, submenu_open/close,
    /// confirm, invalid). Visual feedback synced to the physical
    /// motor.
    pub fn trigger_ripple(&mut self) {
        self.ripple_started = Some(Instant::now());
    }

    /// Kick off a dispatch-burst shader pass anchored at slice
    /// `slot_idx` (0..slot_count). Called from the app layer
    /// when the user actually fires a slice (drag-release or
    /// toggle-mode click that resolves to an action). Mirrors
    /// `trigger_ripple` but adds a slice anchor so the burst
    /// reads as originating from the activated wedge.
    pub fn trigger_dispatch_burst(&mut self, slot_idx: usize) {
        self.dispatch_started = Some(Instant::now());
        self.dispatch_origin = Some(slot_idx);
    }

    pub fn show(&mut self) {
        self.menu.set_target(1.0, &self.anim_config.menu.enter);
        self.show_time = Some(Instant::now());
        self.toggle_mode = false;
        // Pick the page based on the current focused-class cache.
        // The cache is repopulated by `apply_focused_class` from
        // the GNOME-extension query that fires alongside Show, so
        // the first frame may use a stale value; the swap-on-arrival
        // path handles the correction transparently during fade-in.
        //
        // When the cache is empty OR the class doesn't match any
        // app-context page, default to page 0 — gives the user a
        // predictable "menu reopened" baseline rather than leaving
        // them on whatever page the wheel last cycled to.
        let target = self
            .focused_class
            .as_deref()
            .and_then(|c| match_page_for_class(&self.pages, c))
            .unwrap_or(0);
        self.set_active_page(target);
        // Show the active page name in the centre puck on
        // app-context opens (i.e. when an app-specific page took
        // over). Default page (index 0) is the implicit "home" so
        // we don't flash for it — would feel like noise on every
        // open. No previous name + direction=0 → no slide, just
        // a fade-in.
        if target != 0 && self.visuals.page_name_show {
            self.flash_page_name(None, 0);
        } else {
            // Clear any stale flash from a prior session so the
            // centre doesn't briefly show an unrelated page name.
            self.page_name_flash = None;
        }
        // Reset any stale highlights from a previous show.
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.target_slice = None;
        self.target_slice_since = None;
        self.submenu = None;
        // Reset any in-flight page transition so a re-open after a
        // fast cycle doesn't paint an extra phantom ring.
        self.previous_slices = None;
        self.page_transition = Tween::at(1.0);
    }

    /// Update the cached focused window class and, when the menu
    /// is currently open, swap to the matching app-context page if
    /// one exists. Called by the GNOME-extension focus query that
    /// fires alongside every `Show` — the class typically arrives
    /// during the menu's open fade-in, so the page swap is hidden
    /// inside the entry animation.
    ///
    /// Idempotent — a second call with the same class is a no-op.
    /// Stays a no-op when the menu is closed (the cached class is
    /// still updated for the next `show()`, but no slices are
    /// rebuilt).
    pub fn apply_focused_class(&mut self, class: Option<String>) {
        // Normalise: empty strings collapse to None so callers
        // don't have to special-case them.
        let class = class.filter(|s| !s.is_empty());
        let unchanged = self.focused_class.as_deref() == class.as_deref();
        self.focused_class = class.clone();
        if unchanged {
            return;
        }
        if !self.is_open() {
            return;
        }
        // Resolve to a target page: matching app-context page if the
        // class hits one, else page 0. Mirrors `show()` so a Show
        // followed by a stale-then-fresh class arrives at the same
        // page state the next Show would.
        let target_page = class
            .as_deref()
            .and_then(|cls| match_page_for_class(&self.pages, cls))
            .unwrap_or(0);
        if target_page != self.active_page {
            // Drop hover state on the old page so the new page
            // doesn't briefly flash a slot the user wasn't aiming at.
            for a in &mut self.highlights {
                a.set_target(0.0, &self.anim_config.slice_highlight.exit);
            }
            self.target_slice = None;
            self.target_slice_since = None;
            self.submenu = None;
            self.set_active_page(target_page);
            // Mid-open page swap (focus class arrived after Show
            // and pointed at a non-default page) → flash the new
            // page's name so the user notices the auto-swap.
            // Skip when target = 0 since that's "default home".
            // No-slide because the swap wasn't user-initiated as
            // a cycle direction.
            if target_page != 0 && self.visuals.page_name_show {
                self.flash_page_name(None, 0);
            }
        }
    }

    /// Replace the active page index; refresh the cached `slices`
    /// snapshot so painter/hit-test see the new page. Clamps to a
    /// valid index when given garbage (defensive — keeps the
    /// overlay alive after a config reload deletes the active page).
    pub fn set_active_page(&mut self, idx: usize) {
        if self.pages.is_empty() {
            self.active_page = 0;
            self.slices.clear();
            return;
        }
        self.active_page = idx.min(self.pages.len() - 1);
        self.refresh_active_slices();
    }

    fn refresh_active_slices(&mut self) {
        self.slices = self
            .pages
            .get(self.active_page)
            .map(|p| p.slices.clone())
            .unwrap_or_default();
    }

    /// Cycle the active page by `direction` (+1 next, -1 previous)
    /// through `cycle_pages_cache`. No-op when fewer than two pages
    /// are in the cycle. Resets per-slice highlights / target so
    /// the new page starts in a clean visual state.
    pub fn cycle_page(&mut self, direction: i32) {
        if self.cycle_pages_cache.len() < 2 || direction == 0 {
            return;
        }
        // Debounce — hi-res scroll wheels send dozens of tick
        // events per physical click, and even regular wheels can
        // double-fire on a fast flick. The user-tunable knob caps
        // how close together two cycles can land; default 250 ms
        // still allows deliberate rapid-fire scrolling but
        // collapses accidental doubles into a single advance.
        let debounce_ms = self.anim_config.page_transition.cycle_debounce_ms;
        if debounce_ms > 0 {
            let now = Instant::now();
            if let Some(last) = self.last_cycle {
                if now.duration_since(last)
                    < Duration::from_millis(debounce_ms as u64)
                {
                    return;
                }
            }
            self.last_cycle = Some(now);
        }
        // Find where the current active page sits in the cycle
        // list. If the active page isn't in the cycle (an
        // app-context page reachable only via focus), step from
        // the start of the cycle in the requested direction.
        let n = self.cycle_pages_cache.len() as i32;
        let pos = self
            .cycle_pages_cache
            .iter()
            .position(|&i| i == self.active_page)
            .map(|p| p as i32)
            .unwrap_or(if direction > 0 { -1 } else { 0 });
        let next = ((pos + direction).rem_euclid(n)) as usize;
        let new_active = self.cycle_pages_cache[next];
        if new_active == self.active_page {
            return;
        }
        // Snapshot the outgoing page's name BEFORE we set
        // `active_page` so the page-name flash can cross-slide
        // it out as the new name slides in.
        let outgoing_name = self
            .pages
            .get(self.active_page)
            .map(|p| p.name.clone())
            .filter(|n| !n.trim().is_empty());
        // Snapshot the outgoing page's slices so the renderer can
        // draw both rings during the transition (old fading/spinning
        // out, new fading/spinning in). Dropped in
        // `advance_animations` once the tween settles.
        let pt_cfg = self.anim_config.page_transition.clone();
        let pt_style = pt_cfg.style;
        let animate = !matches!(pt_style, oxidemx_shared::PageTransitionStyle::None)
            && pt_cfg.duration_ms > 0;
        if animate {
            self.previous_slices = Some(self.slices.clone());
            self.page_transition_dir = (direction.signum()) as f32;
            // Reset to 0 so the eased value starts from "old fully
            // visible" each transition, regardless of where the
            // last one settled.
            self.page_transition = Tween::at(0.0);
            self.page_transition.set_target(1.0, &pt_cfg.as_transition_config());
        } else {
            // Animation disabled or duration 0: clear any stale
            // transition state and skip straight to the new page.
            self.previous_slices = None;
            self.page_transition = Tween::at(1.0);
        }
        // Drop hover state on the old page so the new page doesn't
        // light up a slot the user wasn't aiming at.
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.target_slice = None;
        self.target_slice_since = None;
        self.submenu = None;
        self.set_active_page(new_active);
        // Flash the page name on every scroll-cycle (including
        // back to the default page) — the user just performed
        // an explicit input gesture, so they want to see "where
        // did I land?" feedback. The cycle direction drives
        // the slide direction (next → old slides left, new in
        // from right; previous → reversed). Skipped when the
        // user has the page-name flash disabled in settings.
        if self.visuals.page_name_show {
            self.flash_page_name(outgoing_name, direction.signum());
        }
    }

    /// Number of pages eligible for the scroll cycle. Used by the
    /// painter to decide whether to render the page-indicator dots.
    pub fn cycle_page_count(&self) -> usize {
        self.cycle_pages_cache.len()
    }

    /// Position of the active page within the scroll cycle, or None
    /// if the active page isn't part of the cycle (app-context
    /// page with `include_in_scroll = false`). Drives the
    /// page-indicator dots in the centre puck.
    pub fn cycle_page_position(&self) -> Option<usize> {
        self.cycle_pages_cache
            .iter()
            .position(|&i| i == self.active_page)
    }

    /// True when the menu is currently visible OR mid-exit-fade.
    /// The painter uses this to keep drawing during the close
    /// animation; once it returns false the canvas can short-
    /// circuit to a clear frame.
    pub fn is_drawable(&self) -> bool {
        self.menu.current > 0.001 || self.menu.target > 0.001
    }

    /// True when the menu is logically "open" — the daemon hasn't
    /// hidden us yet (or we're in toggle mode). Used by hit-test +
    /// dispatch paths so a request that lands during the exit fade
    /// doesn't accidentally trigger an action.
    /// Currently-targeted slice slot (0..7), if any. Exposed so
    /// the app loop can detect target-change transitions and fire
    /// the slice-change haptic only on positive crossings.
    pub fn target_slice(&self) -> Option<usize> {
        self.target_slice
    }

    /// How many wedges the active page renders. Defaults to 8;
    /// users can configure this per page (clamped to 2..=8 by
    /// `RadialPage::effective_slot_count`).
    pub fn active_slot_count(&self) -> usize {
        self.pages
            .get(self.active_page)
            .map(|p| p.effective_slot_count() as usize)
            .unwrap_or(8)
    }

    pub fn is_open(&self) -> bool {
        self.menu.target > 0.5
    }

    /// Handle the daemon's `Hide` signal (gesture button release).
    ///
    /// Two paths:
    ///   * **Tap** (release within `TAP_THRESHOLD` of show, no
    ///     drag highlight) → enter toggle mode. The menu stays
    ///     visible; OS mouse events (forwarded by
    ///     `Painter::update`) drive the selection until the user
    ///     clicks or escapes.
    ///   * **Drag-select** → run the highlighted slice's action
    ///     and close.
    pub fn hide(&mut self) {
        let was_drag_select = self.target_slice.is_some();
        let dur = self.show_time.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
        if !was_drag_select && dur < TAP_THRESHOLD {
            // Tap detected — switch to toggle mode rather than closing.
            self.toggle_mode = true;
            tracing::debug!(?dur, "tap → entering toggle mode");
            return;
        }
        self.dispatch_and_close();
    }

    /// Toggle-mode click handler — fires the highlighted slice and
    /// closes the menu.
    pub fn click_select(&mut self) {
        self.dispatch_and_close();
    }

    /// Toggle-mode dismiss without dispatching (right click / Esc /
    /// click outside).
    pub fn dismiss(&mut self) {
        self.target_slice = None;
        self.target_slice_since = None;
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.menu.set_target(0.0, &self.anim_config.menu.exit);
        self.toggle_mode = false;
        // Trigger the submenu's exit fade rather than dropping it
        // immediately, so any in-flight pop-out gets to play out.
        // `begin_exit` extends the tween duration to cover the full
        // chain so late stagger items don't freeze mid-flight.
        if let Some(sub) = self.submenu.as_mut() {
            sub.begin_exit(&self.anim_config.submenu);
        }
    }

    fn dispatch_and_close(&mut self) {
        self.menu.set_target(0.0, &self.anim_config.menu.exit);
        // Submenu sub-item wins over the parent slice — if the user
        // released while hovering one, fire that. Falling back to
        // the parent only when no sub-item was hovered keeps the
        // muscle-memory single-press-to-AI flow alive.
        if let Some(sub) = self.submenu.take() {
            if let Some(child_idx) = sub.highlighted {
                if let Some(parent) = self.slices.get(sub.parent) {
                    if let Some(child) = parent.submenu.get(child_idx) {
                        let allowed = child
                            .visible_if
                            .as_ref()
                            .map(|c| c.eval())
                            .unwrap_or(true);
                        if allowed {
                            crate::actions::dispatch(child);
                        }
                    }
                }
                self.reset_after_dispatch();
                return;
            }
            // Submenu was open but no sub-item targeted — fall
            // through to the parent slice handling below.
        }
        let selected = self.target_slice;
        if let Some(idx) = selected {
            if let Some(slice) = self.slices.get(idx) {
                let visible = slice
                    .visible_if
                    .as_ref()
                    .map(|c| c.eval())
                    .unwrap_or(true);
                if visible {
                    crate::actions::dispatch(slice);
                }
            }
        }
        self.reset_after_dispatch();
    }

    fn reset_after_dispatch(&mut self) {
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.target_slice = None;
        self.target_slice_since = None;
        self.toggle_mode = false;
        if let Some(sub) = self.submenu.as_mut() {
            sub.begin_exit(&self.anim_config.submenu);
        }
    }

    /// True when the menu is in toggle mode and the canvas should
    /// react to OS-level mouse events.
    pub fn is_toggle_mode(&self) -> bool {
        self.toggle_mode
    }

    /// Update the highlight from a toggle-mode cursor position.
    /// `local_x`/`local_y` are widget-local pixels (the canvas's
    /// own coordinate space — `(0,0)` at the top-left of the
    /// 484×484 surface, centre at `(WINDOW_SIZE/2, WINDOW_SIZE/2)`).
    pub fn on_toggle_cursor(&mut self, local_x: f64, local_y: f64) {
        let dx = local_x - (WINDOW_SIZE / 2.0);
        let dy = local_y - (WINDOW_SIZE / 2.0);
        // Toggle mode is the only path that opens submenus —
        // matches the legacy overlay's mouseMoveEvent vs. drag
        // split (the gesture-button drag never pops the submenu).
        self.update_pointer(dx, dy, true);
    }

    /// Drag-mode delta from the daemon's CursorMoved signal.
    /// `dx, dy` are accumulated REL_X / REL_Y values from the
    /// gesture-button press point (NOT absolute screen coords).
    pub fn on_cursor_moved(&mut self, dx: i32, dy: i32) {
        // Drag-mode also opens submenus: dragging onto a Submenu
        // slice pops out its sub-items so the user can continue
        // the drag onto a sub-item and release to fire it. Visual
        // pace of the pop-out is governed by
        // `AnimationConfig.submenu.enter` so the user can dial it
        // up if it feels slow.
        self.update_pointer(dx as f64, dy as f64, true);
    }

    /// Shared cursor-update path: updates the highlighted slice and,
    /// when the pointer is over a submenu sub-item, the sub-item
    /// highlight too. Submenu state is opened automatically (only
    /// when `may_open_submenu` is true) when the pointer lands on a
    /// Submenu-kind slice with a non-empty `submenu` vec, and
    /// closed when the pointer leaves both the parent slice and
    /// the popped-out arc.
    fn update_pointer(&mut self, dx: f64, dy: f64, may_open_submenu: bool) {
        // Stamp the latest pointer position so cursor-aware
        // shaders (hover_tilt) can read it without going back
        // through the input layer. Same canvas-convention coords
        // as the rest of the radial code (+X right, +Y down).
        self.pointer_dx = dx;
        self.pointer_dy = dy;

        // 1. Resolve the new slice under the cursor (if any).
        //    Sub-items live just beyond the ring, but the parent
        //    slice is what determines "is the user still inside
        //    the menu's gravity well".
        let slot_count = self.active_slot_count();
        let raw_target = crate::input::slice_index_at(
            dx,
            dy,
            crate::geometry::CENTER_ZONE_RADIUS,
            crate::geometry::MENU_RADIUS,
            slot_count,
        );

        // 2. Submenu maintenance — runs FIRST so step 3 can use
        //    the resulting submenu state to compute the effective
        //    highlight target.
        //    a) An open submenu's highlight follows the cursor
        //       (or drops to None when over the parent wedge but
        //       not over an item).
        //    b) The submenu stays open as long as ANY of:
        //         - cursor is over a sub-item (highlighted),
        //         - cursor is still in the parent wedge,
        //         - cursor is past the outer ring (radius >
        //           MENU_RADIUS) — covers the transition band
        //           between the wedge edge (~150 px) and the
        //           sub-item hit zone (~163-227 px), where the
        //           cursor is genuinely *travelling* toward an
        //           item. Without this third predicate the
        //           submenu dies mid-flight and the user never
        //           reaches the sub-item they were aiming for.
        //       Only entering a *different* parent slice's wedge
        //       closes the submenu — matches the legacy Python
        //       overlay's "bias toward keeping the submenu open"
        //       rule.
        //    c) Landing on a fresh Submenu-kind slice opens its
        //       submenu and resets the grow animation.
        let mut should_close = false;
        let dist = (dx * dx + dy * dy).sqrt();
        if let Some(sub) = self.submenu.as_mut() {
            let hit = subitem_at(dx, dy, sub.parent, &self.slices);
            if hit.is_some() {
                sub.highlighted = hit;
            } else if raw_target == Some(sub.parent)
                || dist > crate::geometry::MENU_RADIUS
            {
                sub.highlighted = None;
            } else {
                should_close = true;
            }
        }
        // When we should_close: trigger an EXIT tween on the
        // submenu's progress so its sub-items animate out
        // (radius shrinks, opacity fades, scale pops down)
        // instead of vanishing instantly. `advance_animations`
        // drops the SubmenuState once the tween settles to 0.
        // Exception: if the cursor is moving directly into a
        // *different* Submenu wedge, we want the new submenu
        // to take over immediately rather than waiting for the
        // old one to finish exiting — drop the old, open the
        // new in step (c) below.
        let entering_new_submenu = should_close
            && raw_target
                .and_then(|i| self.slices.get(i))
                .map(|s| {
                    matches!(s.kind, ActionKind::Submenu)
                        && !s.submenu.is_empty()
                })
                .unwrap_or(false);
        if should_close {
            if entering_new_submenu {
                self.submenu = None;
            } else if let Some(sub) = self.submenu.as_mut() {
                sub.begin_exit(&self.anim_config.submenu);
                sub.highlighted = None;
            }
        }
        if may_open_submenu {
            if let Some(idx) = raw_target {
                if let Some(slice) = self.slices.get(idx) {
                    if matches!(slice.kind, ActionKind::Submenu)
                        && !slice.submenu.is_empty()
                    {
                        // Skip if the same submenu is already open
                        // and not exiting — `progress.target > 0.5`
                        // = "open or opening", < 0.5 = "exiting".
                        let already_open = self
                            .submenu
                            .as_ref()
                            .map(|s| {
                                s.parent == idx && s.progress.target > 0.5
                            })
                            .unwrap_or(false);
                        if !already_open {
                            let item_count = slice.submenu.len();
                            self.submenu = Some(SubmenuState::new(
                                idx,
                                item_count,
                                &self.anim_config.submenu,
                            ));
                        }
                    }
                }
            }
        }

        // 3. Effective highlight target. While a submenu is open
        //    AND not exiting, force the parent slot to be the
        //    highlight target — keeps the parent visually lit and
        //    its tooltip showing while the user navigates into the
        //    sub-item arc. Without this, target_slice goes None
        //    the moment the cursor crosses into a sub-item (which
        //    sits past MENU_RADIUS, outside the hit-test) and the
        //    parent's hover state collapses.
        let effective_target = self
            .submenu
            .as_ref()
            .filter(|s| s.progress.target > 0.5)
            .map(|s| s.parent)
            .or(raw_target);

        // Slice-highlight transition driven by effective_target.
        if effective_target != self.target_slice {
            if let Some(prev) = self.target_slice {
                self.highlights[prev]
                    .set_target(0.0, &self.anim_config.slice_highlight.exit);
            }
            if let Some(next) = effective_target {
                self.highlights[next]
                    .set_target(1.0, &self.anim_config.slice_highlight.enter);
            }
            self.target_slice = effective_target;
            // Reset the tooltip dwell timer on every target
            // change. Some(now) when entering a new slice, None
            // when leaving all slices.
            self.target_slice_since =
                if effective_target.is_some() { Some(Instant::now()) } else { None };
        }
    }

    /// Step every tween by real-time `dt_ms` (slice highlights,
    /// menu open/close, open-submenu grow). Cheap when nothing is
    /// animating because each `Tween::step` short-circuits on idle.
    pub fn advance_animations(&mut self) {
        let now = Instant::now();
        let dt_ms = match self.last_tick {
            Some(t) => {
                let dt = now.duration_since(t).as_secs_f32() * 1000.0;
                // Clamp to a sane range — long stalls (compositor
                // freeze, debugger pause) shouldn't fast-forward
                // animations through their entire window.
                dt.clamp(0.0, 100.0)
            }
            None => 0.0,
        };
        self.last_tick = Some(now);
        for a in &mut self.highlights {
            a.step(dt_ms);
        }
        self.menu.step(dt_ms);
        // Page-transition tween advances independently of menu /
        // submenu / highlights. When it settles, drop the cached
        // outgoing slices so the renderer falls back to the
        // single-ring fast path.
        self.page_transition.step(dt_ms);
        if self.page_transition.is_idle() && self.previous_slices.is_some() {
            self.previous_slices = None;
        }
        // Drop the ripple state once its duration has elapsed so
        // the shader stops running for nothing on subsequent
        // frames.
        if let Some(t) = self.ripple_started {
            if t.elapsed() >= Duration::from_millis(RIPPLE_DURATION_MS) {
                self.ripple_started = None;
            }
        }
        if let Some(t) = self.dispatch_started {
            if t.elapsed() >= Duration::from_millis(BURST_DURATION_MS) {
                self.dispatch_started = None;
                self.dispatch_origin = None;
            }
        }
        // Drop the page-name flash once its full timeline has
        // elapsed so we don't keep checking elapsed_ms on every
        // frame for nothing.
        if let Some(flash) = self.page_name_flash.as_ref() {
            let total_ms = (2 * self.visuals.page_name_transition_ms
                + self.visuals.page_name_visible_ms) as u64;
            if flash.started_at.elapsed().as_millis() as u64 >= total_ms {
                self.page_name_flash = None;
            }
        }
        let sub_settled_to_zero = match self.submenu.as_mut() {
            Some(sub) => {
                sub.progress.step(dt_ms);
                sub.progress.is_idle() && sub.progress.target <= 0.001
            }
            None => false,
        };
        if sub_settled_to_zero {
            // Exit fade finished — drop the submenu so the renderer
            // skips it and a future activation starts from progress 0.
            self.submenu = None;
        }
    }

    /// Refresh from a freshly-loaded config (called by the inotify
    /// watcher after the editor saves). Replaces theme + slices +
    /// animation + visual settings in place; in-flight tweens
    /// continue playing but pick up the new durations / easings on
    /// their next `set_target`. Drops any open submenu since slice
    /// indices may have changed.
    pub fn reload_from(&mut self, config: &AppConfig) {
        self.theme = ActiveTheme::resolve(&config.theme);
        self.pages = pages_from_config(config);
        self.cycle_pages_cache = config.radial_menu.cycle_pages();
        // Try to keep the same active page across reloads — if the
        // user just edited a slice on page 1, don't yank them back
        // to page 0. Falls back to 0 when the index is now stale
        // (page deleted, or out of range after a shrink).
        let keep = self.active_page;
        if keep < self.pages.len() {
            self.active_page = keep;
        } else {
            self.active_page = 0;
        }
        self.refresh_active_slices();
        self.anim_config = config.radial_menu.animation.clone();
        self.visuals = config.radial_menu.visuals.clone();
        for a in &mut self.highlights {
            *a = Tween::at(0.0);
        }
        self.target_slice = None;
        self.target_slice_since = None;
        self.submenu = None;
    }
}

/// Build the page list to seed `RadialState`. Mirrors the
/// loader's `normalize_pages` semantics so a config built in code
/// (e.g. `AppConfig::default()`) still yields a non-empty page
/// vec.
fn pages_from_config(config: &AppConfig) -> Vec<RadialPage> {
    let mut pages = config.radial_menu.pages.clone();
    if pages.is_empty() {
        // Defensive: should never happen post-`normalize_pages`,
        // but a hand-built `AppConfig::default()` (used by the
        // overlay's panic path) has pages.is_empty()==true.
        if !config.radial_menu.slices.is_empty() {
            pages.push(RadialPage {
                name: "Default".into(),
                slices: config.radial_menu.slices.clone(),
                app_classes: Vec::new(),
                include_in_scroll: true,
                slot_count: 8,
            });
        } else {
            pages.push(RadialPage::default());
        }
    }
    pages
}

/// Find the first page whose `app_classes` contains the given
/// focused window class (case-insensitive). Empty class returns
/// None so the caller keeps whatever page was previously active.
fn match_page_for_class(pages: &[RadialPage], class: &str) -> Option<usize> {
    if class.is_empty() {
        return None;
    }
    let lc = class.to_lowercase();
    pages.iter().position(|p| {
        p.app_classes.iter().any(|c| c.to_lowercase() == lc)
    })
}

/// Hit-test the sub-items of slot `parent`. Returns the index of the
/// sub-item whose centre is within `SUBITEM_HIT_RADIUS` of the
/// cursor, or `None`. `dx`/`dy` are the cursor offset from the menu
/// centre (same convention as `slice_index_at`).
///
/// Sub-items are arranged on an arc of radius `SUBMENU_RADIUS`
/// centred on the parent slice's bisector, with
/// `SUBITEM_HIT_SPREAD_DEG` between adjacent items. Mirrors
/// `_get_subitem_at_position` in the legacy Python overlay.
pub fn subitem_at(dx: f64, dy: f64, parent: usize, slices: &[Slice]) -> Option<usize> {
    let parent_slice = slices.get(parent)?;
    if parent_slice.submenu.is_empty() {
        return None;
    }
    let n = parent_slice.submenu.len() as f32;
    let parent_angle_deg = (parent as f32) * 45.0 - 90.0;
    for (i, _) in parent_slice.submenu.iter().enumerate() {
        let offset_deg = (i as f32 - (n - 1.0) / 2.0) * SUBITEM_HIT_SPREAD_DEG;
        let item_angle = (parent_angle_deg + offset_deg).to_radians();
        let item_x = SUBMENU_RADIUS * item_angle.cos();
        let item_y = SUBMENU_RADIUS * item_angle.sin();
        let dist = ((dx - item_x as f64).powi(2) + (dy - item_y as f64).powi(2)).sqrt();
        if dist < SUBITEM_HIT_RADIUS as f64 {
            return Some(i);
        }
    }
    None
}

/// Per-frame canvas state.
pub struct Painter<'a> {
    state: &'a RadialState,
}

impl<'a> Painter<'a> {
    pub fn new(state: &'a RadialState) -> Self {
        Self { state }
    }
}

impl<'a> canvas::Program<crate::app::Message> for Painter<'a> {
    type State = ();

    /// Forward toggle-mode mouse events as `Message`s the iced
    /// app loop dispatches into `RadialState`. We only react when
    /// the menu is in toggle mode — drag mode is driven entirely
    /// by the daemon's CursorMoved signals (see app.rs).
    fn update(
        &self,
        _canvas_state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<crate::app::Message>> {
        if !self.state.is_open() || !self.state.is_toggle_mode() {
            return None;
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let p = cursor.position_in(bounds)?;
                Some(Action::publish(crate::app::Message::ToggleCursor {
                    x: p.x as f64,
                    y: p.y as f64,
                }))
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                // Page cycling fires only when the cursor is sitting
                // over the centre puck — keeps the user from
                // accidentally swapping pages while drag-aiming a
                // slice. Drag mode skips this branch entirely
                // (drag has no real cursor; the daemon sends REL_X
                // / REL_Y deltas). With fewer than two cycle pages
                // the cycle is a no-op, so we don't publish.
                if self.state.cycle_page_count() < 2 {
                    return None;
                }
                let p = cursor.position_in(bounds)?;
                let dx = p.x as f64 - WINDOW_SIZE / 2.0;
                let dy = p.y as f64 - WINDOW_SIZE / 2.0;
                let dist_sq = dx * dx + dy * dy;
                if dist_sq > CENTER_ZONE_RADIUS * CENTER_ZONE_RADIUS {
                    return None;
                }
                let dy_scroll = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if dy_scroll.abs() < f32::EPSILON {
                    return None;
                }
                // Scroll up (positive y) → next page; scroll down →
                // previous page. Matches the way the radial reads
                // visually: pages "stack downward" with the active
                // page on top, so flicking the wheel up advances
                // through the stack (same direction the user's
                // finger is moving).
                let direction = if dy_scroll > 0.0 { 1 } else { -1 };
                Some(Action::publish(crate::app::Message::CyclePage(direction)))
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => match button {
                mouse::Button::Left => {
                    Some(Action::publish(crate::app::Message::ToggleClickSelect))
                }
                _ => Some(Action::publish(crate::app::Message::ToggleDismiss)),
            },
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
                if matches!(
                    key,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                ) {
                    Some(Action::publish(crate::app::Message::ToggleDismiss))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        if !self.state.is_drawable() {
            // Fully closed (no in-flight exit fade) — render a clear
            // frame and bail.
            return vec![frame.into_geometry()];
        }

        let geom = RadialGeometry::default();
        let center = Point::new(geom.cx as f32, geom.cy as f32);
        let palette = &self.state.theme.theme.colors;

        // Whole-menu visual modulation. `evaluate_composed` returns
        // a `ComposedTransform` carrying alpha + translate + rotate
        // + scale + flip-scale. The preset path (no custom_tracks)
        // produces `(alpha, scale)` matching the legacy
        // `Visual { scale, opacity }` and zero translate/rotate/flip,
        // so existing user configs render exactly as before.
        let menu_t = crate::anim::evaluate_composed(
            &self.state.menu,
            &self.state.anim_config.menu.enter,
            &self.state.anim_config.menu.exit,
        );
        let mscale = menu_t.scale.max(0.0);
        let mopacity = menu_t.alpha.clamp(0.0, 1.0);

        // Apply the menu-level translate / rotate / flip-scale to
        // the frame BEFORE any drawing. Identity transforms (the
        // preset path) bypass the matrix updates entirely. Scale
        // (uniform) keeps living in `mscale` so radii baked into
        // the wedge math still work without doubling up.
        crate::render::animation::apply_composed_transform(
            &mut frame,
            center,
            &menu_t,
        );

        // The user's static "background opacity" knob multiplies
        // into the halo + base wedge fills (everything that draws
        // the wheel substrate). Highlights/icons are gated by a
        // separate knob below.
        let bg_op = self.state.visuals.menu_background_opacity.clamp(0.0, 1.0);
        // SDF wedge spike: when the SDF layer is active, fade the
        // canvas-side wedge fills so the SDF is what the user
        // actually sees. `wfm` (wedge_fill_mul) feeds through
        // draw_ring_transformed → draw_slice → fill alphas. At
        // SDF intensity 0 this is 1.0 and the canvas behaves
        // exactly as it always has.
        let sdf_intensity = self
            .state
            .visuals
            .sdf_ring_intensity
            .clamp(0.0, 1.0);
        let wfm = 1.0 - sdf_intensity;
        let highlight_op = self
            .state
            .visuals
            .slice_highlight_opacity
            .clamp(0.0, 1.0);

        // Faint shadow halo so the disc reads against transparent
        // backgrounds.
        let halo = Path::circle(center, (MENU_RADIUS as f32 + 6.0) * mscale);
        frame.fill(
            &halo,
            Color::from_rgba(0.0, 0.0, 0.0, 0.35 * mopacity * bg_op),
        );

        let outer_r = ((MENU_RADIUS as f32) - RING_OUTER_INSET) * mscale;
        let inner_r = ((geom.center_radius as f32) + RING_INNER_INSET) * mscale;
        let icon_r = (geom.icon_radius as f32) * mscale;
        let icon_bg_r = ICON_BG_RADIUS * mscale;

        // Page-transition state: when previous_slices is Some we're
        // mid-cycle and need to render BOTH rings (old fading out,
        // new fading in). The tween's eased current value drives
        // the crossfade and any per-ring transform.
        //
        // Per-ring transforms are computed as `ComposedTransform`
        // values so the preset path and the custom-track path
        // share one renderer signature. When the user has set
        // custom tracks on `page_transition.animation.enter` /
        // `.exit`, those override the preset; otherwise the
        // preset's `style` (None / CrossfadeScale / SpinCrossfade
        // / CenterPulse / Flip / Dissolve / Plasma) feeds the
        // ComposedTransform fields directly.
        let pt_progress = self.state.page_transition.current.clamp(0.0, 1.0);
        let pt_active = self.state.previous_slices.is_some() && pt_progress < 1.0;
        let pt_cfg = &self.state.anim_config.page_transition;
        let pt_style = pt_cfg.style;
        let pt_dir = self.state.page_transition_dir;
        let pt_rot_max = pt_cfg.rotation_deg.to_radians();

        let pt_uses_custom_tracks = pt_cfg.animation.enter.is_custom()
            || pt_cfg.animation.exit.is_custom();

        let (old_t, new_t) = if !pt_active {
            // No transition in flight — incoming ring at rest, no
            // outgoing ring (its alpha is 0 so it won't render).
            let mut hidden = oxidemx_shared::ComposedTransform::IDENTITY;
            hidden.alpha = 0.0;
            (hidden, oxidemx_shared::ComposedTransform::IDENTITY)
        } else if pt_uses_custom_tracks {
            // Track-based: outgoing ring uses Exit semantics,
            // incoming uses Enter. Same tween drives both — just
            // evaluate twice with explicit directions.
            let new_t = crate::anim::evaluate_composed_for(
                &self.state.page_transition,
                &pt_cfg.animation.enter,
                oxidemx_shared::TransitionDirection::Enter,
            );
            let old_t = crate::anim::evaluate_composed_for(
                &self.state.page_transition,
                &pt_cfg.animation.exit,
                oxidemx_shared::TransitionDirection::Exit,
            );
            (old_t, new_t)
        } else {
            // Preset path. The shape of each style matches the
            // legacy code that returned (rot, scale, alpha) tuples —
            // we just lift them into ComposedTransform.
            use oxidemx_shared::ComposedTransform as CT;
            use oxidemx_shared::PageTransitionStyle as PTS;
            let p = pt_progress;
            let inv = 1.0 - p;
            let mut old_t = CT::IDENTITY;
            let mut new_t = CT::IDENTITY;
            match pt_style {
                PTS::None => {
                    old_t.alpha = 0.0;
                    new_t.alpha = 1.0;
                }
                PTS::CrossfadeScale => {
                    let scale_min = 0.92_f32;
                    old_t.scale = 1.0 + (scale_min - 1.0) * p; // 1.0 → 0.92
                    old_t.alpha = inv;
                    new_t.scale = scale_min + (1.0 - scale_min) * p;
                    new_t.alpha = p;
                }
                PTS::SpinCrossfade => {
                    old_t.rotate_rad = pt_rot_max * pt_dir * p;
                    old_t.alpha = inv;
                    new_t.rotate_rad = -pt_rot_max * pt_dir * inv;
                    new_t.alpha = p;
                }
                PTS::CenterPulse => {
                    // Slices don't move; centre puck pulses.
                    old_t.alpha = 0.0;
                    new_t.alpha = 1.0;
                }
                PTS::Dissolve | PTS::Plasma => {
                    // Shader styles paint their own thing on top
                    // via the page-fx layer; canvas does a plain
                    // crossfade underneath.
                    old_t.alpha = inv;
                    new_t.alpha = p;
                }
                PTS::Flip => {
                    // Card-flip around the Y axis. Old ring scales
                    // its X axis from 1 → 0 over the FIRST HALF of
                    // the transition (rotating 0° → 90°). New ring
                    // scales X from 0 → 1 over the SECOND HALF
                    // (rotating 90° → 0°). cos/sin of (p * π/2)
                    // gives the natural physical-flip feel.
                    let half_pi = std::f32::consts::FRAC_PI_2;
                    let cos_p = (p * half_pi).cos();
                    let sin_p = (p * half_pi).sin();
                    old_t.flip_scale = cos_p;
                    old_t.flip_axis = oxidemx_shared::Axis::Y;
                    old_t.alpha = cos_p;
                    new_t.flip_scale = sin_p;
                    new_t.flip_axis = oxidemx_shared::Axis::Y;
                    new_t.alpha = sin_p;
                }
            }
            (old_t, new_t)
        };

        // Outgoing ring — only when actively transitioning.
        if pt_active {
            if let Some(old_slices) = self.state.previous_slices.as_ref() {
                if old_t.alpha > 0.001 {
                    crate::render::slices::draw_ring_transformed(
                        &mut frame,
                        center,
                        inner_r,
                        outer_r,
                        icon_r,
                        icon_bg_r,
                        old_slices,
                        palette,
                        // No hover highlights on the outgoing ring.
                        &[0.0; 8],
                        mopacity,
                        bg_op,
                        highlight_op,
                        &self.state.icons,
                        &old_t,
                        None,
                        self.state.active_slot_count(),
                        wfm,
                    );
                }
            }
        }

        // Per-slot transforms drive the slice-highlight custom-track
        // path. Each slot's tween + slice_highlight enter/exit
        // animate independently, so the per-slot transform is
        // wrapped inside `draw_ring_transformed` per slice. With
        // no custom tracks the array is all-identity and the inner
        // loop skips the per-slot `with_save` entirely (None branch).
        let slot_transforms: [oxidemx_shared::ComposedTransform; 8] =
            self.state.highlights.map(|tween| {
                crate::anim::evaluate_composed(
                    &tween,
                    &self.state.anim_config.slice_highlight.enter,
                    &self.state.anim_config.slice_highlight.exit,
                )
            });
        let slice_uses_custom_tracks = self
            .state
            .anim_config
            .slice_highlight
            .enter
            .is_custom()
            || self.state.anim_config.slice_highlight.exit.is_custom();
        let slot_transforms_arg = if slice_uses_custom_tracks {
            Some(&slot_transforms)
        } else {
            None
        };

        // Incoming / current ring. Outside a transition `new_t` is
        // identity, so this is the normal single-ring path with
        // no extra transform cost.
        crate::render::slices::draw_ring_transformed(
            &mut frame,
            center,
            inner_r,
            outer_r,
            icon_r,
            icon_bg_r,
            &self.state.slices,
            palette,
            &self.state.highlights.map(|t| t.current),
            mopacity,
            bg_op,
            highlight_op,
            &self.state.icons,
            &new_t,
            slot_transforms_arg,
            self.state.active_slot_count(),
            wfm,
        );

        // Centre label + description: prefer the hovered submenu
        // item (deepest selection wins) → the hovered top-level
        // slice → nothing. Description is the slice's optional
        // longer-form notes field — when present it renders as a
        // smaller subtitle under the label inside the puck.
        let hovered_slice: Option<&oxidemx_shared::config::Slice> = self
            .state
            .submenu
            .as_ref()
            .and_then(|sub| {
                sub.highlighted.and_then(|child_idx| {
                    self.state
                        .slices
                        .get(sub.parent)
                        .and_then(|p| p.submenu.get(child_idx))
                })
            })
            .or_else(|| {
                self.state
                    .target_slice
                    .and_then(|i| self.state.slices.get(i))
            });
        // Centre-label resolution: hovered slice always wins (the
        // user's pointer intent is the strongest signal). When
        // no slice is hovered, the page-name flash takes the
        // centre — but it's drawn by `draw_page_name_transition`
        // *after* `draw_center` so the slide-in / cross-slide
        // animation happens out-of-band. Pass center_label=None
        // here when the flash is active so draw_center doesn't
        // also render a static label that fights with it.
        let page_name_active = self
            .state
            .page_name_flash
            .as_ref()
            .filter(|_| self.state.visuals.page_name_show)
            .map(|f| {
                let elapsed = f.started_at.elapsed().as_millis() as u64;
                let total_ms = (2 * self.state.visuals.page_name_transition_ms
                    + self.state.visuals.page_name_visible_ms)
                    as u64;
                elapsed < total_ms
            })
            .unwrap_or(false);
        let center_label: Option<String> = if let Some(s) = hovered_slice {
            Some(s.label.clone())
        } else if page_name_active {
            None
        } else {
            None
        };
        let center_label_alpha_mul: f32 = 1.0;
        // Description used to render as a centre-puck subtitle;
        // moved to an arced tooltip around the outer ring (see
        // below). Pass `None` so the puck stays clean when the
        // user is dwelling on a slice with notes.
        let center_description: Option<String> = None;
        // Tooltip description: pulled from `target_slice`, NOT
        // `hovered_slice`. With the submenu-keep-parent-lit
        // logic in `update_pointer`, target_slice stays the
        // parent slot while a submenu is open, so the tooltip
        // arc shows the parent's description while the user
        // navigates into sub-items. hovered_slice (which prefers
        // sub-item) drives the centre puck label instead.
        let hovered_description: Option<String> = self
            .state
            .target_slice
            .and_then(|i| self.state.slices.get(i))
            .map(|s| s.description.clone())
            .filter(|d| !d.trim().is_empty());

        // CenterPulse style: brief radius bulge + accent flash on
        // the puck during a page-cycle transition. Peaks at the
        // halfway point so the eye lands on the centre as the new
        // ring's first frame appears. No effect for other styles.
        let pulse_factor = if pt_active
            && matches!(
                pt_style,
                oxidemx_shared::PageTransitionStyle::CenterPulse
            ) {
            // Triangle wave: 0 at p=0, 1 at p=0.5, 0 at p=1.
            1.0 - (2.0 * pt_progress - 1.0).abs()
        } else {
            0.0
        };
        let center_radius =
            (geom.center_radius as f32) * mscale * (1.0 + 0.18 * pulse_factor);
        crate::render::slices::draw_center(
            &mut frame,
            center,
            center_radius,
            palette,
            mopacity,
            bg_op,
            center_label.as_deref(),
            center_description.as_deref(),
            self.state.visuals.center_label_size,
            crate::fonts::resolve(&self.state.visuals.font_family),
            pulse_factor,
            center_label_alpha_mul,
        );

        // Page-name transition: drawn AFTER the puck so the
        // slide-in / cross-slide-out happens on top of the
        // surface0 fill + accent rim. Skipped when the user is
        // hovering a slice (hover label takes priority) or when
        // the flash has elapsed / is disabled in settings.
        if hovered_slice.is_none() && page_name_active {
            if let Some(flash) = self.state.page_name_flash.as_ref() {
                let elapsed = flash.started_at.elapsed().as_millis() as u64;
                let v = &self.state.visuals;
                // Font resolution mirrors the arc-tooltip path:
                // monospace toggle wins (forces platform mono),
                // else use the page-name-specific override, else
                // inherit the menu font_family. Arc layouts
                // really need monospace because every angular
                // slot has uniform width — proportional fonts
                // create visible "gaps" around narrow glyphs.
                let font = if v.page_name_use_monospace {
                    iced::Font::MONOSPACE
                } else {
                    let family = if v.page_name_font_family.trim().is_empty() {
                        v.font_family.as_str()
                    } else {
                        v.page_name_font_family.as_str()
                    };
                    crate::fonts::resolve(family)
                };
                crate::render::slices::draw_page_name_transition(
                    &mut frame,
                    center,
                    center_radius,
                    palette,
                    mopacity,
                    &flash.current_name,
                    flash.previous_name.as_deref(),
                    flash.direction,
                    elapsed,
                    v.page_name_visible_ms,
                    v.page_name_transition_ms,
                    v.page_name_slide_distance_px,
                    v.center_label_size,
                    font,
                    v.page_name_arced,
                );
            }
        }

        // Page indicator dots — only appear when the menu has more
        // than one page in the scroll cycle. Sits inside the centre
        // puck, below any hover label.
        crate::render::slices::draw_page_indicator(
            &mut frame,
            center,
            center_radius,
            palette,
            mopacity,
            self.state.cycle_page_count(),
            self.state.cycle_page_position(),
        );

        // Submenu pop-out (drawn AFTER the centre so its sub-items
        // sit cleanly on top of the ring instead of being clipped
        // by the wedges they're popping out of). Custom-track
        // transforms apply to the submenu independently of the
        // menu's own transform — wrapping in `with_save` keeps
        // them scoped to this block.
        if let Some(sub) = self.state.submenu.as_ref() {
            let submenu_t = crate::anim::evaluate_composed(
                &sub.progress,
                &self.state.anim_config.submenu.enter,
                &self.state.anim_config.submenu.exit,
            );
            frame.with_save(|f| {
                crate::render::animation::apply_composed_transform(
                    f,
                    center,
                    &submenu_t,
                );
                crate::render::slices::draw_submenu(
                    f,
                    center,
                    sub,
                    &self.state.slices,
                    palette,
                    &self.state.anim_config.submenu,
                    mopacity * submenu_t.alpha.clamp(0.0, 1.0),
                    &self.state.icons,
                );
            });
        }

        // Arced tooltip — only when the user has dwelled on a
        // slice for at least `tooltip_delay_ms` AND that slice
        // has a non-empty description. Skip during page
        // transitions (the moving ring would drag the tooltip
        // along with it visually).
        if !pt_active {
            if let (Some(idx), Some(desc), Some(since)) = (
                self.state.target_slice,
                hovered_description.as_deref(),
                self.state.target_slice_since,
            ) {
                let elapsed_ms = since.elapsed().as_millis() as u32;
                let delay = self.state.visuals.tooltip_delay_ms;
                let font_size = self.state.visuals.tooltip_font_size;
                if elapsed_ms >= delay && font_size > 0.5 {
                    // Fade in over 150 ms once the delay expires.
                    const FADE_MS: f32 = 150.0;
                    let alpha = ((elapsed_ms - delay) as f32 / FADE_MS)
                        .clamp(0.0, 1.0);
                    let outer_r = ((MENU_RADIUS as f32) - RING_OUTER_INSET) * mscale;
                    let v = &self.state.visuals;
                    let font = if v.tooltip_use_monospace {
                        iced::Font::MONOSPACE
                    } else {
                        let family = if v.tooltip_font_family.trim().is_empty() {
                            v.font_family.as_str()
                        } else {
                            v.tooltip_font_family.as_str()
                        };
                        crate::fonts::resolve(family)
                    };
                    let bg_hex = palette
                        .lookup(&v.tooltip_bg_color)
                        .unwrap_or(palette.crust.as_str());
                    let fg_hex = palette
                        .lookup(&v.tooltip_text_color)
                        .unwrap_or(palette.text.as_str());
                    let (br, bg_g, bb, ba) =
                        oxidemx_shared::theme::parse_hex_rgba(bg_hex)
                            .unwrap_or((0.0, 0.0, 0.0, 1.0));
                    let (fr, fg_g, fb, fa) =
                        oxidemx_shared::theme::parse_hex_rgba(fg_hex)
                            .unwrap_or((1.0, 1.0, 1.0, 1.0));
                    let style = crate::render::slices::ArcTooltipStyle {
                        font,
                        monospace: v.tooltip_use_monospace,
                        fg: iced::Color::from_rgba(fr as f32, fg_g as f32, fb as f32, fa as f32),
                        bg: iced::Color::from_rgba(br as f32, bg_g as f32, bb as f32, ba as f32),
                        bg_alpha: v.tooltip_bg_alpha,
                    };
                    crate::render::slices::draw_arc_tooltip(
                        &mut frame,
                        center,
                        outer_r,
                        idx,
                        self.state.active_slot_count(),
                        desc,
                        palette,
                        mopacity,
                        alpha,
                        font_size * mscale,
                        style,
                    );
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

#[allow(dead_code)]
fn _link_window_size() -> u32 {
    WINDOW_SIZE as u32
}

#[allow(dead_code)]
fn _link_parse() -> Option<(f64, f64, f64, f64)> {
    parse_hex_rgba("#000000")
}

// Slice references kept private but visible to the painter via the
// state borrow above; this `_use` ensures cargo doesn't drop the
// type from the public surface when we add it elsewhere.
#[allow(dead_code)]
fn _slice_ref(_s: &Slice) {}

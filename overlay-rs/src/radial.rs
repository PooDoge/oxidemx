//! Application state + canvas Painter for the radial menu.
//!
//! `RadialState` is the model `iced::application(boot, update, view)`
//! drives. `Painter` is a per-frame snapshot that implements
//! `canvas::Program` and renders the wedges / icons / centre puck
//! into `iced::widget::canvas::Frame` (cairo-equivalent calls in
//! pure Rust).

use iced::widget::canvas::{self, Action, Frame, Geometry, Path};
use iced::{mouse, Color, Event, Point, Rectangle, Renderer, Theme};
use juhradial_shared::{
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
    pub progress: Tween,
    /// Currently-hovered sub-item index, or `None` for "between
    /// items, mouse over the parent wedge".
    pub highlighted: Option<usize>,
}

impl SubmenuState {
    fn new(parent: usize, anim: &ElementAnimation) -> Self {
        let mut progress = Tween::at(0.0);
        progress.set_target(1.0, &anim.enter);
        Self {
            parent,
            progress,
            highlighted: None,
        }
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
    highlights: [Tween; 8],
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
    show_time: Option<Instant>,
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
}

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
        }
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
        // Reset any stale highlights from a previous show.
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.target_slice = None;
        self.submenu = None;
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
            self.submenu = None;
            self.set_active_page(target_page);
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
        // Drop hover state on the old page so the new page doesn't
        // light up a slot the user wasn't aiming at.
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.target_slice = None;
        self.submenu = None;
        self.set_active_page(new_active);
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
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.menu.set_target(0.0, &self.anim_config.menu.exit);
        self.toggle_mode = false;
        // Trigger the submenu's exit fade rather than dropping it
        // immediately, so any in-flight pop-out gets to play out.
        if let Some(sub) = self.submenu.as_mut() {
            sub.progress.set_target(0.0, &self.anim_config.submenu.exit);
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
        self.toggle_mode = false;
        if let Some(sub) = self.submenu.as_mut() {
            sub.progress.set_target(0.0, &self.anim_config.submenu.exit);
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
        // 1. Resolve the new slice under the cursor (if any). We
        //    deliberately use the same hit-test as the no-submenu
        //    path — sub-items live just beyond the ring, but the
        //    parent slice is what determines "is the user still
        //    inside the menu's gravity well".
        let new_target = crate::input::slice_index_at(
            dx,
            dy,
            crate::geometry::CENTER_ZONE_RADIUS,
            crate::geometry::MENU_RADIUS,
        );

        // 2. Slice highlight transition (parent ring) — drives the
        //    per-slice tween via the configured highlight enter/exit.
        if new_target != self.target_slice {
            if let Some(prev) = self.target_slice {
                self.highlights[prev]
                    .set_target(0.0, &self.anim_config.slice_highlight.exit);
            }
            if let Some(next) = new_target {
                self.highlights[next]
                    .set_target(1.0, &self.anim_config.slice_highlight.enter);
            }
            self.target_slice = new_target;
        }

        // 3. Submenu maintenance.
        //    a) An open submenu's highlight follows the cursor (or
        //       drops to None when over the parent wedge but not
        //       over an item).
        //    b) The submenu stays open as long as ANY of:
        //         - cursor is over a sub-item (highlighted),
        //         - cursor is still in the parent wedge,
        //         - cursor is past the outer ring (radius >
        //           MENU_RADIUS) — covers the transition band
        //           between the wedge edge (~150 px) and the
        //           sub-item hit zone (~163-227 px), where the
        //           cursor is genuinely *travelling* toward an
        //           item. Without this third predicate the submenu
        //           dies mid-flight and the user never reaches the
        //           sub-item they were aiming for.
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
            } else if new_target == Some(sub.parent)
                || dist > crate::geometry::MENU_RADIUS
            {
                sub.highlighted = None;
            } else {
                should_close = true;
            }
        }
        if should_close {
            self.submenu = None;
        }
        if may_open_submenu && self.submenu.is_none() {
            if let Some(idx) = new_target {
                if let Some(slice) = self.slices.get(idx) {
                    if matches!(slice.kind, ActionKind::Submenu)
                        && !slice.submenu.is_empty()
                    {
                        self.submenu =
                            Some(SubmenuState::new(idx, &self.anim_config.submenu));
                    }
                }
            }
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
                // Scroll up (positive y) → previous page; scroll
                // down → next page. Matches typical browser-tab
                // wheel cycling.
                let direction = if dy_scroll > 0.0 { -1 } else { 1 };
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

        // Whole-menu visual modulation. `Visual.scale` becomes a
        // canvas transform around the centre; `Visual.opacity` is
        // multiplied into every fill alpha downstream. When the
        // menu animation is `None` (default), `menu_vis` reduces
        // to (1.0, 1.0) and the geometry below renders unchanged.
        let menu_vis = crate::anim::evaluate(
            &self.state.menu,
            &self.state.anim_config.menu.enter,
            &self.state.anim_config.menu.exit,
        );
        // iced's `Frame::scale_to` doesn't exist as a save/restore
        // primitive, so we apply scale manually by adjusting radii.
        // Keeping it to radii (not a true affine) keeps stroke widths
        // consistent — a "shrinking" exit looks like the disc itself
        // contracting toward the centre rather than the whole canvas
        // zooming.
        let mscale = menu_vis.scale.max(0.0);
        let mopacity = menu_vis.opacity.clamp(0.0, 1.0);

        // The user's static "background opacity" knob multiplies
        // into the halo + base wedge fills (everything that draws
        // the wheel substrate). Highlights/icons are gated by a
        // separate knob below.
        let bg_op = self.state.visuals.menu_background_opacity.clamp(0.0, 1.0);
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

        for i in 0..8 {
            let highlight = self.state.highlights[i].current;
            // Apply the slice's visible_if predicate. Slot stays
            // empty (we still draw an unfilled wedge so the ring is
            // continuous) when the predicate is false — preserves
            // muscle-memory layout, only hides the glyph.
            let slice_for_render = self.state.slices.get(i).filter(|s| {
                s.visible_if.as_ref().map(|c| c.eval()).unwrap_or(true)
            });
            crate::render::slices::draw_slice(
                &mut frame,
                center,
                inner_r,
                outer_r,
                icon_r,
                icon_bg_r,
                i,
                slice_for_render,
                palette,
                highlight,
                mopacity,
                bg_op,
                highlight_op,
                &self.state.icons,
            );
        }

        // Centre label: prefer the hovered submenu item (deepest
        // selection wins) → the hovered top-level slice → nothing.
        let center_label: Option<String> = self
            .state
            .submenu
            .as_ref()
            .and_then(|sub| {
                sub.highlighted.and_then(|child_idx| {
                    self.state
                        .slices
                        .get(sub.parent)
                        .and_then(|p| p.submenu.get(child_idx))
                        .map(|s| s.label.clone())
                })
            })
            .or_else(|| {
                self.state
                    .target_slice
                    .and_then(|i| self.state.slices.get(i))
                    .map(|s| s.label.clone())
            });

        let center_radius = (geom.center_radius as f32) * mscale;
        crate::render::slices::draw_center(
            &mut frame,
            center,
            center_radius,
            palette,
            mopacity,
            bg_op,
            center_label.as_deref(),
            self.state.visuals.center_label_size,
            crate::fonts::resolve(&self.state.visuals.font_family),
        );

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
        // by the wedges they're popping out of).
        if let Some(sub) = self.state.submenu.as_ref() {
            crate::render::slices::draw_submenu(
                &mut frame,
                center,
                sub,
                &self.state.slices,
                palette,
                &self.state.anim_config.submenu,
                mopacity,
                &self.state.icons,
            );
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

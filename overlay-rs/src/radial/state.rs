//! `RadialState` behaviour: construction, open/close lifecycle,
//! pointer targeting + submenu interaction, page cycling, chat
//! thread management, animation stepping, and config reload.

use std::rc::Rc;
use std::time::{Duration, Instant};

use oxidemx_shared::{ActionKind, AppConfig};

use crate::anim::Tween;
use crate::geometry::WINDOW_SIZE;
use crate::render::icons::IconCache;
use crate::theme::ActiveTheme;

use super::pages::{match_page_for_class, pages_from_config};
use super::{
    load_chat_threads, subitem_at, ChatThread, PageNameTransition, RadialState, SubmenuState,
    WidgetData, AI_PAGE_NAME, BURST_DURATION_MS, RIPPLE_DURATION_MS, TAP_THRESHOLD,
};

impl RadialState {
    pub fn new(config: &AppConfig) -> Self {
        let theme = ActiveTheme::resolve(&config.theme);
        let pages = pages_from_config(config);
        let mut cycle_pages_cache = config.radial_menu.cycle_pages();
        if !pages.is_empty() {
            cycle_pages_cache.push(pages.len() - 1);
        }
        let slices = pages.first().map(|p| p.slices.clone()).unwrap_or_default();
        let anim_config = config.radial_menu.animation.clone();
        let ai_threads = load_chat_threads();
        let visuals = config.radial_menu.visuals.clone();
        RadialState {
            theme,
            slices,
            pages,
            active_page: 0,
            cycle_pages_cache,
            focused_class: None,
            ai_editor: iced::widget::text_editor::Content::new(),
            ai_loading: false,
            // Start on the freshly-appended empty thread (always
            // last — load_chat_threads guarantees ≥ 1).
            ai_active: ai_threads.len() - 1,
            ai_threads,
            ai_show_threads: false,
            ai_pending_question: None,
            ai_stream: None,
            ai_stream_md: Vec::new(),
            ai_context_menu: None,
            ai_select: None,
            ai_chat_at_bottom: true,
            ai_scroll_tween: Tween::at(1.0),
            ai_scroll_active: false,
            ai_toast: None,
            ai_show_skills: false,
            ai_skills: Vec::new(),
            ai_skills_enabled: std::collections::HashSet::new(),
            ai_skills_query: String::new(),
            ai_palette: None,
            ai_flow_cache: Vec::new(),
            ai_threads_query: String::new(),
            ai_attachment: None,
            ai_image_cache: std::collections::HashMap::new(),
            ai_turn_tokens: (0, 0),
            ai_card_expanded: std::collections::HashSet::new(),
            ai_artifact_expanded: std::collections::HashSet::new(),
            ai_artifact_cache: std::collections::HashMap::new(),
            ai_lightbox: None,
            ai_activity: None,
            ai_abort: None,
            ai_hover_msg: None,
            ai_renaming: None,
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
            ai_morph: Tween::at(0.0),
            chat_focus_pending: false,
            ai_handoff: crate::handoff::AiHandoff::Inactive,
            win_size: {
                let s = crate::chat_shell::effective_window_size(config.overlay.chat_size);
                (s.width, s.height)
            },
            chat_size_pending_save: None,
            ai_show_memories: false,
            ai_memories_query: String::new(),
            ai_memories: Vec::new(),
            ai_memories_bytes: 0,
            ai_show_tasks: false,
            ai_tasks: Vec::new(),
            widgets: WidgetData::default(),
            widget_scenes: std::collections::HashMap::new(),
            widget_failed: std::collections::HashMap::new(),
            widget_registry: std::collections::HashMap::new(),
            previous_page_name: None,
            vision_shot_taken: false,
            use_agentd: config.overlay.ai.use_agentd,
            ai_agentd_approval: None,
            chat_window_mode: false,
            activity: crate::activity::ActivityState::default(),
        }
    }

    /// `InstanceId` for slot `idx` of the active page, if it holds a
    /// `WidgetSource::Custom` slice. Derivation MUST match the
    /// worker's (`desired_instances`): explicit `instance_key` wins,
    /// otherwise `<page-slug>.slot<N>` from the shared helper.
    pub(crate) fn custom_instance_id(&self, idx: usize) -> Option<oxidemx_widget_host::InstanceId> {
        let slice = self.slices.get(idx)?;
        let w = slice.widget.as_ref()?;
        let oxidemx_shared::WidgetSource::Custom(widget_id) = &w.source else {
            return None;
        };
        let page = self
            .pages
            .get(self.active_page)
            .map(|p| p.name.as_str())
            .unwrap_or("Default");
        let instance_key = w
            .instance_key
            .clone()
            .unwrap_or_else(|| oxidemx_shared::widgets::instance_key(page, idx));
        Some(oxidemx_widget_host::InstanceId {
            instance_key,
            widget_id: widget_id.clone(),
        })
    }

    /// Live wedge geometry for slot `idx` — painter layout math at
    /// rest scale plus the slot's current hover-tween progress.
    pub(crate) fn widget_geom(&self, idx: usize) -> oxidemx_widget_proto::WedgeGeom {
        let hovered = self.highlights.get(idx).map(|t| t.current).unwrap_or(0.0);
        crate::widget_host::wedge_geom_for_slot(idx, self.active_slot_count(), hovered)
    }

    /// Forward a pointer event on slot `idx` to the host worker iff
    /// the slot is a Custom-widget slice. Non-blocking (`try_send`
    /// inside `widget_host::send`); no-op for every other slice.
    pub(crate) fn send_widget_slice_event(&self, idx: usize, ev: oxidemx_widget_host::SliceEvent) {
        let Some(instance) = self.custom_instance_id(idx) else {
            return;
        };
        crate::widget_host::send(oxidemx_widget_host::HostCtl::Slice {
            instance,
            ev,
            geom: self.widget_geom(idx),
        });
    }

    /// Trigger a page-name transition. `previous_name` and
    /// `direction` describe the cycle that just happened:
    ///   * direction = ±1: incoming slides in from `±slide`,
    ///     outgoing slides out to `∓slide`.
    ///   * direction = 0: no slide; incoming just fades in.
    ///
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
        // Every Show starts as a clean disc — any chat-shell morph
        // from the previous session is hard-reset.
        self.ai_morph = Tween::at(0.0);
        self.chat_focus_pending = false;
        self.ai_handoff.reset();
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
        self.previous_page_name = None;
        self.page_transition = Tween::at(1.0);
        // Wake the widget host: resets the closed-menu timer latches
        // and hands every guest its MenuOpened "refresh if stale"
        // hook (spec §8).
        crate::widget_host::send(oxidemx_widget_host::HostCtl::MenuOpened {
            page: self
                .pages
                .get(self.active_page)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Default".into()),
        });
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
        self.sync_ai_morph();
    }

    /// The active chat thread. Total — `ai_threads` is never empty
    /// and the index is clamped defensively.
    pub fn chat(&self) -> &ChatThread {
        let i = self.ai_active.min(self.ai_threads.len() - 1);
        &self.ai_threads[i]
    }

    pub fn chat_mut(&mut self) -> &mut ChatThread {
        let i = self.ai_active.min(self.ai_threads.len() - 1);
        &mut self.ai_threads[i]
    }

    /// Set a thread's `working` flag and mirror it onto `ai_loading`
    /// when the thread is the currently-active one.
    pub fn set_thread_working(&mut self, idx: usize, on: bool) {
        if let Some(t) = self.ai_threads.get_mut(idx) {
            t.working = on;
        }
        self.ai_loading = crate::radial::chat_threads::mirror_loading(
            self.ai_active, idx, on, self.ai_loading,
        );
    }

    /// Start a fresh conversation. Reuses the current thread when
    /// it's still empty (no stacking of blank threads); otherwise
    /// appends a new one and switches to it.
    pub fn ai_new_chat(&mut self) {
        if !self.chat().history.is_empty() {
            self.ai_threads.push(ChatThread::default());
            self.ai_active = self.ai_threads.len() - 1;
        }
        self.ai_loading = self.ai_threads
            .get(self.ai_active)
            .map(|t| t.working)
            .unwrap_or(false);
        self.ai_show_threads = false;
        self.ai_show_memories = false;
        self.ai_show_tasks = false;
        self.ai_pending_question = None;
        self.ai_editor = iced::widget::text_editor::Content::new();
        self.ai_hover_msg = None;
        self.ai_renaming = None;
    }

    /// Switch the conversation view to thread `idx`.
    pub fn ai_select_chat(&mut self, idx: usize) {
        if idx < self.ai_threads.len() {
            self.ai_active = idx;
        }
        self.ai_loading = self.ai_threads
            .get(self.ai_active)
            .map(|t| t.working)
            .unwrap_or(false);
        self.ai_show_threads = false;
        self.ai_show_memories = false;
        self.ai_show_tasks = false;
        self.ai_pending_question = None;
        self.ai_hover_msg = None;
        self.ai_renaming = None;
    }

    /// Delete thread `idx`, keeping the invariant `ai_threads ≥ 1`
    /// and a valid `ai_active` (deleting the active thread lands on
    /// a fresh empty one).
    pub fn ai_delete_thread(&mut self, idx: usize) {
        if idx >= self.ai_threads.len() {
            return;
        }
        self.ai_threads.remove(idx);
        if self.ai_threads.is_empty() {
            self.ai_threads.push(ChatThread::default());
        }
        if self.ai_active >= self.ai_threads.len() {
            self.ai_active = self.ai_threads.len() - 1;
        } else if idx < self.ai_active {
            self.ai_active -= 1;
        }
        self.ai_renaming = None;
        self.ai_hover_msg = None;
    }

    /// Retarget the disc ↔ chat morph to match the active page.
    /// Entering the AI page also forces toggle mode — the chat
    /// needs a real cursor and keyboard, and there's no slice to
    /// drag-select anyway.
    fn sync_ai_morph(&mut self) {
        let on_ai = self.is_ai_page();
        if on_ai && self.ai_morph.target < 0.5 {
            self.ai_morph
                .set_target(1.0, &self.anim_config.ai_morph.enter);
            self.toggle_mode = true;
            // Landing on the AI page arms the puck instead of
            // activating the chat: wheel input keeps cycling pages
            // and the chat must not grab keyboard focus (no caret).
            // `chat_focus_pending` is set by `activate_chat()` when
            // the user disarms with a deliberate click / mouse-out.
            self.ai_handoff = crate::handoff::AiHandoff::armed(Instant::now());
            self.chat_focus_pending = false;
        } else if !on_ai && self.ai_morph.target > 0.5 {
            self.ai_morph
                .set_target(0.0, &self.anim_config.ai_morph.exit);
            self.chat_focus_pending = false;
            self.ai_handoff.reset();
        }
    }

    /// Disarm the puck → the chat becomes the real interaction
    /// target: keyboard focus moves to the input (via the Tick
    /// consumer of `chat_focus_pending`), wheel over the body
    /// scrolls the conversation, and only the header puck keeps
    /// cycling pages.
    pub fn activate_chat(&mut self) {
        if self.ai_handoff.is_armed() {
            self.ai_handoff.activate();
            self.chat_focus_pending = true;
        }
    }

    /// Feed a window-local click into the armed handoff. Clicks
    /// outside the travelling puck's hit circle activate the chat;
    /// clicks on the puck leave it armed (it's the page-cycle
    /// affordance).
    pub fn handoff_click(&mut self, x: f64, y: f64) {
        use crate::handoff::{HEADER_PUCK_HIT_SLOP, P};
        let (pc, pr) = crate::chat_shell::puck_geom(
            self.ai_morph_progress(),
            self.win_size.0,
            self.win_size.1,
        );
        if self.ai_handoff.on_click(
            P::new(x as f32, y as f32),
            P::new(pc.x, pc.y),
            pr + HEADER_PUCK_HIT_SLOP,
        ) {
            self.chat_focus_pending = true;
        }
    }

    /// Feed a window-local pointer position into the armed handoff.
    /// Activates the chat when the motion satisfies the deliberate-
    /// travel rule (see `crate::handoff`).
    pub fn handoff_pointer(&mut self, x: f64, y: f64) {
        use crate::handoff::P;
        // The disc is anchored in the window's top-left 484 px
        // square — its centre is fixed regardless of window size.
        let disc_center = P::new(
            crate::geometry::CENTER as f32,
            crate::geometry::CENTER as f32,
        );
        if self.ai_handoff.on_pointer(
            P::new(x as f32, y as f32),
            disc_center,
            crate::geometry::CENTER_ZONE_RADIUS as f32,
        ) {
            self.chat_focus_pending = true;
        }
    }

    /// True when the active page is the auto-appended AI Assistant
    /// page (the one the chat shell morphs over).
    pub fn is_ai_page(&self) -> bool {
        self.pages
            .get(self.active_page)
            .map(|p| p.name == AI_PAGE_NAME)
            .unwrap_or(false)
    }

    /// Current disc ↔ chat morph progress (0 = disc, 1 = chat).
    pub fn ai_morph_progress(&self) -> f32 {
        self.ai_morph.current
    }

    /// Whole-menu open/close alpha — multiplied into the chat
    /// shell so dismiss-from-chat fades everything together.
    pub fn menu_open_alpha(&self) -> f32 {
        self.menu.current.clamp(0.0, 1.0)
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
                if now.duration_since(last) < Duration::from_millis(debounce_ms as u64) {
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
            // Keep the outgoing ring's page name so its custom
            // widgets still derive the right instance keys mid-spin.
            self.previous_page_name = self.pages.get(self.active_page).map(|p| p.name.clone());
            self.page_transition_dir = (direction.signum()) as f32;
            // Reset to 0 so the eased value starts from "old fully
            // visible" each transition, regardless of where the
            // last one settled.
            self.page_transition = Tween::at(0.0);
            self.page_transition
                .set_target(1.0, &pt_cfg.as_transition_config());
        } else {
            // Animation disabled or duration 0: clear any stale
            // transition state and skip straight to the new page.
            self.previous_slices = None;
            self.previous_page_name = None;
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
        // Open → closed transition only (dismiss() can be re-entered
        // by focus churn while already closed) — relaxes the widget
        // host's timers (spec §8 closed-menu latch).
        if self.menu.target > 0.5 {
            crate::widget_host::send(oxidemx_widget_host::HostCtl::MenuClosed);
        }
        self.target_slice = None;
        self.target_slice_since = None;
        for a in &mut self.highlights {
            a.set_target(0.0, &self.anim_config.slice_highlight.exit);
        }
        self.menu.set_target(0.0, &self.anim_config.menu.exit);
        self.toggle_mode = false;
        self.ai_handoff.reset();
        self.chat_focus_pending = false;
        // Trigger the submenu's exit fade rather than dropping it
        // immediately, so any in-flight pop-out gets to play out.
        // `begin_exit` extends the tween duration to cover the full
        // chain so late stagger items don't freeze mid-flight.
        if let Some(sub) = self.submenu.as_mut() {
            sub.begin_exit(&self.anim_config.submenu);
        }
    }

    fn dispatch_and_close(&mut self) {
        if self.menu.target > 0.5 {
            crate::widget_host::send(oxidemx_widget_host::HostCtl::MenuClosed);
        }
        self.menu.set_target(0.0, &self.anim_config.menu.exit);
        // Submenu sub-item wins over the parent slice — if the user
        // released while hovering one, fire that. Falling back to
        // the parent only when no sub-item was hovered keeps the
        // muscle-memory single-press-to-AI flow alive.
        if let Some(sub) = self.submenu.take() {
            if let Some(child_idx) = sub.highlighted {
                if let Some(parent) = self.slices.get(sub.parent) {
                    if let Some(child) = parent.submenu.get(child_idx) {
                        let allowed = child.visible_if.as_ref().map(|c| c.eval()).unwrap_or(true);
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
                let visible = slice.visible_if.as_ref().map(|c| c.eval()).unwrap_or(true);
                if visible {
                    // Custom-widget wedges route the click to the
                    // plugin instead of the (empty) command path.
                    match self.custom_instance_id(idx) {
                        Some(instance) => crate::actions::dispatch_custom_widget(
                            slice,
                            instance,
                            self.widget_geom(idx),
                        ),
                        None => crate::actions::dispatch(slice),
                    }
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

    /// Vision-harness hook (`OXIDEMX_VISION_HOVER=<slot>`): force a
    /// fully-dwelled hover on `idx` so screenshots can capture
    /// hover-only chrome (tooltip arc, weather popup) without a real
    /// pointer. Backdates the dwell timer past any tooltip delay.
    pub(crate) fn vision_force_hover(&mut self, idx: usize) {
        if idx >= self.active_slot_count() {
            return;
        }
        self.highlights[idx].set_target(1.0, &self.anim_config.slice_highlight.enter);
        self.target_slice = Some(idx);
        self.target_slice_since = Some(Instant::now() - std::time::Duration::from_secs(10));
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
            } else if raw_target == Some(sub.parent) || dist > crate::geometry::MENU_RADIUS {
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
                .map(|s| matches!(s.kind, ActionKind::Submenu) && !s.submenu.is_empty())
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
                    if matches!(slice.kind, ActionKind::Submenu) && !slice.submenu.is_empty() {
                        // Skip if the same submenu is already open
                        // and not exiting — `progress.target > 0.5`
                        // = "open or opening", < 0.5 = "exiting".
                        let already_open = self
                            .submenu
                            .as_ref()
                            .map(|s| s.parent == idx && s.progress.target > 0.5)
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
                self.highlights[prev].set_target(0.0, &self.anim_config.slice_highlight.exit);
                // Custom-widget hover leave (no-op for other slices).
                self.send_widget_slice_event(prev, oxidemx_widget_host::SliceEvent::Hover(false));
            }
            if let Some(next) = effective_target {
                self.highlights[next].set_target(1.0, &self.anim_config.slice_highlight.enter);
                // Custom-widget hover enter — carries live geometry
                // so the guest's next render uses real wedge bounds.
                self.send_widget_slice_event(next, oxidemx_widget_host::SliceEvent::Hover(true));
            }
            self.target_slice = effective_target;
            // Reset the tooltip dwell timer on every target
            // change. Some(now) when entering a new slice, None
            // when leaving all slices.
            self.target_slice_since = if effective_target.is_some() {
                Some(Instant::now())
            } else {
                None
            };
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
        self.ai_morph.step(dt_ms);
        // Eased "↓ Latest" auto-scroll; the Tick handler reads
        // `ai_scroll_tween.current` and emits the per-frame snap_to.
        self.ai_scroll_tween.step(dt_ms);
        // Page-transition tween advances independently of menu /
        // submenu / highlights. When it settles, drop the cached
        // outgoing slices so the renderer falls back to the
        // single-ring fast path.
        self.page_transition.step(dt_ms);
        if self.page_transition.is_idle() && self.previous_slices.is_some() {
            self.previous_slices = None;
            self.previous_page_name = None;
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
        if !self.pages.is_empty() {
            self.cycle_pages_cache.push(self.pages.len() - 1);
        }
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
        // Keep the chat morph consistent with whatever page survived
        // the reload (the AI page is re-appended by pages_from_config,
        // so an open chat stays open across config edits).
        self.sync_ai_morph();
        for a in &mut self.highlights {
            *a = Tween::at(0.0);
        }
        self.target_slice = None;
        self.target_slice_since = None;
        self.submenu = None;
        self.use_agentd = config.overlay.ai.use_agentd;
    }
}

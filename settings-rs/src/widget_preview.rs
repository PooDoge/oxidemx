//! Live wedge preview for the widget options card (Plan 3 Task 5,
//! spec §10d "live preview" — options-card only; picker-tile minis
//! stay static in v1).
//!
//! While the options card is open for a Ready custom widget, a real
//! `oxidemx_widget_host::worker` runs in the background against the
//! REAL widgets dir and a synthetic one-page/one-slice `AppConfig`
//! for just the selected instance. Every option edit re-resolves the
//! settings bag; when it changes, the worker gets a `ConfigChanged`
//! (which reloads the instance with the new settings — same path the
//! overlay takes) followed by a hovered `Slice` event so the preview
//! shows the design's hover-state geometry. Incoming `Scene`s are
//! replayed through `oxidemx-scene-render` — the exact code path the
//! overlay's ring uses — into a small canvas wedge.
//!
//! Lifecycle: [`sync`] runs after every `update()` dispatch. The
//! preview spawns when the card becomes visible, re-feeds on edits,
//! and is dropped (closing the ctl channel → worker exits → event
//! stream ends) the moment the card no longer applies. Previews hit
//! real HTTP APIs — accepted; the host's permission gate, cache and
//! manifest refresh floor all still apply.

use std::sync::Arc;

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Task, Theme as IcedTheme};
use oxidemx_shared::theme::{Theme, ThemeColors, ThemeName};
use oxidemx_shared::widgets::JsonBag;
use oxidemx_shared::{ActionKind, AppConfig, RadialPage, Slice, WidgetScope, WidgetSource};
use oxidemx_widget_host::{
    HostCtl, HostEvent, InstanceId, ReqwestFetcher, SliceEvent, WidgetRegistry,
};
use oxidemx_widget_proto::{Scene, WedgeGeom};

use crate::{Message, State};

/// Hovered wedge geometry handed to the previewed widget — the same
/// proportions as the host worker's default geometry (a 45° slice of
/// a 60→160 px ring), with `hovered` pinned high so the widget
/// renders its hover response (matches the design mockup's preview).
const PREVIEW_GEOM: WedgeGeom = WedgeGeom {
    width: 200.0,
    height: 160.0,
    inner_radius: 60.0,
    outer_radius: 160.0,
    angle_start: -std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_8,
    angle_end: -std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_8,
    hovered: 1.0,
};

/// Canvas size + where the wedge lands inside it. The icon anchor
/// (scene origin) sits at mid-ring radius, the ring centre below the
/// canvas bottom edge — same construction as the overlay's ring, so
/// the preview wedge reads like an actual slice cut out of the menu.
const CANVAS_W: f32 = 224.0;
const CANVAS_H: f32 = 192.0;
const ANCHOR: Point = Point::new(CANVAS_W / 2.0, 70.0);

/// One live preview worker. Dropping the handle closes the `ctl`
/// channel; the worker task exits on the next recv and its event
/// stream (driven by `Task::run`) completes.
pub struct PreviewHandle {
    pub instance: InstanceId,
    ctl: async_channel::Sender<HostCtl>,
    /// Latest validated scene from the worker, if any arrived yet.
    pub scene: Option<Scene>,
    pub revision: u64,
    /// Set when the worker reports `InstanceFailed` (bad wasm, three
    /// strikes…) — rendered in place of the wedge.
    pub failed: Option<String>,
    /// Resolved settings bag + scope + slice colour last fed to the
    /// worker / painter. Diffed by [`sync`] to decide when to re-send
    /// `ConfigChanged`.
    last_bag: JsonBag,
    last_scope: WidgetScope,
    /// Theme palette for scene colour resolution, resolved at spawn.
    theme: Option<ThemeColors>,
}

/// What the preview *should* be showing right now, derived from the
/// same conditions that make `widget_options::options_section` render
/// the card.
struct Desired {
    instance: InstanceId,
    scope: WidgetScope,
    bag: JsonBag,
    cfg: AppConfig,
}

fn desired_preview(state: &State) -> Option<Desired> {
    // The card lives on the Menu tab's slice editor.
    if state.tab != crate::Tab::Menu {
        return None;
    }
    let idx = state.selected_slice?;
    let page = state.config.radial_menu.pages.get(state.active_page)?;
    let slice = page.slices.get(idx)?;
    if slice.kind != ActionKind::Widget {
        return None;
    }
    let w = slice.widget.as_ref()?;
    let WidgetSource::Custom(id) = &w.source else {
        return None;
    };
    let summary = state.widget_registry.iter().find(|s| s.id == *id)?;
    if !summary.ready {
        return None;
    }
    let manifest = state.widget_manifests.get(id)?;
    if manifest.options.is_empty() {
        // No options → no card → no preview (matches options_section).
        return None;
    }

    let ikey = crate::tabs::buttons::widget_options::effective_instance_key(
        w.instance_key.as_deref(),
        &page.name,
        idx,
    );
    let defaults = manifest.defaults();
    let bag = state
        .config
        .widgets
        .resolve(id, Some(&ikey), w.scope, &defaults);

    // Synthetic one-page/one-slice config: the selected slice (with
    // its instance key pinned) + the real settings bags, so the
    // worker resolves exactly what the overlay would.
    let mut preview_slice = slice.clone();
    if let Some(wc) = preview_slice.widget.as_mut() {
        wc.instance_key = Some(ikey.clone());
    }
    let preview_page = RadialPage {
        name: page.name.clone(),
        slices: vec![preview_slice],
        ..RadialPage::default()
    };
    let mut cfg = AppConfig::default();
    cfg.radial_menu.pages = vec![preview_page];
    cfg.widgets = state.config.widgets.clone();

    Some(Desired {
        instance: InstanceId {
            instance_key: ikey,
            widget_id: id.clone(),
        },
        scope: w.scope,
        bag,
        cfg,
    })
}

/// Reconcile the preview worker against the current UI state. Runs
/// after every `update()` dispatch (see main.rs): spawns when the
/// options card becomes visible, feeds `ConfigChanged` on option
/// edits, drops the worker when the card goes away.
pub fn sync(state: &mut State) -> Task<Message> {
    let Some(desired) = desired_preview(state) else {
        state.widget_preview = None; // drop → ctl closes → worker exits
        return Task::none();
    };

    if let Some(p) = state.widget_preview.as_mut() {
        if p.instance == desired.instance {
            if p.last_bag != desired.bag || p.last_scope != desired.scope {
                p.last_bag = desired.bag;
                p.last_scope = desired.scope;
                let instance = p.instance.clone();
                // Settings changes reload the instance (v1 ABI), so a
                // fresh hover event re-establishes the preview's
                // hovered geometry afterwards. try_send: unbounded
                // channel, only fails when the worker died — in which
                // case `failed` already tells the story.
                let _ = p.ctl.try_send(HostCtl::ConfigChanged(Arc::new(desired.cfg)));
                let _ = p.ctl.try_send(HostCtl::Slice {
                    instance,
                    ev: SliceEvent::Hover(true),
                    geom: PREVIEW_GEOM,
                });
            }
            return Task::none();
        }
    }

    spawn_preview(state, desired)
}

/// Spawn the worker for a fresh instance and stream its events back
/// as `Message::WidgetPreviewEvent`. The worker itself is started
/// inside the stream's first poll so `tokio::spawn` runs on iced's
/// tokio executor.
fn spawn_preview(state: &mut State, desired: Desired) -> Task<Message> {
    use futures_util::StreamExt;
    let Some(widgets_dir) = WidgetRegistry::widgets_dir() else {
        state.widget_preview = None;
        return Task::none();
    };

    let (ctl_tx, ctl_rx) = async_channel::unbounded::<HostCtl>();
    let (ev_tx, ev_rx) = async_channel::unbounded::<HostEvent>();

    state.widget_preview = Some(PreviewHandle {
        instance: desired.instance.clone(),
        ctl: ctl_tx.clone(),
        scene: None,
        revision: 0,
        failed: None,
        last_bag: desired.bag,
        last_scope: desired.scope,
        theme: resolve_theme_colors(&state.config.theme),
    });

    let cfg = Arc::new(desired.cfg);
    let instance = desired.instance;
    let stream = futures_util::stream::once(async move {
        oxidemx_widget_host::worker::spawn_with(
            ctl_rx,
            ev_tx,
            widgets_dir,
            Box::new(ReqwestFetcher::new()),
        );
        let _ = ctl_tx.send(HostCtl::ConfigChanged(cfg)).await;
        let _ = ctl_tx
            .send(HostCtl::Slice {
                instance,
                ev: SliceEvent::Hover(true),
                geom: PREVIEW_GEOM,
            })
            .await;
        ev_rx
    })
    .flatten();

    Task::run(stream, Message::WidgetPreviewEvent)
}

/// `Message::WidgetPreviewEvent` handler — store scenes/failures for
/// the instance the preview currently tracks; stale events from a
/// replaced worker are dropped by the instance-id check.
pub fn on_event(state: &mut State, event: HostEvent) {
    let Some(p) = state.widget_preview.as_mut() else {
        return;
    };
    match event {
        HostEvent::Scene {
            instance,
            scene,
            revision,
        } if instance == p.instance => {
            p.scene = Some(scene);
            p.revision = revision;
            p.failed = None;
        }
        HostEvent::InstanceFailed { instance, error } if instance == p.instance => {
            p.failed = Some(error);
        }
        _ => {}
    }
}

fn resolve_theme_colors(name: &ThemeName) -> Option<ThemeColors> {
    Theme::load(name)
        .or_else(|| Theme::load(&ThemeName::CatppuccinMocha))
        .map(|t| t.colors)
}

// ============================================================================
// View — the wedge canvas on the options card's right column
// ============================================================================

/// The preview element for the options card, when the running
/// preview matches the card's instance. `None` while no worker is
/// up (sync hasn't run yet for this selection) — the card simply
/// renders without the right column for that frame.
pub fn preview_element<'a>(
    state: &'a State,
    widget_id: &str,
    instance_key: &str,
    slice: &Slice,
) -> Option<Element<'a, Message>> {
    let p = state.widget_preview.as_ref()?;
    if p.instance.widget_id != widget_id || p.instance.instance_key != instance_key {
        return None;
    }
    let pal = &state.palette;
    let slice_color =
        crate::tabs::buttons::widget_options::palette_color(pal, &slice.color);

    let caption: Element<Message> = match &p.failed {
        Some(err) => iced::widget::text(format!("preview failed — {err}"))
            .size(9)
            .style(oxidemx_widgets::style::text_faint(pal))
            .into(),
        None => iced::widget::text("Live preview · hover state")
            .size(9)
            .style(oxidemx_widgets::style::text_faint(pal))
            .into(),
    };

    let painter = WedgePainter {
        scene: p.failed.is_none().then(|| p.scene.clone()).flatten(),
        theme: p.theme.clone(),
        slice_color,
        text_color: pal.subtext0,
    };
    let canvas = iced::widget::canvas(painter)
        .width(Length::Fixed(CANVAS_W))
        .height(Length::Fixed(CANVAS_H));

    Some(
        iced::widget::column![canvas, caption]
            .spacing(4)
            .align_x(iced::Alignment::Center)
            .into(),
    )
}

/// Canvas program: the wedge backdrop (hover wash + ring outline)
/// plus the scene replay anchored at the icon point. Re-painted per
/// frame like the radial preview — scenes are tiny and the card is
/// only visible while editing.
struct WedgePainter {
    scene: Option<Scene>,
    theme: Option<ThemeColors>,
    slice_color: Color,
    text_color: Color,
}

impl canvas::Program<Message> for WedgePainter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &IcedTheme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        // Ring centre such that the icon anchor sits at mid-radius
        // ((60+160)/2 = 110 px) straight up from it.
        let center = Point::new(ANCHOR.x, ANCHOR.y + 110.0);
        let (a0, a1) = (PREVIEW_GEOM.angle_start, PREVIEW_GEOM.angle_end);
        let (inner, outer) = (PREVIEW_GEOM.inner_radius, PREVIEW_GEOM.outer_radius);

        // Wedge: outer arc a0→a1, straight edge in, inner arc back.
        let wedge = Path::new(|b| {
            b.move_to(Point::new(
                center.x + outer * a0.cos(),
                center.y + outer * a0.sin(),
            ));
            b.arc(canvas::path::Arc {
                center,
                radius: outer,
                start_angle: iced::Radians(a0),
                end_angle: iced::Radians(a1),
            });
            b.line_to(Point::new(
                center.x + inner * a1.cos(),
                center.y + inner * a1.sin(),
            ));
            b.arc(canvas::path::Arc {
                center,
                radius: inner,
                start_angle: iced::Radians(a1),
                end_angle: iced::Radians(a0),
            });
            b.close();
        });
        // Hover-state styling: accent wash + slot-colour ring, like
        // the overlay's hovered slice.
        frame.fill(&wedge, Color { a: 0.16, ..self.slice_color });
        frame.stroke(
            &wedge,
            Stroke::default()
                .with_color(Color { a: 0.75, ..self.slice_color })
                .with_width(1.5),
        );

        match (&self.scene, &self.theme) {
            (Some(scene), Some(theme)) => {
                oxidemx_scene_render::draw_custom_widget(
                    &mut frame,
                    scene,
                    ANCHOR,
                    theme,
                    self.slice_color,
                    1.0,
                );
            }
            _ => {
                // Worker still booting (wasm load + first render) or
                // no theme palette — quiet placeholder.
                let label = "rendering…";
                let approx_w = label.chars().count() as f32 * 10.0 * 0.55;
                frame.fill_text(canvas::Text {
                    content: label.to_string(),
                    position: Point::new(ANCHOR.x - approx_w / 2.0, ANCHOR.y - 5.0),
                    color: self.text_color,
                    size: 10.0.into(),
                    ..canvas::Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }
}

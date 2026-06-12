//! Custom animation editor — full-panel UI for building track-based
//! animations on a single element (Menu / Submenu / SliceHighlight /
//! PageTransition).
//!
//! Layout:
//!   * Header (title + element-name dropdown + Reset to preset).
//!   * Two columns side-by-side: Enter (left) and Exit (right).
//!   * Each column has a list of tracks. Each row: track type name,
//!     "Edit" affordance via click, an X button to delete.
//!   * Below the two columns: a parameter editor for the currently
//!     selected track (if any). Type combo box at top, then per-kind
//!     parameter sliders, then delay/duration/easing.
//!
//! Picking "Customize" on a preset row in the Animation tab opens
//! this view on the matching element. The editor mutates
//! `state.config.radial_menu.animation.<element>.{enter,exit}.
//! custom_tracks` directly via Message::AnimationEditor* messages
//! handled in main.rs::update.
//!
//! See memory entry `project_track_animation_system` for the data
//! model and what's wired in the renderer.

use crate::{style, Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, Space};
use iced::{Alignment, Element, Length};
use oxidemx_shared::{AnimationTrack, Axis, Easing, ElementAnimation, TrackKind};
use oxidemx_widgets::widgets::{labeled_int_slider, labeled_slider};

/// Which top-level element the editor is currently editing. Drives
/// the dispatch in main.rs that picks
/// `state.config.radial_menu.animation.<element>` for read/write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimEditorElement {
    Menu,
    Submenu,
    SliceHighlight,
    PageTransition,
    AiMorph,
}

impl AnimEditorElement {
    pub fn label(self) -> &'static str {
        match self {
            AnimEditorElement::Menu => "Menu (open / close)",
            AnimEditorElement::Submenu => "Submenu pop-out",
            AnimEditorElement::SliceHighlight => "Slice highlight (per-slot hover)",
            AnimEditorElement::PageTransition => "Page transition (page-cycle)",
            AnimEditorElement::AiMorph => "AI chat morph (disc → chat)",
        }
    }
}

/// Which direction (Enter or Exit) of the active element's
/// `ElementAnimation` is being targeted by a structural / param
/// edit. Both columns share one parameter-editor pane below the
/// list, so `selected` carries (direction, index) — only one
/// track is "open for editing" at a time across both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimEditorDirection {
    Enter,
    Exit,
}

#[derive(Debug, Clone)]
pub struct AnimationEditorState {
    pub element: AnimEditorElement,
    pub selected: Option<(AnimEditorDirection, usize)>,
}

impl AnimationEditorState {
    pub fn new(element: AnimEditorElement) -> Self {
        Self {
            element,
            selected: None,
        }
    }
}

/// Resolve the active `ElementAnimation` from config. Used by the
/// view + the main.rs handlers to read/write the right slot.
pub fn element_animation<'a>(state: &'a State, el: AnimEditorElement) -> &'a ElementAnimation {
    let cfg = &state.config.radial_menu.animation;
    match el {
        AnimEditorElement::Menu => &cfg.menu,
        AnimEditorElement::Submenu => &cfg.submenu,
        AnimEditorElement::SliceHighlight => &cfg.slice_highlight,
        AnimEditorElement::PageTransition => &cfg.page_transition.animation,
        AnimEditorElement::AiMorph => &cfg.ai_morph,
    }
}

pub fn element_animation_mut<'a>(
    state: &'a mut State,
    el: AnimEditorElement,
) -> &'a mut ElementAnimation {
    let cfg = &mut state.config.radial_menu.animation;
    match el {
        AnimEditorElement::Menu => &mut cfg.menu,
        AnimEditorElement::Submenu => &mut cfg.submenu,
        AnimEditorElement::SliceHighlight => &mut cfg.slice_highlight,
        AnimEditorElement::PageTransition => &mut cfg.page_transition.animation,
        AnimEditorElement::AiMorph => &mut cfg.ai_morph,
    }
}

pub fn view<'a>(state: &'a State, editor: &'a AnimationEditorState) -> Element<'a, Message> {
    let pal = &state.palette;
    let anim = element_animation(state, editor.element);

    let header = row![
        text(format!("Element: {}", editor.element.label())).size(13),
        Space::new().width(Length::Fill),
        button(text("Reset to preset").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::AnimationEditorReset),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let intro = text(
        "Build a custom animation by stacking tracks. Each track \
         (Fade / Translate / Rotate / Scale / Flip) runs simultaneously \
         on its own delay, duration, and easing curve. Empty track lists \
         on both sides falls back to the preset on the Animation tab.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let enter_col = direction_column(
        state,
        editor,
        AnimEditorDirection::Enter,
        &anim.enter.custom_tracks,
    );
    let exit_col = direction_column(
        state,
        editor,
        AnimEditorDirection::Exit,
        &anim.exit.custom_tracks,
    );

    let columns = row![
        container(enter_col).width(Length::FillPortion(1)),
        Space::new().width(Length::Fixed(16.0)),
        container(exit_col).width(Length::FillPortion(1)),
    ]
    .spacing(0);

    let editor_panel = parameter_panel(state, editor, anim);

    column![
        header,
        intro,
        rule::horizontal(1).style(style::rule_style(pal)),
        columns,
        rule::horizontal(1).style(style::rule_style(pal)),
        editor_panel,
    ]
    .spacing(12)
    .into()
}

fn direction_column<'a>(
    state: &'a State,
    editor: &'a AnimationEditorState,
    direction: AnimEditorDirection,
    tracks: &'a [AnimationTrack],
) -> Element<'a, Message> {
    let pal = &state.palette;
    let title = match direction {
        AnimEditorDirection::Enter => "Enter (off → at rest)",
        AnimEditorDirection::Exit => "Exit (at rest → off)",
    };
    let header = row![
        text(title).size(14),
        Space::new().width(Length::Fill),
        add_track_dropdown(direction),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let mut col = column![header].spacing(6);

    if tracks.is_empty() {
        col = col.push(
            text("No tracks. Use \"+ Add\" to start.")
                .size(11)
                .style(style::text_dim(pal)),
        );
    }

    for (i, track) in tracks.iter().enumerate() {
        let is_selected = editor.selected == Some((direction, i));
        let label_text = describe_track(track);
        // `.style()` returns a generic `impl Fn` whose opaque
        // type differs between btn_primary / btn_secondary, so
        // we can't if/else them into a single binding. Apply
        // the style branch-wise on otherwise-identical buttons.
        let base = button(text(label_text).size(12))
            .width(Length::Fill)
            .on_press(Message::AnimationEditorSelectTrack(direction, i));
        let label_btn: Element<Message> = if is_selected {
            base.style(style::btn_primary(pal)).into()
        } else {
            base.style(style::btn_secondary(pal)).into()
        };
        let delete_btn = button(text("X").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::AnimationEditorDeleteTrack(direction, i));
        let row = row![
            label_btn,
            Space::new().width(Length::Fixed(4.0)),
            delete_btn
        ]
        .align_y(Alignment::Center);
        col = col.push(row);
    }

    container(col)
        .padding(10)
        .style(style::card_quiet(pal))
        .into()
}

/// Combobox-style "+ Add" button. We use a pick_list with a
/// placeholder so picking a kind triggers AddTrack — saves a
/// modal/popover for picking the type.
fn add_track_dropdown(direction: AnimEditorDirection) -> Element<'static, Message> {
    let opts: Vec<TrackKindOption> = TRACK_KIND_OPTIONS
        .iter()
        .copied()
        .map(TrackKindOption)
        .collect();
    pick_list(opts, None::<TrackKindOption>, move |opt| {
        Message::AnimationEditorAddTrack(direction, opt.0)
    })
    .placeholder("+ Add track")
    .text_size(11)
    .into()
}

fn describe_track(track: &AnimationTrack) -> String {
    let kind = match &track.kind {
        TrackKind::Fade { off_alpha } => format!("Fade — off α {:.0}%", off_alpha * 100.0),
        TrackKind::Translate { axis, offset_px } => {
            let a = match axis {
                Axis::X => "X",
                Axis::Y => "Y",
            };
            format!("Translate — {} {:+.0} px", a, offset_px)
        }
        TrackKind::Rotate { offset_deg } => format!("Rotate — {:+.0}°", offset_deg),
        TrackKind::Scale { offset_pct } => format!("Scale — {:.0}%", offset_pct),
        TrackKind::Flip { axis, offset_deg } => {
            let a = match axis {
                Axis::X => "X",
                Axis::Y => "Y",
            };
            format!("Flip — {}-axis {:.0}°", a, offset_deg)
        }
    };
    format!("{}  ({}ms+{}ms)", kind, track.delay_ms, track.duration_ms)
}

fn parameter_panel<'a>(
    state: &'a State,
    editor: &'a AnimationEditorState,
    anim: &'a ElementAnimation,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let Some((direction, idx)) = editor.selected else {
        return container(
            text("Select a track above to edit its parameters.")
                .size(11)
                .style(style::text_dim(pal)),
        )
        .padding(10)
        .into();
    };
    let cfg = match direction {
        AnimEditorDirection::Enter => &anim.enter,
        AnimEditorDirection::Exit => &anim.exit,
    };
    let Some(track) = cfg.custom_tracks.get(idx) else {
        return container(
            text("Selected track no longer exists. Pick another.")
                .size(11)
                .style(style::text_dim(pal)),
        )
        .padding(10)
        .into();
    };

    // Type pick_list — keep it cheap to swap kinds while
    // preserving delay/duration/easing.
    let cur_kind_name = track.kind.variant_name();
    let cur_opt = TrackKindOption(cur_kind_name);
    let opts: Vec<TrackKindOption> = TRACK_KIND_OPTIONS
        .iter()
        .copied()
        .map(TrackKindOption)
        .collect();
    let kind_picker = pick_list(opts, Some(cur_opt), move |opt| {
        Message::AnimationEditorChangeKind(direction, idx, opt.0)
    })
    .text_size(12);
    let kind_row = row![
        text("Type").size(13).width(Length::Fixed(120.0)),
        Space::new().width(Length::Fill),
        kind_picker,
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let mut params = column![kind_row].spacing(8);

    // Per-kind sliders.
    match &track.kind {
        TrackKind::Fade { off_alpha } => {
            params = params.push(labeled_slider(
                "Off-state α",
                *off_alpha,
                0.0..=1.0,
                0.01,
                |x| format!("{:.0} %", x * 100.0),
                move |v| Message::AnimationEditorSetParam(direction, idx, TrackParam::FadeAlpha(v)),
            ));
        }
        TrackKind::Translate { axis, offset_px } => {
            params = params.push(axis_picker(*axis, direction, idx));
            params = params.push(labeled_slider(
                match direction {
                    AnimEditorDirection::Enter => "Starting offset",
                    AnimEditorDirection::Exit => "Ending offset",
                },
                *offset_px,
                -200.0..=200.0,
                1.0,
                |x| format!("{:+.0} px", x),
                move |v| {
                    Message::AnimationEditorSetParam(direction, idx, TrackParam::TranslatePx(v))
                },
            ));
        }
        TrackKind::Rotate { offset_deg } => {
            params = params.push(labeled_slider(
                match direction {
                    AnimEditorDirection::Enter => "Starting angle",
                    AnimEditorDirection::Exit => "Ending angle",
                },
                *offset_deg,
                -360.0..=360.0,
                1.0,
                |x| format!("{:+.0}°", x),
                move |v| Message::AnimationEditorSetParam(direction, idx, TrackParam::RotateDeg(v)),
            ));
        }
        TrackKind::Scale { offset_pct } => {
            params = params.push(labeled_slider(
                match direction {
                    AnimEditorDirection::Enter => "Starting scale",
                    AnimEditorDirection::Exit => "Ending scale",
                },
                *offset_pct,
                0.0..=200.0,
                1.0,
                |x| format!("{:.0} %", x),
                move |v| Message::AnimationEditorSetParam(direction, idx, TrackParam::ScalePct(v)),
            ));
        }
        TrackKind::Flip { axis, offset_deg } => {
            params = params.push(axis_picker(*axis, direction, idx));
            params = params.push(labeled_slider(
                match direction {
                    AnimEditorDirection::Enter => "Starting flip angle",
                    AnimEditorDirection::Exit => "Ending flip angle",
                },
                *offset_deg,
                -360.0..=360.0,
                1.0,
                |x| format!("{:+.0}°", x),
                move |v| Message::AnimationEditorSetParam(direction, idx, TrackParam::FlipDeg(v)),
            ));
        }
    }

    // Common: delay, duration, easing.
    params = params.push(labeled_int_slider(
        "Delay",
        track.delay_ms,
        0..=2000,
        |ms| format!("{ms} ms"),
        move |ms| Message::AnimationEditorSetParam(direction, idx, TrackParam::DelayMs(ms)),
    ));
    params = params.push(labeled_int_slider(
        "Duration",
        track.duration_ms,
        16..=4000,
        |ms| format!("{ms} ms"),
        move |ms| Message::AnimationEditorSetParam(direction, idx, TrackParam::DurationMs(ms)),
    ));
    // Easing: type pick_list, plus stiffness/damping when Spring.
    let allow_spring = !matches!(track.kind, TrackKind::Fade { .. });
    let easing_opt = EasingPickOption::from(track.easing);
    let easing_opts: Vec<EasingPickOption> = if allow_spring {
        vec![
            EasingPickOption::Linear,
            EasingPickOption::EaseIn,
            EasingPickOption::EaseOut,
            EasingPickOption::EaseInOut,
            EasingPickOption::Spring,
        ]
    } else {
        vec![
            EasingPickOption::Linear,
            EasingPickOption::EaseIn,
            EasingPickOption::EaseOut,
            EasingPickOption::EaseInOut,
        ]
    };
    let easing_picker = pick_list(easing_opts, Some(easing_opt), move |opt| {
        Message::AnimationEditorSetEasingKind(direction, idx, opt)
    })
    .text_size(12);
    let easing_row = row![
        text("Easing").size(13).width(Length::Fixed(120.0)),
        Space::new().width(Length::Fill),
        easing_picker,
    ]
    .align_y(Alignment::Center)
    .spacing(8);
    params = params.push(easing_row);

    if let Easing::Spring { stiffness, damping } = track.easing {
        params = params.push(labeled_slider(
            "Stiffness",
            stiffness,
            10.0..=400.0,
            1.0,
            |v| format!("{v:.0}"),
            move |v| {
                Message::AnimationEditorSetParam(direction, idx, TrackParam::SpringStiffness(v))
            },
        ));
        params = params.push(labeled_slider(
            "Damping",
            damping,
            1.0..=40.0,
            0.5,
            |v| format!("{v:.1}"),
            move |v| Message::AnimationEditorSetParam(direction, idx, TrackParam::SpringDamping(v)),
        ));
    }

    container(params)
        .padding(12)
        .style(style::card_quiet(pal))
        .into()
}

fn axis_picker(cur: Axis, direction: AnimEditorDirection, idx: usize) -> Element<'static, Message> {
    let cur_opt = AxisOption(cur);
    let opts = vec![AxisOption(Axis::X), AxisOption(Axis::Y)];
    let picker = pick_list(opts, Some(cur_opt), move |opt| {
        Message::AnimationEditorSetParam(direction, idx, TrackParam::Axis(opt.0))
    })
    .text_size(12);
    row![
        text("Axis").size(13).width(Length::Fixed(120.0)),
        Space::new().width(Length::Fill),
        picker,
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// Tagged struct for the type pick_list. The `&'static str` is
/// the variant name returned by `TrackKind::variant_name`, which
/// we hand back to `TrackKind::default_for` on the backend to
/// build a fresh track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackKindOption(&'static str);

impl std::fmt::Display for TrackKindOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

const TRACK_KIND_OPTIONS: &[&str] = &["Fade", "Translate", "Rotate", "Scale", "Flip"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AxisOption(Axis);

impl std::fmt::Display for AxisOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            Axis::X => "X (horizontal)",
            Axis::Y => "Y (vertical)",
        })
    }
}

/// Easing picker option. Mirrors `Easing` but flattened — the
/// Spring variant carries its own stiffness/damping which we
/// edit via separate sliders below the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingPickOption {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Spring,
}

impl From<Easing> for EasingPickOption {
    fn from(e: Easing) -> Self {
        match e {
            Easing::Linear => EasingPickOption::Linear,
            Easing::EaseIn => EasingPickOption::EaseIn,
            Easing::EaseOut => EasingPickOption::EaseOut,
            Easing::EaseInOut => EasingPickOption::EaseInOut,
            Easing::Spring { .. } => EasingPickOption::Spring,
        }
    }
}

impl std::fmt::Display for EasingPickOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EasingPickOption::Linear => "Linear",
            EasingPickOption::EaseIn => "Ease in",
            EasingPickOption::EaseOut => "Ease out",
            EasingPickOption::EaseInOut => "Ease in/out",
            EasingPickOption::Spring => "Spring",
        })
    }
}

/// Granular parameter mutation messages — built into a single
/// enum so `update()` has one match arm with all the cases
/// instead of a dozen tiny messages. Each variant carries the
/// new value; the handler resolves the (element, direction, idx)
/// into a mutable track and applies.
#[derive(Debug, Clone, Copy)]
pub enum TrackParam {
    FadeAlpha(f32),
    Axis(Axis),
    TranslatePx(f32),
    RotateDeg(f32),
    ScalePct(f32),
    FlipDeg(f32),
    DelayMs(u32),
    DurationMs(u32),
    SpringStiffness(f32),
    SpringDamping(f32),
}

/// Mutate a single track field in place. Centralises the matching
/// of TrackParam variants against TrackKind variants so the
/// `update()` handler doesn't repeat the dispatch. Silently
/// ignores params that don't apply to the current kind (e.g.
/// FadeAlpha on a Translate track) — the UI doesn't expose those
/// combinations, but the safety net costs nothing.
pub fn apply_track_param(track: &mut AnimationTrack, param: TrackParam) {
    match param {
        TrackParam::FadeAlpha(v) => {
            if let TrackKind::Fade { off_alpha } = &mut track.kind {
                *off_alpha = v;
            }
        }
        TrackParam::Axis(a) => match &mut track.kind {
            TrackKind::Translate { axis, .. } => *axis = a,
            TrackKind::Flip { axis, .. } => *axis = a,
            _ => {}
        },
        TrackParam::TranslatePx(v) => {
            if let TrackKind::Translate { offset_px, .. } = &mut track.kind {
                *offset_px = v;
            }
        }
        TrackParam::RotateDeg(v) => {
            if let TrackKind::Rotate { offset_deg } = &mut track.kind {
                *offset_deg = v;
            }
        }
        TrackParam::ScalePct(v) => {
            if let TrackKind::Scale { offset_pct } = &mut track.kind {
                *offset_pct = v;
            }
        }
        TrackParam::FlipDeg(v) => {
            if let TrackKind::Flip { offset_deg, .. } = &mut track.kind {
                *offset_deg = v;
            }
        }
        TrackParam::DelayMs(ms) => track.delay_ms = ms,
        TrackParam::DurationMs(ms) => track.duration_ms = ms.max(1),
        TrackParam::SpringStiffness(v) => {
            if let Easing::Spring { stiffness, .. } = &mut track.easing {
                *stiffness = v;
            }
        }
        TrackParam::SpringDamping(v) => {
            if let Easing::Spring { damping, .. } = &mut track.easing {
                *damping = v;
            }
        }
    }
}

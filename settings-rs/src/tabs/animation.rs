//! "Animation" tab — per-element, per-direction transition controls.
//!
//! Layout: a section per element (Menu / Submenu / Slice highlight),
//! each containing two columns (Enter / Exit). Per direction the user
//! picks a transition kind, easing, and duration; the relevant
//! kind-specific knobs (initial scale, initial / final opacity) appear
//! conditionally so the panel only shows what's relevant.
//!
//! For the Submenu element a `chain` row exposes the per-item
//! stagger slider (0–250 ms).

use crate::widgets::{labeled_int_slider, labeled_slider, section_header};
use crate::{AnimDirection, AnimElement, Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, Space};
use iced::{Alignment, Element, Length};
use juhradial_shared::{Easing, ElementAnimation, TransitionConfig, TransitionKind};

// ============================================================================
// Top-level: stack the three element sections.
// ============================================================================

pub fn view(state: &State) -> Element<'_, Message> {
    let elements = [
        AnimElement::Menu,
        AnimElement::Submenu,
        AnimElement::SliceHighlight,
    ];
    let mut col = column![
        section_header("Per-element animation"),
        text(
            "Each element has independent enter (open) and exit (close) transitions. \
             Edits autosave; the running overlay picks them up live via inotify.",
        )
        .size(12),
    ]
    .spacing(12);
    for (i, e) in elements.iter().enumerate() {
        let anim = e.get(&state.config.radial_menu.animation);
        col = col.push(element_section(*e, anim));
        if i < elements.len() - 1 {
            col = col.push(Space::new().height(Length::Fixed(8.0)));
        }
    }
    container(col).into()
}

// ============================================================================
// One element's full block (header + enter | exit + chain).
// ============================================================================

fn element_section<'a>(
    element: AnimElement,
    anim: &'a ElementAnimation,
) -> Element<'a, Message> {
    let header = row![
        text(element.label()).size(17),
        Space::new().width(Length::Fill),
        button("Reset").on_press(Message::ResetElementAnimation(element)),
    ]
    .align_y(Alignment::Center)
    .padding([4, 0]);

    let description = text(element.description()).size(12);

    let enter = direction_panel(element, AnimDirection::Enter, &anim.enter);
    let exit = direction_panel(element, AnimDirection::Exit, &anim.exit);
    let pair = row![enter, Space::new().width(Length::Fixed(16.0)), exit].spacing(0);

    let mut block = column![header, description, rule::horizontal(1), pair].spacing(10);

    if element.supports_chain() {
        block = block.push(chain_panel(element, anim));
    }

    container(block)
        .padding(14)
        .style(container::bordered_box)
        .into()
}

// ============================================================================
// One column: a single direction (Enter or Exit).
// ============================================================================

fn direction_panel<'a>(
    element: AnimElement,
    dir: AnimDirection,
    cfg: &TransitionConfig,
) -> Element<'a, Message> {
    let cfg_owned = cfg.clone();

    // --- Kind picker
    let kind_picker = {
        let cur = cfg.kind;
        let cfg_for_msg = cfg_owned.clone();
        pick_list(
            KIND_OPTIONS.as_slice(),
            Some(KindOption::from(cur)),
            move |opt| {
                let mut next = cfg_for_msg.clone();
                next.kind = opt.into();
                Message::SetTransition(element, dir, next)
            },
        )
    };

    // --- Easing picker
    let easing_picker = {
        let cur = EasingOption::from(cfg.easing);
        let cfg_for_msg = cfg_owned.clone();
        pick_list(EASING_OPTIONS.as_slice(), Some(cur), move |opt| {
            let mut next = cfg_for_msg.clone();
            next.easing = opt.into();
            Message::SetTransition(element, dir, next)
        })
    };

    // --- Duration slider
    let duration = {
        let cfg_for_msg = cfg_owned.clone();
        labeled_int_slider(
            "Duration",
            cfg.duration_ms,
            0..=2000,
            |ms| format!("{ms} ms"),
            move |ms| {
                let mut next = cfg_for_msg.clone();
                next.duration_ms = ms;
                Message::SetTransition(element, dir, next)
            },
        )
    };

    // --- Delay slider
    let delay = {
        let cfg_for_msg = cfg_owned.clone();
        labeled_int_slider(
            "Pre-roll delay",
            cfg.delay_ms,
            0..=1000,
            |ms| format!("{ms} ms"),
            move |ms| {
                let mut next = cfg_for_msg.clone();
                next.delay_ms = ms;
                Message::SetTransition(element, dir, next)
            },
        )
    };

    // --- Conditional kind-specific knobs
    let needs_scale = matches!(cfg.kind, TransitionKind::Grow | TransitionKind::GrowAndFade);
    let needs_opacity =
        matches!(cfg.kind, TransitionKind::Fade | TransitionKind::GrowAndFade);

    let mut col = column![
        text(format!("{} transition", dir.label())).size(14),
        labelled_pair("Kind", kind_picker.into()),
        labelled_pair("Easing", easing_picker.into()),
        duration,
        delay,
    ]
    .spacing(12);

    // Label conventions for the kind-specific knobs depend on the
    // direction. The on-disk schema keeps fixed names
    // (`initial_scale`, `initial_opacity`, `final_opacity`) but the
    // *meaning* flips for an exit transition: when current=0 the
    // element is fully gone, so `initial_scale` is actually the
    // scale the element shrinks DOWN to before disappearing, and
    // the opacity pair represents start (was-visible) → end
    // (is-gone). Relabel to match what the user sees on screen.
    let scale_label = match dir {
        AnimDirection::Enter => "Initial scale",
        AnimDirection::Exit => "End scale",
    };
    let opacity_a_label = match dir {
        AnimDirection::Enter => "Initial opacity",
        AnimDirection::Exit => "End opacity",
    };
    let opacity_b_label = match dir {
        AnimDirection::Enter => "Final opacity",
        AnimDirection::Exit => "Start opacity",
    };

    if needs_scale {
        let cfg_for_msg = cfg_owned.clone();
        col = col.push(labeled_slider(
            scale_label,
            cfg.initial_scale,
            0.0..=1.0,
            0.01,
            |v| format!("{:.0} %", v * 100.0),
            move |v| {
                let mut next = cfg_for_msg.clone();
                next.initial_scale = v;
                Message::SetTransition(element, dir, next)
            },
        ));
    }
    if needs_opacity {
        let cfg_init = cfg_owned.clone();
        col = col.push(labeled_slider(
            opacity_a_label,
            cfg.initial_opacity,
            0.0..=1.0,
            0.01,
            |v| format!("{:.0} %", v * 100.0),
            move |v| {
                let mut next = cfg_init.clone();
                next.initial_opacity = v;
                Message::SetTransition(element, dir, next)
            },
        ));
        let cfg_final = cfg_owned.clone();
        col = col.push(labeled_slider(
            opacity_b_label,
            cfg.final_opacity,
            0.0..=1.0,
            0.01,
            |v| format!("{:.0} %", v * 100.0),
            move |v| {
                let mut next = cfg_final.clone();
                next.final_opacity = v;
                Message::SetTransition(element, dir, next)
            },
        ));
    }

    // Spring-only knobs.
    if let Easing::Spring { stiffness, damping } = cfg.easing {
        let cfg_stiff = cfg_owned.clone();
        col = col.push(labeled_slider(
            "Spring stiffness",
            stiffness,
            10.0..=500.0,
            1.0,
            |v| format!("{v:.0}"),
            move |v| {
                let mut next = cfg_stiff.clone();
                next.easing = Easing::Spring {
                    stiffness: v,
                    damping,
                };
                Message::SetTransition(element, dir, next)
            },
        ));
        let cfg_damp = cfg_owned.clone();
        col = col.push(labeled_slider(
            "Spring damping",
            damping,
            1.0..=40.0,
            0.5,
            |v| format!("{v:.1}"),
            move |v| {
                let mut next = cfg_damp.clone();
                next.easing = Easing::Spring {
                    stiffness,
                    damping: v,
                };
                Message::SetTransition(element, dir, next)
            },
        ));
    }

    container(col)
        .width(Length::FillPortion(1))
        .padding(8)
        .into()
}

fn labelled_pair<'a>(label: &str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label.to_string()).size(13).width(Length::Fixed(80.0)),
        control,
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

// ============================================================================
// Chain panel (per-item stagger). Submenu only.
// ============================================================================

fn chain_panel<'a>(element: AnimElement, anim: &'a ElementAnimation) -> Element<'a, Message> {
    let stagger = anim.chain.as_ref().map(|c| c.stagger_ms).unwrap_or(0);
    container(
        column![
            text("Chain (per-item stagger)").size(14),
            text(
                "Delay between each sub-item starting its transition. 0 ms = all sub-items \
                 animate simultaneously."
            )
            .size(11),
            labeled_int_slider(
                "Stagger",
                stagger,
                0..=250,
                |ms| format!("{ms} ms"),
                move |ms| Message::SetChainStagger(element, ms),
            ),
        ]
        .spacing(8),
    )
    .padding([10, 8])
    .into()
}

// ============================================================================
// PickList option wrappers — iced's pick_list wants T: Display + PartialEq + Clone
// and we don't want to put Display on the canonical types in the
// shared crate.
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindOption(pub TransitionKind);

impl From<TransitionKind> for KindOption {
    fn from(k: TransitionKind) -> Self {
        KindOption(k)
    }
}
impl From<KindOption> for TransitionKind {
    fn from(o: KindOption) -> Self {
        o.0
    }
}
impl std::fmt::Display for KindOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            TransitionKind::None => "None (instant)",
            TransitionKind::Fade => "Fade",
            TransitionKind::Grow => "Grow (scale)",
            TransitionKind::GrowAndFade => "Grow + Fade",
        })
    }
}

const KIND_OPTIONS: [KindOption; 4] = [
    KindOption(TransitionKind::None),
    KindOption(TransitionKind::Fade),
    KindOption(TransitionKind::Grow),
    KindOption(TransitionKind::GrowAndFade),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EasingOption {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Spring,
}

impl From<Easing> for EasingOption {
    fn from(e: Easing) -> Self {
        match e {
            Easing::Linear => EasingOption::Linear,
            Easing::EaseIn => EasingOption::EaseIn,
            Easing::EaseOut => EasingOption::EaseOut,
            Easing::EaseInOut => EasingOption::EaseInOut,
            Easing::Spring { .. } => EasingOption::Spring,
        }
    }
}

impl From<EasingOption> for Easing {
    fn from(o: EasingOption) -> Self {
        match o {
            EasingOption::Linear => Easing::Linear,
            EasingOption::EaseIn => Easing::EaseIn,
            EasingOption::EaseOut => Easing::EaseOut,
            EasingOption::EaseInOut => Easing::EaseInOut,
            // Spring needs parameters — use Motion.dev's gentle
            // preset as the entry point. Users can then drag the
            // stiffness/damping sliders.
            EasingOption::Spring => Easing::Spring {
                stiffness: 180.0,
                damping: 14.0,
            },
        }
    }
}

impl std::fmt::Display for EasingOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EasingOption::Linear => "Linear",
            EasingOption::EaseIn => "Ease in",
            EasingOption::EaseOut => "Ease out",
            EasingOption::EaseInOut => "Ease in/out",
            EasingOption::Spring => "Spring",
        })
    }
}

const EASING_OPTIONS: [EasingOption; 5] = [
    EasingOption::Linear,
    EasingOption::EaseIn,
    EasingOption::EaseOut,
    EasingOption::EaseInOut,
    EasingOption::Spring,
];

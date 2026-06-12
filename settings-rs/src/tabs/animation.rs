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

use crate::{AnimDirection, AnimElement, Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, Space};
use iced::{Alignment, Element, Length};
use oxidemx_shared::{
    Easing, ElementAnimation, PageTransitionConfig, PageTransitionShaderStyle, PageTransitionStyle,
    TransitionConfig, TransitionKind,
};
use oxidemx_widgets::widgets::{labeled_int_slider, labeled_slider, section_header};

// ============================================================================
// Top-level: stack the three element sections.
// ============================================================================

pub fn view(state: &State) -> Element<'_, Message> {
    let elements = [
        AnimElement::Menu,
        AnimElement::Submenu,
        AnimElement::AiMorph,
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
    col = col.push(Space::new().height(Length::Fixed(8.0)));
    col = col.push(page_transition_section(
        &state.config.radial_menu.animation.page_transition,
    ));
    container(col).into()
}

// ============================================================================
// Page-transition card — animation played when the user cycles
// between radial-menu pages via the scroll wheel over the centre
// puck. Style picker + duration + easing + (style-specific)
// rotation slider.
// ============================================================================

fn page_transition_section(cfg: &PageTransitionConfig) -> Element<'_, Message> {
    let is_custom = cfg.animation.enter.is_custom() || cfg.animation.exit.is_custom();
    let customize_label = if is_custom {
        "Edit custom…"
    } else {
        "Customize…"
    };
    let header = row![
        text("Page transition").size(17),
        Space::new().width(Length::Fill),
        button("Open radial menu").on_press(Message::OpenOverlayForPreview),
        button(customize_label).on_press(Message::OpenAnimationEditor(
            crate::animation_editor::AnimEditorElement::PageTransition,
        )),
        button("Reset").on_press(Message::ResetPageTransition),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .padding([4, 0]);

    let description = text(
        "Animation played when the scroll wheel cycles between radial-menu \
         pages. Style picker controls the visual feel; duration / easing \
         tune timing. Click \"Open radial menu\" to test — once it's up, \
         scroll the wheel over the centre puck to cycle pages and see the \
         animation live.",
    )
    .size(12);

    let cfg_owned = cfg.clone();
    let style_picker = {
        let cur = PageTransitionStyleOption::from(cfg.style);
        let cfg_for_msg = cfg_owned.clone();
        pick_list(
            PAGE_TRANSITION_STYLE_OPTIONS.as_slice(),
            Some(cur),
            move |opt| {
                let mut next = cfg_for_msg.clone();
                next.style = opt.into();
                Message::SetPageTransition(next)
            },
        )
    };

    let easing_picker = {
        let cur = EasingOption::from(cfg.easing);
        let cfg_for_msg = cfg_owned.clone();
        pick_list(EASING_OPTIONS.as_slice(), Some(cur), move |opt| {
            let mut next = cfg_for_msg.clone();
            next.easing = opt.into();
            Message::SetPageTransition(next)
        })
    };

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
                Message::SetPageTransition(next)
            },
        )
    };

    let mut col = column![
        header,
        description,
        rule::horizontal(1),
        labelled_pair("Style", style_picker.into()),
        labelled_pair("Easing", easing_picker.into()),
        duration,
    ]
    .spacing(10);

    // Rotation slider only applies to SpinCrossfade — keep the panel
    // tight by hiding it for the other styles.
    if matches!(cfg.style, PageTransitionStyle::SpinCrossfade) {
        let cfg_for_msg = cfg_owned.clone();
        col = col.push(labeled_slider(
            "Rotation",
            cfg.rotation_deg,
            0.0..=90.0,
            0.5,
            |v| format!("{v:.1}°"),
            move |v| {
                let mut next = cfg_for_msg.clone();
                next.rotation_deg = v;
                Message::SetPageTransition(next)
            },
        ));
    }

    // Debounce — protects against hi-res scroll wheels firing
    // dozens of cycles per physical click. 0 disables the
    // debounce (raw scroll-tick behaviour); higher values are
    // safer for fast / sensitive wheels.
    let cfg_for_debounce = cfg_owned.clone();
    col = col.push(labeled_int_slider(
        "Scroll debounce",
        cfg.cycle_debounce_ms,
        0..=1000,
        |ms| {
            if ms == 0 {
                "off".to_string()
            } else {
                format!("{ms} ms")
            }
        },
        move |ms| {
            let mut next = cfg_for_debounce.clone();
            next.cycle_debounce_ms = ms;
            Message::SetPageTransition(next)
        },
    ));

    // Shader overlay sub-card: orthogonal to the canvas style
    // above. Layers a wgpu fragment effect (Dissolve / Plasma)
    // on top of whatever canvas animation the user picked, so
    // e.g. SpinCrossfade canvas + Plasma shader compose into
    // one transition.
    col = col.push(rule::horizontal(1));
    col = col.push(page_transition_shader_subsection(&cfg_owned));

    container(col)
        .padding(14)
        .style(container::bordered_box)
        .into()
}

// ============================================================================
// Page-transition shader sub-card.
// ============================================================================

fn page_transition_shader_subsection(cfg: &PageTransitionConfig) -> Element<'static, Message> {
    let header = text("Shader overlay").size(15);
    let description = text(
        "GPU fragment-shader pass that runs on top of the canvas animation \
         above. Layer e.g. \"Spin + crossfade\" with \"Plasma\" to combine a \
         spin under a wash of plasma waves. Pick \"None\" for canvas-only.",
    )
    .size(11);

    let cfg_owned = cfg.clone();
    let style_picker = {
        let cur = PageTransitionShaderStyleOption::from(cfg.shader.style);
        let cfg_for_msg = cfg_owned.clone();
        pick_list(
            PAGE_TRANSITION_SHADER_STYLE_OPTIONS.as_slice(),
            Some(cur),
            move |opt| {
                let mut next = cfg_for_msg.clone();
                next.shader.style = opt.into();
                Message::SetPageTransition(next)
            },
        )
    };

    let mut sub = column![
        header,
        description,
        labelled_pair("Style", style_picker.into()),
    ]
    .spacing(10);

    let active = !matches!(cfg.shader.style, PageTransitionShaderStyle::None);
    // Show legacy-fallback hint when style: None but canvas
    // style is one of the shader-only legacy variants — that
    // path still runs the shader, but with default params and
    // no per-style sliders. Tells the user how to access the
    // new tunables.
    let legacy_active = !active
        && matches!(
            cfg.style,
            PageTransitionStyle::Dissolve | PageTransitionStyle::Plasma
        );
    if legacy_active {
        sub = sub.push(
            text(
                "Legacy mode: the canvas style is set to a shader-only \
                 variant, so the shader still runs with default \
                 parameters. Pick a Style here to expose the per-effect \
                 tuning sliders below.",
            )
            .size(11),
        );
    }

    if active {
        // Common: intensity multiplier.
        let cfg_for_intensity = cfg_owned.clone();
        sub = sub.push(labeled_slider(
            "Intensity",
            cfg.shader.intensity,
            0.0..=1.0,
            0.01,
            |x| {
                if x <= 0.005 {
                    "off".to_string()
                } else {
                    format!("{:.0} %", x * 100.0)
                }
            },
            move |v| {
                let mut next = cfg_for_intensity.clone();
                next.shader.intensity = v;
                Message::SetPageTransition(next)
            },
        ));

        match cfg.shader.style {
            PageTransitionShaderStyle::Dissolve => {
                let cfg_for_noise = cfg_owned.clone();
                sub = sub.push(labeled_slider(
                    "Dissolve grain",
                    cfg.shader.dissolve_noise_scale,
                    1.0..=24.0,
                    0.1,
                    |v| format!("{v:.1}×"),
                    move |v| {
                        let mut next = cfg_for_noise.clone();
                        next.shader.dissolve_noise_scale = v;
                        Message::SetPageTransition(next)
                    },
                ));
                let cfg_for_band = cfg_owned.clone();
                sub = sub.push(labeled_slider(
                    "Dissolve softness",
                    cfg.shader.dissolve_band_softness,
                    0.02..=0.6,
                    0.005,
                    |v| format!("{:.2}", v),
                    move |v| {
                        let mut next = cfg_for_band.clone();
                        next.shader.dissolve_band_softness = v;
                        Message::SetPageTransition(next)
                    },
                ));
            }
            PageTransitionShaderStyle::Plasma => {
                let cfg_for_scale = cfg_owned.clone();
                sub = sub.push(labeled_slider(
                    "Plasma wave scale",
                    cfg.shader.plasma_wave_scale,
                    1.0..=18.0,
                    0.1,
                    |v| format!("{v:.1}×"),
                    move |v| {
                        let mut next = cfg_for_scale.clone();
                        next.shader.plasma_wave_scale = v;
                        Message::SetPageTransition(next)
                    },
                ));
                let cfg_for_speed = cfg_owned.clone();
                sub = sub.push(labeled_slider(
                    "Plasma speed",
                    cfg.shader.plasma_wave_speed,
                    0.0..=4.0,
                    0.05,
                    |v| format!("{v:.2}×"),
                    move |v| {
                        let mut next = cfg_for_speed.clone();
                        next.shader.plasma_wave_speed = v;
                        Message::SetPageTransition(next)
                    },
                ));
            }
            PageTransitionShaderStyle::None => {}
        }
    }

    sub.into()
}

// ============================================================================
// One element's full block (header + enter | exit + chain).
// ============================================================================

fn element_section<'a>(element: AnimElement, anim: &'a ElementAnimation) -> Element<'a, Message> {
    let editor_element = match element {
        AnimElement::Menu => crate::animation_editor::AnimEditorElement::Menu,
        AnimElement::Submenu => crate::animation_editor::AnimEditorElement::Submenu,
        AnimElement::SliceHighlight => crate::animation_editor::AnimEditorElement::SliceHighlight,
        AnimElement::AiMorph => crate::animation_editor::AnimEditorElement::AiMorph,
    };
    let is_custom = anim.enter.is_custom() || anim.exit.is_custom();

    let header = row![
        text(element.label()).size(17),
        Space::new().width(Length::Fill),
        button(if is_custom {
            "Edit custom…"
        } else {
            "Customize…"
        })
        .on_press(Message::OpenAnimationEditor(editor_element)),
        button("Reset").on_press(Message::ResetElementAnimation(element)),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .padding([4, 0]);

    let description = text(element.description()).size(12);

    let mut block = column![header, description, rule::horizontal(1)].spacing(10);

    if is_custom {
        // When custom tracks are configured, hide the per-direction
        // preset panels and show a compact summary so the user
        // doesn't have two competing UIs for one element. Edit goes
        // through the dedicated full-panel editor.
        block = block.push(custom_summary_panel(anim));
    } else {
        let enter = direction_panel(element, AnimDirection::Enter, &anim.enter);
        let exit = direction_panel(element, AnimDirection::Exit, &anim.exit);
        let pair = row![enter, Space::new().width(Length::Fixed(16.0)), exit].spacing(0);
        block = block.push(pair);
    }

    if element.supports_chain() {
        block = block.push(chain_panel(element, anim));
    }

    container(block)
        .padding(14)
        .style(container::bordered_box)
        .into()
}

fn custom_summary_panel<'a>(anim: &'a ElementAnimation) -> Element<'a, Message> {
    let enter_count = anim.enter.custom_tracks.len();
    let exit_count = anim.exit.custom_tracks.len();
    let summary = format!(
        "Custom animation active — {enter_count} enter track{} + {exit_count} exit track{}. Click \"Edit custom…\" above or \"Reset\" to go back to presets.",
        if enter_count == 1 { "" } else { "s" },
        if exit_count == 1 { "" } else { "s" },
    );
    text(summary).size(12).into()
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
    let needs_opacity = matches!(cfg.kind, TransitionKind::Fade | TransitionKind::GrowAndFade);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTransitionStyleOption(pub PageTransitionStyle);

impl From<PageTransitionStyle> for PageTransitionStyleOption {
    fn from(s: PageTransitionStyle) -> Self {
        PageTransitionStyleOption(s)
    }
}
impl From<PageTransitionStyleOption> for PageTransitionStyle {
    fn from(o: PageTransitionStyleOption) -> Self {
        o.0
    }
}
impl std::fmt::Display for PageTransitionStyleOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            PageTransitionStyle::None => "None (instant)",
            PageTransitionStyle::CrossfadeScale => "Crossfade + scale pulse",
            PageTransitionStyle::SpinCrossfade => "Spin + crossfade",
            PageTransitionStyle::CenterPulse => "Centre-puck pulse",
            PageTransitionStyle::Flip => "Flip (card-flip around Y axis)",
            PageTransitionStyle::Dissolve => "Dissolve (GPU shader)",
            PageTransitionStyle::Plasma => "Plasma waves (GPU shader)",
        })
    }
}

const PAGE_TRANSITION_STYLE_OPTIONS: [PageTransitionStyleOption; 7] = [
    PageTransitionStyleOption(PageTransitionStyle::None),
    PageTransitionStyleOption(PageTransitionStyle::CrossfadeScale),
    PageTransitionStyleOption(PageTransitionStyle::SpinCrossfade),
    PageTransitionStyleOption(PageTransitionStyle::CenterPulse),
    PageTransitionStyleOption(PageTransitionStyle::Flip),
    PageTransitionStyleOption(PageTransitionStyle::Dissolve),
    PageTransitionStyleOption(PageTransitionStyle::Plasma),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTransitionShaderStyleOption(pub PageTransitionShaderStyle);

impl From<PageTransitionShaderStyle> for PageTransitionShaderStyleOption {
    fn from(s: PageTransitionShaderStyle) -> Self {
        PageTransitionShaderStyleOption(s)
    }
}
impl From<PageTransitionShaderStyleOption> for PageTransitionShaderStyle {
    fn from(o: PageTransitionShaderStyleOption) -> Self {
        o.0
    }
}
impl std::fmt::Display for PageTransitionShaderStyleOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            PageTransitionShaderStyle::None => "None",
            PageTransitionShaderStyle::Dissolve => "Dissolve (noise sweep)",
            PageTransitionShaderStyle::Plasma => "Plasma (sin/cos waves)",
        })
    }
}

const PAGE_TRANSITION_SHADER_STYLE_OPTIONS: [PageTransitionShaderStyleOption; 3] = [
    PageTransitionShaderStyleOption(PageTransitionShaderStyle::None),
    PageTransitionShaderStyleOption(PageTransitionShaderStyle::Dissolve),
    PageTransitionShaderStyleOption(PageTransitionShaderStyle::Plasma),
];

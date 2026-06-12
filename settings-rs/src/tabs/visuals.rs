//! "Visuals" tab — static visual knobs (background opacity,
//! highlight intensity).

use crate::{FontChoice, Message, State, VisualField};
use iced::widget::{
    button, column, combo_box, container, pick_list, row, text, text_input, toggler, Space,
};
use iced::{Alignment, Element, Length};
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::{labeled_int_slider, labeled_slider};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let v = &state.config.radial_menu.visuals;

    let bg = labeled_slider(
        "Menu background opacity",
        v.menu_background_opacity,
        0.0..=1.0,
        0.01,
        |x| format!("{:.0} %", x * 100.0),
        |x| Message::SetVisual(VisualField::MenuBackgroundOpacity, x),
    );
    let hl = labeled_slider(
        "Slice highlight intensity",
        v.slice_highlight_opacity,
        0.0..=1.0,
        0.01,
        |x| format!("{:.0} %", x * 100.0),
        |x| Message::SetVisual(VisualField::SliceHighlightOpacity, x),
    );
    let center_size = labeled_slider(
        "Centre label size",
        v.center_label_size,
        0.0..=24.0,
        0.5,
        |x| {
            if x <= 0.5 {
                "off".to_string()
            } else {
                format!("{x:.0} px")
            }
        },
        |x| Message::SetVisual(VisualField::CenterLabelSize, x),
    );
    let shaders_card = gpu_shaders_card(state);
    let ai_fx_card = ai_fx_card(state);
    let tooltip_card = tooltip_settings_card(state);
    let page_name_card = page_name_settings_card(state);

    let selected = FontChoice::from_config_value(&v.font_family);
    let font_combo = combo_box(
        &state.font_picker,
        "Search installed fonts…",
        Some(&selected),
        |choice: FontChoice| Message::SetFontFamily(choice.as_config_value()),
    )
    .size(12)
    .width(Length::Fill);

    let font_count = crate::fonts::system_families().len();
    let font_count_hint = if font_count == 0 {
        "Could not enumerate system fonts (fc-list not available). \
         You can still type a family name and it will be used if \
         installed."
            .to_string()
    } else {
        format!(
            "Override the font used for the centre label and other rendered \
             text. {font_count} installed families detected — search to \
             filter; pick \"(System default)\" to fall back."
        )
    };

    let font_row = column![
        text("Font family").size(13),
        text(font_count_hint).size(11).style(style::text_dim(pal)),
        font_combo,
    ]
    .spacing(6);

    container(
        column![
            text(
                "Tweak the static look of the radial menu. Animations are configured \
                 separately under the Animation tab.",
            )
            .size(12)
            .style(style::text_dim(pal)),
            container(
                column![
                    bg,
                    hl,
                    center_size,
                    font_row,
                    tooltip_card,
                    page_name_card,
                    shaders_card,
                    ai_fx_card
                ]
                .spacing(20),
            )
            .padding(12),
        ]
        .spacing(12),
    )
    .into()
}

/// "GPU shaders" card. Groups every wgpu effect together with
/// a 0..=1 intensity slider per effect (0 = disabled, no shader
/// pass runs at all → zero GPU cost). Each slider is otherwise
/// independent so users can pick the look they want.
fn gpu_shaders_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let v = &state.config.radial_menu.visuals;

    let aurora = shader_slider_row(
        pal,
        "Aurora backdrop",
        "Slow conic gradient between the active accents painted \
         behind the menu. Animated, theme-reactive.",
        v.aurora_intensity,
        VisualField::AuroraIntensity,
    );
    let ripple = shader_slider_row(
        pal,
        "Haptic ripple",
        "Expanding ring from menu centre on every haptic event \
         (menu open, slice change, page cycle, dispatch). Synced \
         to the device's motor pulse.",
        v.ripple_intensity,
        VisualField::RippleIntensity,
    );
    let hover_glow = shader_slider_row(
        pal,
        "Hover glow",
        "Resolution-independent SDF outline + soft aura around \
         the wedge under the cursor. Coloured by the slice's \
         palette key, scales with the highlight tween.",
        v.hover_glow_intensity,
        VisualField::HoverGlowIntensity,
    );
    let dispatch_burst = dispatch_burst_row(pal, v);
    let hover_tilt = hover_tilt_row(pal, v);
    let disc_bevel = shader_slider_row(
        pal,
        "Disc bevel",
        "Directional rim light along the outer edge + carved \
         inset shadow at the inner ring. Frames the menu like a \
         beveled coin — always-on 3D regardless of hover.",
        v.disc_bevel_intensity,
        VisualField::DiscBevelIntensity,
    );
    let center_dome = shader_slider_row(
        pal,
        "Centre dome",
        "Phong-shaded sphere over the centre puck — Lambert wash \
         + tight specular highlight. Reads as a physical button \
         instead of a flat circle.",
        v.center_dome_intensity,
        VisualField::CenterDomeIntensity,
    );
    let slice_bevel = shader_slider_row(
        pal,
        "Slice bevel",
        "Carved grooves between every wedge with directional rim \
         lighting on the lit edge. Each slice reads as its own \
         raised 3D button.",
        v.slice_bevel_intensity,
        VisualField::SliceBevelIntensity,
    );
    let specular_sweep = specular_sweep_row(pal, v);
    let drop_shadow = shader_slider_row(
        pal,
        "Drop shadow",
        "Soft falloff shadow OUTSIDE the disc, offset away from \
         the light. Makes the menu read as a floating physical \
         object casting a real cast shadow onto whatever is behind.",
        v.drop_shadow_intensity,
        VisualField::DropShadowIntensity,
    );

    // Global light-direction knob. Drives all 3D-framing shaders
    // simultaneously so highlights and shadows stay coherent.
    // Slider range covers a full revolution; the displayed
    // direction label tells the user what rotation lands at any
    // given angle. Default `-3π/4` rad = upper-left.
    let light_angle = labeled_slider(
        "Light direction",
        v.light_angle_rad,
        -std::f32::consts::PI..=std::f32::consts::PI,
        0.01,
        |x| {
            // Convert radians to canvas-convention degrees and
            // attach a human-readable direction label.
            let deg = x.to_degrees();
            let label = light_angle_label(x);
            format!("{deg:.0}\u{00B0} {label}")
        },
        |x| Message::SetVisual(VisualField::LightAngleRad, x),
    );
    let light_card = column![
        text("Light direction").size(13),
        text(
            "Rotates the virtual light source for every 3D-\
              framing shader at once. Upper-left is the universal \
              \"this is 3D\" convention; rotate clockwise to move \
              the highlight around."
        )
        .size(11)
        .style(style::text_dim(pal)),
        light_angle,
    ]
    .spacing(4);
    let sdf_ring = shader_slider_row(
        pal,
        "SDF wedge ring (spike)",
        "Replaces canvas-tessellated wedge fills with a \
         signed-distance-field shader. Resolution-independent — \
         crisper edges at fractional / HiDPI scaling than the \
         canvas path. As you slide intensity up the canvas \
         wedges fade out and the SDF wedges fade in (icons + \
         text + centre puck stay on the canvas). Spike feature; \
         hover highlights still come from the canvas.",
        v.sdf_ring_intensity,
        VisualField::SdfRingIntensity,
    );

    let header = row![
        text("GPU shaders").size(14),
        Space::new().width(Length::Fill),
        text(format!(
            "{} active",
            [
                v.aurora_intensity > 0.005,
                v.ripple_intensity > 0.005,
                v.hover_glow_intensity > 0.005,
                v.dispatch_burst_intensity > 0.005,
                v.hover_tilt_intensity > 0.005,
                v.disc_bevel_intensity > 0.005,
                v.center_dome_intensity > 0.005,
                v.slice_bevel_intensity > 0.005,
                v.drop_shadow_intensity > 0.005,
                v.specular_sweep_intensity > 0.005,
                v.sdf_ring_intensity > 0.005,
            ]
            .iter()
            .filter(|on| **on)
            .count(),
        ))
        .size(11)
        .style(style::text_dim(pal)),
    ]
    .align_y(Alignment::Center);

    container(
        column![
            header,
            text(
                "Custom wgpu shaders that layer with the radial canvas. \
                  Each effect has its own intensity (0 = off, no GPU work). \
                  Settings live-preview on the running overlay."
            )
            .size(11)
            .style(style::text_dim(pal)),
            light_card,
            drop_shadow,
            disc_bevel,
            specular_sweep,
            slice_bevel,
            center_dome,
            aurora,
            ripple,
            hover_glow,
            hover_tilt,
            dispatch_burst,
            sdf_ring,
        ]
        .spacing(12),
    )
    .padding(12)
    .style(style::card_quiet(pal))
    .into()
}

/// Dispatch-burst row: intensity slider plus a style picker. The
/// burst has three selectable looks (Sparks, Shockwave, Glow) so
/// it gets its own row builder rather than the generic
/// `shader_slider_row` helper used by single-knob effects.
fn dispatch_burst_row<'a>(
    pal: &'a oxidemx_widgets::palette::Palette,
    v: &'a oxidemx_shared::VisualSettings,
) -> Element<'a, Message> {
    let intensity = labeled_slider(
        "Intensity",
        v.dispatch_burst_intensity,
        0.0..=1.0,
        0.01,
        |x| {
            if x <= 0.005 {
                "off".to_string()
            } else {
                format!("{:.0} %", x * 100.0)
            }
        },
        |x| Message::SetVisual(VisualField::DispatchBurstIntensity, x),
    );

    let cur = DispatchBurstStyleOption(v.dispatch_burst_style);
    let picker = pick_list(
        DISPATCH_BURST_STYLE_OPTIONS,
        Some(cur),
        |opt: DispatchBurstStyleOption| Message::SetDispatchBurstStyle(opt.0),
    )
    .style(style::pick_list_style(pal))
    .text_size(12);

    let style_row = row![
        text("Style").size(13).width(Length::Fixed(140.0)),
        Space::new().width(Length::Fill),
        picker,
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    column![
        text("Dispatch burst").size(13),
        text(
            "Celebratory flourish anchored at the activated slice when \
              you fire an action. Three looks: scattered Sparks, cascading \
              Shockwave rings, or a quick centred Glow."
        )
        .size(11)
        .style(style::text_dim(pal)),
        intensity,
        style_row,
    ]
    .spacing(4)
    .into()
}

/// Specular-sweep row: intensity slider + revolution-period slider.
/// Two knobs because users care about both "how strong is the
/// sheen?" and "how often does it pass?" — combining them into one
/// slider would conflate two perceptually independent dimensions.
fn specular_sweep_row<'a>(
    pal: &'a oxidemx_widgets::palette::Palette,
    v: &'a oxidemx_shared::VisualSettings,
) -> Element<'a, Message> {
    let intensity = labeled_slider(
        "Intensity",
        v.specular_sweep_intensity,
        0.0..=1.0,
        0.01,
        |x| {
            if x <= 0.005 {
                "off".to_string()
            } else {
                format!("{:.0} %", x * 100.0)
            }
        },
        |x| Message::SetVisual(VisualField::SpecularSweepIntensity, x),
    );
    let period = labeled_slider(
        "Period",
        v.specular_sweep_period_s,
        1.0..=30.0,
        0.1,
        |x| format!("{x:.1} s/rev"),
        |x| Message::SetVisual(VisualField::SpecularSweepPeriod, x),
    );

    column![
        text("Specular sweep").size(13),
        text(
            "Animated narrow band of light that rotates slowly \
              around the disc rim — like a polished surface \
              catching ambient light. Lit-side gated so it fades \
              on the shadow hemisphere."
        )
        .size(11)
        .style(style::text_dim(pal)),
        intensity,
        period,
    ]
    .spacing(4)
    .into()
}

/// Human-readable direction label for a canvas-convention light
/// angle in radians. Maps 8 cardinal/intercardinal directions so
/// the user knows where the light is pointing at any slider
/// value. Ranges are 22.5° wide centred on each cardinal.
fn light_angle_label(rad: f32) -> &'static str {
    use std::f32::consts::PI;
    // Normalise to [0, 2π).
    let mut a = rad % (2.0 * PI);
    if a < 0.0 {
        a += 2.0 * PI;
    }
    // Canvas convention: -π/2 = top, 0 = right, π/2 = bottom.
    // Map to direction labels by 45° wedges.
    let octant = ((a / (PI / 4.0)) + 0.5) as i32 % 8;
    match octant {
        0 => "(right)",
        1 => "(lower-right)",
        2 => "(bottom)",
        3 => "(lower-left)",
        4 => "(left)",
        5 => "(upper-left)",
        6 => "(top)",
        _ => "(upper-right)",
    }
}

/// Parallax-tilt row: intensity slider plus shadow-strength and
/// specular-sharpness knobs. Pure presentation; the apply path is
/// shared with every other shader in the card via `SetVisual`.
fn hover_tilt_row<'a>(
    pal: &'a oxidemx_widgets::palette::Palette,
    v: &'a oxidemx_shared::VisualSettings,
) -> Element<'a, Message> {
    let intensity = labeled_slider(
        "Intensity",
        v.hover_tilt_intensity,
        0.0..=1.0,
        0.01,
        |x| {
            if x <= 0.005 {
                "off".to_string()
            } else {
                format!("{:.0} %", x * 100.0)
            }
        },
        |x| Message::SetVisual(VisualField::HoverTiltIntensity, x),
    );
    let shadow = labeled_slider(
        "Shadow",
        v.hover_tilt_shadow,
        0.0..=1.0,
        0.01,
        |x| format!("{:.0} %", x * 100.0),
        |x| Message::SetVisual(VisualField::HoverTiltShadow, x),
    );
    let sharpness = labeled_slider(
        "Sharpness",
        v.hover_tilt_sharpness,
        0.0..=1.0,
        0.01,
        |x| format!("{:.0} %", x * 100.0),
        |x| Message::SetVisual(VisualField::HoverTiltSharpness, x),
    );

    column![
        text("Hover tilt (parallax)").size(13),
        text(
            "Inside the hovered slice: a soft specular highlight \
              that tracks your cursor + a darker wash on the side \
              facing away. Reads as a subtle 3D tilt without \
              moving any geometry. Pairs with hover glow."
        )
        .size(11)
        .style(style::text_dim(pal)),
        intensity,
        shadow,
        sharpness,
    ]
    .spacing(4)
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DispatchBurstStyleOption(oxidemx_shared::DispatchBurstStyle);

impl std::fmt::Display for DispatchBurstStyleOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use oxidemx_shared::DispatchBurstStyle as S;
        f.write_str(match self.0 {
            S::Sparks => "Sparks (fan outward)",
            S::Shockwave => "Shockwave (rings cascade)",
            S::Glow => "Glow (quick centred flash)",
        })
    }
}

const DISPATCH_BURST_STYLE_OPTIONS: [DispatchBurstStyleOption; 3] = [
    DispatchBurstStyleOption(oxidemx_shared::DispatchBurstStyle::Sparks),
    DispatchBurstStyleOption(oxidemx_shared::DispatchBurstStyle::Shockwave),
    DispatchBurstStyleOption(oxidemx_shared::DispatchBurstStyle::Glow),
];

/// Generic 0..=1 intensity row for one shader effect. Combines
/// title + description above a slider; "off" label at 0,
/// percentage above. Reduces boilerplate in the GPU-shaders
/// card and keeps the visual rhythm consistent across effects.
fn shader_slider_row<'a>(
    pal: &'a oxidemx_widgets::palette::Palette,
    title: &'a str,
    description: &'a str,
    value: f32,
    field: VisualField,
) -> Element<'a, Message> {
    column![
        text(title.to_string()).size(13),
        text(description.to_string())
            .size(11)
            .style(style::text_dim(pal)),
        labeled_slider(
            "Intensity",
            value,
            0.0..=1.0,
            0.01,
            |x| {
                if x <= 0.005 {
                    "off".to_string()
                } else {
                    format!("{:.0} %", x * 100.0)
                }
            },
            move |x| Message::SetVisual(field, x),
        ),
    ]
    .spacing(4)
    .into()
}

/// Page-name flash card: master toggle + visible duration +
/// transition duration + horizontal slide distance. Drives the
/// brief centre-puck announcement on page swaps (scroll cycle,
/// app-context auto-swap on open).
fn page_name_settings_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let v = &state.config.radial_menu.visuals;

    let toggle = row![
        column![
            text("Show page name on swap").size(13),
            text(
                "Briefly displays the active page's name in the centre \
                 puck on scroll-cycle or app-context auto-swap. Hover \
                 labels always take precedence — the flash only shows \
                 when no slice is hovered.",
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        toggler(v.page_name_show)
            .on_toggle(Message::SetPageNameShow)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let visible_slider = labeled_int_slider(
        "Visible duration",
        v.page_name_visible_ms,
        0..=4000,
        |ms| format!("{ms} ms"),
        Message::SetPageNameVisibleMs,
    );
    let transition_slider = labeled_int_slider(
        "Transition duration",
        v.page_name_transition_ms,
        50..=1000,
        |ms| format!("{ms} ms"),
        Message::SetPageNameTransitionMs,
    );
    let slide_slider = labeled_slider(
        "Slide distance",
        v.page_name_slide_distance_px,
        0.0..=80.0,
        1.0,
        |v| {
            if v < 0.5 {
                "off (pure crossfade)".to_string()
            } else {
                format!("{v:.0} px")
            }
        },
        Message::SetPageNameSlideDistance,
    );

    let arced_toggle = row![
        column![
            text("Arc above centre").size(13),
            text(
                "Lifts the label above the centre puck and arcs it \
                 along the menu's circular geometry. Keeps the text \
                 out of the way of the cursor (which sits on the puck \
                 when scrolling to cycle pages). Off = flat label \
                 inside the puck.",
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        toggler(v.page_name_arced)
            .on_toggle(Message::SetPageNameArced)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let mono_toggle = row![
        column![
            text("Monospace").size(13),
            text(
                "Forces a fixed-width font so each character on the \
                 arc spaces evenly. Strongly recommended for arced \
                 layouts — proportional fonts leave gaps around \
                 narrow glyphs (the \"i\" in \"Coding\" looks isolated). \
                 Turn off to use a proportional family for the flat \
                 layout.",
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        toggler(v.page_name_use_monospace)
            .on_toggle(Message::SetPageNameUseMonospace)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let pn_font_selected = FontChoice::from_config_value(&v.page_name_font_family);
    let pn_font_combo = combo_box(
        &state.font_picker,
        "Inherit menu font…",
        Some(&pn_font_selected),
        |choice: FontChoice| Message::SetPageNameFontFamily(choice.as_config_value()),
    )
    .size(12)
    .width(Length::Fill);
    let pn_font_row = column![
        text("Page-name font").size(13),
        text(
            "Page-name-specific font override. Empty = inherit the \
             menu's font family. Ignored while \"Monospace\" is on.",
        )
        .size(11)
        .style(style::text_dim(pal)),
        pn_font_combo,
    ]
    .spacing(6);

    let mut col = column![toggle].spacing(10);
    if v.page_name_show {
        col = col.push(arced_toggle);
        col = col.push(mono_toggle);
        col = col.push(pn_font_row);
        col = col.push(visible_slider);
        col = col.push(transition_slider);
        col = col.push(slide_slider);
    }

    container(
        column![
            row![text("Page name").size(14)].align_y(Alignment::Center),
            text(
                "Slide direction follows the cycle: scrolling forward \
                 slides the old name out to the left and the new name \
                 in from the right. App-context swaps fade in without a \
                 slide. Set distance to 0 for pure crossfade.",
            )
            .size(11)
            .style(style::text_dim(pal)),
            col,
        ]
        .spacing(10),
    )
    .padding(12)
    .style(style::card_quiet(pal))
    .into()
}

/// Tooltip-specific settings card: size + hover delay + font +
/// background colour + alpha + text colour. Kept in its own card
/// so the related knobs cluster visually instead of bleeding into
/// the rest of the visuals.
fn tooltip_settings_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let v = &state.config.radial_menu.visuals;

    let size_slider = labeled_slider(
        "Text size",
        v.tooltip_font_size,
        0.0..=20.0,
        0.5,
        |x| {
            if x <= 0.5 {
                "off".to_string()
            } else {
                format!("{x:.0} px")
            }
        },
        |x| Message::SetVisual(VisualField::TooltipFontSize, x),
    );
    let delay_slider = labeled_int_slider(
        "Hover delay",
        v.tooltip_delay_ms,
        0..=2000,
        |ms| {
            if ms == 0 {
                "instant".to_string()
            } else {
                format!("{ms} ms")
            }
        },
        Message::SetTooltipDelay,
    );

    let mono_toggle = row![
        column![
            text("Monospace").size(13),
            text(
                "Forces a fixed-width font so each character on the arc \
                  spaces evenly. Turn off to use a proportional family — \
                  arc spacing will approximate, narrow chars may bunch."
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        toggler(v.tooltip_use_monospace)
            .on_toggle(Message::SetTooltipUseMonospace)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    // Font picker — only useful when monospace is OFF. We always
    // render it so the user can pre-pick a family before flipping
    // the toggle. Empty draft = inherit menu font_family.
    let tooltip_font_selected = FontChoice::from_config_value(&v.tooltip_font_family);
    let tooltip_font_combo = combo_box(
        &state.font_picker,
        "Inherit menu font…",
        Some(&tooltip_font_selected),
        |choice: FontChoice| Message::SetTooltipFontFamily(choice.as_config_value()),
    )
    .size(12)
    .width(Length::Fill);
    let tooltip_font_row = column![
        text("Tooltip font").size(13),
        text(
            "Picked from the same system font index as the menu font. \
              Empty = inherit the menu's font_family. Ignored while \
              \"Monospace\" is on."
        )
        .size(11)
        .style(style::text_dim(pal)),
        tooltip_font_combo,
    ]
    .spacing(6);

    let bg_color_picker = palette_color_picker(
        pal,
        "Background colour",
        &v.tooltip_bg_color,
        SURFACE_KEYS,
        Message::SetTooltipBgColor,
    );
    let bg_alpha_slider = labeled_slider(
        "Background alpha",
        v.tooltip_bg_alpha,
        0.0..=1.0,
        0.01,
        |x| format!("{:.0} %", x * 100.0),
        Message::SetTooltipBgAlpha,
    );
    let text_color_picker = palette_color_picker(
        pal,
        "Text colour",
        &v.tooltip_text_color,
        TEXT_KEYS,
        Message::SetTooltipTextColor,
    );

    let reset_btn = button(text("Reset tooltip").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::ResetTooltipStyle);

    container(
        column![
            row![
                text("Tooltip").size(14),
                Space::new().width(Length::Fill),
                reset_btn,
            ]
            .align_y(Alignment::Center),
            text(
                "Description text that arcs around the outer ring when the \
                  user dwells on a slice. Configure size, hover delay, \
                  font, and the dark ribbon behind the text here."
            )
            .size(11)
            .style(style::text_dim(pal)),
            size_slider,
            delay_slider,
            mono_toggle,
            tooltip_font_row,
            bg_color_picker,
            bg_alpha_slider,
            text_color_picker,
        ]
        .spacing(10),
    )
    .padding(12)
    .style(style::card_quiet(pal))
    .into()
}

const SURFACE_KEYS: &[&str] = &[
    "crust", "mantle", "base", "surface0", "surface1", "surface2", "overlay0", "overlay1",
];
const TEXT_KEYS: &[&str] = &["text", "subtext1", "subtext0", "accent", "accent2"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaletteKeyOption(String);

impl std::fmt::Display for PaletteKeyOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn palette_color_picker<'a>(
    pal: &'a oxidemx_widgets::palette::Palette,
    label: &'a str,
    current: &str,
    keys: &'static [&'static str],
    on_select: fn(String) -> Message,
) -> Element<'a, Message> {
    let options: Vec<PaletteKeyOption> = keys
        .iter()
        .map(|k| PaletteKeyOption((*k).to_string()))
        .collect();
    let selected = PaletteKeyOption(current.to_string());
    let picker = pick_list(options, Some(selected), move |opt: PaletteKeyOption| {
        on_select(opt.0)
    })
    .style(style::pick_list_style(pal))
    .text_size(12);
    row![
        text(label.to_string()).size(13).width(Length::Fixed(140.0)),
        Space::new().width(Length::Fill),
        picker,
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

// `row`/`Alignment`/`Space` are imported for callers that grow this
// view later; suppress unused warning so a fresh build stays clean.
#[allow(dead_code)]
fn _imports_link() -> (
    iced::widget::Space,
    iced::Alignment,
    iced::widget::Row<'static, Message>,
) {
    (Space::new(), Alignment::Start, row![])
}

/// "AI window effects" card. Per-status (thinking / awaiting
/// approval / idle) shader-effect picker + intensity/speed knobs
/// for the chat window, plus optional hex colour overrides
/// (blank = follow the active theme's accents).
fn ai_fx_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let fx = &state.config.radial_menu.visuals.ai_fx;

    let effect_names: Vec<String> = oxidemx_shared::config::AI_FX_EFFECTS
        .iter()
        .map(|(_, name)| name.to_string())
        .collect();
    let display_for = |slug: &str| -> String {
        oxidemx_shared::config::AI_FX_EFFECTS
            .iter()
            .find(|(s, _)| *s == slug)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| "Aurora".to_string())
    };

    let status_row = |idx: usize,
                      title: &'static str,
                      blurb: &'static str,
                      cfg: &oxidemx_shared::config::AiStatusFx|
     -> Element<'static, Message> {
        let picker = pick_list(
            effect_names.clone(),
            Some(display_for(&cfg.effect)),
            move |name: String| {
                let slug = oxidemx_shared::config::AI_FX_EFFECTS
                    .iter()
                    .find(|(_, n)| *n == name)
                    .map(|(s, _)| s.to_string())
                    .unwrap_or_else(|| "aurora".to_string());
                Message::SetAiFxEffect(idx, slug)
            },
        )
        .text_size(12)
        .width(Length::Fixed(170.0));
        column![
            text(title).size(13),
            text(blurb).size(11).style(style::text_dim(pal)),
            row![
                picker,
                labeled_slider(
                    "Intensity",
                    cfg.intensity,
                    0.0..=1.0,
                    0.01,
                    |x| {
                        if x <= 0.005 {
                            "off".to_string()
                        } else {
                            format!("{:.0} %", x * 100.0)
                        }
                    },
                    move |x| Message::SetAiFxIntensity(idx, x),
                ),
                labeled_slider(
                    "Speed",
                    cfg.speed,
                    0.25..=3.0,
                    0.05,
                    |x| format!("{x:.2}x"),
                    move |x| Message::SetAiFxSpeed(idx, x),
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(4)
        .into()
    };

    let colors = fx
        .custom_colors
        .clone()
        .unwrap_or_else(|| [String::new(), String::new(), String::new()]);
    let color_input = |i: usize, placeholder: &'static str, val: &str| {
        text_input(placeholder, val)
            .size(12)
            .padding(6)
            .width(Length::Fixed(120.0))
            .on_input(move |v| Message::SetAiFxColor(i, v))
    };

    container(
        column![
            text("AI window effects").size(15),
            text(
                "Animated shader backdrop for the AI chat, per status. \
                 Colours follow the active theme's accents unless \
                 overridden below. Intensity 0 disables a status's \
                 effect entirely (no GPU work)."
            )
            .size(11)
            .style(style::text_dim(pal)),
            status_row(
                0,
                "Thinking",
                "While a turn is in flight — the model is generating \
                 or a tool is running. Breathes with the activity \
                 pulse.",
                &fx.thinking,
            ),
            status_row(
                1,
                "Awaiting approval",
                "While the agent waits on your choice (the window \
                 border also breathes yellow).",
                &fx.awaiting,
            ),
            status_row(
                2,
                "Idle",
                "Chat open, nothing in flight — a calm ambient wash.",
                &fx.idle,
            ),
            column![
                text("Custom colours").size(13),
                text(
                    "Optional hex overrides for the effect palette \
                     (e.g. #00d4ff). Leave blank to follow the \
                     theme's accent / accent2 / accent dim."
                )
                .size(11)
                .style(style::text_dim(pal)),
                row![
                    color_input(0, "accent…", &colors[0]),
                    color_input(1, "accent2…", &colors[1]),
                    color_input(2, "accent dim…", &colors[2]),
                ]
                .spacing(8),
            ]
            .spacing(4),
        ]
        .spacing(12),
    )
    .padding(12)
    .style(style::card_quiet(pal))
    .into()
}

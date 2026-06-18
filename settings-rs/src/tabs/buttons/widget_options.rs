//! Schema-driven widget options card (spec §5/§6/§10d).
//!
//! Rendered under the behavior chip when the selected slice hosts a
//! Ready custom widget that declares `options[]`. The card is
//! generated entirely from the manifest schema — widgets never ship
//! their own settings UI. First control is always the scope toggle
//! (This slice only / All {Name} slices); values read through the
//! two-bag merge (`WidgetStore::resolve`) and edits write to the
//! scoped bag, flowing through the normal debounced save path.

use iced::widget::{button, column, container, row, text, text_input, toggler, Space};
use iced::{Alignment, Background, Border, Element, Font, Length};
use oxidemx_shared::widgets::{JsonBag, WidgetStore};
use oxidemx_shared::{ActionKind, RadialPage, Slice, WidgetScope, WidgetSource};
use oxidemx_widget_proto::{OptionSpec, WidgetManifest};
use oxidemx_widgets::icons::icon;
use oxidemx_widgets::{palette::Palette, style};
use serde_json::Value;

use crate::{Message, State};

// ============================================================================
// Pure helpers (unit-tested below; also used by the main.rs handlers)
// ============================================================================

/// Which settings-app control an `OptionSpec` renders as (spec §5
/// table). `Unknown` covers forward-compat types from newer widgets
/// on this host — disabled row, never a parse failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    /// `enum` with ≤ 4 values — segmented button group.
    Segmented,
    /// `enum` with > 4 values, or `select` — dropdown.
    Select,
    /// `string` — text input with placeholder/maxlen.
    Text,
    /// `number` with min+max+step covering ≤ 100 steps — slider.
    NumberSlider,
    /// `number` otherwise — text input parsed on change.
    NumberText,
    /// `boolean` — switch.
    Boolean,
    /// `color` — curated swatch row over the slice palette keys.
    Color,
    /// `location` — geocoder search + pinned chip.
    Location,
    /// Anything else — disabled "Update OxideMX" row.
    Unknown,
}

/// Map one manifest option to its control (spec §5).
pub fn control_kind(spec: &OptionSpec) -> ControlKind {
    match spec.kind.as_str() {
        "enum" => {
            if spec.values.as_ref().map_or(0, |v| v.len()) <= 4 {
                ControlKind::Segmented
            } else {
                ControlKind::Select
            }
        }
        "select" => ControlKind::Select,
        "string" => ControlKind::Text,
        "number" => match (spec.min, spec.max, spec.step) {
            (Some(min), Some(max), Some(step))
                if step > 0.0 && max >= min && (max - min) / step <= 100.0 =>
            {
                ControlKind::NumberSlider
            }
            _ => ControlKind::NumberText,
        },
        "boolean" => ControlKind::Boolean,
        "color" => ControlKind::Color,
        "location" => ControlKind::Location,
        _ => ControlKind::Unknown,
    }
}

/// "Every 15 minutes"-style label for second-valued options
/// (`unit: "s"`). Falls back to "Every {n} s" for awkward values.
pub fn humanize_secs(n: u64) -> String {
    match n {
        3600 => "Every hour".to_string(),
        n if n > 0 && n % 3600 == 0 => format!("Every {} hours", n / 3600),
        60 => "Every minute".to_string(),
        n if n > 0 && n % 60 == 0 => format!("Every {} minutes", n / 60),
        n => format!("Every {n} s"),
    }
}

/// Display label for one option value. `unit: "s"` gets the
/// humanized refresh phrasing; other units append verbatim.
pub fn value_label(v: &Value, unit: Option<&str>) -> String {
    match (unit, v) {
        (Some("s"), Value::Number(n)) => {
            if let Some(secs) = n.as_u64() {
                humanize_secs(secs)
            } else {
                format!("{n} s")
            }
        }
        (Some(u), Value::Number(n)) => format!("{n}{u}"),
        (_, Value::String(s)) => s.clone(),
        (_, other) => other.to_string(),
    }
}

/// Write one option edit to the scoped bag (spec §6): Global →
/// `widgets.global[id][key]`, Instance → `widgets.instances[ikey][id][key]`.
pub fn write_option(
    store: &mut WidgetStore,
    widget_id: &str,
    instance_key: &str,
    scope: WidgetScope,
    key: &str,
    value: Value,
) {
    match scope {
        WidgetScope::Global => {
            store
                .global
                .entry(widget_id.to_string())
                .or_default()
                .insert(key.to_string(), value);
        }
        WidgetScope::Instance => {
            store
                .instances
                .entry(instance_key.to_string())
                .or_default()
                .entry(widget_id.to_string())
                .or_default()
                .insert(key.to_string(), value);
        }
    }
}

/// Per-option reset (spec §6 table): Instance scope removes the key
/// from the instance bag ("Reset to global"); Global removes it from
/// the global bag ("Reset to default"). Empty bags are pruned so
/// config.json stays tidy.
pub fn reset_option(
    store: &mut WidgetStore,
    widget_id: &str,
    instance_key: &str,
    scope: WidgetScope,
    key: &str,
) {
    match scope {
        WidgetScope::Global => {
            if let Some(bag) = store.global.get_mut(widget_id) {
                bag.remove(key);
                if bag.is_empty() {
                    store.global.remove(widget_id);
                }
            }
        }
        WidgetScope::Instance => {
            if let Some(bags) = store.instances.get_mut(instance_key) {
                if let Some(bag) = bags.get_mut(widget_id) {
                    bag.remove(key);
                    if bag.is_empty() {
                        bags.remove(widget_id);
                    }
                }
                if bags.is_empty() {
                    store.instances.remove(instance_key);
                }
            }
        }
    }
}

/// Whether the scoped bag currently holds an override for `key` —
/// drives the per-option reset button's enabled state.
pub fn has_override(
    store: &WidgetStore,
    widget_id: &str,
    instance_key: &str,
    scope: WidgetScope,
    key: &str,
) -> bool {
    match scope {
        WidgetScope::Global => store
            .global
            .get(widget_id)
            .is_some_and(|b| b.contains_key(key)),
        WidgetScope::Instance => store
            .instances
            .get(instance_key)
            .and_then(|bags| bags.get(widget_id))
            .is_some_and(|b| b.contains_key(key)),
    }
}

/// Every slice across all pages referencing the custom widget `id` —
/// `(page display name, slot index)`. Drives the global-scope blast
/// radius banner + the "Shared by {n} instances" count.
pub fn affected_instances(pages: &[RadialPage], widget_id: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (pi, page) in pages.iter().enumerate() {
        let page_name = if page.name.trim().is_empty() {
            format!("Page {}", pi + 1)
        } else {
            page.name.clone()
        };
        for (si, slice) in page.slices.iter().enumerate() {
            let is_ref = slice.kind == ActionKind::Widget
                && slice
                    .widget
                    .as_ref()
                    .map(|w| matches!(&w.source, WidgetSource::Custom(id) if id == widget_id))
                    .unwrap_or(false);
            if is_ref {
                out.push((page_name.clone(), si));
            }
        }
    }
    out
}

/// The slice's instance key — stored when present, derived from
/// page/slot for hand-edited configs (the handlers write the derived
/// key back into the slice on first edit).
pub fn effective_instance_key(stored: Option<&str>, page_name: &str, slot: usize) -> String {
    match stored {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => oxidemx_shared::widgets::instance_key(page_name, slot),
    }
}

/// Mono breadcrumb of the exact config path being written (§10d).
pub fn breadcrumb(scope: WidgetScope, widget_id: &str, instance_key: &str) -> String {
    match scope {
        WidgetScope::Instance => {
            format!("config.json → widgets.instances[\"{instance_key}\"].{widget_id}")
        }
        WidgetScope::Global => format!("config.json → widgets.global[\"{widget_id}\"]"),
    }
}

/// Parse a number-control text edit. Invalid input → `None` (no
/// write). Whole numbers store as JSON integers so the bag matches
/// what manifests declare (`"default": 900`, not `900.0`).
pub fn parse_number(s: &str) -> Option<Value> {
    let v: f64 = s.trim().parse().ok()?;
    if !v.is_finite() {
        return None;
    }
    Some(number_value(v))
}

/// f64 → JSON number, preferring integer representation.
pub fn number_value(v: f64) -> Value {
    if v.fract() == 0.0 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
        Value::from(v as i64)
    } else {
        Value::from(v)
    }
}

// ============================================================================
// View — the options card (and the incompatible note)
// ============================================================================

/// Rendered under the behavior chip in the slice editor. Decides
/// internally what (if anything) applies:
///   * Custom widget, Ready, has options → the full card
///   * Custom widget, Incompatible       → reason line (spec §9)
///   * Custom widget, missing            → nothing (the chip itself
///     shows the missing-widget summary + Reinstall, spec §10e)
///   * anything else                     → nothing
pub fn options_section<'a>(state: &'a State, idx: usize, slice: &'a Slice) -> Element<'a, Message> {
    let nothing = || -> Element<'a, Message> { Space::new().height(Length::Fixed(0.0)).into() };
    if slice.kind != ActionKind::Widget {
        return nothing();
    }
    let Some(cfg) = slice.widget.as_ref() else {
        return nothing();
    };
    let WidgetSource::Custom(id) = &cfg.source else {
        return nothing();
    };
    let Some(summary) = state.widget_registry.iter().find(|w| w.id == *id) else {
        // Missing widget — the chip carries the summary + Reinstall.
        return nothing();
    };
    if !summary.ready {
        let reason = summary.reason.as_deref().unwrap_or("incompatible");
        return container(
            text(format!("Widget incompatible — {reason}"))
                .size(11)
                .style(style::text_faint(&state.palette)),
        )
        .padding(8)
        .width(Length::Fill)
        .style(style::card_quiet(&state.palette))
        .into();
    }
    let Some(manifest) = state.widget_manifests.get(id) else {
        return nothing();
    };
    if manifest.options.is_empty() {
        return nothing();
    }
    options_card(state, idx, slice, summary, manifest)
}

fn options_card<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
    summary: &'a super::picker::WidgetSummaryLite,
    manifest: &'a WidgetManifest,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let cfg = slice.widget.as_ref().expect("checked by caller");
    let scope = cfg.scope;
    let page_name = state
        .config
        .radial_menu
        .pages
        .get(state.active_page)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let ikey = effective_instance_key(cfg.instance_key.as_deref(), &page_name, idx);
    let defaults: JsonBag = manifest.defaults();
    let resolved = state
        .config
        .widgets
        .resolve(&manifest.id, Some(&ikey), scope, &defaults);
    let affected = affected_instances(&state.config.radial_menu.pages, &manifest.id);

    let mut col = column![header_row(state, summary, manifest)].spacing(10);

    // Scope toggle FIRST (spec §10d) — where edits land must be the
    // first decision the user sees.
    col = col.push(scope_toggle(
        pal,
        idx,
        scope,
        &manifest.name,
        &page_name,
        affected.len(),
    ));
    if scope == WidgetScope::Global {
        col = col.push(blast_radius_banner(pal, &affected, &page_name, idx));
    }

    for spec in &manifest.options {
        col = col.push(option_row(
            state,
            idx,
            &manifest.id,
            &ikey,
            scope,
            spec,
            resolved.get(&spec.key),
        ));
    }

    col = col.push(footer_row(pal, scope, &manifest.id, &ikey));

    // Live wedge preview on the right column while the preview
    // worker tracks this instance (Plan 3 Task 5). `None` for one
    // frame after selection until the post-update sync spawns it.
    let body: Element<Message> =
        match crate::widget_preview::preview_element(state, &manifest.id, &ikey, slice) {
            Some(preview) => row![
                container(col).width(Length::FillPortion(3)),
                container(preview).padding([4, 0]),
            ]
            .spacing(12)
            .into(),
            None => col.into(),
        };

    container(body)
        .padding(12)
        .width(Length::Fill)
        .style(card_accent(pal))
        .into()
}

/// Header: icon, "{Name} — widget options", declared-by line,
/// version + author tags.
fn header_row<'a>(
    state: &'a State,
    summary: &'a super::picker::WidgetSummaryLite,
    manifest: &'a WidgetManifest,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let icon: Element<Message> = summary
        .icon_path
        .as_ref()
        .and_then(|p| {
            crate::radial_preview::resolve_icon_handle(
                &state.icons,
                &state.iced_handles,
                &p.to_string_lossy(),
                24,
                pal.text,
            )
        })
        .map(|h| -> Element<Message> {
            iced::widget::image(h)
                .width(Length::Fixed(24.0))
                .height(Length::Fixed(24.0))
                .into()
        })
        .unwrap_or_else(|| {
            text(
                manifest
                    .name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
            )
            .size(14)
            .style(style::text_accent(pal))
            .into()
        });

    row![
        icon,
        column![
            text(format!("{} — widget options", manifest.name)).size(13),
            text("Declared by the widget · rendered by OxideMX")
                .size(9)
                .style(style::text_faint(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        container(
            text(format!("v{}", manifest.version))
                .size(9)
                .style(style::text_dim(pal))
        )
        .padding([2, 6])
        .style(style::chip(pal)),
        container(
            text(manifest.author.clone())
                .size(9)
                .style(style::text_dim(pal))
        )
        .padding([2, 6])
        .style(style::chip(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// "This slice only / All {Name} slices" — two segmented buttons.
fn scope_toggle<'a>(
    pal: &'a Palette,
    idx: usize,
    scope: WidgetScope,
    widget_name: &str,
    page_name: &str,
    instance_count: usize,
) -> Element<'a, Message> {
    let page_display = if page_name.trim().is_empty() {
        "this page".to_string()
    } else {
        page_name.to_string()
    };
    let instance_sub = format!("Stored with {page_display} › Slot {}", idx + 1);
    let global_sub = format!(
        "Shared by {instance_count} instance{}",
        if instance_count == 1 { "" } else { "s" }
    );

    let seg = |title: String, sub: String, target: WidgetScope, active: bool| {
        button(
            column![
                text(title).size(11),
                text(sub).size(8).style(style::text_faint(pal)),
            ]
            .spacing(2),
        )
        .padding(8)
        .width(Length::FillPortion(1))
        .style(seg_style(pal, active))
        .on_press(Message::SetWidgetScope(idx, target))
    };

    row![
        seg(
            "This slice only".to_string(),
            instance_sub,
            WidgetScope::Instance,
            scope == WidgetScope::Instance,
        ),
        seg(
            format!("All {widget_name} slices"),
            global_sub,
            WidgetScope::Global,
            scope == WidgetScope::Global,
        ),
    ]
    .spacing(8)
    .into()
}

/// Global-scope banner: every affected instance as a chip, the
/// currently-edited one highlighted (spec §6 — blast radius is
/// explicit before the user types).
fn blast_radius_banner<'a>(
    pal: &'a Palette,
    affected: &[(String, usize)],
    current_page: &str,
    current_slot: usize,
) -> Element<'a, Message> {
    let mut chips = row![].spacing(6);
    let current_page_display = if current_page.trim().is_empty() {
        // affected_instances substitutes "Page N" for unnamed pages;
        // an unnamed current page can't be matched by name, so the
        // slot index alone decides the highlight in that edge case.
        String::new()
    } else {
        current_page.to_string()
    };
    for (page, slot) in affected {
        let is_current = *slot == current_slot
            && (page == &current_page_display || current_page_display.is_empty());
        let label = format!("{page} · Slot {}", slot + 1);
        let chip: Element<Message> = if is_current {
            container(text(label).size(9).style(style::text_accent(pal)))
                .padding([2, 6])
                .style(chip_accent(pal))
                .into()
        } else {
            container(text(label).size(9).style(style::text_dim(pal)))
                .padding([2, 6])
                .style(style::chip(pal))
                .into()
        };
        chips = chips.push(chip);
    }
    container(
        column![
            text("Edits here update every instance of this widget:")
                .size(10)
                .style(style::text_dim(pal)),
            chips,
        ]
        .spacing(6),
    )
    .padding(8)
    .width(Length::Fill)
    .style(style::card_quiet(pal))
    .into()
}

/// Footer: THIS SLICE / GLOBAL tag + mono breadcrumb of the exact
/// config path being written.
fn footer_row<'a>(
    pal: &'a Palette,
    scope: WidgetScope,
    widget_id: &str,
    instance_key: &str,
) -> Element<'a, Message> {
    let tag = match scope {
        WidgetScope::Instance => "THIS SLICE",
        WidgetScope::Global => "GLOBAL",
    };
    row![
        container(text(tag).size(8).style(style::text_accent(pal)))
            .padding([2, 6])
            .style(chip_accent(pal)),
        text(breadcrumb(scope, widget_id, instance_key))
            .size(9)
            .font(Font::MONOSPACE)
            .style(style::text_faint(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

// ============================================================================
// Per-option rows
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn option_row<'a>(
    state: &'a State,
    idx: usize,
    widget_id: &str,
    instance_key: &str,
    scope: WidgetScope,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let kind = control_kind(spec);

    let mut label_col = column![row![
        text(spec.label.clone()).size(12),
        if spec.required {
            text(" *").size(11).style(style::text_accent(pal))
        } else {
            text("")
        },
    ]]
    .spacing(2)
    .width(Length::FillPortion(2));
    if let Some(hint) = &spec.hint {
        label_col = label_col.push(text(hint.clone()).size(9).style(style::text_faint(pal)));
    }

    let control: Element<Message> = match kind {
        ControlKind::Segmented => segmented_control(pal, idx, spec, value),
        ControlKind::Select => select_control(pal, idx, spec, value),
        ControlKind::Text => text_control(idx, spec, value),
        ControlKind::NumberSlider => slider_control(pal, idx, spec, value),
        ControlKind::NumberText => number_text_control(idx, spec, value),
        ControlKind::Boolean => boolean_control(pal, idx, spec, value),
        ControlKind::Color => color_control(pal, idx, spec, value),
        ControlKind::Location => {
            // Location renders its own multi-line block (chip +
            // search + results), so it takes over the whole row.
            return location_control(state, idx, widget_id, instance_key, scope, spec, value);
        }
        ControlKind::Unknown => text("Update OxideMX to edit this option")
            .size(11)
            .style(style::text_faint(pal))
            .into(),
    };

    row![
        label_col,
        container(control).width(Length::FillPortion(3)),
        reset_button(state, idx, widget_id, instance_key, scope, &spec.key),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// Small "↺" per option — Instance scope: "Reset to global" (drops
/// the instance override); Global scope: "Reset to default" (drops
/// the global value). Disabled when the scoped bag has no override.
fn reset_button<'a>(
    state: &'a State,
    idx: usize,
    widget_id: &str,
    instance_key: &str,
    scope: WidgetScope,
    key: &str,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let overridden = has_override(&state.config.widgets, widget_id, instance_key, scope, key);
    let mut btn = button(icon("retry", 11.0, pal.text)).style(style::btn_secondary(pal));
    if overridden {
        btn = btn.on_press(Message::ResetWidgetOption {
            slice: idx,
            key: key.to_string(),
        });
    }
    btn.into()
}

/// enum ≤ 4 → segmented buttons.
fn segmented_control<'a>(
    pal: &'a Palette,
    idx: usize,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let mut r = row![].spacing(4);
    for v in spec.values.iter().flatten() {
        let active = value == Some(v);
        let label = value_label(v, spec.unit.as_deref());
        let key = spec.key.clone();
        let v_owned = v.clone();
        r = r.push(
            button(text(label).size(10))
                .padding([4, 10])
                .style(seg_style(pal, active))
                .on_press(Message::SetWidgetOption {
                    slice: idx,
                    key,
                    value: v_owned,
                }),
        );
    }
    r.into()
}

/// Wrapper so pick_list can show a friendly (unit-formatted) label
/// while storing the raw manifest value.
#[derive(Debug, Clone, PartialEq)]
struct SelectChoice {
    value: Value,
    display: String,
}

impl std::fmt::Display for SelectChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

/// select (or enum > 4) → dropdown.
fn select_control<'a>(
    pal: &'a Palette,
    idx: usize,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let options: Vec<SelectChoice> = spec
        .values
        .iter()
        .flatten()
        .map(|v| SelectChoice {
            value: v.clone(),
            display: value_label(v, spec.unit.as_deref()),
        })
        .collect();
    let selected = value.and_then(|v| options.iter().find(|o| &o.value == v).cloned());
    let key = spec.key.clone();
    iced::widget::pick_list(options, selected, move |choice: SelectChoice| {
        Message::SetWidgetOption {
            slice: idx,
            key: key.clone(),
            value: choice.value,
        }
    })
    .style(style::pick_list_style(pal))
    .text_size(11)
    .into()
}

/// string → text input. `maxlen` is enforced by truncating the edit.
fn text_control<'a>(
    idx: usize,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let current = value.and_then(|v| v.as_str()).unwrap_or("");
    let placeholder = spec.placeholder.clone().unwrap_or_default();
    let key = spec.key.clone();
    let maxlen = spec.maxlen;
    text_input(&placeholder, current)
        .on_input(move |mut s| {
            if let Some(max) = maxlen {
                let max = max as usize;
                if s.chars().count() > max {
                    s = s.chars().take(max).collect();
                }
            }
            Message::SetWidgetOption {
                slice: idx,
                key: key.clone(),
                value: Value::String(s),
            }
        })
        .padding(5)
        .size(11)
        .into()
}

/// number with a sane bounded range → slider + live value label.
fn slider_control<'a>(
    pal: &'a Palette,
    idx: usize,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let (min, max, step) = (
        spec.min.unwrap_or(0.0),
        spec.max.unwrap_or(100.0),
        spec.step.unwrap_or(1.0),
    );
    let current = value
        .and_then(Value::as_f64)
        .or(spec.default.as_ref().and_then(Value::as_f64))
        .unwrap_or(min)
        .clamp(min, max);
    let key = spec.key.clone();
    let display = value_label(&number_value(current), spec.unit.as_deref());
    row![
        iced::widget::slider(min..=max, current, move |v| Message::SetWidgetOption {
            slice: idx,
            key: key.clone(),
            value: number_value(v),
        })
        .step(step)
        .style(style::slider_style(pal))
        .width(Length::Fill),
        text(display).size(10).style(style::text_dim(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// number without a sliderable range → text input parsed on change;
/// invalid input is a no-op (no write).
fn number_text_control<'a>(
    idx: usize,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let current = value
        .and_then(Value::as_f64)
        .map(|v| {
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                v.to_string()
            }
        })
        .unwrap_or_default();
    let key = spec.key.clone();
    let (min, max) = (spec.min, spec.max);
    text_input("number", &current)
        .on_input(move |s| match parse_number(&s) {
            Some(v) => {
                // Out-of-range edits are swallowed like parse
                // failures — never write a value the widget's
                // declared bounds reject.
                let f = v.as_f64().unwrap_or(0.0);
                if min.is_some_and(|m| f < m) || max.is_some_and(|m| f > m) {
                    Message::Noop
                } else {
                    Message::SetWidgetOption {
                        slice: idx,
                        key: key.clone(),
                        value: v,
                    }
                }
            }
            None => Message::Noop,
        })
        .padding(5)
        .size(11)
        .width(Length::Fixed(120.0))
        .into()
}

/// boolean → switch.
fn boolean_control<'a>(
    pal: &'a Palette,
    idx: usize,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let current = value.and_then(Value::as_bool).unwrap_or(false);
    let key = spec.key.clone();
    container(
        toggler(current)
            .on_toggle(move |v| Message::SetWidgetOption {
                slice: idx,
                key: key.clone(),
                value: Value::Bool(v),
            })
            .style(style::toggler_style(pal)),
    )
    .align_x(iced::alignment::Horizontal::Right)
    .width(Length::Fill)
    .into()
}

/// color → swatch row over the slice palette tokens (same key list
/// as the slice colour pick_list; keeps the ring coherent, spec §5).
fn color_control<'a>(
    pal: &'a Palette,
    idx: usize,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let current = value.and_then(|v| v.as_str()).unwrap_or("");
    let mut r = row![].spacing(4);
    for key_name in super::SLICE_PALETTE_KEYS {
        let selected = current == key_name;
        let c = palette_color(pal, key_name);
        let key = spec.key.clone();
        r = r.push(
            button(
                Space::new()
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(16.0)),
            )
            .padding(2)
            .style(swatch_style(pal, c, selected))
            .on_press(Message::SetWidgetOption {
                slice: idx,
                key,
                value: Value::String(key_name.to_string()),
            }),
        );
    }
    r.into()
}

/// location → pinned chip + geocoder search + result list. Stores
/// `{"name", "lat", "lon"}` (spec §5); the shared Open-Meteo
/// geocoder does the lookup — widgets never see raw keystrokes.
#[allow(clippy::too_many_arguments)]
fn location_control<'a>(
    state: &'a State,
    idx: usize,
    widget_id: &str,
    instance_key: &str,
    scope: WidgetScope,
    spec: &'a OptionSpec,
    value: Option<&Value>,
) -> Element<'a, Message> {
    let pal = &state.palette;

    let mut label_col = column![row![
        text(spec.label.clone()).size(12),
        if spec.required {
            text(" *").size(11).style(style::text_accent(pal))
        } else {
            text("")
        },
    ]]
    .spacing(2)
    .width(Length::FillPortion(2));
    if let Some(hint) = &spec.hint {
        label_col = label_col.push(text(hint.clone()).size(9).style(style::text_faint(pal)));
    }

    // Pinned chip: the currently-stored place.
    let pinned: Element<Message> = match value.and_then(|v| v.get("name")).and_then(Value::as_str) {
        Some(name) => container(
            row![
                icon("pin", 10.0, pal.accent),
                text(name.to_string()).size(10)
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([2, 8])
        .style(chip_accent(pal))
        .into(),
        None => text("No location set")
            .size(10)
            .style(style::text_faint(pal))
            .into(),
    };

    // This control owns the shared search state only while it is the
    // active target — a second location option on the same card gets
    // an empty field until the user types into it.
    let is_target = state.widget_loc_target.as_ref() == Some(&(idx, spec.key.clone()));
    let query = if is_target {
        state.widget_loc_query.as_str()
    } else {
        ""
    };
    let key_for_input = spec.key.clone();
    let key_for_submit = spec.key.clone();
    let key_for_btn = spec.key.clone();
    let search_input = text_input(
        spec.placeholder
            .as_deref()
            .unwrap_or("City name (e.g. Oslo)…"),
        query,
    )
    .on_input(move |s| Message::WidgetLocQuery {
        slice: idx,
        key: key_for_input.clone(),
        text: s,
    })
    .on_submit(Message::WidgetLocSearch {
        slice: idx,
        key: key_for_submit,
    })
    .padding(5)
    .size(11)
    .width(Length::Fill);
    let search_btn: Element<Message> = if state.widget_loc_searching && is_target {
        button(text("Searching…").size(10))
            .style(style::btn_secondary(pal))
            .into()
    } else {
        button(text("Search").size(10))
            .style(style::btn_secondary(pal))
            .on_press(Message::WidgetLocSearch {
                slice: idx,
                key: key_for_btn,
            })
            .into()
    };

    let mut block = column![row![
        label_col,
        container(
            column![
                pinned,
                row![search_input, search_btn]
                    .align_y(Alignment::Center)
                    .spacing(6),
            ]
            .spacing(4)
        )
        .width(Length::FillPortion(3)),
        reset_button(state, idx, widget_id, instance_key, scope, &spec.key),
    ]
    .align_y(Alignment::Center)
    .spacing(8),]
    .spacing(4);

    if is_target {
        for hit in &state.widget_loc_results {
            let key = spec.key.clone();
            block = block.push(
                button(text(hit.name.clone()).size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::WidgetLocPick {
                        slice: idx,
                        key,
                        name: hit.name.clone(),
                        lat: hit.lat,
                        lon: hit.lon,
                    }),
            );
        }
    }

    block.into()
}

// ============================================================================
// Local styles
// ============================================================================

/// The options card container — accent-washed like the widget chip
/// so the chip + card read as one unit.
fn card_accent(pal: &Palette) -> impl Fn(&iced::Theme) -> iced::widget::container::Style + 'static {
    let bg = pal.accent_06;
    let border = pal.accent_40;
    let text_color = pal.text;
    move |_| iced::widget::container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Some(text_color),
        ..Default::default()
    }
}

/// Accent-bordered chip (highlighted banner instance, scope tag,
/// pinned location).
fn chip_accent(pal: &Palette) -> impl Fn(&iced::Theme) -> iced::widget::container::Style + 'static {
    let bg = pal.accent_06;
    let border = pal.accent;
    move |_| iced::widget::container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

/// Segmented-group button: accent ring + wash when active, quiet
/// surface otherwise (matches the picker tile styling language).
fn seg_style(
    pal: &Palette,
    active: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'static {
    let bg = if active { pal.accent_06 } else { pal.surface0 };
    let hover_bg = pal.row_hover;
    let border = if active { pal.accent } else { pal.hairline };
    let text_color = pal.text;
    move |_, status| {
        let hovered = matches!(status, iced::widget::button::Status::Hovered);
        iced::widget::button::Style {
            background: Some(Background::Color(if hovered && !active {
                hover_bg
            } else {
                bg
            })),
            text_color,
            border: Border {
                color: border,
                width: if active { 2.0 } else { 1.0 },
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

/// One colour swatch — the token colour fills the button; the
/// selected token gets an accent ring.
fn swatch_style(
    pal: &Palette,
    color: iced::Color,
    selected: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'static {
    let ring = if selected { pal.accent } else { pal.hairline };
    move |_, _| iced::widget::button::Style {
        background: Some(Background::Color(color)),
        border: Border {
            color: ring,
            width: if selected { 2.0 } else { 1.0 },
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

/// Slice palette token → display colour (same lookup the radial
/// preview uses for slice tints). Shared with the live wedge
/// preview (`widget_preview.rs`) for the wedge's hover wash.
pub(crate) fn palette_color(pal: &Palette, key: &str) -> iced::Color {
    match key {
        "green" => pal.green,
        "yellow" => pal.yellow,
        "red" => pal.red,
        "blue" => pal.blue,
        "mauve" => pal.mauve,
        "pink" => pal.pink,
        "peach" => pal.peach,
        "teal" => pal.teal,
        "sapphire" => pal.sapphire,
        "lavender" => pal.lavender,
        _ => pal.accent,
    }
}

// ============================================================================
// Tests — pure helpers only (view code is exercised by cargo check)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(kind: &str) -> OptionSpec {
        OptionSpec {
            key: "k".into(),
            kind: kind.into(),
            label: "K".into(),
            hint: None,
            required: false,
            values: None,
            default: None,
            unit: None,
            placeholder: None,
            maxlen: None,
            min: None,
            max: None,
            step: None,
        }
    }

    // --- control-kind mapping (spec §5 table) ---

    #[test]
    fn enum_maps_by_value_count() {
        let mut s = spec("enum");
        s.values = Some(vec![json!("a"), json!("b"), json!("c"), json!("d")]);
        assert_eq!(control_kind(&s), ControlKind::Segmented);
        s.values = Some(vec![
            json!("a"),
            json!("b"),
            json!("c"),
            json!("d"),
            json!("e"),
        ]);
        assert_eq!(control_kind(&s), ControlKind::Select);
    }

    #[test]
    fn number_maps_by_step_count() {
        let mut s = spec("number");
        assert_eq!(control_kind(&s), ControlKind::NumberText);
        s.min = Some(0.0);
        s.max = Some(100.0);
        s.step = Some(1.0);
        assert_eq!(control_kind(&s), ControlKind::NumberSlider); // 100 steps
        s.step = Some(0.5);
        assert_eq!(control_kind(&s), ControlKind::NumberText); // 200 steps
        s.step = Some(0.0);
        assert_eq!(control_kind(&s), ControlKind::NumberText); // degenerate
    }

    #[test]
    fn remaining_kinds_map_directly() {
        assert_eq!(control_kind(&spec("select")), ControlKind::Select);
        assert_eq!(control_kind(&spec("string")), ControlKind::Text);
        assert_eq!(control_kind(&spec("boolean")), ControlKind::Boolean);
        assert_eq!(control_kind(&spec("color")), ControlKind::Color);
        assert_eq!(control_kind(&spec("location")), ControlKind::Location);
        // forward-compat: a type from a newer widget → disabled row
        assert_eq!(control_kind(&spec("matrix")), ControlKind::Unknown);
    }

    // --- humanize ---

    #[test]
    fn humanize_secs_known_values() {
        assert_eq!(humanize_secs(300), "Every 5 minutes");
        assert_eq!(humanize_secs(900), "Every 15 minutes");
        assert_eq!(humanize_secs(1800), "Every 30 minutes");
        assert_eq!(humanize_secs(3600), "Every hour");
        assert_eq!(humanize_secs(7200), "Every 2 hours");
        assert_eq!(humanize_secs(60), "Every minute");
        assert_eq!(humanize_secs(45), "Every 45 s");
    }

    #[test]
    fn value_label_applies_units() {
        assert_eq!(value_label(&json!(900), Some("s")), "Every 15 minutes");
        assert_eq!(value_label(&json!(1600), Some(" dpi")), "1600 dpi");
        assert_eq!(value_label(&json!("temp"), None), "temp");
        assert_eq!(value_label(&json!(true), None), "true");
    }

    // --- write-target path (scope → bag) ---

    #[test]
    fn write_option_targets_the_scoped_bag() {
        let mut store = WidgetStore::default();
        write_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "units",
            json!("f"),
        );
        assert_eq!(store.global["weather"]["units"], json!("f"));
        assert!(store.instances.is_empty());

        write_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Instance,
            "units",
            json!("c"),
        );
        assert_eq!(
            store.instances["apps.slot4"]["weather"]["units"],
            json!("c")
        );
        // global bag untouched by the instance write
        assert_eq!(store.global["weather"]["units"], json!("f"));
    }

    // --- reset removes the right key ---

    #[test]
    fn reset_option_removes_only_the_scoped_key() {
        let mut store = WidgetStore::default();
        write_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "units",
            json!("f"),
        );
        write_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "refresh",
            json!(300),
        );
        write_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Instance,
            "units",
            json!("c"),
        );

        // instance reset drops the override; global value survives
        reset_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Instance,
            "units",
        );
        assert!(!store.instances.contains_key("apps.slot4")); // pruned empty
        assert_eq!(store.global["weather"]["units"], json!("f"));

        // global reset drops only that key
        reset_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "units",
        );
        assert!(!store.global["weather"].contains_key("units"));
        assert_eq!(store.global["weather"]["refresh"], json!(300));

        // last key pruned → whole bag gone
        reset_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "refresh",
        );
        assert!(store.global.is_empty());
    }

    #[test]
    fn has_override_tracks_the_scoped_bag() {
        let mut store = WidgetStore::default();
        assert!(!has_override(
            &store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "units"
        ));
        write_option(
            &mut store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "units",
            json!("f"),
        );
        assert!(has_override(
            &store,
            "weather",
            "apps.slot4",
            WidgetScope::Global,
            "units"
        ));
        // instance scope checks the instance bag, not global
        assert!(!has_override(
            &store,
            "weather",
            "apps.slot4",
            WidgetScope::Instance,
            "units"
        ));
    }

    // --- affected-instances count ---

    fn widget_slice(id: &str) -> Slice {
        Slice {
            action_id: None,
            label: id.to_string(),
            kind: ActionKind::Widget,
            command: String::new(),
            color: "accent".into(),
            icon: String::new(),
            submenu: Vec::new(),
            visible_if: None,
            icon_untinted: false,
            description: String::new(),
            widget: Some(oxidemx_shared::WidgetConfig {
                source: WidgetSource::Custom(id.to_string()),
                format: None,
                scope: WidgetScope::Instance,
                instance_key: None,
            }),
            dial: None,
        }
    }

    fn exec_slice() -> Slice {
        Slice {
            action_id: None,
            label: "x".into(),
            kind: ActionKind::Exec,
            command: "true".into(),
            color: "accent".into(),
            icon: String::new(),
            submenu: Vec::new(),
            visible_if: None,
            icon_untinted: false,
            description: String::new(),
            widget: None,
            dial: None,
        }
    }

    fn page(name: &str, slices: Vec<Slice>) -> RadialPage {
        RadialPage {
            name: name.into(),
            slices,
            app_classes: Vec::new(),
            include_in_scroll: true,
            slot_count: 8,
        }
    }

    #[test]
    fn affected_instances_walks_all_pages() {
        let p1 = page(
            "Apps",
            vec![exec_slice(), widget_slice("weather"), widget_slice("clock")],
        );
        // unnamed → "Page 2"
        let p2 = page("", vec![widget_slice("weather")]);

        let hits = affected_instances(&[p1, p2], "weather");
        assert_eq!(
            hits,
            vec![("Apps".to_string(), 1), ("Page 2".to_string(), 0)]
        );
        // built-in widget sources never count
        let p3 = page(
            "X",
            vec![{
                let mut s = widget_slice("weather");
                s.widget.as_mut().unwrap().source = WidgetSource::Cpu;
                s
            }],
        );
        assert!(affected_instances(&[p3], "weather").is_empty());
    }

    // --- misc helpers ---

    #[test]
    fn effective_instance_key_prefers_stored() {
        assert_eq!(
            effective_instance_key(Some("apps.slot4"), "Other", 9),
            "apps.slot4"
        );
        assert_eq!(effective_instance_key(None, "My Page", 2), "my-page.slot2");
        assert_eq!(
            effective_instance_key(Some(""), "My Page", 2),
            "my-page.slot2"
        );
    }

    #[test]
    fn breadcrumb_shows_the_write_path() {
        assert_eq!(
            breadcrumb(WidgetScope::Instance, "weather", "apps.slot4"),
            "config.json → widgets.instances[\"apps.slot4\"].weather"
        );
        assert_eq!(
            breadcrumb(WidgetScope::Global, "weather", "apps.slot4"),
            "config.json → widgets.global[\"weather\"]"
        );
    }

    #[test]
    fn parse_number_rejects_garbage_and_keeps_integers() {
        assert_eq!(parse_number("900"), Some(json!(900)));
        assert_eq!(parse_number("1.5"), Some(json!(1.5)));
        assert_eq!(parse_number("1."), Some(json!(1)));
        assert_eq!(parse_number(" 42 "), Some(json!(42)));
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("NaN"), None);
        assert_eq!(parse_number("inf"), None);
    }
}

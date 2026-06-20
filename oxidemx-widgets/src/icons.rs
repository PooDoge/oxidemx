//! Monochrome line-icon set (P0 visual-system spec): 24×24 viewBox,
//! 1.7px stroke, round caps/joins, no fill, recolored per-use via iced's
//! `svg::Style.color`. Replaces the ad-hoc Unicode glyphs. SVG path data
//! is taken verbatim from the design doc.

#![allow(dead_code)]

use iced::widget::svg;
use iced::{Color, Element, Length};
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Inner SVG markup (paths) for each icon, keyed by name. The `sparkle`
/// glyph carries its own fill; the rest are stroked.
fn paths(name: &str) -> Option<&'static str> {
    Some(match name {
        "message" => {
            r#"<path d="M4 5h16a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H9l-4 4v-4H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/>"#
        }
        "flask" => {
            r#"<path d="M10 3v6l-5.2 8.4A2 2 0 0 0 6.5 21h11a2 2 0 0 0 1.7-3.1L14 9V3"/><path d="M9 3h6"/><path d="M7.5 14h9"/>"#
        }
        "memory" => {
            r#"<ellipse cx="12" cy="5.5" rx="7" ry="2.6"/><path d="M5 5.5v13c0 1.4 3.1 2.6 7 2.6s7-1.2 7-2.6v-13"/><path d="M5 12c0 1.4 3.1 2.6 7 2.6s7-1.2 7-2.6"/>"#
        }
        "clock" => r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>"#,
        "plus" => r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#,
        "close" => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
        "commandcenter" => {
            r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/><path d="M12 9h6M12 13h6"/>"#
        }
        "agents" => {
            r#"<circle cx="12" cy="12" r="2.4"/><circle cx="12" cy="4" r="1.8"/><circle cx="5" cy="18.5" r="1.8"/><circle cx="19" cy="18.5" r="1.8"/><path d="M12 6.4v3.2M10.3 13.4 6.2 17M13.7 13.4l4.1 3.6"/>"#
        }
        "mcp" => {
            r#"<path d="M9 2v4M15 2v4"/><rect x="7" y="6" width="10" height="6" rx="1.5"/><path d="M12 12v4a3 3 0 0 0 3 3h1"/>"#
        }
        "model" => {
            r#"<rect x="7" y="7" width="10" height="10" rx="1.5"/><rect x="10" y="10" width="4" height="4" rx="1"/><path d="M10 7V4M14 7V4M10 20v-3M14 20v-3M7 10H4M7 14H4M20 10h-3M20 14h-3"/>"#
        }
        "copy" => {
            r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>"#
        }
        "select" => {
            r#"<path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2"/><path d="M11 8v8M9 8h4M9 16h4"/>"#
        }
        "attach" => {
            r#"<path d="M21 8.5 12 17.5a4 4 0 0 1-5.7-5.7L13.5 4.6a2.5 2.5 0 0 1 3.6 3.5l-7.2 7.2a1 1 0 0 1-1.5-1.4l6.6-6.6"/>"#
        }
        "export" => {
            r#"<path d="M12 15V3"/><path d="M8 7l4-4 4 4"/><path d="M4 13v6a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-6"/>"#
        }
        "retry" => r#"<path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/>"#,
        "send" => r#"<path d="M22 2 11 13"/><path d="M22 2l-7 20-4-9-9-4z"/>"#,
        "stop" => r#"<rect x="6" y="6" width="12" height="12" rx="2.5"/>"#,
        "search" => r#"<circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/>"#,
        "pin" => r#"<path d="M12 17v5"/><path d="M9 3h6l-1 6 3 3v2H7v-2l3-3-1-6z"/>"#,
        "trash" => {
            r#"<path d="M4 7h16"/><path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"/><path d="M6 7l1 13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-13"/>"#
        }
        "chevron" => r#"<path d="M6 9l6 6 6-6"/>"#,
        "doc" => {
            r#"<path d="M14 3v5h5"/><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M9 13h6M9 17h4"/>"#
        }
        "sparkle" => {
            r#"<path d="M12 3l1.9 5.6L19.5 10.5l-5.6 1.9L12 18l-1.9-5.6L4.5 10.5l5.6-1.9z" fill="currentColor" stroke="none"/>"#
        }
        "pencil" => r#"<path d="M14.5 4.5l5 5M4 20l1-4L16.5 4.5a2.1 2.1 0 0 1 3 3L8 19z"/>"#,
        "clipboard" => r#"<rect x="5" y="4.5" width="14" height="16.5" rx="2.5"/><path d="M9 4.5a3 3 0 0 1 6 0M9 11h6M9 15h4"/>"#,
        "globe" => r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.6 2.5 2.6 15.5 0 18M12 3c-2.6 2.5-2.6 15.5 0 18"/>"#,
        "bolt" => r#"<path d="M13 2L5 13h6l-1 9 8-12h-6l1-8z" fill="currentColor" stroke="none"/>"#,
        "terminal" => {
            r#"<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M6 9l3 3-3 3M12 15h5"/>"#
        }
        "monitor" => {
            r#"<rect x="2" y="4" width="20" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>"#
        }
        "shield" => {
            r#"<path d="M12 3l8 3v6c0 5-3.5 7.5-8 9-4.5-1.5-8-4-8-9V6z"/><path d="M9 12l2 2 4-4"/>"#
        }
        "imgx" => {
            r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 16l5-5 4 4M21 14l-3-3-2 2"/><path d="M3 3l18 18"/>"#
        }
        "rename" => r#"<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>"#,
        "check" => r#"<path d="M5 12.5l4.5 4.5L19 7"/>"#,
        // Directional chevrons (the base `chevron` points down).
        "chevron-up" => r#"<path d="M6 15l6-6 6 6"/>"#,
        "chevron-left" => r#"<path d="M15 6l-6 6 6 6"/>"#,
        "chevron-right" => r#"<path d="M9 6l6 6-6 6"/>"#,
        // Full arrows (reorder / navigation / move).
        "arrow-up" => r#"<path d="M12 19V5"/><path d="M6 11l6-6 6 6"/>"#,
        "arrow-down" => r#"<path d="M12 5v14"/><path d="M6 13l6 6 6-6"/>"#,
        "arrow-left" => r#"<path d="M19 12H5"/><path d="M11 6l-6 6 6 6"/>"#,
        "arrow-right" => r#"<path d="M5 12h14"/><path d="M13 6l6 6-6 6"/>"#,
        // Solid play triangle (test / preview / run).
        "play" => r#"<path d="M7 4.5v15l13-7.5z" fill="currentColor" stroke="none"/>"#,
        // Settings gear.
        "gear" => {
            r#"<circle cx="12" cy="12" r="3.2"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>"#
        }
        // Outline circle (radio / indicator placeholder) + crosshair.
        "circle" => r#"<circle cx="12" cy="12" r="8"/>"#,
        "target" => r#"<circle cx="12" cy="12" r="8"/><path d="M12 2v4M12 18v4M2 12h4M18 12h4"/>"#,
        _ => return None,
    })
}

const NAMES: &[&str] = &[
    "message",
    "flask",
    "memory",
    "clock",
    "plus",
    "close",
    "commandcenter",
    "agents",
    "mcp",
    "model",
    "copy",
    "select",
    "attach",
    "export",
    "retry",
    "send",
    "stop",
    "search",
    "pin",
    "trash",
    "chevron",
    "doc",
    "sparkle",
    "terminal",
    "monitor",
    "shield",
    "imgx",
    "rename",
    "check",
    "chevron-up",
    "chevron-left",
    "chevron-right",
    "arrow-up",
    "arrow-down",
    "arrow-left",
    "arrow-right",
    "play",
    "gear",
    "circle",
    "target",
    "pencil",
    "clipboard",
    "globe",
    "bolt",
];

static HANDLES: Lazy<HashMap<&'static str, svg::Handle>> = Lazy::new(|| {
    NAMES
        .iter()
        .filter_map(|&name| {
            let inner = paths(name)?;
            let doc = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{inner}</svg>"#
            );
            Some((name, svg::Handle::from_memory(doc.into_bytes())))
        })
        .collect()
});

/// A recolored icon at `size` px square. Unknown names render empty.
pub fn icon<'a, Message: 'a>(name: &str, size: f32, color: Color) -> Element<'a, Message> {
    let Some(handle) = HANDLES.get(name).cloned() else {
        return iced::widget::Space::new().into();
    };
    svg(handle)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

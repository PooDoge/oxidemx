//! SVG icons for the Composer, ported from design/composer/icons.jsx (24px feather/lucide style).
use freya::prelude::*;

/// Inner SVG markup (paths only) for `name`, or `None` if unmapped.
pub fn svg_for(name: &str) -> Option<&'static str> {
    Some(match name {
        "plus" => r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#,
        "send" => r#"<path d="M22 2L11 13M22 2l-7 20-4-9-9-4z"/>"#,
        "stop" => r#"<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/>"#,
        "chevronDown" => r#"<path d="M6 9l6 6 6-6"/>"#,
        "sparkle" => r#"<path d="M12 3l1.9 5.6L19.5 10.5l-5.6 1.9L12 18l-1.9-5.6L4.5 10.5l5.6-1.9zM19 3l.7 2 2 .7-2 .7-.7 2-.7-2-2-.7 2-.7zM19 16l.6 1.7 1.7.6-1.7.6-.6 1.7-.6-1.7-1.7-.6 1.7-.6z" fill="currentColor" stroke="none"/>"#,
        "brain" => r#"<path d="M9.5 3A2.5 2.5 0 0 0 7 5.5v.6A3 3 0 0 0 4.5 9 3 3 0 0 0 3 11.6 3 3 0 0 0 4.6 14 2.8 2.8 0 0 0 4 15.8 2.8 2.8 0 0 0 6.8 18.6 2.5 2.5 0 0 0 9.3 21h.2A2.5 2.5 0 0 0 12 18.5v-13A2.5 2.5 0 0 0 9.5 3z"/><path d="M14.5 3A2.5 2.5 0 0 1 17 5.5v.6A3 3 0 0 1 19.5 9a3 3 0 0 1 1.5 2.6 3 3 0 0 1-1.6 2.4 2.8 2.8 0 0 1 .6 1.8 2.8 2.8 0 0 1-2.8 2.8A2.5 2.5 0 0 1 14.7 21h-.2A2.5 2.5 0 0 1 12 18.5"/>"#,
        "chip" => r#"<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="10" y="10" width="4" height="4"/><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"/>"#,
        "folder" => r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
        "clipboard" => r#"<rect x="5" y="4" width="14" height="17" rx="2"/><rect x="9" y="2" width="6" height="4" rx="1"/>"#,
        "terminal" => r#"<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M6 9l3 3-3 3"/><path d="M12 15h6"/>"#,
        "camera" => r#"<path d="M3 8a2 2 0 0 1 2-2h2l2-2h6l2 2h2a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><circle cx="12" cy="13" r="3.5"/>"#,
        "download" => r#"<path d="M12 3v12"/><path d="M7 11l5 5 5-5"/><path d="M4 21h16"/>"#,
        "gear" => r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3h.1a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8v.1a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>"#,
        "check" => r#"<path d="M4 12l5 5L20 6"/>"#,
        "close" => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
        "disk" => r#"<rect x="3" y="7" width="18" height="10" rx="2"/><circle cx="17" cy="12" r="1" fill="currentColor" stroke="none"/><path d="M6 12h5"/>"#,
        "switch" => r#"<path d="M7 7h12"/><path d="M15 3l4 4-4 4"/><path d="M17 17H5"/><path d="M9 13l-4 4 4 4"/>"#,
        _ => return None,
    })
}

fn hex(c: Color) -> String { format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b()) }

/// Full standalone SVG string with the stroke color injected.
pub fn svg_string(name: &str, color: Color) -> String {
    let paths = svg_for(name).unwrap_or("");
    let color_hex = hex(color);
    let svg = format!(
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="{}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        &color_hex, paths
    );
    svg.replace("fill=\"currentColor\"", &format!("fill=\"{}\"", color_hex))
}

/// Render an icon as a Freya element at `size` px with the given stroke `color`.
pub fn icon(name: &str, size: f32, color: Color) -> Element {
    svg(svg_string(name, color).into_bytes())
        .width(Size::px(size))
        .height(Size::px(size))
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya::prelude::Color;

    #[test]
    fn known_icon_renders_non_empty_svg() {
        assert!(svg_for("send").is_some());
        assert!(svg_for("plus").is_some());
        assert!(svg_for("definitely-not-an-icon").is_none());
    }

    #[test]
    fn color_is_injected() {
        let s = svg_string("send", Color::from_rgb(0, 212, 255));
        assert!(s.contains("stroke=\"#00d4ff\"") || s.contains("rgb(0, 212, 255)"));
        assert!(!s.contains("currentColor"));
    }

    #[test]
    fn fill_color_injected_for_fill_icons() {
        let s = svg_string("stop", Color::from_rgb(0, 212, 255));
        assert!(s.contains("fill=\"#00d4ff\""));
        assert!(!s.contains("currentColor"));

        let s2 = svg_string("sparkle", Color::from_rgb(100, 150, 200));
        assert!(s2.contains("fill=\"#6496c8\""));
        assert!(!s2.contains("currentColor"));

        let s3 = svg_string("disk", Color::from_rgb(255, 100, 50));
        assert!(s3.contains("fill=\"#ff6432\""));
        assert!(!s3.contains("currentColor"));
    }
}

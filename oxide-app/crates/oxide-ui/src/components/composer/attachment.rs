//! Attachment chips + attach-source registry for the Composer (Task 6).
//!
//! `ATTACH_SOURCES` lists the six attachment types a user can pick from.
//! `sample_attachment(source_id)` returns a pre-filled `Attachment` for each.
//! `AttachmentChip` renders a single tone-tinted pill with a remove button.
//! `AttachmentRow` renders a horizontal row of chips (caller only emits it when non-empty).
use freya::prelude::*;
use crate::tokens::{Theme, Tone};
use super::icons::icon;

// ── Data types ────────────────────────────────────────────────────────────────

/// A file / context attachment that has been added to the composer.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub icon: &'static str,
    pub tone: Tone,
    pub name: String,
    pub meta: String,
}

/// One entry in the attach-source picker.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachSource {
    pub id:    &'static str,
    pub icon:  &'static str,
    pub label: &'static str,
    pub hint:  &'static str,
}

/// The six attachment sources surfaced in the attach menu.
/// Labels and hints are verbatim from `design/composer/composer-feature.jsx` ATTACH_SOURCES.
pub const ATTACH_SOURCES: [AttachSource; 6] = [
    AttachSource { id: "upload", icon: "disk",      label: "Upload file",         hint: "from disk"         },
    AttachSource { id: "repo",   icon: "folder",    label: "Reference repo file", hint: "@ file in project" },
    AttachSource { id: "image",  icon: "camera",    label: "Image / screenshot",  hint: "png · jpg"         },
    AttachSource { id: "paste",  icon: "clipboard", label: "Paste from clipboard",hint: "current contents"  },
    AttachSource { id: "code",   icon: "terminal",  label: "Code snippet",        hint: "fenced block"      },
    AttachSource { id: "camera", icon: "camera",    label: "Camera",              hint: "capture now"       },
];

/// Return a sample `Attachment` for the given `source_id`, or `None` if unknown.
pub fn sample_attachment(source_id: &str) -> Option<Attachment> {
    let a = match source_id {
        "upload" => Attachment {
            icon: "disk",
            tone: Tone::Blue,
            name: "metrics-export.csv".to_string(),
            meta: "42 KB".to_string(),
        },
        "repo" => Attachment {
            icon: "folder",
            tone: Tone::Accent,
            name: "run_bridge.rs".to_string(),
            meta: "agentd/src".to_string(),
        },
        "image" => Attachment {
            icon: "camera",
            tone: Tone::Mauve,
            name: "screenshot.png".to_string(),
            meta: "1440×900".to_string(),
        },
        "paste" => Attachment {
            icon: "clipboard",
            tone: Tone::Teal,
            name: "Clipboard".to_string(),
            meta: "text · 1.2 KB".to_string(),
        },
        "code" => Attachment {
            icon: "terminal",
            tone: Tone::Green,
            name: "snippet.ts".to_string(),
            meta: "12 lines".to_string(),
        },
        "camera" => Attachment {
            icon: "camera",
            tone: Tone::Peach,
            name: "capture.jpg".to_string(),
            meta: "live".to_string(),
        },
        _ => return None,
    };
    Some(a)
}

// ── AttachmentChip ────────────────────────────────────────────────────────────

/// A tone-tinted pill showing icon + name + meta + a remove button.
///
/// Builder usage:
/// ```ignore
/// AttachmentChip::new(att, theme)
///     .on_remove(|_: ()| println!("removed"))
/// ```
#[derive(Clone, PartialEq)]
pub struct AttachmentChip {
    att:       Attachment,
    theme:     Theme,
    on_remove: Option<EventHandler<()>>,
}

impl AttachmentChip {
    pub fn new(att: Attachment, theme: Theme) -> Self {
        Self { att, theme, on_remove: None }
    }

    pub fn on_remove(mut self, handler: impl Into<EventHandler<()>>) -> Self {
        self.on_remove = Some(handler.into());
        self
    }
}

impl Component for AttachmentChip {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let tone_color = th.tone(self.att.tone);
        let fill   = Theme::with_alpha(tone_color, 0x16);
        let border = Theme::with_alpha(tone_color, 0x33);

        // The name label uses Size::flex so the row needs Content::Flex.
        let name_label = label()
            .text(self.att.name.clone())
            .font_size(11.5)
            .color(th.text())
            .max_lines(1_usize)
            .into_element();

        let meta_label = label()
            .text(self.att.meta.clone())
            .font_size(10.)
            .color(th.subtext())
            .max_lines(1_usize)
            .into_element();

        // Close / remove button
        let remove_btn: Element = {
            let handler = self.on_remove.clone();
            let btn = rect()
                .width(Size::px(16.))
                .height(Size::px(16.))
                .main_align(Alignment::Center)
                .cross_align(Alignment::Center)
                .corner_radius(CornerRadius::new_all(999.))
                .background(Theme::with_alpha(tone_color, 0x22));
            if let Some(h) = handler {
                btn.on_press(move |_: Event<PressEventData>| h.call(()))
                    .child(icon("close", 10., tone_color))
                    .into_element()
            } else {
                btn.child(icon("close", 10., tone_color))
                    .into_element()
            }
        };

        // Name + meta stacked vertically inside a flex column
        let text_col = rect()
            .direction(Direction::Vertical)
            .width(Size::flex(1.0))
            .child(name_label)
            .child(meta_label)
            .into_element();

        rect()
            .direction(Direction::Horizontal)
            // Content::Flex required because text_col has Size::flex(1.0)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(5.)
            .padding(Gaps::new(4., 8., 4., 6.))
            .corner_radius(CornerRadius::new_all(10.))
            .background(fill)
            .border(Border::new().fill(border).width(1.))
            .child(icon(self.att.icon, 14., tone_color))
            .child(text_col)
            .child(remove_btn)
    }
}

// ── AttachmentRow ─────────────────────────────────────────────────────────────

/// A horizontal row of `AttachmentChip`s.
///
/// The caller is responsible for only rendering this when `items` is non-empty.
///
/// Builder usage:
/// ```ignore
/// AttachmentRow::new(attachments, theme)
///     .on_remove(|idx: usize| println!("remove attachment at {idx}"))
/// ```
#[derive(Clone, PartialEq)]
pub struct AttachmentRow {
    items:     Vec<Attachment>,
    theme:     Theme,
    on_remove: Option<EventHandler<usize>>,
}

impl AttachmentRow {
    pub fn new(items: Vec<Attachment>, theme: Theme) -> Self {
        Self { items, theme, on_remove: None }
    }

    pub fn on_remove(mut self, handler: impl Into<EventHandler<usize>>) -> Self {
        self.on_remove = Some(handler.into());
        self
    }
}

impl Component for AttachmentRow {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;

        let mut row = rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .width(Size::fill())
            .padding(Gaps::new(6., 12., 6., 12.));

        for (idx, att) in self.items.iter().enumerate() {
            let handler = self.on_remove.clone();
            let chip = AttachmentChip::new(att.clone(), th);
            let chip = if let Some(h) = handler {
                chip.on_remove(move |_: ()| h.call(idx))
            } else {
                chip
            };
            row = row.child(chip);
        }

        row
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_attach_sources() {
        assert_eq!(ATTACH_SOURCES.len(), 6);
        assert_eq!(ATTACH_SOURCES[0].label, "Upload file");
        assert_eq!(ATTACH_SOURCES[1].label, "Reference repo file");
    }

    /// Guard against copy drift vs. `design/composer/composer-feature.jsx` ATTACH_SOURCES.
    #[test]
    fn attach_source_verbatim_copy() {
        // upload
        assert_eq!(ATTACH_SOURCES[0].label, "Upload file");
        assert_eq!(ATTACH_SOURCES[0].hint,  "from disk");
        // repo
        assert_eq!(ATTACH_SOURCES[1].label, "Reference repo file");
        assert_eq!(ATTACH_SOURCES[1].hint,  "@ file in project");
        // image
        assert_eq!(ATTACH_SOURCES[2].label, "Image / screenshot");
        assert_eq!(ATTACH_SOURCES[2].hint,  "png · jpg");
        // paste
        assert_eq!(ATTACH_SOURCES[3].label, "Paste from clipboard");
        assert_eq!(ATTACH_SOURCES[3].hint,  "current contents");
        // code
        assert_eq!(ATTACH_SOURCES[4].label, "Code snippet");
        assert_eq!(ATTACH_SOURCES[4].hint,  "fenced block");
        // camera
        assert_eq!(ATTACH_SOURCES[5].label, "Camera");
        assert_eq!(ATTACH_SOURCES[5].hint,  "capture now");
    }

    #[test]
    fn sample_payload_for_repo_source() {
        let a = sample_attachment("repo").unwrap();
        assert_eq!(a.name, "run_bridge.rs");
        assert_eq!(a.tone, crate::tokens::Tone::Accent);
        assert!(sample_attachment("nope").is_none());
    }
}

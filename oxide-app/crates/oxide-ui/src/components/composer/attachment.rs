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

/// The broad category of an attachment — determines how the data is handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachKind {
    Image,
    Text,
    File,
}

/// The payload carried by an attachment (may be absent until the user confirms).
#[derive(Clone, Debug, PartialEq)]
pub enum AttachData {
    Image(Vec<u8>),
    Text(String),
    None,
}

/// Map an icon name to the broad `AttachKind` it implies.
///
/// - `"camera"` → `Image`
/// - `"clipboard"` | `"terminal"` → `Text`
/// - anything else → `File`
pub fn attach_kind_for_icon(icon: &str) -> AttachKind {
    match icon {
        "camera"               => AttachKind::Image,
        "clipboard" | "terminal" => AttachKind::Text,
        _                      => AttachKind::File,
    }
}

/// A file / context attachment that has been added to the composer.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub icon: &'static str,
    pub tone: Tone,
    pub name: String,
    pub meta: String,
    pub kind: AttachKind,
    pub data: AttachData,
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
        "upload" => {
            let icon_name = "disk";
            Attachment {
                icon: icon_name,
                tone: Tone::Blue,
                name: "metrics-export.csv".to_string(),
                meta: "42 KB".to_string(),
                kind: attach_kind_for_icon(icon_name),
                data: AttachData::None,
            }
        },
        "repo" => {
            let icon_name = "folder";
            Attachment {
                icon: icon_name,
                tone: Tone::Accent,
                name: "run_bridge.rs".to_string(),
                meta: "agentd/src".to_string(),
                kind: attach_kind_for_icon(icon_name),
                data: AttachData::None,
            }
        },
        "image" => {
            let icon_name = "camera";
            Attachment {
                icon: icon_name,
                tone: Tone::Mauve,
                name: "screenshot.png".to_string(),
                meta: "1440×900".to_string(),
                kind: attach_kind_for_icon(icon_name),
                data: AttachData::None,
            }
        },
        "paste" => {
            let icon_name = "clipboard";
            Attachment {
                icon: icon_name,
                tone: Tone::Teal,
                name: "Clipboard".to_string(),
                meta: "text · 1.2 KB".to_string(),
                kind: attach_kind_for_icon(icon_name),
                data: AttachData::None,
            }
        },
        "code" => {
            let icon_name = "terminal";
            Attachment {
                icon: icon_name,
                tone: Tone::Green,
                name: "snippet.ts".to_string(),
                meta: "12 lines".to_string(),
                kind: attach_kind_for_icon(icon_name),
                data: AttachData::None,
            }
        },
        "camera" => {
            let icon_name = "camera";
            Attachment {
                icon: icon_name,
                tone: Tone::Peach,
                name: "capture.jpg".to_string(),
                meta: "live".to_string(),
                kind: attach_kind_for_icon(icon_name),
                data: AttachData::None,
            }
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
///
/// // compact clickable form (icon + name only, body fires on_view):
/// AttachmentChip::new(att, theme)
///     .compact(true)
///     .on_view(|_: ()| println!("view"))
///     .on_remove(|_: ()| println!("removed"))
/// ```
#[derive(Clone, PartialEq)]
pub struct AttachmentChip {
    att:       Attachment,
    theme:     Theme,
    on_remove: Option<EventHandler<()>>,
    on_view:   Option<EventHandler<()>>,
    compact:   bool,
}

impl AttachmentChip {
    pub fn new(att: Attachment, theme: Theme) -> Self {
        Self { att, theme, on_remove: None, on_view: None, compact: false }
    }

    pub fn on_remove(mut self, handler: impl Into<EventHandler<()>>) -> Self {
        self.on_remove = Some(handler.into());
        self
    }

    /// When `true`, renders a compact pill: icon + name only (no meta subtitle),
    /// `Content::Fit`, radius 8. The body fires `on_view`; the × fires `on_remove`
    /// with stop_propagation so it does not also trigger the body press.
    pub fn compact(mut self, yes: bool) -> Self {
        self.compact = yes;
        self
    }

    /// Called when the user clicks the chip body in compact mode.
    pub fn on_view(mut self, handler: impl Into<EventHandler<()>>) -> Self {
        self.on_view = Some(handler.into());
        self
    }
}

impl Component for AttachmentChip {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let tone_color = th.tone(self.att.tone);
        let fill   = Theme::with_alpha(tone_color, 0x16);
        let border = Theme::with_alpha(tone_color, 0x33);

        if self.compact {
            // ── compact layout: Content::Fit, icon + name only, radius 8 ─────
            //
            // Body on_press → on_view; × on_press → on_remove with stop_propagation
            // so the × does not also fire the body handler.

            let name_label = label()
                .text(self.att.name.clone())
                .font_size(11.5)
                .color(th.text())
                .max_lines(1_usize)
                .into_element();

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
                    btn.on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            h.call(());
                        })
                        .child(icon("close", 10., tone_color))
                        .into_element()
                } else {
                    btn.child(icon("close", 10., tone_color))
                        .into_element()
                }
            };

            let body = rect()
                .direction(Direction::Horizontal)
                .content(Content::Fit)
                .cross_align(Alignment::Center)
                .spacing(5.)
                .padding(Gaps::new(4., 8., 4., 6.))
                .corner_radius(CornerRadius::new_all(8.))
                .background(fill)
                .border(Border::new().fill(border).width(1.))
                .child(icon(self.att.icon, 14., tone_color))
                .child(name_label)
                .child(remove_btn);

            return if let Some(h) = self.on_view.clone() {
                body.on_press(move |_: Event<PressEventData>| h.call(()))
                    .into_element()
            } else {
                body.into_element()
            };
        }

        // ── full (non-compact) layout ─────────────────────────────────────────

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
            .into_element()
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

    // ── new tests (Task 1) ────────────────────────────────────────────────────

    #[test]
    fn kind_inferred_from_icon() {
        assert_eq!(attach_kind_for_icon("camera"),    AttachKind::Image);
        assert_eq!(attach_kind_for_icon("clipboard"), AttachKind::Text);
        assert_eq!(attach_kind_for_icon("terminal"),  AttachKind::Text);
        assert_eq!(attach_kind_for_icon("folder"),    AttachKind::File);
        assert_eq!(attach_kind_for_icon("disk"),      AttachKind::File);
    }

    #[test]
    fn sample_attachment_has_kind_and_no_data() {
        let a = sample_attachment("image").unwrap();
        assert_eq!(a.kind, AttachKind::Image);
        assert_eq!(a.data, AttachData::None);

        let b = sample_attachment("paste").unwrap();
        assert_eq!(b.kind, AttachKind::Text);
        assert_eq!(b.data, AttachData::None);

        let c = sample_attachment("repo").unwrap();
        assert_eq!(c.kind, AttachKind::File);
        assert_eq!(c.data, AttachData::None);
    }

    #[test]
    fn compact_chip_renders_name_and_fires_handlers() {
        use freya_testing::prelude::*;
        fn app() -> impl IntoElement {
            let att = sample_attachment("repo").unwrap();
            AttachmentChip::new(att, Theme::default()).compact(true)
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("run_bridge.rs"))
        });
        assert!(found.is_some(), "compact chip should render the attachment name");
    }
}

//! Lightbox popup for attachments (Task 3).
//!
//! `AttachmentViewer` wraps the Freya built-in `Popup` with two rendering
//! modes driven by the attachment kind:
//!
//! - `Image` kind + `Image(bytes)` data — image lightbox via `ImageViewer`
//!   with `AspectRatio::Min` (scale-to-fit) inside a max 720×540 box.
//! - `Image` kind + no bytes — large placeholder (48px icon + name).
//! - `Text` / `File` kinds — compact info card (20px icon + name + meta + type).
//!
//! Dismissal is fully handled by `Popup::on_close_request` (backdrop press +
//! Escape key). No bespoke global-press handler.
use freya::prelude::*;

use super::attachment::{AttachData, AttachKind, Attachment};
use super::icons::icon;
use crate::tokens::Theme;

// ── AttachmentViewer ─────────────────────────────────────────────────────────

/// Lightbox / info-card popup for a single attachment.
///
/// Always rendered visible — the caller gates visibility.
///
/// Builder usage:
/// ```ignore
/// AttachmentViewer {
///     attachment: att,
///     theme: th,
/// }
/// .on_dismiss(move |()| show_viewer.set(false))
/// ```
#[derive(Clone, PartialEq)]
pub struct AttachmentViewer {
    pub attachment: Attachment,
    pub theme: Theme,
    on_dismiss: Option<EventHandler<()>>,
}

impl AttachmentViewer {
    pub fn new(attachment: Attachment, theme: Theme) -> Self {
        Self { attachment, theme, on_dismiss: None }
    }

    pub fn on_dismiss(mut self, handler: impl Into<EventHandler<()>>) -> Self {
        self.on_dismiss = Some(handler.into());
        self
    }
}

impl Component for AttachmentViewer {
    fn render(&self) -> impl IntoElement {
        let att = self.attachment.clone();
        let th = self.theme;
        let on_dismiss = self.on_dismiss.clone();

        let tone_color = th.tone(att.tone);

        let body: Element = match (&att.kind, &att.data) {
            (AttachKind::Image, AttachData::Image(bytes)) => {
                let source: ImageSource = (att.name.clone(), Bytes::from(bytes.clone())).into();
                PopupContent::new()
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .cross_align(Alignment::Center)
                            .spacing(10.)
                            .child(
                                rect()
                                    .max_width(Size::px(720.))
                                    .max_height(Size::px(540.))
                                    .child(
                                        ImageViewer::new(source)
                                            .aspect_ratio(AspectRatio::Min),
                                    ),
                            )
                            .child(
                                label()
                                    .text(att.name.clone())
                                    .font_size(12.)
                                    .color(th.subtext()),
                            ),
                    )
                    .into_element()
            }
            (AttachKind::Image, _) => {
                PopupContent::new()
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .cross_align(Alignment::Center)
                            .spacing(12.)
                            .padding(Gaps::new(24., 32., 24., 32.))
                            .child(icon(att.icon, 48., tone_color))
                            .child(
                                label()
                                    .text(att.name.clone())
                                    .font_size(14.)
                                    .color(th.text()),
                            ),
                    )
                    .into_element()
            }
            _ => {
                let kind_label = format!("{:?}", att.kind);
                PopupContent::new()
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .spacing(6.)
                            .padding(Gaps::new(8., 16., 8., 16.))
                            .child(
                                rect()
                                    .direction(Direction::Horizontal)
                                    .cross_align(Alignment::Center)
                                    .spacing(8.)
                                    .child(icon(att.icon, 20., tone_color))
                                    .child(
                                        label()
                                            .text(att.name.clone())
                                            .font_size(14.)
                                            .font_weight(FontWeight::BOLD)
                                            .color(th.text()),
                                    ),
                            )
                            .child(
                                label()
                                    .text(att.meta.clone())
                                    .font_size(12.)
                                    .color(th.subtext()),
                            )
                            .child(
                                label()
                                    .text(kind_label)
                                    .font_size(11.)
                                    .color(th.faint()),
                            ),
                    )
                    .into_element()
            }
        };

        Popup::new()
            .on_close_request(move |_| {
                if let Some(h) = &on_dismiss {
                    h.call(());
                }
            })
            .maybe(true, |p| p.child(body))
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    use crate::components::composer::attachment::{sample_attachment, AttachData, AttachKind};

    /// Minimal 1×1 transparent PNG (67 bytes, valid PNG header).
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
        0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR length + "IHDR"
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // width=1, height=1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // bit depth=8, color=RGB, CRC
        0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, // IDAT length + "IDAT"
        0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, // deflate stream
        0x00, 0x00, 0x02, 0x00, 0x01, 0xe2, 0x21, 0xbc, // CRC
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, // IEND length + "IEND"
        0x44, 0xae, 0x42, 0x60, 0x82,                   // "IEND" CRC
    ];

    /// Image attachment with real bytes mounts without panic; the caption label
    /// rendered inside the PopupContent shows the filename.
    #[test]
    fn viewer_image_renders() {
        let mut att = sample_attachment("image").unwrap();
        att.kind = AttachKind::Image;
        att.data = AttachData::Image(TINY_PNG.to_vec());

        let att_clone = att.clone();
        let mut t = launch_test(move || {
            let att = att_clone.clone();
            let mut dismissed = use_state(|| false);
            rect()
                .width(Size::fill())
                .height(Size::fill())
                .child(
                    AttachmentViewer::new(att, Theme::default())
                        .on_dismiss(move |()| dismissed.set(true)),
                )
        });
        t.sync_and_update();

        let found = t.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("screenshot.png"))
        });
        assert!(found.is_some(), "image viewer should render the filename caption");
    }

    /// File attachment info card renders the attachment name and the kind label.
    #[test]
    fn viewer_infocard_for_file() {
        let att = sample_attachment("repo").unwrap();
        assert_eq!(att.kind, AttachKind::File);

        let att_clone = att.clone();
        let mut t = launch_test(move || {
            let att = att_clone.clone();
            let mut dismissed = use_state(|| false);
            rect()
                .width(Size::fill())
                .height(Size::fill())
                .child(
                    AttachmentViewer::new(att, Theme::default())
                        .on_dismiss(move |()| dismissed.set(true)),
                )
        });
        t.sync_and_update();

        let found_name = t.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("run_bridge.rs"))
        });
        assert!(found_name.is_some(), "info card should render the attachment name");

        let found_kind = t.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("File"))
        });
        assert!(found_kind.is_some(), "info card should render the attachment kind");
    }
}

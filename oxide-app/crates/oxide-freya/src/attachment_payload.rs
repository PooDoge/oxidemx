//! Maps `oxide_ui::components::composer::Attachment` → `oxide_client::dto::AttachmentPayload`.
//!
//! `to_payload` is the single conversion point: it encodes inline data as base64
//! and fills mime / kind from the attachment's `AttachData` / `AttachKind`.
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use oxide_client::dto::AttachmentPayload;
use oxide_ui::components::composer::attachment::{AttachData, AttachKind, Attachment};

/// Convert a composer `Attachment` to the wire `AttachmentPayload`.
///
/// - `AttachData::Image(bytes)` → `mime="image/png"`, `kind="image"`, data base64-encoded.
/// - `AttachData::Text(s)`     → `mime="text/plain"`, `kind="text"`, data base64-encoded.
/// - `AttachData::None`        → `data_b64=None`; kind derived from `att.kind`
///   (`Image`→`"image"`, `Text`→`"text"`, `File`→`"file"`);
///   mime=`"application/octet-stream"`.
pub fn to_payload(att: &Attachment) -> AttachmentPayload {
    match &att.data {
        AttachData::Image(bytes) => AttachmentPayload {
            name:     att.name.clone(),
            mime:     "image/png".into(),
            kind:     "image".into(),
            data_b64: Some(B64.encode(bytes)),
        },
        AttachData::Text(s) => AttachmentPayload {
            name:     att.name.clone(),
            mime:     "text/plain".into(),
            kind:     "text".into(),
            data_b64: Some(B64.encode(s.as_bytes())),
        },
        AttachData::None => {
            let kind = match att.kind {
                AttachKind::Image => "image",
                AttachKind::Text  => "text",
                AttachKind::File  => "file",
            };
            AttachmentPayload {
                name:     att.name.clone(),
                mime:     "application/octet-stream".into(),
                kind:     kind.into(),
                data_b64: None,
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;
    use oxide_ui::components::composer::attachment::{AttachData, AttachKind, Attachment};
    use oxide_ui::tokens::Tone;

    use super::to_payload;

    fn image_att_with_bytes(bytes: Vec<u8>) -> Attachment {
        Attachment {
            icon: "camera",
            tone: Tone::Mauve,
            name: "screenshot.png".to_string(),
            meta: "1440×900".to_string(),
            kind: AttachKind::Image,
            data: AttachData::Image(bytes),
        }
    }

    fn none_data_att(kind: AttachKind) -> Attachment {
        Attachment {
            icon: "disk",
            tone: Tone::Blue,
            name: "file.bin".to_string(),
            meta: "".to_string(),
            kind,
            data: AttachData::None,
        }
    }

    fn text_att(s: &str) -> Attachment {
        Attachment {
            icon: "clipboard",
            tone: Tone::Teal,
            name: "paste.txt".to_string(),
            meta: "".to_string(),
            kind: AttachKind::Text,
            data: AttachData::Text(s.to_string()),
        }
    }

    #[test]
    fn image_bytes_encode_and_decode() {
        let bytes = vec![1u8, 2, 3, 4];
        let att = image_att_with_bytes(bytes.clone());
        let p = to_payload(&att);

        assert_eq!(p.kind, "image", "kind must be \"image\" for AttachData::Image");
        assert_eq!(p.mime, "image/png");
        assert_eq!(p.name, "screenshot.png");

        let encoded = p.data_b64.expect("data_b64 must be Some for image bytes");
        let decoded = B64.decode(&encoded).expect("data_b64 must be valid base64");
        assert_eq!(decoded, bytes, "decoded bytes must round-trip to original");
    }

    #[test]
    fn none_data_image_kind_has_no_payload() {
        let att = none_data_att(AttachKind::Image);
        let p = to_payload(&att);
        assert_eq!(p.data_b64, None);
        assert_eq!(p.kind, "image");
        assert_eq!(p.mime, "application/octet-stream");
    }

    #[test]
    fn none_data_text_kind_maps_kind_string() {
        let att = none_data_att(AttachKind::Text);
        let p = to_payload(&att);
        assert_eq!(p.data_b64, None);
        assert_eq!(p.kind, "text");
    }

    #[test]
    fn none_data_file_kind_maps_kind_string() {
        let att = none_data_att(AttachKind::File);
        let p = to_payload(&att);
        assert_eq!(p.data_b64, None);
        assert_eq!(p.kind, "file");
    }

    #[test]
    fn text_data_encodes_and_decodes() {
        let s = "hello, world";
        let att = text_att(s);
        let p = to_payload(&att);

        assert_eq!(p.kind, "text");
        assert_eq!(p.mime, "text/plain");
        let encoded = p.data_b64.expect("data_b64 must be Some for text data");
        let decoded = B64.decode(&encoded).expect("valid base64");
        assert_eq!(decoded, s.as_bytes());
    }

    #[test]
    fn name_is_preserved_across_all_variants() {
        let img = to_payload(&image_att_with_bytes(vec![0]));
        assert_eq!(img.name, "screenshot.png");

        let none = to_payload(&none_data_att(AttachKind::File));
        assert_eq!(none.name, "file.bin");

        let txt = to_payload(&text_att("x"));
        assert_eq!(txt.name, "paste.txt");
    }
}

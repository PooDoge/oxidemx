//! The Composer: the chat input chassis (Slice 1).
pub mod activity_line;
pub mod attach_menu;
pub mod attachment;
pub mod config;
pub mod editor;
pub mod icons;
pub mod prediction;
pub mod provider_menu;
pub mod toolbar;
pub use activity_line::ActivityLine;
pub use attach_menu::AttachMenu;
pub use attachment::{Attachment, AttachSource, AttachmentChip, AttachmentRow, ATTACH_SOURCES, sample_attachment};
pub use config::{ComposerConfig, Model, Prediction, ProviderId, Thinking, DEFAULT_MODEL_ID, MODELS, model_by_id};
pub use editor::ComposerEditor;
pub use prediction::{predict, PredictMode, PredictionStrip, Suggestions};
pub use provider_menu::ProviderMenu;
pub use toolbar::{send_state, SendState, Toolbar};

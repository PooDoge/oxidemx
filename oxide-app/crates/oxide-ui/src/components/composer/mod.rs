//! The Composer: the chat input chassis (Slice 1).
pub mod attachment;
pub mod config;
pub mod icons;
pub mod prediction;
pub use attachment::{Attachment, AttachSource, AttachmentChip, AttachmentRow, ATTACH_SOURCES, sample_attachment};
pub use config::{ComposerConfig, Model, Prediction, ProviderId, Thinking, DEFAULT_MODEL_ID, MODELS, model_by_id};
pub use prediction::{predict, PredictMode, PredictionStrip, Suggestions};

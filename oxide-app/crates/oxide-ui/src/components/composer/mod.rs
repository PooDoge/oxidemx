//! The Composer: the chat input chassis (Slice 1).
pub mod config;
pub mod icons;
pub mod prediction;
pub use config::{ComposerConfig, Model, Prediction, ProviderId, Thinking, DEFAULT_MODEL_ID, MODELS, model_by_id};
pub use prediction::{predict, PredictMode, Suggestions};

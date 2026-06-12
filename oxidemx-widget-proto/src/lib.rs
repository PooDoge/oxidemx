//! Wire types shared by the OxideMX widget host (overlay-rs / settings-rs)
//! and widget guests (compiled to wasm32-wasip1).
//!
//! Everything crossing the wasm boundary is postcard-encoded inside a
//! tagged [`Envelope`] so unknown message kinds can be skipped instead of
//! failing the decode — that is the forward-compatibility story within an
//! `API_VERSION` (see the spec §2 / §8).

pub mod envelope;
pub mod event;
pub mod scene;
pub mod settings;
#[cfg(feature = "manifest")]
pub mod manifest;

pub use envelope::Envelope;
pub use event::{Event, HostCmd};
pub use scene::{Color, PathOp, Prim, Scene, Stroke, TextAlign, TextWeight, WedgeGeom};
pub use settings::SettingValue;
#[cfg(feature = "manifest")]
pub use manifest::{OptionSpec, SliceMeta, WidgetManifest};

/// Boundary API version. The host refuses widgets whose manifest
/// `api_version` is outside its supported range (just `== 1` for now).
pub const API_VERSION: u32 = 1;

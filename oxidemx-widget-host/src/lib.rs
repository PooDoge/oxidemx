//! OxideMX widget host — loads installed widget plugins (wasm32-wasip1
//! modules described by `widget.json`) and runs them under wasmi with
//! fuel metering and strike-based failure isolation.
//!
//! Wire types live in `oxidemx-widget-proto`; everything crossing the
//! wasm boundary goes through that crate's `envelope` functions.

pub mod instance;
pub mod registry;

pub use instance::{CallOutcome, InstanceError, WidgetInstance};
pub use registry::{InstalledWidget, WidgetRegistry, WidgetState};

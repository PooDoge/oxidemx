//! OxideMX widget host — loads installed widget plugins (wasm32-wasip1
//! modules described by `widget.json`) and runs them under wasmi with
//! fuel metering and strike-based failure isolation. The `worker` module
//! is the runtime: one tokio task owning every instance, its timers, and
//! the permission-gated HTTP cache, talking to the UI over channels.
//!
//! Wire types live in `oxidemx-widget-proto`; everything crossing the
//! wasm boundary goes through that crate's `envelope` functions.

pub mod http;
pub mod instance;
pub mod registry;
pub mod signing;
pub mod stats;
pub mod worker;

pub use http::{HttpCache, HttpFetcher, ReqwestFetcher};
pub use instance::{CallOutcome, InstanceError, WidgetInstance};
pub use registry::{InstalledWidget, SignatureState, WidgetRegistry, WidgetState};
pub use stats::{ProcStatsSource, StatsSource};
pub use worker::{spawn, HostCtl, HostEvent, InstanceId, SliceEvent, WidgetSummary};

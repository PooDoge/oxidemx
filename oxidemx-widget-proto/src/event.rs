//! Host → widget events and widget → host commands.
//!
//! WIRE FORMAT: postcard encodes enum variants by declaration index. Every
//! enum in this crate is APPEND-ONLY — never reorder, remove, or insert
//! variants mid-list within an `API_VERSION`. Appending is also not free:
//! an *older* decoder hitting a new variant returns `Err` (treated as a
//! corrupt message, not skipped), so ship new variants only behind an
//! `API_VERSION` bump or a host-first rollout.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    Init,
    Timer(String),
    MenuOpened { page: String },
    MenuClosed,
    SliceVisible,
    SliceHidden,
    Hover { entering: bool },
    Click,
    Scroll { delta: f32 },
    SettingsChanged,
    HttpResponse { id: String, status: u16, body: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HostCmd {
    SetTimer { id: String, secs: u64 },
    CancelTimer { id: String },
    HttpGet { id: String, url: String },
    OpenUrl(String),
    Exec(String),
    HapticPulse(String),
    Log { level: u8, msg: String },
}

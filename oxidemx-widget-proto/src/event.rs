//! Host → widget events and widget → host commands.

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

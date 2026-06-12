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
    /// Host-pushed system stats (1 Hz while the menu is open, plus once
    /// on MenuOpened). Delivered only to instances whose manifest
    /// `permissions` contain `"system-stats"` — guests never sample.
    SystemStats(SystemStatsSnapshot),
}

/// One tick of host-sampled system stats. Every field is `None` when the
/// source is unavailable (or, for `battery_*`, not yet host-fed — those
/// stay native-only in v1).
///
/// WIRE FORMAT: postcard encodes struct fields in declaration order. This
/// struct is APPEND-ONLY — never reorder, remove, or insert fields
/// mid-list within an `API_VERSION`; new fields go at the end.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SystemStatsSnapshot {
    /// Aggregate CPU busy %, 0–100 (delta of /proc/stat between samples;
    /// `None` on the first sample — no delta yet).
    pub cpu_pct: Option<f32>,
    /// Logical core count.
    pub cpu_cores: Option<u32>,
    /// CPU package temperature, °C.
    pub cpu_temp_c: Option<f32>,
    /// MemTotal − MemAvailable, GiB.
    pub mem_used_gb: Option<f32>,
    /// MemTotal, GiB.
    pub mem_total_gb: Option<f32>,
    /// Download rate across all non-lo interfaces, Mbit/s.
    pub net_down_mbps: Option<f32>,
    /// Upload rate across all non-lo interfaces, Mbit/s.
    pub net_up_mbps: Option<f32>,
    /// Free space on the user's data filesystem, GB (decimal).
    pub disk_free_gb: Option<f32>,
    /// Scheduled OxideMX tasks due within 24 h.
    pub tasks_due: Option<u32>,
    /// Mouse battery %, 0–100. Always `None` in v1 (native-only feed).
    pub battery_pct: Option<u8>,
    /// Mouse charging state. Always `None` in v1 (native-only feed).
    pub battery_charging: Option<bool>,
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

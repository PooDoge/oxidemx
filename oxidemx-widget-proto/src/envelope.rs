//! Tagged transport envelope. Unknown tags are skipped, never an error —
//! this is the boundary's forward-compatibility mechanism (postcard enum
//! indices alone would hard-fail on unknown variants).

use crate::event::{Event, HostCmd};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub tag: u32,
    pub payload: Vec<u8>,
}

// Append-only tag spaces. Events: 0x0001_xxxx, commands: 0x0002_xxxx.
const TAG_EVENT: u32 = 0x0001_0000;
const TAG_CMD: u32 = 0x0002_0000;

pub fn encode_event(ev: &Event) -> postcard::Result<Vec<u8>> {
    let payload = postcard::to_allocvec(ev)?;
    postcard::to_allocvec(&Envelope { tag: TAG_EVENT, payload })
}

/// `Ok(None)` = unknown tag (skip); `Err` = corrupt bytes.
pub fn decode_event(bytes: &[u8]) -> postcard::Result<Option<Event>> {
    let env: Envelope = postcard::from_bytes(bytes)?;
    if env.tag != TAG_EVENT {
        return Ok(None);
    }
    Ok(Some(postcard::from_bytes(&env.payload)?))
}

pub fn encode_cmd(cmd: &HostCmd) -> postcard::Result<Vec<u8>> {
    let payload = postcard::to_allocvec(cmd)?;
    postcard::to_allocvec(&Envelope { tag: TAG_CMD, payload })
}

pub fn decode_cmd(bytes: &[u8]) -> postcard::Result<Option<HostCmd>> {
    let env: Envelope = postcard::from_bytes(bytes)?;
    if env.tag != TAG_CMD {
        return Ok(None);
    }
    Ok(Some(postcard::from_bytes(&env.payload)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, HostCmd, SystemStatsSnapshot};

    #[test]
    fn event_round_trips_through_envelope() {
        let events = vec![
            Event::Init,
            Event::Timer("refresh".into()),
            Event::MenuOpened { page: "Apps".into() },
            Event::MenuClosed,
            Event::SliceVisible,
            Event::SliceHidden,
            Event::Hover { entering: true },
            Event::Click,
            Event::Scroll { delta: -1.0 },
            Event::SettingsChanged,
            Event::HttpResponse { id: "w".into(), status: 200, body: b"{}".to_vec() },
        ];
        for ev in events {
            let bytes = encode_event(&ev).unwrap();
            let back = decode_event(&bytes).unwrap();
            assert_eq!(Some(ev), back);
        }
    }

    #[test]
    fn system_stats_round_trips_through_envelope() {
        // Fully populated snapshot.
        let snap = SystemStatsSnapshot {
            cpu_pct: Some(23.4),
            cpu_cores: Some(8),
            cpu_temp_c: Some(52.0),
            mem_used_gb: Some(11.2),
            mem_total_gb: Some(32.0),
            net_down_mbps: Some(84.5),
            net_up_mbps: Some(12.1),
            disk_free_gb: Some(412.0),
            tasks_due: Some(3),
            battery_pct: Some(80),
            battery_charging: Some(false),
        };
        let ev = Event::SystemStats(snap);
        let bytes = encode_event(&ev).unwrap();
        assert_eq!(Some(ev), decode_event(&bytes).unwrap());

        // Default snapshot: every field None.
        let default = SystemStatsSnapshot::default();
        assert_eq!(default.cpu_pct, None);
        assert_eq!(default.battery_charging, None);
        let ev = Event::SystemStats(default);
        let bytes = encode_event(&ev).unwrap();
        assert_eq!(Some(ev), decode_event(&bytes).unwrap());
    }

    #[test]
    fn unknown_event_tag_is_skipped_not_an_error() {
        let env = Envelope { tag: 0xDEAD_BEEF, payload: vec![1, 2, 3] };
        let bytes = postcard::to_allocvec(&env).unwrap();
        assert_eq!(decode_event(&bytes).unwrap(), None);
    }

    #[test]
    fn host_cmd_round_trips_through_envelope() {
        let cmds = vec![
            HostCmd::SetTimer { id: "refresh".into(), secs: 900 },
            HostCmd::CancelTimer { id: "refresh".into() },
            HostCmd::HttpGet { id: "w".into(), url: "https://api.open-meteo.com/v1".into() },
            HostCmd::OpenUrl("https://example.com".into()),
            HostCmd::Exec("playerctl play-pause".into()),
            HostCmd::HapticPulse("tick".into()),
            HostCmd::Log { level: 1, msg: "hello".into() },
        ];
        for cmd in cmds {
            let bytes = encode_cmd(&cmd).unwrap();
            let back = decode_cmd(&bytes).unwrap();
            assert_eq!(Some(cmd), back);
        }
    }

    #[test]
    fn unknown_cmd_tag_is_skipped_not_an_error() {
        let env = Envelope { tag: 0xFFFF_0001, payload: vec![] };
        let bytes = postcard::to_allocvec(&env).unwrap();
        assert_eq!(decode_cmd(&bytes).unwrap(), None);
    }
}

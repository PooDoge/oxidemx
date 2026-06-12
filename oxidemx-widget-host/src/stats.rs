//! Host-side system sampling for the `system-stats` push feed (spec §16).
//!
//! Sandboxed guests cannot read /proc or spawn processes, so the worker
//! samples here and pushes `Event::SystemStats` to permission-holding
//! instances (1 Hz while the menu is open, plus once on MenuOpened).
//!
//! The read logic is factored from `overlay-rs/src/sampler.rs` — the
//! overlay keeps its own copy for the native widget wedges; deduplicating
//! the two is a later cleanup. Differences from the overlay sampler:
//!
//! - network rates scale by the real elapsed time between samples (the
//!   overlay loop could assume a fixed 1 s tick; this source is long-lived
//!   across menu open/close gaps);
//! - disk free is cached for 3 s and tasks-due for 60 s inside the source
//!   (the overlay used tick-counter modulos);
//! - `battery_*` stays `None` — the mouse battery feed is native-only in
//!   v1 (needs daemon D-Bus plumbed into the host; follow-up).

use std::time::{Duration, Instant};

use oxidemx_widget_proto::SystemStatsSnapshot;

/// One-shot sampler the worker polls while the menu is open. `Send`
/// because the boxed source lives inside the spawned worker task.
pub trait StatsSource: Send {
    fn sample(&mut self) -> SystemStatsSnapshot;
}

/// Production source: /proc + /sys reads, `df` for disk free, and
/// `systemctl --user list-timers` for the tasks-due count.
#[derive(Default)]
pub struct ProcStatsSource {
    /// `(busy, total)` jiffies from the previous sample — CPU % is the
    /// delta ratio, so the first sample yields `None`.
    prev_cpu: Option<(u64, u64)>,
    /// `(rx bytes, tx bytes, sampled at)` from the previous sample.
    prev_net: Option<(u64, u64, Instant)>,
    /// `df` spawn, cached for 3 s.
    disk: Option<(Instant, Option<f32>)>,
    /// `systemctl` spawn, cached for 60 s.
    tasks: Option<(Instant, Option<u32>)>,
}

const DISK_TTL: Duration = Duration::from_secs(3);
const TASKS_TTL: Duration = Duration::from_secs(60);

impl ProcStatsSource {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StatsSource for ProcStatsSource {
    fn sample(&mut self) -> SystemStatsSnapshot {
        let now = Instant::now();
        // SystemStatsSnapshot is #[non_exhaustive]: build via Default and
        // assign fields (struct literals are sealed outside the proto crate).
        let mut snap = SystemStatsSnapshot::default();

        if let Some((busy, total)) = read_proc_stat() {
            if let Some((pb, pt)) = self.prev_cpu {
                let db = busy.saturating_sub(pb) as f32;
                let dt = total.saturating_sub(pt) as f32;
                if dt > 0.0 {
                    snap.cpu_pct = Some((db / dt * 100.0).clamp(0.0, 100.0));
                }
            }
            self.prev_cpu = Some((busy, total));
        }
        snap.cpu_cores = std::thread::available_parallelism().ok().map(|n| n.get() as u32);
        snap.cpu_temp_c = read_cpu_temp();
        if let Some((used, total)) = read_meminfo() {
            snap.mem_used_gb = Some(used);
            snap.mem_total_gb = Some(total);
        }
        if let Some((rx_b, tx_b)) = read_net_dev() {
            if let Some((prx, ptx, pat)) = self.prev_net {
                // bytes over the elapsed window → Mbit/s. Clamp the window
                // so a scheduler hiccup can't explode the rate.
                let secs = now.duration_since(pat).as_secs_f32().max(0.5);
                snap.net_down_mbps =
                    Some(rx_b.saturating_sub(prx) as f32 * 8.0 / 1_000_000.0 / secs);
                snap.net_up_mbps =
                    Some(tx_b.saturating_sub(ptx) as f32 * 8.0 / 1_000_000.0 / secs);
            }
            self.prev_net = Some((rx_b, tx_b, now));
        }

        let stale = |at: Option<Instant>, ttl: Duration| {
            at.is_none_or(|t| now.duration_since(t) >= ttl)
        };
        if stale(self.disk.map(|(t, _)| t), DISK_TTL) {
            self.disk = Some((now, read_disk_free()));
        }
        snap.disk_free_gb = self.disk.and_then(|(_, v)| v);
        if stale(self.tasks.map(|(t, _)| t), TASKS_TTL) {
            self.tasks = Some((now, read_tasks_due()));
        }
        snap.tasks_due = self.tasks.and_then(|(_, v)| v);

        // battery_pct / battery_charging stay None (native-only feed, v1).
        snap
    }
}

/// `(busy, total)` jiffies from the aggregate cpu line of /proc/stat.
fn read_proc_stat() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    let line = s.lines().next()?;
    let vals: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    if vals.len() < 4 {
        return None;
    }
    let idle = vals[3] + vals.get(4).copied().unwrap_or(0); // idle + iowait
    let total: u64 = vals.iter().sum();
    Some((total - idle, total))
}

/// `(used GiB, total GiB)` from MemTotal − MemAvailable.
fn read_meminfo() -> Option<(f32, f32)> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total_kb = v.trim().trim_end_matches(" kB").trim().parse().ok()?;
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail_kb = v.trim().trim_end_matches(" kB").trim().parse().ok()?;
        }
    }
    if total_kb == 0 {
        return None;
    }
    let gib = |kb: u64| kb as f32 / 1024.0 / 1024.0;
    Some((gib(total_kb - avail_kb.min(total_kb)), gib(total_kb)))
}

/// Sum of `(rx, tx)` bytes across all non-loopback interfaces.
fn read_net_dev() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/net/dev").ok()?;
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in s.lines().skip(2) {
        let (name, rest) = line.split_once(':')?;
        if name.trim() == "lo" {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        if fields.len() >= 9 {
            rx += fields[0];
            tx += fields[8];
        }
    }
    Some((rx, tx))
}

/// First thermal zone that looks like a CPU package sensor.
fn read_cpu_temp() -> Option<f32> {
    for i in 0..10 {
        let ty = std::fs::read_to_string(format!("/sys/class/thermal/thermal_zone{i}/type"))
            .unwrap_or_default();
        if ty.contains("pkg") || ty.contains("x86") || ty.contains("cpu") {
            let raw =
                std::fs::read_to_string(format!("/sys/class/thermal/thermal_zone{i}/temp")).ok()?;
            return raw.trim().parse::<f32>().ok().map(|m| m / 1000.0);
        }
    }
    None
}

/// Free bytes on the user's data filesystem via `df` (avoids a libc
/// statvfs binding). Measures `/home` rather than `/` — on atomic/ostree
/// systems the root is a read-only composefs that reports 0 available.
fn read_disk_free() -> Option<f32> {
    for mount in ["/home", "/var", "/"] {
        let Ok(out) = std::process::Command::new("df")
            .args(["-B1", "--output=avail", mount])
            .output()
        else {
            continue;
        };
        let s = String::from_utf8_lossy(&out.stdout);
        if let Some(bytes) = s.lines().nth(1).and_then(|l| l.trim().parse::<u64>().ok()) {
            if bytes > 0 {
                return Some(bytes as f32 / 1_000_000_000.0);
            }
        }
    }
    None
}

/// Scheduled OxideMX tasks due within 24 h, via the same systemd query
/// the overlay's agent uses (`systemctl --user list-timers`).
fn read_tasks_due() -> Option<u32> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "list-timers", "--all", "--output=json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json = String::from_utf8_lossy(&out.stdout);
    Some(due_in_window(&json, unix_now(), 86_400))
}

/// Count `oxidemx-task-*` timers whose next run falls within
/// `window_secs` of `now_secs`. Pure half of [`read_tasks_due`].
fn due_in_window(json: &str, now_secs: u64, window_secs: u64) -> u32 {
    let Ok(rows) = serde_json::from_str::<serde_json::Value>(json) else {
        return 0;
    };
    let mut count = 0;
    for row in rows.as_array().into_iter().flatten() {
        let Some(unit) = row["unit"].as_str() else {
            continue;
        };
        if !unit.starts_with("oxidemx-task-") {
            continue;
        }
        if let Some(next_usec) = row["next"].as_u64() {
            let next_secs = next_usec / 1_000_000;
            if next_secs >= now_secs && next_secs <= now_secs + window_secs {
                count += 1;
            }
        }
    }
    count
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_stat_parses_on_this_machine() {
        let (busy, total) = read_proc_stat().expect("/proc/stat readable");
        assert!(busy <= total);
        assert!(total > 0);
    }

    #[test]
    fn meminfo_parses_on_this_machine() {
        let (used, total) = read_meminfo().expect("/proc/meminfo readable");
        assert!(used >= 0.0 && total > 0.0 && used <= total);
    }

    #[test]
    fn second_sample_has_deltas_and_battery_stays_none() {
        let mut src = ProcStatsSource::new();
        let first = src.sample();
        // First sample: no previous counters → no delta-derived fields.
        assert_eq!(first.cpu_pct, None);
        assert_eq!(first.net_down_mbps, None);
        assert_eq!(first.net_up_mbps, None);
        // Non-delta fields are live immediately.
        assert!(first.mem_total_gb.is_some());
        assert!(first.cpu_cores.is_some());

        std::thread::sleep(Duration::from_millis(30));
        let second = src.sample();
        let cpu = second.cpu_pct.expect("second sample has a cpu delta");
        assert!((0.0..=100.0).contains(&cpu));
        assert!(second.net_down_mbps.is_some());
        assert!(second.net_up_mbps.is_some());

        // The battery feed is native-only in v1.
        for s in [&first, &second] {
            assert_eq!(s.battery_pct, None);
            assert_eq!(s.battery_charging, None);
        }
    }

    #[test]
    fn due_in_window_counts_only_oxidemx_tasks_in_range() {
        let now = 1_000_000u64; // seconds
        let usec = |secs: u64| secs * 1_000_000;
        let json = serde_json::json!([
            { "unit": "oxidemx-task-a.timer", "next": usec(now + 60) },        // due
            { "unit": "oxidemx-task-b.timer", "next": usec(now + 86_400) },    // edge: due
            { "unit": "oxidemx-task-c.timer", "next": usec(now + 86_401) },    // too far
            { "unit": "oxidemx-task-d.timer", "next": usec(now - 10) },        // past
            { "unit": "oxidemx-task-e.timer" },                                 // unscheduled
            { "unit": "other.timer", "next": usec(now + 60) },                  // foreign
        ])
        .to_string();
        assert_eq!(due_in_window(&json, now, 86_400), 2);
        assert_eq!(due_in_window("not json", now, 86_400), 0);
    }
}

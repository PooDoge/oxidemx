//! Live-data sampling for the Splice Widgets page (and the Device
//! page's dial values / night-light dot).
//!
//! One tokio loop ticks every second while the subscription is
//! alive (the app only subscribes while the menu is drawable, so a
//! closed overlay costs nothing) and emits a [`WidgetSnapshot`]:
//!
//!   * CPU % — delta of `/proc/stat` between ticks (first tick has
//!     no delta and keeps the previous value);
//!   * memory — `/proc/meminfo` MemTotal/MemAvailable;
//!   * network ↓↑ — delta of `/proc/net/dev` across all non-lo
//!     interfaces, in Mbit/s;
//!   * disk free — `statvfs("/")` via `df -B1 --output=avail /`;
//!   * mouse battery — daemon D-Bus, every 30 s;
//!   * night light / brightness / volume — gsettings + brightnessctl
//!     + wpctl, every 3 s (process spawns, kept off the hot tick);
//!   * weather — Open-Meteo (keyless) when `overlay.weather_location`
//!     is configured, refreshed every 15 min.

use std::time::Duration;

/// One sampling tick's worth of widget data. `None`/`0` fields mean
/// "not sampled yet" or "source unavailable" — the wedge renderer
/// shows its stub for those.
#[derive(Debug, Clone, Default)]
pub struct WidgetSnapshot {
    pub cpu_percent: Option<f32>,
    pub cpu_cores: usize,
    pub cpu_temp_c: Option<f32>,
    pub mem_used_gb: Option<f32>,
    pub mem_total_gb: Option<f32>,
    pub net_down_mbps: Option<f32>,
    pub net_up_mbps: Option<f32>,
    pub disk_free_gb: Option<f32>,
    /// `(percentage, charging)` from the daemon.
    pub mouse_battery: Option<(u8, bool)>,
    pub night_light_on: Option<bool>,
    pub brightness_percent: Option<u8>,
    pub volume_percent: Option<u8>,
    /// Current conditions + 7-day forecast from Open-Meteo. The
    /// wedge renders the condition icon; the hover popup renders
    /// the rest.
    pub weather: Option<WeatherInfo>,
    /// Scheduled OxideMX tasks due within 24 h.
    pub tasks_due: Option<u32>,
}

/// Current weather + daily forecast, already in the configured
/// temperature unit.
#[derive(Debug, Clone)]
pub struct WeatherInfo {
    /// Current temperature in the configured unit.
    pub temp: f32,
    /// Current WMO 4677 weather code.
    pub code: u64,
    /// Short condition label for the current code ("Partly cloudy").
    pub label: String,
    /// Configured place name ("Oslo, NO"), when one is set.
    pub place: Option<String>,
    /// `true` = temps are °F, `false` = °C.
    pub fahrenheit: bool,
    /// Up to 7 days starting today.
    pub daily: Vec<DayForecast>,
}

/// One day of the Open-Meteo daily forecast.
#[derive(Debug, Clone)]
pub struct DayForecast {
    /// Short weekday name ("Mon"), "Today" for the first entry.
    pub day: String,
    /// WMO weather code for the day.
    pub code: u64,
    pub t_min: f32,
    pub t_max: f32,
}

/// Stream of snapshots, one per second. Same channel-bridge shape
/// as `dbus::stream` / `config::watch_stream`.
pub fn stream() -> impl futures_util::stream::Stream<Item = WidgetSnapshot> {
    let (tx, rx) = async_channel::bounded::<WidgetSnapshot>(4);

    tokio::spawn(async move {
        let overlay_cfg = crate::config::load().map(|c| c.overlay).unwrap_or_default();
        let weather_loc = overlay_cfg.weather_location;
        let weather_place = overlay_cfg.weather_place;
        let weather_fahrenheit = !overlay_cfg.weather_celsius;

        let mut prev_cpu: Option<(u64, u64)> = None; // (busy, total)
        let mut prev_net: Option<(u64, u64)> = None; // (rx, tx bytes)
        let mut snap = WidgetSnapshot::default();
        let mut tick: u64 = 0;

        loop {
            // ---- every tick: cheap procfs reads ----
            if let Some((busy, total)) = read_proc_stat() {
                if let Some((pb, pt)) = prev_cpu {
                    let db = busy.saturating_sub(pb) as f32;
                    let dt = total.saturating_sub(pt) as f32;
                    if dt > 0.0 {
                        snap.cpu_percent = Some((db / dt * 100.0).clamp(0.0, 100.0));
                    }
                }
                prev_cpu = Some((busy, total));
            }
            snap.cpu_cores = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(0);
            if let Some((used, total)) = read_meminfo() {
                snap.mem_used_gb = Some(used);
                snap.mem_total_gb = Some(total);
            }
            if let Some((rx_b, tx_b)) = read_net_dev() {
                if let Some((prx, ptx)) = prev_net {
                    // bytes/s → Mbit/s (tick interval is 1 s).
                    snap.net_down_mbps = Some(rx_b.saturating_sub(prx) as f32 * 8.0 / 1_000_000.0);
                    snap.net_up_mbps = Some(tx_b.saturating_sub(ptx) as f32 * 8.0 / 1_000_000.0);
                }
                prev_net = Some((rx_b, tx_b));
            }
            snap.cpu_temp_c = read_cpu_temp();

            // ---- every 3 s: process-spawn sources ----
            if tick.is_multiple_of(3) {
                snap.disk_free_gb = read_disk_free();
                snap.night_light_on = read_night_light().await;
                snap.brightness_percent = read_brightness().await;
                snap.volume_percent = read_volume().await;
            }

            // ---- every 30 s: daemon battery ----
            if tick.is_multiple_of(30) {
                snap.mouse_battery = crate::haptic_client::battery_status().await;
            }

            // ---- every minute: scheduled-task due count ----
            if tick.is_multiple_of(60) {
                snap.tasks_due =
                    tokio::task::spawn_blocking(crate::agent::tasks::due_within_24h_count)
                        .await
                        .ok();
            }

            // ---- every 15 min: weather ----
            if tick.is_multiple_of(900) {
                if let Some((lat, lon)) = weather_loc {
                    if let Some(mut info) = fetch_weather(lat, lon, weather_fahrenheit).await {
                        info.place = weather_place.clone();
                        snap.weather = Some(info);
                    }
                }
            }

            if tx.send(snap.clone()).await.is_err() {
                // Subscription dropped (menu closed) — stop sampling.
                return;
            }
            tick += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    rx
}

/// `(busy, total)` jiffies from the aggregate cpu line.
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

/// `(used GB, total GB)` from MemTotal − MemAvailable.
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

/// Free bytes on the user's data filesystem via `df` (avoids a
/// libc statvfs binding). Measures `/home` rather than `/` — on
/// atomic/ostree systems the root is a read-only composefs that
/// reports 0 available.
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

async fn read_night_light() -> Option<bool> {
    let out = tokio::process::Command::new("gsettings")
        .args([
            "get",
            "org.gnome.settings-daemon.plugins.color",
            "night-light-enabled",
        ])
        .output()
        .await
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim() == "true")
}

async fn read_brightness() -> Option<u8> {
    // `brightnessctl -m` → device,class,current,percent%,max
    let out = tokio::process::Command::new("brightnessctl")
        .arg("-m")
        .output()
        .await
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let pct = s.split(',').nth(3)?.trim().trim_end_matches('%');
    pct.parse().ok()
}

async fn read_volume() -> Option<u8> {
    // `wpctl get-volume @DEFAULT_AUDIO_SINK@` → "Volume: 0.45 [MUTED]"
    let out = tokio::process::Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .await
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let v: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some((v * 100.0).round().clamp(0.0, 200.0) as u8)
}

/// One Open-Meteo current-conditions + 7-day-forecast fetch.
/// Keyless API; ~1 req / 15 min is far inside its fair-use budget.
/// Open-Meteo does the unit conversion server-side via
/// `temperature_unit`.
async fn fetch_weather(lat: f64, lon: f64, fahrenheit: bool) -> Option<WeatherInfo> {
    let unit = if fahrenheit { "fahrenheit" } else { "celsius" };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,weather_code\
         &daily=weather_code,temperature_2m_max,temperature_2m_min\
         &forecast_days=7&timezone=auto&temperature_unit={unit}"
    );
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?
        .get(url)
        .send()
        .await
        .ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    let cur = &v["current"];
    let temp = cur["temperature_2m"].as_f64()? as f32;
    let code = cur["weather_code"].as_u64().unwrap_or(0);

    let daily = &v["daily"];
    let dates = daily["time"].as_array();
    let codes = daily["weather_code"].as_array();
    let maxs = daily["temperature_2m_max"].as_array();
    let mins = daily["temperature_2m_min"].as_array();
    let mut days = Vec::new();
    if let (Some(dates), Some(codes), Some(maxs), Some(mins)) = (dates, codes, maxs, mins) {
        for (i, date) in dates.iter().take(7).enumerate() {
            let day = if i == 0 {
                "Today".to_string()
            } else {
                date.as_str()
                    .and_then(weekday_short)
                    .unwrap_or("—")
                    .to_string()
            };
            days.push(DayForecast {
                day,
                code: codes.get(i).and_then(|c| c.as_u64()).unwrap_or(0),
                t_min: mins.get(i).and_then(|t| t.as_f64()).unwrap_or(0.0) as f32,
                t_max: maxs.get(i).and_then(|t| t.as_f64()).unwrap_or(0.0) as f32,
            });
        }
    }

    Some(WeatherInfo {
        temp,
        code,
        label: weather_label(code).to_string(),
        place: None, // filled in by the sampling loop from config
        fahrenheit,
        daily: days,
    })
}

/// Weekday abbreviation for an ISO `YYYY-MM-DD` date. Civil-days
/// algorithm (Howard Hinnant's `days_from_civil`) — avoids pulling
/// chrono in for one weekday lookup.
fn weekday_short(iso: &str) -> Option<&'static str> {
    let mut parts = iso.splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468; // days since 1970-01-01
    let dow = (days + 4).rem_euclid(7); // 1970-01-01 was a Thursday
    Some(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][dow as usize])
}

/// WMO weather-code → XDG symbolic icon name (Adwaita ships the
/// full `weather-*-symbolic` set).
pub fn weather_icon_name(code: u64) -> &'static str {
    match code {
        0 => "weather-clear-symbolic",
        1 | 2 => "weather-few-clouds-symbolic",
        3 => "weather-overcast-symbolic",
        45 | 48 => "weather-fog-symbolic",
        51..=57 => "weather-showers-scattered-symbolic",
        61..=67 | 80..=82 => "weather-showers-symbolic",
        71..=77 | 85 | 86 => "weather-snow-symbolic",
        95..=99 => "weather-storm-symbolic",
        _ => "weather-overcast-symbolic",
    }
}

/// WMO weather-code → short label (Open-Meteo uses WMO 4677 codes).
fn weather_label(code: u64) -> &'static str {
    match code {
        0 => "Clear",
        1..=3 => "Partly cloudy",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95..=99 => "Thunderstorm",
        _ => "Cloudy",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_stat_parses_on_this_machine() {
        // Smoke test against the live procfs — busy ≤ total.
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
    fn weather_labels_cover_wmo_ranges() {
        assert_eq!(weather_label(0), "Clear");
        assert_eq!(weather_label(2), "Partly cloudy");
        assert_eq!(weather_label(63), "Rain");
        assert_eq!(weather_label(96), "Thunderstorm");
        assert_eq!(weather_label(123), "Cloudy");
    }

    #[test]
    fn weather_icons_cover_wmo_ranges() {
        assert_eq!(weather_icon_name(0), "weather-clear-symbolic");
        assert_eq!(weather_icon_name(2), "weather-few-clouds-symbolic");
        assert_eq!(weather_icon_name(3), "weather-overcast-symbolic");
        assert_eq!(weather_icon_name(53), "weather-showers-scattered-symbolic");
        assert_eq!(weather_icon_name(63), "weather-showers-symbolic");
        assert_eq!(weather_icon_name(75), "weather-snow-symbolic");
        assert_eq!(weather_icon_name(96), "weather-storm-symbolic");
        assert_eq!(weather_icon_name(123), "weather-overcast-symbolic");
    }

    #[test]
    fn weekday_short_known_dates() {
        assert_eq!(weekday_short("1970-01-01"), Some("Thu"));
        assert_eq!(weekday_short("2026-06-12"), Some("Fri"));
        assert_eq!(weekday_short("2000-02-29"), Some("Tue"));
        assert_eq!(weekday_short("not-a-date"), None);
        assert_eq!(weekday_short("2026-13-01"), None);
    }
}

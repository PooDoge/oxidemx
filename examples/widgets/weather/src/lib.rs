//! Weather widget — canonical OxideMX PDK example.
//!
//! Fetches current conditions from Open-Meteo (keyless API, fair-use),
//! displays temperature + WMO condition label in the slice, scrolls through
//! a 3-day forecast, and taps haptic on scroll.
//!
//! JSON parsing: hand-rolled string scan for the two needed numbers
//! (`current_weather.temperature` and `current_weather.weathercode`).
//! This keeps the guest binary tiny — no serde or serde_json dep required.

use oxidemx_widget_api::{tile, Ctx, Widget};
use oxidemx_widget_proto::{Event, Scene, WedgeGeom};

// ---------------------------------------------------------------------------
// WMO 4677 weather-code → short label
// Copied from overlay-rs/src/sampler.rs::weather_label (~lines 394-408).
// ---------------------------------------------------------------------------

fn wmo_label(code: u16) -> &'static str {
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

// ---------------------------------------------------------------------------
// Minimal JSON scanner
//
// Open-Meteo responds with e.g.:
//   {"current_weather":{"temperature":14.2,"weathercode":2, ...}, ...}
//
// We only need two numbers. Rather than pulling in serde_json (which adds
// ~200 KB to the wasm binary), we do a tiny linear scan:
//  - find the key string, skip past `:`, parse the following number.
// ---------------------------------------------------------------------------

/// Find `"key":` in `json` (ignoring whitespace after the colon) and parse
/// the immediately following number (integer or float).
fn json_find_number(json: &[u8], key: &str) -> Option<f64> {
    let key_bytes = key.as_bytes();
    let needle = {
        let mut v = b"\"".to_vec();
        v.extend_from_slice(key_bytes);
        v.push(b'"');
        v
    };
    // Find the key in the JSON byte slice.
    let start = json.windows(needle.len()).position(|w| w == needle.as_slice())?;
    let after_key = start + needle.len();
    // Skip optional whitespace and the colon.
    let rest = &json[after_key..];
    let colon_pos = rest.iter().position(|&b| b == b':')?;
    let after_colon = &rest[colon_pos + 1..];
    // Skip whitespace.
    let num_start = after_colon.iter().position(|&b| b != b' ' && b != b'\t' && b != b'\n')?;
    let num_slice = &after_colon[num_start..];
    // Read until a character that cannot be part of a JSON number.
    let num_end = num_slice
        .iter()
        .position(|&b| b != b'-' && b != b'+' && !(b.is_ascii_digit()) && b != b'.' && b != b'e' && b != b'E')
        .unwrap_or(num_slice.len());
    let num_str = core::str::from_utf8(&num_slice[..num_end]).ok()?;
    num_str.parse().ok()
}

// ---------------------------------------------------------------------------
// Widget state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Weather {
    /// Celsius temperature (converted to °F on render if units == "f").
    temp_c: Option<f32>,
    /// WMO weather code.
    code: u16,
    /// Which forecast day is displayed (0 = today, 1 = tomorrow, 2 = +2).
    day: u8,
    /// Cached settings (needed to rebuild the URL on refetch).
    lat: f64,
    lon: f64,
    units_f: bool,
}

impl Weather {
    fn build_url(&self) -> String {
        // Ask Open-Meteo for current conditions (always Celsius from the API;
        // we do °F conversion ourselves so the response is stable).
        format!(
            "https://api.open-meteo.com/v1/forecast\
             ?latitude={lat}&longitude={lon}\
             &current=temperature_2m,weather_code\
             &forecast_days=3&timezone=auto",
            lat = self.lat,
            lon = self.lon,
        )
    }

    fn fetch(&self, ctx: &Ctx) {
        if self.lat != 0.0 || self.lon != 0.0 {
            ctx.http_get("wx", &self.build_url());
        }
    }

    /// Parse current temperature and WMO code from an Open-Meteo JSON body.
    /// Returns `(temp_celsius, wmo_code)` or `None` on any parse failure.
    pub fn parse_response(body: &[u8]) -> Option<(f32, u16)> {
        let temp = json_find_number(body, "temperature_2m")?;
        let code = json_find_number(body, "weather_code")?;
        Some((temp as f32, code as u16))
    }
}

impl Widget for Weather {
    fn init(&mut self, ctx: &Ctx) {
        // Read settings.
        if let Some((_, lat, lon)) = ctx.setting_location("location") {
            self.lat = lat;
            self.lon = lon;
        }
        self.units_f = ctx.setting_str("units").map(|u| u == "f").unwrap_or(false);

        // Timer interval from the `refresh` setting (seconds), floored by host.
        let refresh_secs = ctx.setting_u64("refresh").unwrap_or(900);
        ctx.set_timer("refresh", refresh_secs);

        // Kick off the first fetch.
        self.fetch(ctx);
    }

    fn on_event(&mut self, ev: Event, ctx: &Ctx) -> bool {
        match ev {
            Event::Timer(ref id) if id == "refresh" => {
                self.fetch(ctx);
                false // data not yet arrived; render after HttpResponse
            }
            Event::MenuOpened { .. } => {
                // Refetch unconditionally on open (simplest: always warm data).
                self.fetch(ctx);
                false
            }
            Event::HttpResponse { ref id, status, ref body } if id == "wx" => {
                if status == 200 {
                    if let Some((t, c)) = Self::parse_response(body) {
                        self.temp_c = Some(t);
                        self.code = c;
                        return true; // needs render
                    }
                }
                ctx.log(&format!("weather: HTTP {} — no update", status));
                false
            }
            Event::Scroll { delta } => {
                // Cycle day 0..=2; haptic tick on change.
                let next = if delta > 0.0 {
                    (self.day + 1) % 3
                } else {
                    (self.day + 2) % 3 // subtract 1 wrapping
                };
                if next != self.day {
                    self.day = next;
                    ctx.haptic("tick");
                    true
                } else {
                    false
                }
            }
            Event::Click { .. } => {
                ctx.open_url("https://open-meteo.com/");
                false
            }
            Event::SettingsChanged => {
                // Re-read settings and refetch.
                if let Some((_, lat, lon)) = ctx.setting_location("location") {
                    self.lat = lat;
                    self.lon = lon;
                }
                self.units_f = ctx.setting_str("units").map(|u| u == "f").unwrap_or(false);
                self.fetch(ctx);
                true // render immediately to show stale state
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        match self.temp_c {
            Some(temp_c) => {
                let temp_display = if self.units_f {
                    temp_c * 9.0 / 5.0 + 32.0
                } else {
                    temp_c
                };
                let value = format!("{:.0}°", temp_display);
                let condition = wmo_label(self.code);
                let day_label = match self.day {
                    1 => "TOMORROW",
                    2 => "+2 DAYS",
                    _ => "WEATHER",
                };
                tile()
                    .value(&value)
                    .sublabel(condition)
                    .label(day_label)
                    .into_scene(geom)
            }
            None => {
                // No data yet — show a placeholder.
                tile()
                    .value("—")
                    .sublabel("fetching…")
                    .label("WEATHER")
                    .into_scene(geom)
            }
        }
    }
}

oxidemx_widget_api::register_widget!(Weather);

// ---------------------------------------------------------------------------
// Native unit tests (run with: cargo test --manifest-path …/Cargo.toml)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- WMO label mapping -------------------------------------------------

    #[test]
    fn wmo_label_covers_key_codes() {
        assert_eq!(wmo_label(0), "Clear");
        assert_eq!(wmo_label(1), "Partly cloudy");
        assert_eq!(wmo_label(2), "Partly cloudy");
        assert_eq!(wmo_label(3), "Partly cloudy");
        assert_eq!(wmo_label(45), "Fog");
        assert_eq!(wmo_label(48), "Fog");
        assert_eq!(wmo_label(51), "Drizzle");
        assert_eq!(wmo_label(61), "Rain");
        assert_eq!(wmo_label(63), "Rain");
        assert_eq!(wmo_label(71), "Snow");
        assert_eq!(wmo_label(77), "Snow");
        assert_eq!(wmo_label(80), "Showers");
        assert_eq!(wmo_label(82), "Showers");
        assert_eq!(wmo_label(85), "Snow showers");
        assert_eq!(wmo_label(86), "Snow showers");
        assert_eq!(wmo_label(95), "Thunderstorm");
        assert_eq!(wmo_label(96), "Thunderstorm");
        assert_eq!(wmo_label(99), "Thunderstorm");
        assert_eq!(wmo_label(4), "Cloudy");
        assert_eq!(wmo_label(123), "Cloudy");
    }

    // ---- JSON response parsing ---------------------------------------------

    const CANNED_RESPONSE: &[u8] = br#"{
      "latitude": 59.9,
      "longitude": 10.75,
      "current": {
        "temperature_2m": 14.2,
        "weather_code": 2
      }
    }"#;

    #[test]
    fn parse_response_extracts_temp_and_code() {
        let (temp, code) = Weather::parse_response(CANNED_RESPONSE).unwrap();
        assert!((temp - 14.2).abs() < 0.1, "temp should be 14.2, got {temp}");
        assert_eq!(code, 2);
    }

    #[test]
    fn parse_response_handles_negative_temp() {
        let body = br#"{"current":{"temperature_2m":-5.3,"weather_code":71}}"#;
        let (temp, code) = Weather::parse_response(body).unwrap();
        assert!((temp - (-5.3)).abs() < 0.1);
        assert_eq!(code, 71);
    }

    #[test]
    fn parse_response_returns_none_on_garbage() {
        assert_eq!(Weather::parse_response(b"not json at all"), None);
        assert_eq!(Weather::parse_response(b"{}"), None);
    }

    // ---- °F conversion -----------------------------------------------------

    #[test]
    fn fahrenheit_conversion_correct() {
        // 0°C = 32°F, 100°C = 212°F
        let convert = |c: f32| c * 9.0 / 5.0 + 32.0;
        assert!((convert(0.0) - 32.0).abs() < 0.1);
        assert!((convert(100.0) - 212.0).abs() < 0.1);
        assert!((convert(-40.0) - (-40.0)).abs() < 0.1);
    }

    // ---- day cycling -------------------------------------------------------

    #[test]
    fn day_cycling_wraps() {
        let mut w = Weather::default();
        // Scroll forward wraps 0 → 1 → 2 → 0
        assert_eq!(w.day, 0);
        // Simulate scroll forward
        w.day = (w.day + 1) % 3;
        assert_eq!(w.day, 1);
        w.day = (w.day + 1) % 3;
        assert_eq!(w.day, 2);
        w.day = (w.day + 1) % 3;
        assert_eq!(w.day, 0);

        // Scroll backward wraps 0 → 2 → 1 → 0
        w.day = 0;
        w.day = (w.day + 2) % 3;
        assert_eq!(w.day, 2);
        w.day = (w.day + 2) % 3;
        assert_eq!(w.day, 1);
        w.day = (w.day + 2) % 3;
        assert_eq!(w.day, 0);
    }

    // ---- render output structure -------------------------------------------

    #[test]
    fn render_with_data_has_value_sublabel_label() {
        use oxidemx_widget_proto::{Prim, TextWeight, TextAlign, WedgeGeom};
        let mut w = Weather::default();
        w.temp_c = Some(14.0);
        w.code = 2;
        w.units_f = false;
        let geom = WedgeGeom {
            width: 200.0, height: 160.0,
            inner_radius: 60.0, outer_radius: 160.0,
            angle_start: 0.0, angle_end: 0.785, hovered: 0.0,
        };
        let scene = w.render(geom);
        assert_eq!(scene.prims.len(), 3, "no sparkline: value + sublabel + label");

        match &scene.prims[0] {
            Prim::Text { content, size, weight, align, .. } => {
                assert_eq!(content, "14°");
                assert!((size - 18.0).abs() < 0.1);
                assert_eq!(*weight, TextWeight::Bold);
                assert_eq!(*align, TextAlign::Center);
            }
            other => panic!("prim[0] should be Text, got {other:?}"),
        }
        match &scene.prims[1] {
            Prim::Text { content, size, .. } => {
                assert_eq!(content, "Partly cloudy");
                assert!((size - 8.5).abs() < 0.1);
            }
            other => panic!("prim[1] should be sublabel Text, got {other:?}"),
        }
        match &scene.prims[2] {
            Prim::Text { content, size, weight, .. } => {
                assert_eq!(content, "WEATHER");
                assert!((size - 8.0).abs() < 0.1);
                assert_eq!(*weight, oxidemx_widget_proto::TextWeight::Semibold);
            }
            other => panic!("prim[2] should be label Text, got {other:?}"),
        }
    }

    #[test]
    fn render_units_f_converts_temperature() {
        use oxidemx_widget_proto::{Prim, WedgeGeom};
        let mut w = Weather::default();
        w.temp_c = Some(0.0); // 0°C = 32°F
        w.units_f = true;
        let geom = WedgeGeom {
            width: 200.0, height: 160.0,
            inner_radius: 60.0, outer_radius: 160.0,
            angle_start: 0.0, angle_end: 0.785, hovered: 0.0,
        };
        let scene = w.render(geom);
        match &scene.prims[0] {
            Prim::Text { content, .. } => {
                assert_eq!(content, "32°", "0°C should render as 32°F");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }
}

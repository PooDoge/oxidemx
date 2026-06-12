//! Guest PDK for OxideMX widget plugins.
//!
//! Developers implement [`Widget`], call [`register_widget!`], and never
//! touch postcard or extern "C" directly.
//!
//! Compiles natively (for tests — the extern wasm glue is
//! `#[cfg(target_arch = "wasm32")]`) and to `wasm32-wasip1`.

pub mod tile;
pub mod macros;

// Re-export proto types that widget authors need.
pub use oxidemx_widget_proto::{Event, Scene, WedgeGeom};
pub use oxidemx_widget_proto::settings::{Settings, SettingValue};

pub use tile::{tile, TileBuilder};

use std::cell::RefCell;
use oxidemx_widget_proto::HostCmd;

// ---------------------------------------------------------------------------
// Widget trait
// ---------------------------------------------------------------------------

/// The trait widget crates implement. `Default` is required so the macro
/// can zero-initialise the instance before calling `init`.
pub trait Widget: Default {
    /// Called once after the host has loaded the module. Use `ctx` to read
    /// settings and issue initial commands (timers, http_get, …).
    fn init(&mut self, ctx: &Ctx);

    /// Called for every host event. Returns `true` if the widget's visual
    /// state changed and it needs to be re-rendered.
    fn on_event(&mut self, ev: Event, ctx: &Ctx) -> bool;

    /// Produce a retained display list. Called by the host only when the
    /// previous `on_event` returned `true`.
    fn render(&self, geom: WedgeGeom) -> Scene;
}

// ---------------------------------------------------------------------------
// Ctx
// ---------------------------------------------------------------------------

/// Context object passed to every [`Widget`] call. Provides settings access
/// and fire-and-forget host commands.
pub struct Ctx {
    pub(crate) settings: Settings,
    /// Queued commands, drained by the macro glue and flushed to the host.
    pub(crate) cmds: RefCell<Vec<HostCmd>>,
}

impl Ctx {
    /// Build a `Ctx` from a resolved settings bag (as delivered by `omx_init`).
    pub fn new(settings: Settings) -> Self {
        Ctx { settings, cmds: RefCell::new(Vec::new()) }
    }

    /// Drain the command queue. Used by the macro glue after each call.
    pub fn drain_cmds(&self) -> Vec<HostCmd> {
        self.cmds.borrow_mut().drain(..).collect()
    }

    fn push(&self, cmd: HostCmd) {
        self.cmds.borrow_mut().push(cmd);
    }

    // ---- settings accessors ------------------------------------------------

    fn find_value(&self, key: &str) -> Option<&SettingValue> {
        self.settings.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn setting_str(&self, key: &str) -> Option<&str> {
        match self.find_value(key)? {
            SettingValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn setting_f64(&self, key: &str) -> Option<f64> {
        match self.find_value(key)? {
            SettingValue::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn setting_u64(&self, key: &str) -> Option<u64> {
        match self.find_value(key)? {
            SettingValue::Num(n) => Some(*n as u64),
            _ => None,
        }
    }

    pub fn setting_bool(&self, key: &str) -> Option<bool> {
        match self.find_value(key)? {
            SettingValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn setting_location(&self, key: &str) -> Option<(String, f64, f64)> {
        match self.find_value(key)? {
            SettingValue::Location { name, lat, lon } => {
                Some((name.clone(), *lat, *lon))
            }
            _ => None,
        }
    }

    // ---- host commands -----------------------------------------------------

    pub fn set_timer(&self, id: &str, secs: u64) {
        self.push(HostCmd::SetTimer { id: id.to_string(), secs });
    }

    pub fn cancel_timer(&self, id: &str) {
        self.push(HostCmd::CancelTimer { id: id.to_string() });
    }

    pub fn http_get(&self, id: &str, url: &str) {
        self.push(HostCmd::HttpGet { id: id.to_string(), url: url.to_string() });
    }

    pub fn open_url(&self, url: &str) {
        self.push(HostCmd::OpenUrl(url.to_string()));
    }

    pub fn exec(&self, command: &str) {
        self.push(HostCmd::Exec(command.to_string()));
    }

    pub fn haptic(&self, pattern: &str) {
        self.push(HostCmd::HapticPulse(pattern.to_string()));
    }

    pub fn log(&self, msg: &str) {
        self.push(HostCmd::Log { level: 1, msg: msg.to_string() });
    }
}

// ---------------------------------------------------------------------------
// Wasm-side cmd flushing (only compiled into the .wasm module)
// ---------------------------------------------------------------------------

/// Flush all queued commands through the `omx_cmd` host import.
/// On native (test) builds this is a no-op — the queue is inspectable
/// directly through `Ctx::drain_cmds()`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn flush_cmds(ctx: &Ctx) {
    use oxidemx_widget_proto::envelope;
    #[link(wasm_import_module = "oxidemx")]
    extern "C" {
        fn omx_cmd(ptr: u32, len: u32);
    }
    for cmd in ctx.drain_cmds() {
        if let Ok(bytes) = envelope::encode_cmd(&cmd) {
            unsafe { omx_cmd(bytes.as_ptr() as u32, bytes.len() as u32) };
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn flush_cmds(_ctx: &Ctx) {
    // On native the test inspects ctx.drain_cmds() directly — nothing to flush.
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oxidemx_widget_proto::{
        Color, Prim, TextWeight, TextAlign,
        envelope,
    };
    use oxidemx_widget_proto::settings::SettingValue;

    /// tile() must produce prims in order: value Text, sparkline Sparkline,
    /// sublabel Text, label Text — with correct sizes and uppercase label.
    #[test]
    fn tile_builder_produces_expected_prims() {
        let geom = WedgeGeom {
            width: 200.0, height: 160.0,
            inner_radius: 60.0, outer_radius: 160.0,
            angle_start: 0.0, angle_end: 0.785, hovered: 0.0,
        };
        let scene = tile()
            .value("14°")
            .sparkline(&[0.2, 0.5, 0.8, 0.4])
            .sublabel("Partly cloudy")
            .label("weather")
            .into_scene(geom);

        assert_eq!(scene.prims.len(), 4, "expected 4 prims: value, sparkline, sublabel, label");

        // Prim[0]: 18pt bold value Text
        match &scene.prims[0] {
            Prim::Text { content, size, weight, align, .. } => {
                assert_eq!(content, "14°");
                assert!((size - 18.0).abs() < 0.1, "value size should be 18.0, got {size}");
                assert_eq!(*weight, TextWeight::Bold);
                assert_eq!(*align, TextAlign::Center);
            }
            other => panic!("prim[0] should be Text, got {other:?}"),
        }

        // Prim[1]: Sparkline
        match &scene.prims[1] {
            Prim::Sparkline { points, .. } => {
                assert_eq!(points.len(), 4);
            }
            other => panic!("prim[1] should be Sparkline, got {other:?}"),
        }

        // Prim[2]: 8.5pt sublabel Text
        match &scene.prims[2] {
            Prim::Text { content, size, weight, align, .. } => {
                assert_eq!(content, "Partly cloudy");
                assert!((size - 8.5).abs() < 0.1, "sublabel size should be 8.5, got {size}");
                assert_eq!(*weight, TextWeight::Regular);
                assert_eq!(*align, TextAlign::Center);
            }
            other => panic!("prim[2] should be Text, got {other:?}"),
        }

        // Prim[3]: 8pt semibold uppercase label Text
        match &scene.prims[3] {
            Prim::Text { content, size, weight, align, .. } => {
                assert_eq!(content, "WEATHER", "label must be uppercased");
                assert!((size - 8.0).abs() < 0.1, "label size should be 8.0, got {size}");
                assert_eq!(*weight, TextWeight::Semibold);
                assert_eq!(*align, TextAlign::Center);
            }
            other => panic!("prim[3] should be Text, got {other:?}"),
        }
    }

    /// tile() without sparkline produces 3 prims: value, sublabel, label.
    #[test]
    fn tile_builder_no_sparkline() {
        let geom = WedgeGeom {
            width: 200.0, height: 160.0,
            inner_radius: 60.0, outer_radius: 160.0,
            angle_start: 0.0, angle_end: 0.785, hovered: 0.0,
        };
        let scene = tile()
            .value("42%")
            .sublabel("4 cores")
            .label("cpu")
            .into_scene(geom);
        assert_eq!(scene.prims.len(), 3, "without sparkline: value + sublabel + label");
        // label must be uppercased
        match &scene.prims[2] {
            Prim::Text { content, .. } => assert_eq!(content, "CPU"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    /// Ctx setting accessors work for all SettingValue variants.
    #[test]
    fn ctx_setting_accessors() {
        let settings: Settings = vec![
            ("name".into(), SettingValue::Str("Oslo".into())),
            ("temp".into(), SettingValue::Num(15.5)),
            ("metric".into(), SettingValue::Bool(true)),
            ("home".into(), SettingValue::Location {
                name: "Oslo, Norway".into(),
                lat: 59.91,
                lon: 10.75,
            }),
        ];
        let ctx = Ctx::new(settings);

        assert_eq!(ctx.setting_str("name"), Some("Oslo"));
        assert_eq!(ctx.setting_str("missing"), None);
        assert_eq!(ctx.setting_str("temp"), None); // wrong type

        assert!((ctx.setting_f64("temp").unwrap() - 15.5).abs() < 0.001);
        assert_eq!(ctx.setting_f64("name"), None); // wrong type

        assert_eq!(ctx.setting_u64("temp"), Some(15));
        assert_eq!(ctx.setting_bool("metric"), Some(true));
        assert_eq!(ctx.setting_bool("name"), None); // wrong type

        let (name, lat, lon) = ctx.setting_location("home").unwrap();
        assert_eq!(name, "Oslo, Norway");
        assert!((lat - 59.91).abs() < 0.001);
        assert!((lon - 10.75).abs() < 0.001);
        assert_eq!(ctx.setting_location("name"), None); // wrong type
    }

    /// An Envelope with a bogus tag must decode to None (skip), not an error.
    /// This is the path the macro's on_event takes for unknown events.
    #[test]
    fn event_decode_unknown_returns_no_render() {
        use oxidemx_widget_proto::envelope::Envelope;
        let env = Envelope { tag: 0xDEAD_BEEF, payload: vec![1, 2, 3] };
        let bytes = postcard::to_allocvec(&env).unwrap();
        let result = envelope::decode_event(&bytes).unwrap();
        assert_eq!(result, None, "unknown tag must produce None, not an error");
    }

    /// Ctx commands accumulate and can be drained.
    #[test]
    fn ctx_cmd_queue() {
        use oxidemx_widget_proto::HostCmd;
        let ctx = Ctx::new(vec![]);
        ctx.set_timer("refresh", 900);
        ctx.http_get("wx", "https://api.example.com/weather");
        ctx.log("hello");
        let cmds = ctx.drain_cmds();
        assert_eq!(cmds.len(), 3);
        assert!(matches!(cmds[0], HostCmd::SetTimer { ref id, secs: 900 } if id == "refresh"));
        assert!(matches!(cmds[1], HostCmd::HttpGet { .. }));
        assert!(matches!(cmds[2], HostCmd::Log { .. }));
        // Second drain is empty.
        assert!(ctx.drain_cmds().is_empty());
    }
}

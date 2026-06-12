//! Raw-ABI fixture widget (no PDK — this crate pins the wire contract
//! independently of `oxidemx-widget-api`). Behavior switches on the
//! `mode` entry of the settings bag passed to `omx_init`:
//!
//! - `ok`         — event returns 1 (needs render); render returns a 2-prim Scene
//! - `spin`       — event loops forever (host fuel-exhaustion test)
//! - `huge_scene` — render returns > MAX_SCENE_PRIMS prims
//! - `trap`       — event executes a deterministic wasm trap
//!
//! Every `init` issues one `omx_cmd` carrying `HostCmd::Log` to prove the
//! import wiring works.

use std::sync::atomic::{AtomicU8, Ordering};

use oxidemx_widget_proto::envelope;
use oxidemx_widget_proto::settings::Settings;
use oxidemx_widget_proto::{
    Color, Event, HostCmd, PathOp, Prim, Scene, SettingValue, TextAlign, TextWeight,
    API_VERSION, scene::MAX_SCENE_PRIMS,
};

#[link(wasm_import_module = "oxidemx")]
extern "C" {
    fn omx_cmd(ptr: u32, len: u32);
}

const MODE_OK: u8 = 0;
const MODE_SPIN: u8 = 1;
const MODE_HUGE_SCENE: u8 = 2;
const MODE_TRAP: u8 = 3;

static MODE: AtomicU8 = AtomicU8::new(MODE_OK);

fn send_cmd(cmd: &HostCmd) {
    if let Ok(bytes) = envelope::encode_cmd(cmd) {
        unsafe { omx_cmd(bytes.as_ptr() as u32, bytes.len() as u32) };
    }
}

/// Hand a buffer to the host. The host writes into it and passes (ptr, len)
/// back through omx_init/omx_event/omx_render.
#[no_mangle]
pub extern "C" fn omx_alloc(len: u32) -> u32 {
    let mut buf = vec![0u8; len as usize];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr as u32
}

#[no_mangle]
pub extern "C" fn omx_api_version() -> u32 {
    API_VERSION
}

#[no_mangle]
pub unsafe extern "C" fn omx_init(ptr: u32, len: u32) {
    let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    let settings: Settings = postcard::from_bytes(bytes).unwrap_or_default();
    let mode = settings
        .iter()
        .find(|(k, _)| k == "mode")
        .and_then(|(_, v)| match v {
            SettingValue::Str(s) => Some(s.as_str()),
            _ => None,
        })
        .unwrap_or("ok");
    let code = match mode {
        "spin" => MODE_SPIN,
        "huge_scene" => MODE_HUGE_SCENE,
        "trap" => MODE_TRAP,
        _ => MODE_OK,
    };
    MODE.store(code, Ordering::Relaxed);
    send_cmd(&HostCmd::Log { level: 1, msg: format!("strike-widget init mode={mode}") });
}

#[no_mangle]
pub unsafe extern "C" fn omx_event(ptr: u32, len: u32) -> u32 {
    let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    // Unknown envelope tags decode to None — both are "no render needed".
    let ev: Option<Event> = envelope::decode_event(bytes).ok().flatten();
    if ev.is_none() {
        return 0;
    }
    match MODE.load(Ordering::Relaxed) {
        MODE_SPIN => {
            let mut x: u64 = 0;
            loop {
                x = x.wrapping_add(1);
                core::hint::black_box(x);
            }
        }
        MODE_TRAP => core::arch::wasm32::unreachable(),
        _ => 1, // bit0 = needs render
    }
}

#[no_mangle]
pub unsafe extern "C" fn omx_render(ptr: u32, len: u32) -> u64 {
    let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    // Geom is postcard-encoded WedgeGeom; this fixture ignores its values.
    let _ = bytes;
    let scene = match MODE.load(Ordering::Relaxed) {
        // Empty-ops Paths: no per-prim allocation, ~3 encoded bytes each —
        // cheap enough that the host's Scene::validate prim cap (not fuel
        // exhaustion) is what strikes.
        MODE_HUGE_SCENE => Scene {
            prims: (0..=MAX_SCENE_PRIMS)
                .map(|_| Prim::Path { ops: Vec::new(), stroke: None, fill: None })
                .collect(),
        },
        _ => Scene {
            prims: vec![
                Prim::Text {
                    x: 0.0,
                    y: -6.0,
                    content: "42".into(),
                    size: 18.0,
                    color: Color::Palette("accent".into()),
                    weight: TextWeight::Bold,
                    align: TextAlign::Center,
                },
                Prim::Path {
                    ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(4.0, 4.0), PathOp::Close],
                    stroke: None,
                    fill: Some(Color::Rgba(255, 255, 255, 255)),
                },
            ],
        },
    };
    let out = match postcard::to_allocvec(&scene) {
        Ok(o) => o,
        Err(_) => return 0,
    };
    let p = out.as_ptr() as u64;
    let l = out.len() as u64;
    std::mem::forget(out);
    (p << 32) | l
}

//! `register_widget!(MyWidget)` — expands to the raw ABI exports.
//!
//! Only meaningful when compiling to `wasm32-wasip1`; the macro wraps all
//! exports in `#[cfg(target_arch = "wasm32")]` so native test builds compile
//! without linker errors.

/// Register a type that implements [`crate::Widget`] as the wasm module's
/// single widget implementation.
///
/// Expands to five `#[no_mangle] pub extern "C"` functions that match the
/// raw ABI expected by `oxidemx-widget-host`:
///
/// - `omx_api_version() -> u32`
/// - `omx_alloc(len: u32) -> u32`
/// - `omx_init(ptr: u32, len: u32)`
/// - `omx_event(ptr: u32, len: u32) -> u32`  (bit0 = needs_render)
/// - `omx_render(ptr: u32, len: u32) -> u64`  (packed ptr<<32|len)
///
/// All functions are only compiled for `wasm32` targets — native test builds
/// skip the block entirely.
///
/// All proto and postcard references go through `$crate::proto` and
/// `$crate::postcard` — consumer crates only need `oxidemx-widget-api`
/// as a direct dependency.
#[macro_export]
macro_rules! register_widget {
    ($T:ty) => {
        #[cfg(target_arch = "wasm32")]
        mod __widget_abi {
            use super::*;
            use $crate::{Ctx, Widget};
            use $crate::proto::{envelope, API_VERSION};
            use $crate::proto::settings::Settings;

            #[link(wasm_import_module = "oxidemx")]
            extern "C" {
                fn omx_cmd(ptr: u32, len: u32);
            }

            struct State {
                widget: $T,
                settings: Settings,
            }

            static mut WIDGET_STATE: Option<State> = None;

            /// Flush all queued commands through the host import.
            fn flush(ctx: &Ctx) {
                for cmd in ctx.drain_cmds() {
                    if let Ok(bytes) = envelope::encode_cmd(&cmd) {
                        unsafe { omx_cmd(bytes.as_ptr() as u32, bytes.len() as u32) };
                    }
                }
            }

            #[no_mangle]
            pub extern "C" fn omx_api_version() -> u32 {
                API_VERSION
            }

            /// Allocate a buffer the host can write into.  The host calls this
            /// to get a pointer, writes the encoded payload, then calls the
            /// function with (ptr, actual_len).
            #[no_mangle]
            pub extern "C" fn omx_alloc(len: u32) -> u32 {
                let mut buf = vec![0u8; len as usize];
                let ptr = buf.as_mut_ptr() as u32;
                std::mem::forget(buf);
                ptr
            }

            #[no_mangle]
            pub unsafe extern "C" fn omx_init(ptr: u32, len: u32) {
                let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
                // Settings arrive as raw postcard (not an Envelope).
                let settings: Settings =
                    $crate::postcard::from_bytes(bytes).unwrap_or_default();
                let ctx = Ctx::new(settings.clone());
                let mut widget = <$T>::default();
                widget.init(&ctx);
                flush(&ctx);
                WIDGET_STATE = Some(State { widget, settings });
            }

            #[no_mangle]
            pub unsafe extern "C" fn omx_event(ptr: u32, len: u32) -> u32 {
                let state = match WIDGET_STATE.as_mut() {
                    Some(s) => s,
                    None => return 0,
                };
                let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
                // Events arrive wrapped in an Envelope; unknown tags → 0.
                let ev = match envelope::decode_event(bytes) {
                    Ok(Some(e)) => e,
                    _ => return 0,
                };
                let ctx = Ctx::new(state.settings.clone());
                let needs_render = state.widget.on_event(ev, &ctx);
                flush(&ctx);
                if needs_render { 1 } else { 0 }
            }

            #[no_mangle]
            pub unsafe extern "C" fn omx_render(ptr: u32, len: u32) -> u64 {
                let state = match WIDGET_STATE.as_ref() {
                    Some(s) => s,
                    None => return 0,
                };
                let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
                let geom: $crate::proto::WedgeGeom =
                    match $crate::postcard::from_bytes(bytes) {
                        Ok(g) => g,
                        Err(_) => return 0,
                    };
                let scene = state.widget.render(geom);
                let out = match $crate::postcard::to_allocvec(&scene) {
                    Ok(o) => o,
                    Err(_) => return 0,
                };
                let p = out.as_ptr() as u64;
                let l = out.len() as u64;
                std::mem::forget(out);
                (p << 32) | l
            }
        }
    };
}

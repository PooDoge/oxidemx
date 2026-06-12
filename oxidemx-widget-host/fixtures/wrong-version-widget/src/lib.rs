//! Minimal fixture reporting an unsupported ABI version (99). The host
//! must refuse it at load time, before looking up any other export.

#[no_mangle]
pub extern "C" fn omx_api_version() -> u32 {
    99
}

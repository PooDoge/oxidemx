//! One wasmi instance per placed widget: load/init, fuel-metered event and
//! render calls, and the spec §8/§9 strike rules (trap, fuel exhaustion,
//! scene decode failure, scene cap — three strikes disable the instance
//! for the session; success never resets the count).
//!
//! ABI (spec §8): guest exports `omx_api_version()->u32`, `omx_alloc(u32)->u32`,
//! `omx_init(ptr,len)`, `omx_event(ptr,len)->u32` (bit0 = needs render),
//! `omx_render(ptr,len)->u64` (`ptr << 32 | len` of a postcard `Scene`);
//! guest imports `omx_cmd(ptr,len)` from module `"oxidemx"` carrying an
//! envelope-encoded `HostCmd`. Events also travel envelope-encoded
//! (`envelope::encode_event`); init settings and the render geom are raw
//! postcard (`Settings` / `WedgeGeom`).
//!
//! wasm32-wasip1 cdylibs pull in WASI imports (fd_write/proc_exit/environ_*)
//! through std's panic/abort machinery even when they never do I/O, so the
//! linker carries a no-inherit `wasmi_wasi` context.

use std::path::Path;

use oxidemx_widget_proto::envelope;
use oxidemx_widget_proto::scene::MAX_SCENE_BYTES;
use oxidemx_widget_proto::settings::Settings;
use oxidemx_widget_proto::{Event, HostCmd, Scene, WedgeGeom, API_VERSION};
use wasmi::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store, TypedFunc};
use wasmi_wasi::{WasiCtx, WasiCtxBuilder};

/// Fuel budget per `omx_event` call (~5 ms intent; tune once measured).
pub const EVENT_FUEL: u64 = 5_000_000;
/// Fuel budget per `omx_render` call (~2 ms intent).
pub const RENDER_FUEL: u64 = 2_000_000;
/// Strikes before the instance is disabled for the session.
pub const MAX_STRIKES: u8 = 3;

/// Budget for instantiation (data segments) + `omx_api_version` + `omx_init`.
const LOAD_FUEL: u64 = 50_000_000;

#[derive(Debug)]
pub enum InstanceError {
    Io(std::io::Error),
    Wasm(String),
    ApiVersionMismatch { got: u32, want: u32 },
}

impl std::fmt::Display for InstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceError::Io(e) => write!(f, "cannot read widget module: {e}"),
            InstanceError::Wasm(e) => write!(f, "{e}"),
            InstanceError::ApiVersionMismatch { got, want } => {
                write!(f, "widget speaks API version {got} but this host wants {want}")
            }
        }
    }
}

impl std::error::Error for InstanceError {}

#[derive(Debug, PartialEq)]
pub enum CallOutcome {
    /// Event handled; `true` = the widget wants a render pass.
    /// Also returned (as `false`) for a non-disabling strike — check
    /// [`WidgetInstance::last_error`] for the reason.
    NeedsRender(bool),
    /// Render produced a validated scene.
    Scene(Scene),
    /// This call was the strike that exhausted the budget.
    Disabled,
    /// Instance was already disabled; nothing was called.
    Skipped,
}

/// Store payload: the queue the `omx_cmd` import pushes into, plus the
/// WASI shim context.
struct HostState {
    cmds: Vec<HostCmd>,
    wasi: WasiCtx,
}

pub struct WidgetInstance {
    store: Store<HostState>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    event_fn: TypedFunc<(u32, u32), u32>,
    render_fn: TypedFunc<(u32, u32), u64>,
    strikes: u8,
    disabled: bool,
    last_error: Option<String>,
}

impl WidgetInstance {
    /// Instantiate `widget.wasm`, check `omx_api_version`, link `omx_cmd`
    /// (decoded `HostCmd`s queue up for [`Self::drain_cmds`]), then call
    /// `omx_init` with the postcard-encoded settings.
    pub fn load(wasm_path: &Path, settings: &Settings) -> Result<Self, InstanceError> {
        let wasm = std::fs::read(wasm_path).map_err(InstanceError::Io)?;

        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, &wasm)
            .map_err(|e| InstanceError::Wasm(format!("module does not compile: {e}")))?;

        let state = HostState { cmds: Vec::new(), wasi: WasiCtxBuilder::new().build() };
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<HostState>::new(&engine);
        wasmi_wasi::add_to_linker(&mut linker, |s: &mut HostState| &mut s.wasi)
            .map_err(|e| InstanceError::Wasm(format!("WASI linking failed: {e}")))?;
        linker
            .func_wrap(
                "oxidemx",
                "omx_cmd",
                |mut caller: Caller<'_, HostState>, ptr: u32, len: u32| {
                    let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
                        log::warn!("widget called omx_cmd without exporting memory");
                        return;
                    };
                    let mut buf = vec![0u8; len as usize];
                    if mem.read(&caller, ptr as usize, &mut buf).is_err() {
                        log::warn!("widget cmd ptr/len out of bounds");
                        return;
                    }
                    match envelope::decode_cmd(&buf) {
                        Ok(Some(cmd)) => caller.data_mut().cmds.push(cmd),
                        Ok(None) => log::debug!("widget cmd with unknown tag skipped"),
                        Err(e) => log::warn!("widget cmd decode failed: {e}"),
                    }
                },
            )
            .map_err(|e| InstanceError::Wasm(format!("omx_cmd linking failed: {e}")))?;

        store.set_fuel(LOAD_FUEL).expect("fuel metering is enabled");
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| InstanceError::Wasm(format!("instantiation failed: {e}")))?;

        // Version gate first — before requiring any other export.
        let api_version: TypedFunc<(), u32> = instance
            .get_typed_func(&store, "omx_api_version")
            .map_err(|e| InstanceError::Wasm(format!("missing omx_api_version export: {e}")))?;
        let got = api_version
            .call(&mut store, ())
            .map_err(|e| InstanceError::Wasm(format!("omx_api_version trapped: {e}")))?;
        if got != API_VERSION {
            return Err(InstanceError::ApiVersionMismatch { got, want: API_VERSION });
        }

        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| InstanceError::Wasm("widget exports no memory".into()))?;
        let alloc: TypedFunc<u32, u32> = instance
            .get_typed_func(&store, "omx_alloc")
            .map_err(|e| InstanceError::Wasm(format!("missing omx_alloc export: {e}")))?;
        let init: TypedFunc<(u32, u32), ()> = instance
            .get_typed_func(&store, "omx_init")
            .map_err(|e| InstanceError::Wasm(format!("missing omx_init export: {e}")))?;
        let event_fn: TypedFunc<(u32, u32), u32> = instance
            .get_typed_func(&store, "omx_event")
            .map_err(|e| InstanceError::Wasm(format!("missing omx_event export: {e}")))?;
        let render_fn: TypedFunc<(u32, u32), u64> = instance
            .get_typed_func(&store, "omx_render")
            .map_err(|e| InstanceError::Wasm(format!("missing omx_render export: {e}")))?;

        let mut this = WidgetInstance {
            store,
            memory,
            alloc,
            event_fn,
            render_fn,
            strikes: 0,
            disabled: false,
            last_error: None,
        };

        let bytes = postcard::to_allocvec(settings)
            .map_err(|e| InstanceError::Wasm(format!("settings encode failed: {e}")))?;
        let (ptr, len) = this
            .write_guest(&bytes)
            .map_err(|e| InstanceError::Wasm(format!("settings upload failed: {e}")))?;
        init.call(&mut this.store, (ptr, len))
            .map_err(|e| InstanceError::Wasm(format!("omx_init failed: {e}")))?;
        Ok(this)
    }

    /// Envelope-encode `ev` and call `omx_event` under [`EVENT_FUEL`].
    pub fn on_event(&mut self, ev: &Event) -> CallOutcome {
        if self.disabled {
            return CallOutcome::Skipped;
        }
        // Host-side encode failure is our bug, not the widget's — no strike.
        let bytes = match envelope::encode_event(ev) {
            Ok(b) => b,
            Err(e) => {
                log::error!("event encode failed (host bug): {e}");
                return CallOutcome::NeedsRender(false);
            }
        };
        self.store.set_fuel(EVENT_FUEL).expect("fuel metering is enabled");
        let (ptr, len) = match self.write_guest(&bytes) {
            Ok(x) => x,
            Err(e) => return self.strike(format!("event upload failed: {e}")),
        };
        match self.event_fn.call(&mut self.store, (ptr, len)) {
            Ok(bits) => CallOutcome::NeedsRender(bits & 1 == 1),
            Err(e) => self.strike(format!("event call failed: {e}")),
        }
    }

    /// Postcard-encode `geom`, call `omx_render` under [`RENDER_FUEL`],
    /// enforce the byte cap on the returned length BEFORE decoding, then
    /// decode + `Scene::validate`.
    pub fn render(&mut self, geom: &WedgeGeom) -> CallOutcome {
        if self.disabled {
            return CallOutcome::Skipped;
        }
        let bytes = match postcard::to_allocvec(geom) {
            Ok(b) => b,
            Err(e) => {
                log::error!("geom encode failed (host bug): {e}");
                return CallOutcome::NeedsRender(false);
            }
        };
        self.store.set_fuel(RENDER_FUEL).expect("fuel metering is enabled");
        let (ptr, len) = match self.write_guest(&bytes) {
            Ok(x) => x,
            Err(e) => return self.strike(format!("render upload failed: {e}")),
        };
        let packed = match self.render_fn.call(&mut self.store, (ptr, len)) {
            Ok(p) => p,
            Err(e) => return self.strike(format!("render call failed: {e}")),
        };
        let scene_ptr = (packed >> 32) as u32;
        let scene_len = (packed & 0xFFFF_FFFF) as usize;
        // Byte cap on the *returned length*, before reading or decoding.
        if scene_len > MAX_SCENE_BYTES {
            return self.strike(format!("scene is {scene_len} bytes (max {MAX_SCENE_BYTES})"));
        }
        let mut buf = vec![0u8; scene_len];
        if self.memory.read(&self.store, scene_ptr as usize, &mut buf).is_err() {
            return self.strike("scene ptr/len out of guest memory bounds".into());
        }
        let scene: Scene = match postcard::from_bytes(&buf) {
            Ok(s) => s,
            Err(e) => return self.strike(format!("scene decode failed: {e}")),
        };
        if let Err(e) = scene.validate() {
            return self.strike(e);
        }
        CallOutcome::Scene(scene)
    }

    /// Take every `HostCmd` the guest issued since the last drain.
    pub fn drain_cmds(&mut self) -> Vec<HostCmd> {
        std::mem::take(&mut self.store.data_mut().cmds)
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// One strike. At [`MAX_STRIKES`] the instance flips disabled for the
    /// session and this call reports [`CallOutcome::Disabled`]; earlier
    /// strikes report `NeedsRender(false)`. Success never resets the count.
    fn strike(&mut self, reason: String) -> CallOutcome {
        self.strikes += 1;
        log::warn!("widget strike {}/{MAX_STRIKES}: {reason}", self.strikes);
        self.last_error = Some(reason);
        if self.strikes >= MAX_STRIKES {
            self.disabled = true;
            CallOutcome::Disabled
        } else {
            CallOutcome::NeedsRender(false)
        }
    }

    /// `omx_alloc` a guest buffer and copy `bytes` into it. Runs under
    /// whatever fuel the caller just set.
    fn write_guest(&mut self, bytes: &[u8]) -> Result<(u32, u32), String> {
        let len = bytes.len() as u32;
        let ptr = self
            .alloc
            .call(&mut self.store, len)
            .map_err(|e| format!("omx_alloc failed: {e}"))?;
        self.memory
            .write(&mut self.store, ptr as usize, bytes)
            .map_err(|e| format!("guest memory write failed: {e}"))?;
        Ok((ptr, len))
    }
}

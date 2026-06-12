//! The widget host worker: a single tokio task that owns every
//! [`WidgetInstance`] (wasmi stores never cross task boundaries — no
//! mutexes), the registry, per-instance timers, and the permission-gated
//! HTTP cache. It talks to the UI exclusively over channels:
//!
//! - UI → worker: [`HostCtl`] (config reconcile, slice input, menu state)
//! - worker → UI: [`HostEvent`] (validated scenes, failures, registry)
//!
//! The frame path never calls wasm — the painter replays the last
//! [`Scene`] it got; this task is the only place guest code runs.
//!
//! Spec §8 behaviours implemented here: desired-state reconcile from
//! `AppConfig`, settings resolution (defaults ← global ← instance bag)
//! with JSON→`SettingValue` conversion, timer clamping to the manifest
//! refresh floor with a closed-menu fire latch, https-only exact-host
//! HTTP permissions, and the render→revision→`HostEvent::Scene` loop.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use oxidemx_shared::config::{AppConfig, WidgetScope, WidgetSource};
use oxidemx_shared::widgets::{instance_key as derive_instance_key, JsonBag};
use oxidemx_widget_proto::settings::{SettingValue, Settings};
use oxidemx_widget_proto::{Event, HostCmd, Scene, WedgeGeom};

use crate::http::{CacheResult, HttpCache, HttpFetcher, ReqwestFetcher};
use crate::instance::{CallOutcome, WidgetInstance};
use crate::registry::{WidgetRegistry, WidgetState};

/// UI → worker.
pub enum HostCtl {
    /// Config (re)loaded — re-derive the desired instance set.
    ConfigChanged(Arc<AppConfig>),
    /// The widgets install dir changed — rescan and re-reconcile.
    RescanWidgets,
    /// Pointer input on a Custom widget slice. Carries the live wedge
    /// geometry so renders use what the painter actually laid out.
    Slice { instance: InstanceId, ev: SliceEvent, geom: WedgeGeom },
    MenuOpened { page: String },
    MenuClosed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SliceEvent {
    Click,
    Scroll(f32),
    Hover(bool),
}

/// worker → UI.
#[derive(Debug, Clone)]
pub enum HostEvent {
    /// A freshly validated scene for the painter's replay store.
    Scene { instance: InstanceId, scene: Scene, revision: u64 },
    /// The instance died (3 strikes) or failed to load.
    InstanceFailed { instance: InstanceId, error: String },
    /// Emitted after every (re)scan — feeds Plan 3's picker.
    RegistryChanged(Vec<WidgetSummary>),
}

/// `<page-slug>.slot<N>` ↔ widget id pair identifying one placed widget.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceId {
    pub instance_key: String,
    pub widget_id: String,
}

/// Picker-facing digest of one installed widget.
#[derive(Debug, Clone)]
pub struct WidgetSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    /// `"ready"` or `"incompatible: <reason>"`.
    pub state: String,
    pub has_options: bool,
    pub icon_path: PathBuf,
    /// XDG icon name for the disabled/missing fallback wedge
    /// (spec §9), from `manifest.slice.fallback_icon`.
    pub fallback_icon: Option<String>,
}

/// Wedge geometry assumed before the painter has sent a real one.
const DEFAULT_GEOM: WedgeGeom = WedgeGeom {
    width: 200.0,
    height: 160.0,
    inner_radius: 60.0,
    outer_radius: 160.0,
    angle_start: 0.0,
    angle_end: std::f32::consts::FRAC_PI_4,
    hovered: 0.0,
};

/// Spawn the worker with the default install dir and the reqwest fetcher.
pub fn spawn(
    ctl: async_channel::Receiver<HostCtl>,
    events: async_channel::Sender<HostEvent>,
) -> tokio::task::JoinHandle<()> {
    let dir = WidgetRegistry::widgets_dir().unwrap_or_else(|| {
        log::warn!("cannot resolve the widgets dir (no HOME?); using ./widgets");
        PathBuf::from("widgets")
    });
    spawn_with(ctl, events, dir, Box::new(ReqwestFetcher::new()))
}

/// Test/embedding entry point: explicit widgets dir + fetcher.
pub fn spawn_with(
    ctl: async_channel::Receiver<HostCtl>,
    events: async_channel::Sender<HostEvent>,
    widgets_dir: PathBuf,
    fetcher: Box<dyn HttpFetcher>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(Worker::new(ctl, events, widgets_dir, fetcher.into()).run())
}

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

/// Messages the worker sends itself from spawned timer/fetch tasks.
enum Internal {
    TimerFired { instance: InstanceId, timer: String },
    FetchDone { url: String, status: u16, body: Vec<u8> },
}

/// A running timer. Dropping it (instance removed, timer replaced or
/// cancelled, worker shutdown) aborts the tokio task — that is also how
/// timers are "suspended" for unplaced widgets: their instance is gone.
struct TimerState {
    handle: tokio::task::JoinHandle<()>,
    /// Closed-menu latch: at most one fire while the menu is closed.
    fired_while_closed: bool,
}

impl Drop for TimerState {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Host-side bookkeeping for one live instance.
struct Meta {
    permissions: Vec<String>,
    /// `manifest.effective_refresh_ms()` — timer clamp floor + HTTP TTL.
    refresh_ms: u64,
    /// Resolved settings as last delivered; diffed on reconcile.
    settings: Settings,
    /// Last geometry the painter reported (DEFAULT_GEOM until then).
    geom: WedgeGeom,
    revision: u64,
    timers: HashMap<String, TimerState>,
}

struct Worker {
    ctl: async_channel::Receiver<HostCtl>,
    events: async_channel::Sender<HostEvent>,
    widgets_dir: PathBuf,
    registry: WidgetRegistry,
    instances: HashMap<InstanceId, (WidgetInstance, Meta)>,
    cache: HttpCache<(InstanceId, String)>,
    fetcher: Arc<dyn HttpFetcher>,
    menu_open: bool,
    last_cfg: Option<Arc<AppConfig>>,
    internal_tx: tokio::sync::mpsc::UnboundedSender<Internal>,
    internal_rx: tokio::sync::mpsc::UnboundedReceiver<Internal>,
}

impl Worker {
    fn new(
        ctl: async_channel::Receiver<HostCtl>,
        events: async_channel::Sender<HostEvent>,
        widgets_dir: PathBuf,
        fetcher: Arc<dyn HttpFetcher>,
    ) -> Self {
        let (internal_tx, internal_rx) = tokio::sync::mpsc::unbounded_channel();
        Worker {
            ctl,
            events,
            registry: WidgetRegistry::scan(&widgets_dir),
            widgets_dir,
            instances: HashMap::new(),
            cache: HttpCache::new(),
            fetcher,
            menu_open: false,
            last_cfg: None,
            internal_tx,
            internal_rx,
        }
    }

    async fn run(mut self) {
        self.emit_registry().await;
        loop {
            tokio::select! {
                ctl = self.ctl.recv() => match ctl {
                    Ok(c) => self.handle_ctl(c).await,
                    Err(_) => break, // UI dropped the control channel
                },
                Some(msg) = self.internal_rx.recv() => self.handle_internal(msg).await,
            }
        }
        // Dropping `instances` aborts every timer via TimerState::drop.
    }

    async fn handle_ctl(&mut self, ctl: HostCtl) {
        match ctl {
            HostCtl::ConfigChanged(cfg) => {
                self.last_cfg = Some(cfg.clone());
                self.reconcile(&cfg).await;
            }
            HostCtl::RescanWidgets => {
                self.registry = WidgetRegistry::scan(&self.widgets_dir);
                self.emit_registry().await;
                // A widget may have been (un)installed under a placed slice.
                if let Some(cfg) = self.last_cfg.clone() {
                    self.reconcile(&cfg).await;
                }
            }
            HostCtl::Slice { instance, ev, geom } => {
                if let Some((_, meta)) = self.instances.get_mut(&instance) {
                    meta.geom = geom;
                }
                let event = match ev {
                    SliceEvent::Click => Event::Click,
                    SliceEvent::Scroll(delta) => Event::Scroll { delta },
                    SliceEvent::Hover(entering) => Event::Hover { entering },
                };
                self.pump(VecDeque::from([(instance, event)])).await;
            }
            HostCtl::MenuOpened { page } => {
                self.menu_open = true;
                for (_, meta) in self.instances.values_mut() {
                    for t in meta.timers.values_mut() {
                        t.fired_while_closed = false; // reset the latch
                    }
                }
                let queue: VecDeque<_> = self
                    .instances
                    .keys()
                    .cloned()
                    .map(|id| (id, Event::MenuOpened { page: page.clone() }))
                    .collect();
                self.pump(queue).await;
            }
            HostCtl::MenuClosed => {
                self.menu_open = false;
                let queue: VecDeque<_> =
                    self.instances.keys().cloned().map(|id| (id, Event::MenuClosed)).collect();
                self.pump(queue).await;
            }
        }
    }

    async fn handle_internal(&mut self, msg: Internal) {
        match msg {
            Internal::TimerFired { instance, timer } => {
                let Some((_, meta)) = self.instances.get_mut(&instance) else { return };
                if !self.menu_open {
                    let Some(t) = meta.timers.get_mut(&timer) else { return };
                    if t.fired_while_closed {
                        log::debug!("timer {timer:?} suppressed while the menu is closed");
                        return;
                    }
                    t.fired_while_closed = true;
                }
                self.pump(VecDeque::from([(instance, Event::Timer(timer))])).await;
            }
            Internal::FetchDone { url, status, body } => {
                let waiters = self.cache.complete(&url, status, &body);
                let queue: VecDeque<_> = waiters
                    .into_iter()
                    .map(|(instance, req_id)| {
                        (
                            instance,
                            Event::HttpResponse { id: req_id, status, body: body.clone() },
                        )
                    })
                    .collect();
                self.pump(queue).await;
            }
        }
    }

    // -- reconcile ----------------------------------------------------------

    /// Desired-state reconcile (spec §8): make `instances` match the
    /// Custom widget slices in `cfg`, resolving settings through the
    /// two-bag store and diffing them for live instances.
    async fn reconcile(&mut self, cfg: &AppConfig) {
        let desired = desired_instances(cfg);

        // Drop instances that are no longer placed (timers abort on drop).
        let gone: Vec<InstanceId> = self
            .instances
            .keys()
            .filter(|id| !desired.contains_key(*id))
            .cloned()
            .collect();
        for id in gone {
            log::info!("widget instance {}/{} unplaced", id.widget_id, id.instance_key);
            self.instances.remove(&id);
        }

        for (id, scope) in desired {
            let Some(installed) = self.registry.get(&id.widget_id) else {
                log::warn!("config places widget {:?} but it is not installed", id.widget_id);
                self.instances.remove(&id);
                continue;
            };
            if let WidgetState::Incompatible { reason } = &installed.state {
                log::warn!("config places incompatible widget {:?}: {reason}", id.widget_id);
                self.instances.remove(&id);
                continue;
            }
            let manifest = installed.manifest.clone();
            let defaults = manifest.defaults();
            let bag = cfg.widgets.resolve(&id.widget_id, Some(&id.instance_key), scope, &defaults);
            let settings = settings_from_bag(&bag);

            if let Some((_, meta)) = self.instances.get_mut(&id) {
                // Manifest may have changed across a rescan.
                meta.permissions = manifest.permissions.clone();
                meta.refresh_ms = manifest.effective_refresh_ms();
                if meta.settings != settings {
                    // v1: settings changes reload the instance — the ABI has no
                    // settings-refresh path; revisit at api_version 2.
                    // Drop the old instance (timers abort) and fall through to
                    // the "new placement" path below, which re-inits with the
                    // new settings bag.
                    log::info!(
                        "widget instance {}/{} settings changed — reloading",
                        id.widget_id, id.instance_key
                    );
                    self.instances.remove(&id);
                }
                // If settings were unchanged, skip re-init; otherwise fall
                // through (instance was just removed).
                if self.instances.contains_key(&id) {
                    continue;
                }
            }

            // New placement: load + init + initial render.
            let wasm_path = installed.dir.join(&manifest.entry);
            match WidgetInstance::load(&wasm_path, &settings) {
                Ok(instance) => {
                    let meta = Meta {
                        permissions: manifest.permissions.clone(),
                        refresh_ms: manifest.effective_refresh_ms(),
                        settings,
                        geom: DEFAULT_GEOM,
                        revision: 0,
                        timers: HashMap::new(),
                    };
                    self.instances.insert(id.clone(), (instance, meta));

                    // Route the cmds init issued (timers, http_get, …).
                    let cmds =
                        self.instances.get_mut(&id).expect("just inserted").0.drain_cmds();
                    let mut queue = VecDeque::new();
                    for cmd in cmds {
                        self.route_cmd(&id, cmd, &mut queue);
                    }
                    self.render_and_emit(&id).await;
                    self.pump(queue).await;
                }
                Err(e) => {
                    let error = e.to_string();
                    log::warn!("widget {:?} failed to load: {error}", id.widget_id);
                    let _ = self
                        .events
                        .send(HostEvent::InstanceFailed { instance: id, error })
                        .await;
                }
            }
        }
    }

    // -- event/cmd pump -------------------------------------------------------

    /// Deliver queued events to instances; route the cmds each call
    /// produces (which may enqueue follow-up events, e.g. an immediate
    /// denied/cached HttpResponse); render after any `needs_render`.
    async fn pump(&mut self, mut queue: VecDeque<(InstanceId, Event)>) {
        while let Some((id, ev)) = queue.pop_front() {
            let Some((instance, _)) = self.instances.get_mut(&id) else { continue };
            let outcome = instance.on_event(&ev);
            let cmds = instance.drain_cmds();
            for cmd in cmds {
                self.route_cmd(&id, cmd, &mut queue);
            }
            match outcome {
                CallOutcome::NeedsRender(true) => self.render_and_emit(&id).await,
                CallOutcome::Disabled => self.fail(&id).await,
                _ => {}
            }
        }
    }

    /// Execute one guest command (spec §8 command table).
    fn route_cmd(&mut self, id: &InstanceId, cmd: HostCmd, queue: &mut VecDeque<(InstanceId, Event)>) {
        let Some((_, meta)) = self.instances.get_mut(id) else { return };
        match cmd {
            HostCmd::SetTimer { id: timer_id, secs } => {
                // Clamp to the manifest refresh floor (spec §8).
                let floor = meta.refresh_ms / 1000;
                let period = Duration::from_secs(secs.max(floor).max(1));
                let tx = self.internal_tx.clone();
                let instance = id.clone();
                let timer = timer_id.clone();
                let handle = tokio::spawn(async move {
                    let mut tick = tokio::time::interval_at(
                        tokio::time::Instant::now() + period,
                        period,
                    );
                    loop {
                        tick.tick().await;
                        if tx
                            .send(Internal::TimerFired {
                                instance: instance.clone(),
                                timer: timer.clone(),
                            })
                            .is_err()
                        {
                            return; // worker gone
                        }
                    }
                });
                // Replacing an existing timer aborts it (TimerState::drop).
                meta.timers.insert(timer_id, TimerState { handle, fired_while_closed: false });
            }
            HostCmd::CancelTimer { id: timer_id } => {
                meta.timers.remove(&timer_id);
            }
            HostCmd::HttpGet { id: req_id, url } => {
                if let Err(reason) = check_net_permission(&meta.permissions, &url) {
                    log::warn!("widget {:?}: {reason}", id.widget_id);
                    queue.push_back((
                        id.clone(),
                        Event::HttpResponse { id: req_id, status: 0, body: reason.into_bytes() },
                    ));
                    return;
                }
                let ttl = Duration::from_millis(meta.refresh_ms);
                match self.cache.get(&url, ttl) {
                    CacheResult::Hit { status, body } => {
                        queue.push_back((
                            id.clone(),
                            Event::HttpResponse { id: req_id, status, body },
                        ));
                    }
                    CacheResult::Pending => {
                        self.cache.add_waiter(&url, (id.clone(), req_id));
                    }
                    CacheResult::Miss => {
                        self.cache.add_waiter(&url, (id.clone(), req_id));
                        let fetcher = self.fetcher.clone();
                        let tx = self.internal_tx.clone();
                        tokio::spawn(async move {
                            let (status, body) = fetcher.fetch(&url).await;
                            let _ = tx.send(Internal::FetchDone { url, status, body });
                        });
                    }
                }
            }
            HostCmd::OpenUrl(url) => {
                if let Err(reason) = check_open_url(&meta.permissions, &url) {
                    log::warn!("widget {:?}: {reason}", id.widget_id);
                    return;
                }
                if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    log::warn!("xdg-open {url:?} failed: {e}");
                }
            }
            HostCmd::Exec(command) => {
                if !has_permission(&meta.permissions, "exec") {
                    log::warn!("widget {:?} lacks the exec permission", id.widget_id);
                    return;
                }
                if let Err(e) =
                    std::process::Command::new("sh").arg("-c").arg(&command).spawn()
                {
                    log::warn!("widget exec {command:?} failed: {e}");
                }
            }
            HostCmd::HapticPulse(pattern) => {
                if !has_permission(&meta.permissions, "haptics") {
                    log::warn!("widget {:?} lacks the haptics permission", id.widget_id);
                    return;
                }
                // TODO Plan 3: route to daemon haptic_client.
                log::info!(target: "widget", "haptic pulse {pattern:?} (stub)");
            }
            HostCmd::Log { level, msg } => {
                log::info!(target: "widget", "[{}/{level}] {msg}", id.widget_id);
            }
            // HostCmd is #[non_exhaustive]: a newer proto could add variants.
            other => log::debug!("unhandled widget cmd: {other:?}"),
        }
    }

    // -- output ---------------------------------------------------------------

    /// Render with the last-known geometry, bump the revision, and ship
    /// the validated scene to the UI.
    async fn render_and_emit(&mut self, id: &InstanceId) {
        let Some((instance, meta)) = self.instances.get_mut(id) else { return };
        let geom = meta.geom;
        match instance.render(&geom) {
            CallOutcome::Scene(scene) => {
                meta.revision += 1;
                let revision = meta.revision;
                let _ = self
                    .events
                    .send(HostEvent::Scene { instance: id.clone(), scene, revision })
                    .await;
            }
            CallOutcome::Disabled => self.fail(id).await,
            // NeedsRender(false) = a non-disabling strike (already logged);
            // Skipped = instance disabled earlier.
            CallOutcome::NeedsRender(_) | CallOutcome::Skipped => {}
        }
    }

    /// Three strikes: report the death and stop its timers. The disabled
    /// instance stays in the map so later calls cheaply skip.
    async fn fail(&mut self, id: &InstanceId) {
        let error = self
            .instances
            .get_mut(id)
            .map(|(instance, meta)| {
                meta.timers.clear(); // abort all timers
                instance.last_error().unwrap_or("unknown error").to_string()
            })
            .unwrap_or_else(|| "unknown error".into());
        log::warn!("widget instance {}/{} disabled: {error}", id.widget_id, id.instance_key);
        let _ = self
            .events
            .send(HostEvent::InstanceFailed { instance: id.clone(), error })
            .await;
    }

    async fn emit_registry(&mut self) {
        let summaries = self
            .registry
            .iter()
            .map(|w| WidgetSummary {
                id: w.manifest.id.clone(),
                name: w.manifest.name.clone(),
                version: w.manifest.version.clone(),
                author: w.manifest.author.clone(),
                state: match &w.state {
                    WidgetState::Ready => "ready".into(),
                    WidgetState::Incompatible { reason } => format!("incompatible: {reason}"),
                },
                has_options: !w.manifest.options.is_empty(),
                icon_path: w.dir.join(&w.manifest.icon),
                fallback_icon: w.manifest.slice.fallback_icon.clone(),
            })
            .collect();
        let _ = self.events.send(HostEvent::RegistryChanged(summaries)).await;
    }
}

// ---------------------------------------------------------------------------
// pure helpers
// ---------------------------------------------------------------------------

/// Walk the config's pages for placed Custom widgets. Slices without an
/// explicit `instance_key` derive the Plan 1 `<page-slug>.slot<N>` key
/// from their position.
fn desired_instances(cfg: &AppConfig) -> HashMap<InstanceId, WidgetScope> {
    let mut out = HashMap::new();
    let legacy_page; // pre-normalize_pages configs keep slices at the top
    let pages: Vec<(&str, &[oxidemx_shared::config::Slice])> =
        if cfg.radial_menu.pages.is_empty() {
            legacy_page = ("Default", cfg.radial_menu.slices.as_slice());
            vec![legacy_page]
        } else {
            cfg.radial_menu
                .pages
                .iter()
                .map(|p| (p.name.as_str(), p.slices.as_slice()))
                .collect()
        };
    for (page_name, slices) in pages {
        for (slot, slice) in slices.iter().enumerate() {
            let Some(w) = &slice.widget else { continue };
            let WidgetSource::Custom(widget_id) = &w.source else { continue };
            let instance_key = w
                .instance_key
                .clone()
                .unwrap_or_else(|| derive_instance_key(page_name, slot));
            out.insert(
                InstanceId { instance_key, widget_id: widget_id.clone() },
                w.scope,
            );
        }
    }
    out
}

/// Exact-name permission lookup (`"exec"`, `"open-url"`, `"haptics"`).
fn has_permission(permissions: &[String], name: &str) -> bool {
    permissions.iter().any(|p| p == name)
}

/// `open-url` gate: the permission must be declared AND the URL must be a
/// well-formed http/https URL — handing arbitrary schemes (`file:`,
/// `javascript:`, custom protocol handlers) to `xdg-open` is an obvious
/// escalation path.
fn check_open_url(permissions: &[String], url: &str) -> Result<(), String> {
    if !has_permission(permissions, "open-url") {
        return Err("denied: manifest lacks the open-url permission".into());
    }
    let parsed =
        url::Url::parse(url).map_err(|e| format!("denied: {url:?} does not parse: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        s => Err(format!("denied: open-url allows only http/https (got scheme {s:?})")),
    }
}

/// `net:<host>` allowlist check: https only, exact host match (spec §8).
fn check_net_permission(permissions: &[String], url: &str) -> Result<(), String> {
    let parsed =
        url::Url::parse(url).map_err(|e| format!("denied: {url:?} does not parse: {e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!("denied: only https is allowed (got {:?})", parsed.scheme()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("denied: {url:?} has no host"))?;
    // Case-insensitive host comparison (spec §8): manifest entries like
    // `net:API.Open-Meteo.com` must match `api.open-meteo.com` from the
    // parsed URL, which the url crate normalises to ASCII lowercase.
    if permissions
        .iter()
        .any(|p| p.strip_prefix("net:").map(|h| h.eq_ignore_ascii_case(host)).unwrap_or(false))
    {
        Ok(())
    } else {
        Err(format!("denied: manifest lacks the net:{host} permission"))
    }
}

/// JSON bag → postcard `Settings` (spec §6): string/number/bool map
/// directly; a `{name, lat, lon}` object becomes a Location; anything
/// else is skipped with a log line. Output is sorted by key.
fn settings_from_bag(bag: &JsonBag) -> Settings {
    let mut out: Settings = Vec::with_capacity(bag.len());
    for (key, value) in bag {
        let converted = match value {
            serde_json::Value::String(s) => Some(SettingValue::Str(s.clone())),
            serde_json::Value::Number(n) => n.as_f64().map(SettingValue::Num),
            serde_json::Value::Bool(b) => Some(SettingValue::Bool(*b)),
            serde_json::Value::Object(o) => location_from_obj(o),
            _ => None,
        };
        match converted {
            Some(v) => out.push((key.clone(), v)),
            None => log::warn!("widget setting {key:?} has an unsupported shape; skipped"),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn location_from_obj(obj: &JsonBag) -> Option<SettingValue> {
    let name = obj.get("name")?.as_str()?.to_string();
    let lat = obj.get("lat")?.as_f64()?;
    let lon = obj.get("lon")?.as_f64()?;
    Some(SettingValue::Location { name, lat, lon })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_conversion_handles_all_shapes() {
        let bag: JsonBag = serde_json::from_value(json!({
            "z_units": "c",
            "refresh": 900,
            "metric": true,
            "location": { "name": "Oslo", "lat": 59.91, "lon": 10.75 },
            "bogus_array": [1, 2, 3],
            "bogus_obj": { "lat": 1.0 },
            "bogus_null": null,
        }))
        .unwrap();
        let settings = settings_from_bag(&bag);
        // Sorted by key, unsupported shapes dropped.
        assert_eq!(
            settings,
            vec![
                ("location".into(), SettingValue::Location {
                    name: "Oslo".into(),
                    lat: 59.91,
                    lon: 10.75
                }),
                ("metric".into(), SettingValue::Bool(true)),
                ("refresh".into(), SettingValue::Num(900.0)),
                ("z_units".into(), SettingValue::Str("c".into())),
            ]
        );
    }

    #[test]
    fn net_permission_is_https_only_and_exact_host() {
        let perms = vec!["net:api.open-meteo.com".to_string(), "open-url".to_string()];
        assert!(check_net_permission(&perms, "https://api.open-meteo.com/v1?x=1").is_ok());
        // http downgrade
        assert!(check_net_permission(&perms, "http://api.open-meteo.com/v1").is_err());
        // sub/superdomain confusion must not pass an exact-host check
        assert!(check_net_permission(&perms, "https://evil.api.open-meteo.com/").is_err());
        assert!(check_net_permission(&perms, "https://api.open-meteo.com.evil.io/").is_err());
        // unrelated host + garbage
        assert!(check_net_permission(&perms, "https://example.com/").is_err());
        assert!(check_net_permission(&perms, "not a url").is_err());
    }

    #[test]
    fn open_url_requires_permission_and_web_scheme() {
        let perms = vec!["open-url".to_string()];
        assert!(check_open_url(&perms, "https://example.com/page?x=1").is_ok());
        assert!(check_open_url(&perms, "http://example.com/").is_ok());
        // scheme escapes must be refused even with the permission
        assert!(check_open_url(&perms, "file:///etc/passwd").is_err());
        assert!(check_open_url(&perms, "javascript:alert(1)").is_err());
        assert!(check_open_url(&perms, "vscode://malicious/payload").is_err());
        assert!(check_open_url(&perms, "not a url").is_err());
        // missing / wrong permission
        assert!(check_open_url(&[], "https://example.com/").is_err());
        let other = vec!["net:example.com".to_string()];
        assert!(check_open_url(&other, "https://example.com/").is_err());
    }

    #[test]
    fn haptics_and_exec_gates_are_exact_name_permissions() {
        let perms = vec!["haptics".to_string(), "open-url".to_string()];
        assert!(has_permission(&perms, "haptics"));
        assert!(!has_permission(&perms, "exec"));
        assert!(!has_permission(&[], "haptics"));
        // no prefix/substring confusion
        let odd = vec!["haptics-extra".to_string(), "my-exec".to_string()];
        assert!(!has_permission(&odd, "haptics"));
        assert!(!has_permission(&odd, "exec"));
    }

    #[test]
    fn net_permission_case_insensitive_manifest_entry() {
        // Manifests may be authored with mixed-case hosts (e.g. copy-pasted
        // from docs). A `net:API.Open-Meteo.com` entry must match the
        // url-crate-normalised lowercase host `api.open-meteo.com`.
        let perms = vec!["net:API.Open-Meteo.com".to_string()];
        assert!(
            check_net_permission(&perms, "https://api.open-meteo.com/v1?x=1").is_ok(),
            "mixed-case manifest entry should match normalised URL host"
        );
        // Unrelated host still rejected.
        assert!(check_net_permission(&perms, "https://example.com/").is_err());
    }
}

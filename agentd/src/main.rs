//! agentd — production entry point.
//!
//! Starts the org.oxidemx.Agent D-Bus service on the session bus.
//! On `NameTaken` (another agentd already running) it logs a message and exits
//! cleanly so systemd's `Restart=on-failure` does NOT trigger.

#![forbid(unsafe_code)]

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use agentd::connector::auth::load_or_create_token;
use agentd::connector::event_hub::{BroadcastEmitter, EventHub};
use agentd::connector::http::server::{serve, ServeConfig};
use agentd::connector::tailnet::{resolve_bind_addr, CliTailnetSource};
use agentd::interface::{AgentInterface, AgentService, ProjectRegistry};
use agentd::models::ModelControls;
use agentd::host_proxy::HostCapabilityProxy;
use agentd::seams::{AgentEvent, Approver, EventEmitter};
use agentd::sessions::Sessions;
use oxidemx_agent_local::LocalModelService;
use oxidemx_shared::config::HttpConfig;

// ── NullLocalService ──────────────────────────────────────────────────────────

/// Default local-model service stub used when the `mistral` feature is disabled.
///
/// All methods return an `Inference` / `ModelNotFound` error; `unload` and
/// `set_active` are no-ops that succeed so the model-control layer stays alive.
struct NullLocalService;

#[async_trait::async_trait]
impl LocalModelService for NullLocalService {
    async fn chat(
        &self,
        _req: oxidemx_agent_local::types::ChatRequest,
    ) -> Result<oxidemx_agent_local::types::ChatResponse, oxidemx_agent_local::error::LocalError>
    {
        Err(oxidemx_agent_local::error::LocalError::Inference(
            "local model unavailable (no engine configured)".into(),
        ))
    }

    async fn chat_with_model(
        &self,
        _alias: &str,
        _req: oxidemx_agent_local::types::ChatRequest,
    ) -> Result<oxidemx_agent_local::types::ChatResponse, oxidemx_agent_local::error::LocalError>
    {
        Err(oxidemx_agent_local::error::LocalError::Inference(
            "local model unavailable (no engine configured)".into(),
        ))
    }

    async fn ensure_loaded(
        &self,
        _alias: &str,
    ) -> Result<(), oxidemx_agent_local::error::LocalError> {
        Err(oxidemx_agent_local::error::LocalError::ModelNotFound(
            "no engine configured".into(),
        ))
    }

    async fn unload(&self, _alias: &str) -> Result<(), oxidemx_agent_local::error::LocalError> {
        Ok(())
    }

    async fn set_active(
        &self,
        _alias: &str,
    ) -> Result<(), oxidemx_agent_local::error::LocalError> {
        Ok(())
    }

    fn status(&self) -> Vec<oxidemx_agent_local::types::ModelStatusInfo> {
        vec![]
    }
}

// ── fallback_uid ──────────────────────────────────────────────────────────────

/// Best-effort: parse `$UID` from the environment or default 1000.
/// No libc; no unsafe.  Only used when `$XDG_RUNTIME_DIR` is unset.
fn fallback_uid() -> u32 {
    std::env::var("UID").ok().and_then(|s| s.parse().ok()).unwrap_or(1000)
}

// ── select_emitter ────────────────────────────────────────────────────────────

/// Choose the production emitter based on whether the HTTP transport is enabled.
///
/// When `cfg.enabled` is `true` the `BusEmitter` is wrapped in a
/// `BroadcastEmitter` that also fans events into the returned `EventHub` for
/// SSE subscribers.  When disabled the plain `BusEmitter` is returned and the
/// hub is `None` — byte-for-byte identical behaviour to the pre-1b path.
fn select_emitter(
    cfg: &HttpConfig,
    tx: mpsc::Sender<AgentEvent>,
) -> (Arc<dyn EventEmitter>, Option<EventHub>) {
    if cfg.enabled {
        let bus: Arc<dyn EventEmitter> = Arc::new(BusEmitter { tx });
        let hub = EventHub::new(cfg.event_buffer);
        let wrapped: Arc<dyn EventEmitter> = Arc::new(BroadcastEmitter::new(bus, hub.clone()));
        (wrapped, Some(hub))
    } else {
        (Arc::new(BusEmitter { tx }), None)
    }
}

// ── BusEmitter ────────────────────────────────────────────────────────────────

/// Production [`EventEmitter`] that routes events through an mpsc channel to a
/// drain task which emits the zbus signal.
///
/// `emit` never blocks: if the channel is full the event is dropped with a
/// warning log.
struct BusEmitter {
    tx: mpsc::Sender<AgentEvent>,
}

impl EventEmitter for BusEmitter {
    fn emit(&self, ev: AgentEvent) {
        if self.tx.try_send(ev).is_err() {
            tracing::warn!("BusEmitter: event channel full — dropping event");
        }
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialise structured logging.
    tracing_subscriber::fmt::init();

    // 2. Build emitter channel (BusEmitter sends; drain task receives).
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);

    // ── HTTP transport (1b), opt-in via AppConfig.http.enabled ──
    let http_cfg = oxidemx_shared::config::default_config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<oxidemx_shared::config::AppConfig>(&s).ok())
        .map(|c| c.http)
        .unwrap_or_default();

    let (emitter, http_hub): (Arc<dyn EventEmitter>, Option<EventHub>) =
        select_emitter(&http_cfg, tx);

    // 3. Build the local-model service (NullLocalService by default).
    //    With `--features mistral` a real mistral.rs engine would go here.
    let local_svc: Arc<dyn LocalModelService> = Arc::new(NullLocalService);

    // 4. Build AgentService with production wiring.
    let models = Arc::new(ModelControls::new(local_svc, emitter.clone()));
    let sessions = Arc::new(Sessions::new());
    let approver = Arc::new(Approver::new(emitter.clone()));

    // Resolve the store base from XDG_DATA_HOME or HOME.
    let store_base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from(".local/share"))
        .join("oxidemx");

    let svc = AgentService::new(
        ProjectRegistry::with_store_base(store_base),
        sessions,
        models,
        approver,
        emitter.clone(),
        Arc::new(HostCapabilityProxy),
    );
    let svc = Arc::new(svc);

    // 4a. Run startup migration (best-effort; never aborts startup on error).
    if let Err(e) = svc.migrate_on_start().await {
        tracing::warn!("migrate_on_start failed (non-fatal): {e}");
    }

    // 4b. HTTP transport (opt-in). Non-fatal: failures are logged and the daemon
    //     continues without the HTTP surface. The D-Bus path is ALWAYS started.
    if let Some(hub) = http_hub {
        // Token dir = user config dir (XDG_CONFIG_HOME or ~/.config) / oxidemx.
        let cfg_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".config"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from(".config"))
            .join("oxidemx");
        let token = load_or_create_token(&cfg_dir).unwrap_or_default();

        let uds_path = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| {
                std::path::PathBuf::from(format!("/run/user/{}", fallback_uid()))
            })
            .join("oxidemx")
            .join("agentd.sock");

        let tcp_addr = match resolve_bind_addr(
            &CliTailnetSource,
            &http_cfg.bind_override,
            http_cfg.port,
        ) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("http tcp bind disabled: {e}");
                None
            }
        };

        let serve_cfg = ServeConfig {
            svc: svc.clone(),
            hub,
            token,
            uds_path,
            tcp_addr,
            shutdown: CancellationToken::new(),
        };
        if let Err(e) = serve(serve_cfg).await {
            tracing::warn!("http serve failed to start (non-fatal): {e}");
        }
    }

    // 5. Build the zbus session connection, serving at /org/oxidemx/Agent.
    let connection = zbus::connection::Builder::session()?
        .serve_at("/org/oxidemx/Agent", AgentInterface::new(svc.clone()))?
        .build()
        .await?;

    // 6. Request the well-known bus name. On NameTaken exit cleanly so systemd
    //    does not trigger a restart loop.
    if let Err(e) = connection.request_name("org.oxidemx.Agent").await {
        if matches!(e, zbus::Error::NameTaken) {
            tracing::info!("org.oxidemx.Agent already owned — another agentd is running; exiting");
            return Ok(());
        }
        return Err(e.into());
    }
    tracing::info!("org.oxidemx.Agent registered; agentd is ready");

    // 7. Spawn drain task: forward AgentEvents from the channel → D-Bus signals.
    let conn_drain = connection.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let iface = conn_drain
                .object_server()
                .interface::<_, AgentInterface>("/org/oxidemx/Agent")
                .await;
            let iface = match iface {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("drain: interface lookup failed: {e}");
                    continue;
                }
            };
            let se = iface.signal_emitter().clone();
            let payload_str = ev.payload.to_string();
            drop(iface); // Release object-server guard before awaiting signal emissions

            // All events go through the general `event` signal.
            if let Err(e) = AgentInterface::event(
                &se,
                ev.project.clone(),
                ev.thread_or_run.clone(),
                ev.ts,
                payload_str.clone(),
            )
            .await
            {
                tracing::warn!("drain: event signal failed: {e}");
            }

            // model_status events also get their own dedicated signal.
            if ev.payload.get("kind").and_then(|k| k.as_str()) == Some("model_status") {
                let alias = ev
                    .payload
                    .get("alias")
                    .and_then(|a| a.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Err(e) =
                    AgentInterface::model_status_changed(&se, alias, payload_str).await
                {
                    tracing::warn!("drain: model_status_changed signal failed: {e}");
                }
            }

            // C1: ApprovalRequest events also emit the dedicated approval_requested signal
            // so the overlay can pop the approval UI without parsing the generic event.
            // Payload fields set by Approver::request: "kind", "request_id", "card".
            if ev.payload.get("kind").and_then(|k| k.as_str()) == Some("ApprovalRequest") {
                let request_id = ev
                    .payload
                    .get("request_id")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                let card = ev
                    .payload
                    .get("card")
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                if let Err(e) = AgentInterface::approval_requested(
                    &se,
                    ev.project.clone(),
                    ev.thread_or_run.clone(),
                    request_id,
                    card,
                )
                .await
                {
                    tracing::warn!("drain: approval_requested signal failed: {e}");
                }
            }
        }
    });

    // 8. Park the main task forever; the tokio runtime keeps the service alive.
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disabled_cfg() -> HttpConfig {
        HttpConfig { enabled: false, ..HttpConfig::default() }
    }

    fn enabled_cfg() -> HttpConfig {
        HttpConfig { enabled: true, ..HttpConfig::default() }
    }

    fn make_tx() -> mpsc::Sender<AgentEvent> {
        let (tx, _rx) = mpsc::channel::<AgentEvent>(8);
        tx
    }

    #[test]
    fn select_emitter_disabled_yields_no_hub() {
        let (_emitter, hub) = select_emitter(&disabled_cfg(), make_tx());
        assert!(hub.is_none(), "disabled path must not create an EventHub");
    }

    #[test]
    fn select_emitter_enabled_yields_some_hub() {
        let (_emitter, hub) = select_emitter(&enabled_cfg(), make_tx());
        assert!(hub.is_some(), "enabled path must create an EventHub");
    }
}

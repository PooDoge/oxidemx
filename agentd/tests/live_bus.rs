//! Live D-Bus integration test for the agentd interface.
//!
//! Requires a running session bus. Marked `#[ignore]` so the CI suite skips it.
//!
//! Run manually:
//!   cargo test -p agentd --test live_bus -- --ignored
//!
//! What this test verifies:
//! 1. `AgentService` can be served on a real session-bus connection.
//! 2. An `AgentProxy` client can call `send_message`.
//! 3. The `event` D-Bus signal is received by the client.
//! 4. `get_transcript` returns >= 2 turns after `send_message`.

#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures_util::stream::StreamExt;
use tokio::sync::mpsc;

use agentd::error::AgentdError;
use agentd::interface::{AgentInterface, AgentService, ProjectRegistry, TurnRunner};
use agentd::models::ModelControls;
use agentd::projects::ProjectKey;
use agentd::seams::{AgentEvent, Approver, EventEmitter, RecordingEmitter, UnavailableHost};
use agentd::sessions::Sessions;
use oxidemx_agent_local::LocalModelService;
use oxidemx_agent_proxy::AgentProxy;

// ── helpers ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── NullLocal (test-local stub) ───────────────────────────────────────────────

struct NullLocal;

#[async_trait]
impl LocalModelService for NullLocal {
    async fn chat(
        &self,
        _req: oxidemx_agent_local::types::ChatRequest,
    ) -> Result<oxidemx_agent_local::types::ChatResponse, oxidemx_agent_local::error::LocalError>
    {
        Err(oxidemx_agent_local::error::LocalError::Inference(
            "unavailable".into(),
        ))
    }

    async fn chat_with_model(
        &self,
        _alias: &str,
        _req: oxidemx_agent_local::types::ChatRequest,
    ) -> Result<oxidemx_agent_local::types::ChatResponse, oxidemx_agent_local::error::LocalError>
    {
        Err(oxidemx_agent_local::error::LocalError::Inference(
            "unavailable".into(),
        ))
    }

    async fn ensure_loaded(
        &self,
        _alias: &str,
    ) -> Result<(), oxidemx_agent_local::error::LocalError> {
        Err(oxidemx_agent_local::error::LocalError::ModelNotFound(
            "no engine".into(),
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

// ── LiveMock TurnRunner ───────────────────────────────────────────────────────

/// Minimal `TurnRunner` for the live-bus test: returns a canned reply without
/// touching any LLM backend.
struct LiveMock;

#[async_trait]
impl TurnRunner for LiveMock {
    async fn run_turn(
        &self,
        _project: &ProjectKey,
        _thread: &str,
        _text: &str,
        _approver: &Arc<Approver>,
        _emitter: &Arc<dyn EventEmitter>,
    ) -> Result<String, AgentdError> {
        Ok("live-bus-reply".to_string())
    }
}

// ── ChannelEmitter ────────────────────────────────────────────────────────────

/// Routes `AgentEvent`s through an mpsc channel so the drain task can emit
/// the D-Bus signal.  Mirrors the production `BusEmitter` in `main.rs`.
struct ChannelEmitter {
    tx: mpsc::Sender<AgentEvent>,
}

impl EventEmitter for ChannelEmitter {
    fn emit(&self, ev: AgentEvent) {
        if self.tx.try_send(ev).is_err() {
            // best-effort: drop on full rather than block
        }
    }
}

// ── the test ──────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires a running session bus; run with: cargo test -p agentd --test live_bus -- --ignored"]
async fn send_message_emits_event_signal_and_transcript_grows() {
    // --- server side ---

    let tmp = tempfile::tempdir().unwrap();
    let store_base = tmp.path().join("store");
    let cwd = tmp.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();

    let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);
    let emitter: Arc<dyn EventEmitter> = Arc::new(ChannelEmitter { tx });
    let recording_emitter = Arc::new(RecordingEmitter::default());
    // Use recording_emitter as approver's emitter (won't be used in this test
    // since LiveMock never requests approval).
    let approver = Arc::new(Approver::new(recording_emitter));

    let null_svc: Arc<dyn LocalModelService> = Arc::new(NullLocal);
    let models = Arc::new(ModelControls::new(null_svc, emitter.clone()));

    let mut svc = AgentService::new(
        ProjectRegistry::with_store_base(store_base),
        Arc::new(Sessions::new()),
        models,
        approver,
        emitter.clone(),
        Arc::new(UnavailableHost),
    );
    // Override turn_runner with the canned mock — no LLM call.
    svc.turn_runner = Arc::new(LiveMock);

    let svc = Arc::new(svc);

    // Unique bus name to avoid clashing with a running agentd instance.
    let bus_name = format!("org.oxidemx.Agent.test.{}", now_ms());
    let server_conn = zbus::connection::Builder::session()
        .unwrap()
        .name(bus_name.as_str())
        .unwrap()
        .serve_at("/org/oxidemx/Agent", AgentInterface::new(svc.clone()))
        .unwrap()
        .build()
        .await
        .expect("server connection failed — is a session bus running?");

    // Drain task: forward AgentEvents from channel -> event D-Bus signal.
    let conn_drain = server_conn.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let Ok(iface) = conn_drain
                .object_server()
                .interface::<_, AgentInterface>("/org/oxidemx/Agent")
                .await
            else {
                continue;
            };
            let se = iface.signal_emitter().clone();
            let payload_str = ev.payload.to_string();
            let _ = AgentInterface::event(
                &se,
                ev.project,
                ev.thread_or_run,
                ev.ts,
                payload_str,
            )
            .await;
        }
    });

    // --- client side ---

    let client_conn = zbus::Connection::session()
        .await
        .expect("client session bus connection failed");

    // Build a proxy pointed at our unique bus name.
    let proxy = AgentProxy::builder(&client_conn)
        .destination(bus_name.as_str())
        .expect("proxy destination")
        .path("/org/oxidemx/Agent")
        .expect("proxy path")
        .build()
        .await
        .expect("proxy build failed");

    // Subscribe to the event signal BEFORE calling send_message so we don't
    // miss the signal.
    let mut event_stream = proxy
        .receive_event()
        .await
        .expect("receive_event stream failed");

    // Call send_message.
    let cwd_str = cwd.to_str().unwrap();
    let _turn_id = proxy
        .send_message(cwd_str, "live-thread", "hello live bus", "")
        .await
        .expect("send_message D-Bus call failed");

    // Wait for the event signal (timeout: 5 s).
    let received = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        event_stream.next(),
    )
    .await
    .expect("timed out waiting for event signal")
    .expect("event stream ended unexpectedly");

    let args = received.args().expect("failed to parse event signal args");
    assert!(
        !args.payload.is_empty(),
        "event payload should not be empty"
    );

    // Get the transcript and assert >= 2 turns (user + assistant).
    let transcript_json = proxy
        .get_transcript(cwd_str, "live-thread")
        .await
        .expect("get_transcript D-Bus call failed");

    let turns: Vec<serde_json::Value> =
        serde_json::from_str(&transcript_json).expect("transcript JSON parse failed");

    assert!(
        turns.len() >= 2,
        "expected >= 2 turns (user + assistant), got {}: {}",
        turns.len(),
        transcript_json
    );
}

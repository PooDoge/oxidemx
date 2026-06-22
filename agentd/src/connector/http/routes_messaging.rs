//! Messaging-tier routes (mounted on both UDS and TCP listeners).
#![forbid(unsafe_code)]

use std::path::PathBuf;

use axum::extract::{Path as AxPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::{next_message_id, ApiError, AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/projects", get(list_projects))
        .route("/v1/projects/{project_id}/conversations", get(list_conversations))
        .route("/v1/conversations", post(create_conversation))
        .route("/v1/conversations/{id}", get(get_conversation))
        .route("/v1/conversations/{id}/messages", get(history).post(send_message))
        .route("/v1/conversations/{id}/approvals/{request_id}", post(respond_approval))
        .route("/v1/conversations/{id}/events", get(super::sse::events_handler))
        .with_state(state)
}

async fn health() -> &'static str { "ok" }

async fn list_projects(State(st): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let projects = st.svc.list_projects().await?;
    Ok(Json(serde_json::to_value(projects).unwrap_or_default()))
}

async fn list_conversations(
    State(st): State<AppState>,
    AxPath(project_id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let convs = st.svc.list_conversations(&project_id).await?;
    Ok(Json(serde_json::to_value(convs).unwrap_or_default()))
}

#[derive(serde::Deserialize)]
struct CreateConversationBody { project_id: Option<String>, working_dir: Option<String> }

async fn create_conversation(
    State(st): State<AppState>,
    Json(body): Json<CreateConversationBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let project_id = body.project_id.unwrap_or_else(|| "personal".to_string());
    let wd = body.working_dir.map(PathBuf::from);
    let conv = st.svc.create_conversation(&project_id, wd.as_deref()).await?;
    Ok(Json(serde_json::json!({ "conversation_id": conv.id.as_str() })))
}

async fn get_conversation(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match st.svc.get_conversation(&id).await? {
        Some(c) => Ok(Json(serde_json::to_value(c).unwrap_or_default())),
        None => Err(ApiError(crate::error::AgentdError::NotFound(format!("conversation {id}")))),
    }
}

async fn history(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let conv = st.svc.get_conversation(&id).await?
        .ok_or_else(|| crate::error::AgentdError::NotFound(format!("conversation {id}")))?;
    let project = conv.working_dir.to_string_lossy().to_string();
    let turns = st.svc.get_transcript(&project, &id).await?;
    Ok(Json(serde_json::to_value(turns).unwrap_or_default()))
}

#[derive(serde::Deserialize)]
struct SendBody { text: String, model: Option<String> }

async fn send_message(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
    Json(body): Json<SendBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let conv = st.svc.get_conversation(&id).await?
        .ok_or_else(|| crate::error::AgentdError::NotFound(format!("conversation {id}")))?;
    let project = conv.working_dir.to_string_lossy().to_string();
    let message_id = next_message_id(&st);

    // Spawn the turn; deltas stream via the emitter→hub→SSE. The core
    // AgentService already emits Turn + final on success via BroadcastEmitter,
    // so we only publish a terminal `error` event on failure (the core emits
    // no terminal event on error, so this is the only error signal).
    let svc = st.svc.clone();
    let hub = st.hub.clone();
    let conv_id = id.clone();
    let model = body.model.clone();
    let text = body.text.clone();
    let mid = message_id.clone();
    tokio::spawn(async move {
        match svc.send_message(&project, &conv_id, &text, model.as_deref()).await {
            Ok(_reply) => { /* core already emitted Turn + final via the emitter */ }
            Err(e) => {
                hub.publish(&crate::seams::AgentEvent {
                    project: String::new(),
                    thread_or_run: conv_id,
                    ts: super::now_ms(),
                    payload: serde_json::json!({ "kind": "error", "message_id": mid, "message": e.to_string() }),
                });
            }
        }
    });

    Ok(Json(serde_json::json!({ "message_id": message_id })))
}

#[derive(serde::Deserialize)]
struct ApprovalBody { allow: bool, reason: Option<String> }

async fn respond_approval(
    State(st): State<AppState>,
    AxPath((id, request_id)): AxPath<(String, String)>,
    Json(body): Json<ApprovalBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let conv = st.svc.get_conversation(&id).await?
        .ok_or_else(|| crate::error::AgentdError::NotFound(format!("conversation {id}")))?;
    let project = conv.working_dir.to_string_lossy().to_string();
    st.svc.respond_approval(&project, &request_id, body.allow, body.reason.as_deref()).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

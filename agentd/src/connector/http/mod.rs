//! axum HTTP connector. Delegates to the connector-agnostic AgentService.
#![forbid(unsafe_code)]

pub mod routes_messaging;
pub mod sse;            // Task 7
pub mod routes_control; // Task 8
pub mod server;         // Task 8

pub use server::{serve, ServeConfig};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::connector::caps::ConnectorCaps;
use crate::connector::event_hub::EventHub;
use crate::error::AgentdError;
use crate::interface::AgentService;

#[derive(Clone)]
pub struct AppState {
    pub svc: Arc<AgentService>,
    pub hub: EventHub,
    pub caps: ConnectorCaps,
    pub msg_seq: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(svc: Arc<AgentService>, hub: EventHub, caps: ConnectorCaps) -> Self {
        Self { svc, hub, caps, msg_seq: Arc::new(AtomicU64::new(0)) }
    }
}

/// Monotonic-ish message id for action acks.
pub fn next_message_id(state: &AppState) -> String {
    let n = state.msg_seq.fetch_add(1, Ordering::Relaxed) + 1;
    format!("m{}-{}", crate::connector::http::now_ms(), n)
}

pub(crate) fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// HTTP error envelope mapping AgentdError → status + JSON.
pub struct ApiError(pub AgentdError);

impl From<AgentdError> for ApiError {
    fn from(e: AgentdError) -> Self { ApiError(e) }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            AgentdError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AgentdError::Project(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            // Io, Dbus, and any future variants → 500. The wildcard handles
            // #[non_exhaustive] additions without a compile error.
            #[allow(unreachable_patterns)]
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        let body = Json(serde_json::json!({ "error": { "code": code, "message": self.0.to_string() } }));
        (status, body).into_response()
    }
}

pub fn build_messaging_router(state: AppState) -> axum::Router {
    routes_messaging::router(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    // Build an AppState with Control caps over a test AgentService.
    // The TempDir is intentionally leaked so it outlives the async test body.
    // This is acceptable in tests: the process is short-lived.
    fn test_state() -> AppState {
        let (svc, tmp) = crate::interface::tests::test_service();
        std::mem::forget(tmp);
        AppState::new(svc, EventHub::new(16), ConnectorCaps::UDS_LOCAL)
    }

    #[tokio::test]
    async fn health_is_ok() {
        let app = build_messaging_router(test_state());
        let res = app.oneshot(Request::get("/v1/health").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_projects_returns_personal() {
        let app = build_messaging_router(test_state());
        let res = app.oneshot(Request::get("/v1/projects").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.as_array().unwrap().iter().any(|p| p["id"] == "personal"));
    }

    #[tokio::test]
    async fn unknown_conversation_404() {
        let app = build_messaging_router(test_state());
        let res = app.oneshot(
            Request::get("/v1/conversations/does-not-exist/messages").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}

//! Control-tier routes — mounted ONLY on the UDS (Control-scope) listener.
#![forbid(unsafe_code)]

use axum::routing::get;
use axum::{Json, Router};

use super::AppState;

/// Control router. The token is captured so the local app can fetch it for pairing.
pub fn router(state: AppState, token: String) -> Router {
    Router::new()
        .route("/v1/auth/token", get(move || {
            let token = token.clone();
            async move { Json(serde_json::json!({ "token": token })) }
        }))
        .with_state(state)
}

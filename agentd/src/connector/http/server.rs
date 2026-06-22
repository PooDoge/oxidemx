//! Listener setup: UDS (Control) + tailnet TCP (Messaging), scope-by-listener.
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::response::IntoResponse;
use tokio_util::sync::CancellationToken;

use crate::connector::auth::verify_bearer;
use crate::connector::caps::ConnectorCaps;
use crate::connector::event_hub::EventHub;
use crate::interface::AgentService;
use super::{build_messaging_router, routes_control, AppState};

pub struct ServeConfig {
    pub svc: Arc<AgentService>,
    pub hub: EventHub,
    pub token: String,
    pub uds_path: PathBuf,
    pub tcp_addr: Option<SocketAddr>,
    pub shutdown: CancellationToken,
}

/// Messaging routes only (used by the TCP listener + tests).
pub fn messaging_only_router(state: AppState) -> Router {
    build_messaging_router(state)
}

/// Messaging + control routes (used by the UDS listener + tests).
pub fn full_router(state: AppState, token: impl Into<String>) -> Router {
    let token = token.into();
    build_messaging_router(state.clone())
        .merge(routes_control::router(state, token))
}

/// Spawn the listeners. Returns immediately; tasks run until `shutdown` fires.
pub async fn serve(cfg: ServeConfig) -> std::io::Result<()> {
    // ── UDS (Control scope) ──
    let uds_state = AppState::new(cfg.svc.clone(), cfg.hub.clone(), ConnectorCaps::UDS_LOCAL);
    let uds_router = full_router(uds_state, cfg.token.clone());
    if let Some(parent) = cfg.uds_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let _ = std::fs::remove_file(&cfg.uds_path); // clear stale socket
    let uds = tokio::net::UnixListener::bind(&cfg.uds_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cfg.uds_path, std::fs::Permissions::from_mode(0o600))?;
    }
    let uds_shutdown = cfg.shutdown.clone();
    tokio::spawn(async move {
        let r = axum::serve(uds, uds_router)
            .with_graceful_shutdown(async move { uds_shutdown.cancelled().await });
        if let Err(e) = r.await { tracing::warn!("uds serve ended: {e}"); }
    });
    tracing::info!("HTTP UDS listener on {} (Control scope)", cfg.uds_path.display());

    // ── TCP (Messaging scope, bearer auth) ──
    if let Some(addr) = cfg.tcp_addr {
        let tcp_state = AppState::new(cfg.svc.clone(), cfg.hub.clone(), ConnectorCaps::TAILNET_REMOTE);
        let token = cfg.token.clone();
        let tcp_router = messaging_only_router(tcp_state).layer(
            axum::middleware::from_fn(move |req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| {
                let token = token.clone();
                async move {
                    // /v1/health is exempt.
                    if req.uri().path() == "/v1/health" {
                        return next.run(req).await;
                    }
                    let ok = verify_bearer(
                        req.headers().get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()),
                        &token,
                    );
                    if ok { next.run(req).await }
                    else { axum::http::StatusCode::UNAUTHORIZED.into_response() }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let tcp_shutdown = cfg.shutdown.clone();
        tokio::spawn(async move {
            let r = axum::serve(listener, tcp_router)
                .with_graceful_shutdown(async move { tcp_shutdown.cancelled().await });
            if let Err(e) = r.await { tracing::warn!("tcp serve ended: {e}"); }
        });
        tracing::info!("HTTP TCP listener on {addr} (Messaging scope)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn state(caps: crate::connector::caps::ConnectorCaps) -> AppState {
        let (svc, tmp) = crate::interface::tests::test_service();
        std::mem::forget(tmp);
        AppState::new(svc, EventHub::new(16), caps)
    }

    #[tokio::test]
    async fn control_route_present_on_uds_absent_on_tcp() {
        // UDS (Control) router includes auth/token.
        let uds = full_router(state(crate::connector::caps::ConnectorCaps::UDS_LOCAL), "tok");
        let r = uds.oneshot(Request::get("/v1/auth/token").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        // TCP (Messaging) router does NOT mount control routes.
        let tcp = messaging_only_router(state(crate::connector::caps::ConnectorCaps::TAILNET_REMOTE));
        let r = tcp.oneshot(Request::get("/v1/auth/token").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }
}

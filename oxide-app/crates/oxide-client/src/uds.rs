//! `UdsTransport` — agentd over a Unix domain socket using hyper 1.x. Each call
//! opens a fresh connection (agentd closes per response). SSE uses a streaming
//! body fed into `SseParser`, reconnecting with Last-Event-ID on drop.
use std::path::PathBuf;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;

use crate::dto::{AgentEvent, AttachmentPayload, Conversation, MessageId, Project, Turn};
use crate::error::TransportError;
use crate::sse::SseParser;
use crate::transport::Transport;

pub struct UdsTransport {
    sock: PathBuf,
}

impl UdsTransport {
    pub fn new(sock: impl Into<PathBuf>) -> Self { Self { sock: sock.into() } }

    /// `$XDG_RUNTIME_DIR/oxidemx/agentd.sock` (falls back to `/tmp`).
    pub fn default_socket() -> PathBuf {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(base).join("oxidemx").join("agentd.sock")
    }

    async fn send(&self, method: Method, path: &str, body: Option<serde_json::Value>)
        -> Result<(u16, Bytes), TransportError>
    {
        let stream = UnixStream::connect(&self.sock).await
            .map_err(|e| TransportError::Unreachable(e.to_string()))?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
            .map_err(|e| TransportError::Stream(e.to_string()))?;
        tokio::spawn(async move { let _ = conn.await; });

        let payload = body.as_ref().map(|v| v.to_string()).unwrap_or_default();
        let mut builder = Request::builder()
            .method(method).uri(path)
            .header("host", "localhost");
        if body.is_some() { builder = builder.header("content-type", "application/json"); }
        let req = builder
            .body(Full::new(Bytes::from(payload)))
            .map_err(|e| TransportError::Stream(e.to_string()))?;
        let resp = sender.send_request(req).await
            .map_err(|e| TransportError::Stream(e.to_string()))?;
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await
            .map_err(|e| TransportError::Stream(e.to_string()))?.to_bytes();
        Ok((status, bytes))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, TransportError> {
        let (status, bytes) = self.send(Method::GET, path, None).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        serde_json::from_slice(&bytes).map_err(|e| TransportError::Decode(e.to_string()))
    }
}

#[async_trait]
impl Transport for UdsTransport {
    async fn health(&self) -> Result<(), TransportError> {
        let (status, bytes) = self.send(Method::GET, "/v1/health", None).await?;
        if status != 200 { return Err(TransportError::Http(status)); }
        if bytes.as_ref() == b"ok" || bytes.as_ref() == b"ok\n" { Ok(()) }
        else { Err(TransportError::Decode(format!("unexpected health body: {:?}", String::from_utf8_lossy(&bytes)))) }
    }

    async fn list_projects(&self) -> Result<Vec<Project>, TransportError> {
        self.get_json("/v1/projects").await
    }

    async fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>, TransportError> {
        self.get_json(&format!("/v1/projects/{project_id}/conversations")).await
    }

    async fn create_conversation(&self, project_id: &str, working_dir: Option<&str>)
        -> Result<Conversation, TransportError>
    {
        let mut body = serde_json::json!({ "project_id": project_id });
        if let Some(wd) = working_dir { body["working_dir"] = serde_json::json!(wd); }
        let (status, bytes) = self.send(Method::POST, "/v1/conversations", Some(body)).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        // Response is {"conversation_id": "..."}; fetch the full record.
        #[derive(serde::Deserialize)] struct Created { conversation_id: String }
        let created: Created = serde_json::from_slice(&bytes).map_err(|e| TransportError::Decode(e.to_string()))?;
        self.get_json(&format!("/v1/conversations/{}", created.conversation_id)).await
    }

    async fn get_history(&self, conversation_id: &str) -> Result<Vec<Turn>, TransportError> {
        self.get_json(&format!("/v1/conversations/{conversation_id}/messages")).await
    }

    async fn send_message(&self, conversation_id: &str, text: &str, attachments: &[AttachmentPayload]) -> Result<MessageId, TransportError> {
        let body = serde_json::json!({ "text": text, "attachments": attachments });
        let (status, bytes) = self.send(Method::POST,
            &format!("/v1/conversations/{conversation_id}/messages"), Some(body)).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        #[derive(serde::Deserialize)] struct Sent { message_id: String }
        let sent: Sent = serde_json::from_slice(&bytes).map_err(|e| TransportError::Decode(e.to_string()))?;
        Ok(MessageId(sent.message_id))
    }

    async fn delete_conversation(&self, conversation_id: &str) -> Result<(), TransportError> {
        let (status, _) = self.send(Method::DELETE,
            &format!("/v1/conversations/{conversation_id}"), None).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        Ok(())
    }

    fn subscribe(&self, conversation_id: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>> {
        let sock = self.sock.clone();
        let path = format!("/v1/conversations/{conversation_id}/events");
        Box::pin(async_stream::stream! {
            let mut last_id = 0u64;
            loop {
                let stream = match UnixStream::connect(&sock).await {
                    Ok(s) => s,
                    Err(e) => { yield Err(TransportError::Unreachable(e.to_string())); break; }
                };
                let (mut sender, conn) = match hyper::client::conn::http1::handshake(TokioIo::new(stream)).await {
                    Ok(pair) => pair,
                    Err(e) => { yield Err(TransportError::Stream(e.to_string())); break; }
                };
                tokio::spawn(async move { let _ = conn.await; });

                let mut builder = Request::builder()
                    .method(Method::GET).uri(&path)
                    .header("host", "localhost")
                    .header("accept", "text/event-stream");
                if last_id > 0 { builder = builder.header("last-event-id", last_id.to_string()); }
                let req = match builder.body(Full::new(Bytes::new())) {
                    Ok(r) => r,
                    Err(e) => { yield Err(TransportError::Stream(e.to_string())); break; }
                };
                let resp = match sender.send_request(req).await {
                    Ok(r) => r,
                    Err(e) => { yield Err(TransportError::Stream(e.to_string())); break; }
                };

                let mut body = resp.into_body();
                let mut parser = SseParser::new();
                while let Some(frame) = body.frame().await {
                    let frame = match frame {
                        Ok(f) => f,
                        Err(e) => { yield Err(TransportError::Stream(e.to_string())); break; }
                    };
                    if let Some(chunk) = frame.data_ref() {
                        for ev in parser.push(chunk) {
                            last_id = ev.seq;
                            yield Ok(ev);
                        }
                    }
                }
                // Server closed / EOF — notify the consumer that a gap occurred before
                // we sleep and reconnect. The consumer uses this to surface
                // `ConnState::Reconnecting` in the UI (Rule 1).
                yield Err(TransportError::Stream("reconnecting".into()));
                // Back off then reconnect with Last-Event-ID.
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        })
    }
}

//! Transport-layer errors. Drive the UI connection state.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Socket missing or connection refused — agentd not reachable.
    #[error("agentd unreachable: {0}")]
    Unreachable(String),
    /// Non-2xx HTTP status.
    #[error("http status {0}")]
    Http(u16),
    /// Response body failed to decode.
    #[error("decode error: {0}")]
    Decode(String),
    /// SSE stream or connection error mid-stream.
    #[error("stream error: {0}")]
    Stream(String),
}

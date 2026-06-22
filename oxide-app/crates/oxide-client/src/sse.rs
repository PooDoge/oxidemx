//! Incremental Server-Sent-Events parser. Feed raw body bytes; get back
//! normalized `AgentEvent`s. Tracks the last `id:` for Last-Event-ID reconnect.
use crate::dto::AgentEvent;

#[derive(Default)]
pub struct SseParser {
    buf: String,
    cur_id: Option<u64>,
    cur_event: Option<String>,
    cur_data: String,
    last_id: u64,
}

impl SseParser {
    pub fn new() -> Self { Self::default() }
    pub fn last_id(&self) -> u64 { self.last_id }

    /// Push a chunk of the SSE body; returns any events completed by it.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<AgentEvent> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        // Process complete lines (terminated by '\n'); keep any partial tail.
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if let Some(ev) = self.dispatch() { out.push(ev); }
            } else if let Some(rest) = line.strip_prefix(':') {
                let _ = rest; // comment / keep-alive — ignore
            } else if let Some(v) = line.strip_prefix("id:") {
                self.cur_id = v.trim().parse().ok();
            } else if let Some(v) = line.strip_prefix("event:") {
                self.cur_event = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("data:") {
                if !self.cur_data.is_empty() { self.cur_data.push('\n'); }
                self.cur_data.push_str(v.strip_prefix(' ').unwrap_or(v));
            }
        }
        out
    }

    fn dispatch(&mut self) -> Option<AgentEvent> {
        if self.cur_data.is_empty() && self.cur_event.is_none() && self.cur_id.is_none() {
            return None;
        }
        let payload: serde_json::Value =
            serde_json::from_str(&self.cur_data).unwrap_or(serde_json::Value::Null);
        let kind = self.cur_event.take().unwrap_or_else(|| {
            payload.get("kind").and_then(|k| k.as_str()).unwrap_or("event").to_string()
        });
        let seq = self.cur_id.take().unwrap_or(self.last_id);
        if seq > self.last_id { self.last_id = seq; }
        self.cur_data.clear();
        Some(AgentEvent { seq, kind, payload })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_event() {
        let mut p = SseParser::new();
        let evs = p.push(b"id: 7\nevent: delta\ndata: {\"kind\":\"delta\",\"text\":\"hi\"}\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].seq, 7);
        assert_eq!(evs[0].kind, "delta");
        assert_eq!(evs[0].text(), Some("hi"));
        assert_eq!(p.last_id(), 7);
    }

    #[test]
    fn handles_chunk_split_mid_line() {
        let mut p = SseParser::new();
        assert!(p.push(b"id: 1\nevent: del").is_empty());
        let evs = p.push(b"ta\ndata: {\"kind\":\"delta\",\"text\":\"x\"}\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].kind, "delta");
    }

    #[test]
    fn ignores_comments_and_keepalives() {
        let mut p = SseParser::new();
        assert!(p.push(b": keep-alive\n\n").is_empty());
    }

    #[test]
    fn kind_falls_back_to_payload_when_event_line_absent() {
        let mut p = SseParser::new();
        let evs = p.push(b"id: 3\ndata: {\"kind\":\"final\",\"text\":\"done\"}\n\n");
        assert_eq!(evs[0].kind, "final");
        assert_eq!(evs[0].seq, 3);
    }
}

//! SSE transport for the Interactions API: block splitting, event
//! folding into `RoundOutcome`, and the streaming request round.

use serde_json::json;

use super::{RoundOutcome, StreamEvent, StreamSink, INTERACTIONS_URL};

/// Drain complete SSE blocks (separated by a blank line) from `buf`,
/// returning `(event, data)` pairs. Incomplete trailing data stays
/// in the buffer for the next network chunk.
fn split_sse_events(buf: &mut String) -> Vec<(String, String)> {
    let mut events = Vec::new();
    while let Some(pos) = buf.find("\n\n") {
        let block: String = buf.drain(..pos + 2).collect();
        let mut event = String::new();
        let mut data = String::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        if !event.is_empty() || !data.is_empty() {
            events.push((event, data));
        }
    }
    events
}

/// Fold one parsed SSE event into the round outcome, forwarding text
/// deltas to the sink as they arrive.
async fn apply_sse_event(
    event: &str,
    data: &str,
    out: &mut RoundOutcome,
    sink: &Option<StreamSink>,
) {
    match event {
        "interaction.created" | "interaction.completed" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(id) = v["interaction"]["id"].as_str() {
                    out.id = Some(id.to_string());
                }
                if let Some(status) = v["interaction"]["status"].as_str() {
                    out.status = status.to_string();
                }
            }
        }
        "step.start" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                let step = &v["step"];
                if step["type"] == "function_call" {
                    out.calls.push((
                        step["id"].as_str().unwrap_or_default().to_string(),
                        step["name"].as_str().unwrap_or_default().to_string(),
                        step.get("arguments").cloned().unwrap_or(json!({})),
                    ));
                }
            }
        }
        "step.delta" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if v["delta"]["type"] == "text" {
                    if let Some(t) = v["delta"]["text"].as_str() {
                        out.text.push_str(t);
                        if let Some(s) = sink {
                            s.send(StreamEvent::Delta(t.to_string())).await;
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// One request round over SSE. Hard API errors (non-2xx) propagate;
/// a stream that ends without a final status is an error the caller
/// retries via the blocking path.
pub(super) async fn stream_round(
    client: &reqwest::Client,
    api_key: &str,
    req_body: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<RoundOutcome, Box<dyn std::error::Error + Send + Sync>> {
    use futures_util::StreamExt;

    let mut body = req_body.clone();
    body["stream"] = json!(true);
    let res = client
        .post(INTERACTIONS_URL)
        .header("x-goog-api-key", api_key)
        .json(&body)
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        let error_text = res.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<serde_json::Value>(&error_text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(String::from))
            .unwrap_or(error_text);
        return Err(format!("API error ({status}): {detail}").into());
    }

    let mut out = RoundOutcome::default();
    let mut buf = String::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        for (event, data) in split_sse_events(&mut buf) {
            apply_sse_event(&event, &data, &mut out, sink).await;
        }
    }
    if out.status.is_empty() {
        return Err("SSE stream ended without a final interaction status".into());
    }
    Ok(out)
}

#[cfg(test)]
mod sse_tests {
    use super::split_sse_events;

    #[test]
    fn drains_complete_blocks_and_keeps_partials() {
        let mut buf = String::from(
            "event: step.delta\ndata: {\"a\":1}\n\nevent: done\ndata: [DONE]\n\nevent: partial\nda",
        );
        let events = split_sse_events(&mut buf);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], ("step.delta".into(), "{\"a\":1}".into()));
        assert_eq!(events[1], ("done".into(), "[DONE]".into()));
        assert_eq!(buf, "event: partial\nda");
    }

    #[test]
    fn partial_then_completion_across_chunks() {
        let mut buf = String::from("event: x\ndata: {\"t\":");
        assert!(split_sse_events(&mut buf).is_empty());
        buf.push_str("\"hi\"}\n\n");
        let events = split_sse_events(&mut buf);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, "{\"t\":\"hi\"}");
        assert!(buf.is_empty());
    }

    #[test]
    fn multiline_data_joined() {
        let mut buf = String::from("event: e\ndata: line1\ndata: line2\n\n");
        let events = split_sse_events(&mut buf);
        assert_eq!(events[0].1, "line1\nline2");
    }
}

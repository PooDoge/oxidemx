//! SSE subscription: GET /v1/conversations/{id}/events
#![forbid(unsafe_code)]

use std::convert::Infallible;

use axum::extract::{Path as AxPath, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};

use crate::connector::event_hub::SeqEvent;
use super::AppState;

#[cfg(test)]
use crate::connector::event_hub::EventHub;

/// (seq, kind) frames buffered for `conversation` with seq > after. Test seam.
#[cfg(test)]
pub(crate) fn replay_frames(hub: &EventHub, conversation: &str, after: u64) -> Vec<(u64, String)> {
    hub.replay(conversation, after).into_iter()
        .map(|se| (se.seq, kind_of(&se)))
        .collect()
}

fn kind_of(se: &SeqEvent) -> String {
    se.ev.payload.get("kind").and_then(|k| k.as_str()).unwrap_or("event").to_string()
}

fn to_event(se: &SeqEvent) -> Event {
    Event::default()
        .id(se.seq.to_string())
        .event(kind_of(se))
        .data(se.ev.payload.to_string())
}

pub async fn events_handler(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let last: u64 = headers.get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let rx = st.hub.subscribe();
    let replay = st.hub.replay(&id, last);
    let conv = id.clone();

    let replay_stream = stream::iter(replay.into_iter().map(|se| Ok(to_event(&se))));
    let live_stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(move |res| {
            let conv = conv.clone();
            async move {
                match res {
                    Ok(se) if se.conversation == conv => Some(Ok(to_event(&se))),
                    _ => None, // lagged or other conversation
                }
            }
        });

    Sse::new(replay_stream.chain(live_stream)).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::event_hub::EventHub;
    use crate::seams::AgentEvent;

    fn ev(c: &str, kind: &str) -> AgentEvent {
        AgentEvent { project: "/p".into(), thread_or_run: c.into(), ts: 0,
                     payload: serde_json::json!({"kind": kind}) }
    }

    #[test]
    fn replay_after_last_event_id_filters_by_conversation_and_seq() {
        let hub = EventHub::new(16);
        hub.publish(&ev("c1", "a")); // 1
        hub.publish(&ev("c2", "b")); // 2
        hub.publish(&ev("c1", "c")); // 3
        let got = replay_frames(&hub, "c1", 1); // after seq 1
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 3);
        assert_eq!(got[0].1, "c"); // kind
    }
}

//! In-process event fan-out: tees the AgentEvent stream to D-Bus (unchanged) and SSE subscribers.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::seams::{AgentEvent, EventEmitter};

/// An AgentEvent tagged with a monotonic sequence and its owning conversation.
#[derive(Clone, Debug)]
pub struct SeqEvent {
    pub seq: u64,
    pub conversation: String,
    pub ev: AgentEvent,
}

struct Inner {
    seq: AtomicU64,
    tx: tokio::sync::broadcast::Sender<SeqEvent>,
    buffer_per_conv: usize,
    /// conversation_id -> bounded VecDeque of recent SeqEvents.
    rings: Mutex<HashMap<String, std::collections::VecDeque<SeqEvent>>>,
    /// run_id -> conversation_id, so run/flow events route correctly.
    run_links: Mutex<HashMap<String, String>>,
}

/// Cloneable fan-out hub. Cloning shares one underlying broadcast + buffers.
#[derive(Clone)]
pub struct EventHub(Arc<Inner>);

impl EventHub {
    pub fn new(buffer_per_conv: usize) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(1024);
        EventHub(Arc::new(Inner {
            seq: AtomicU64::new(0),
            tx,
            buffer_per_conv: buffer_per_conv.max(1),
            rings: Mutex::new(HashMap::new()),
            run_links: Mutex::new(HashMap::new()),
        }))
    }

    pub fn link_run(&self, run_id: &str, conversation_id: &str) {
        self.0.run_links.lock().unwrap_or_else(|e| e.into_inner())
            .insert(run_id.to_string(), conversation_id.to_string());
    }

    pub fn owning_conversation(&self, ev: &AgentEvent) -> String {
        let links = self.0.run_links.lock().unwrap_or_else(|e| e.into_inner());
        links.get(&ev.thread_or_run).cloned().unwrap_or_else(|| ev.thread_or_run.clone())
    }

    pub fn publish(&self, ev: &AgentEvent) {
        let seq = self.0.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let conversation = self.owning_conversation(ev);
        let item = SeqEvent { seq, conversation: conversation.clone(), ev: ev.clone() };
        {
            let mut rings = self.0.rings.lock().unwrap_or_else(|e| e.into_inner());
            let ring = rings.entry(conversation).or_default();
            ring.push_back(item.clone());
            while ring.len() > self.0.buffer_per_conv {
                ring.pop_front();
            }
        }
        let _ = self.0.tx.send(item); // Err only if no subscribers; fine.
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SeqEvent> {
        self.0.tx.subscribe()
    }

    pub fn replay(&self, conversation_id: &str, after_seq: u64) -> Vec<SeqEvent> {
        let rings = self.0.rings.lock().unwrap_or_else(|e| e.into_inner());
        rings.get(conversation_id)
            .map(|ring| ring.iter().filter(|e| e.seq > after_seq).cloned().collect())
            .unwrap_or_default()
    }
}

/// EventEmitter that tees every event to the in-process hub AND the inner emitter
/// (the production D-Bus `BusEmitter`, unchanged).
pub struct BroadcastEmitter {
    inner: Arc<dyn EventEmitter>,
    hub: EventHub,
}

impl BroadcastEmitter {
    pub fn new(inner: Arc<dyn EventEmitter>, hub: EventHub) -> Self {
        Self { inner, hub }
    }
}

impl EventEmitter for BroadcastEmitter {
    fn emit(&self, ev: AgentEvent) {
        self.hub.publish(&ev);
        self.inner.emit(ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seams::RecordingEmitter;

    fn ev(thread_or_run: &str, kind: &str) -> AgentEvent {
        AgentEvent {
            project: "/p".into(),
            thread_or_run: thread_or_run.into(),
            ts: 0,
            payload: serde_json::json!({ "kind": kind }),
        }
    }

    #[tokio::test]
    async fn subscriber_receives_published_event() {
        let hub = EventHub::new(16);
        let mut rx = hub.subscribe();
        hub.publish(&ev("conv-1", "delta"));
        let got = rx.recv().await.unwrap();
        assert_eq!(got.conversation, "conv-1");
        assert_eq!(got.seq, 1);
    }

    #[test]
    fn replay_returns_events_after_seq_for_that_conversation_only() {
        let hub = EventHub::new(16);
        hub.publish(&ev("conv-1", "a")); // seq 1
        hub.publish(&ev("conv-2", "b")); // seq 2
        hub.publish(&ev("conv-1", "c")); // seq 3
        let replayed = hub.replay("conv-1", 0);
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].seq, 1);
        assert_eq!(replayed[1].seq, 3);
        // after_seq filters:
        assert_eq!(hub.replay("conv-1", 1).len(), 1);
    }

    #[test]
    fn ring_buffer_evicts_past_capacity() {
        let hub = EventHub::new(2);
        for _ in 0..5 { hub.publish(&ev("conv-1", "x")); }
        assert_eq!(hub.replay("conv-1", 0).len(), 2);
    }

    #[test]
    fn run_events_route_to_linked_conversation() {
        let hub = EventHub::new(16);
        hub.link_run("run-9", "conv-7");
        hub.publish(&ev("run-9", "RunFinished")); // thread_or_run is the run id
        let replayed = hub.replay("conv-7", 0);
        assert_eq!(replayed.len(), 1);
        // and NOT under the raw run id:
        assert_eq!(hub.replay("run-9", 0).len(), 0);
    }

    #[test]
    fn broadcast_emitter_tees_to_inner_and_hub() {
        let rec = Arc::new(RecordingEmitter::default());
        let hub = EventHub::new(16);
        let emitter = BroadcastEmitter::new(rec.clone(), hub.clone());
        emitter.emit(ev("conv-1", "delta"));
        assert_eq!(rec.events().len(), 1);             // inner saw it
        assert_eq!(hub.replay("conv-1", 0).len(), 1);  // hub saw it
    }
}

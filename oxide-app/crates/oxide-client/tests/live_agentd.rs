//! Live integration against the running local agentd. Run with:
//!   cargo test -p oxide-client --test live_agentd -- --ignored --nocapture
use std::sync::Arc;
use futures_util::StreamExt;
use oxide_client::{Transport, UdsTransport};

#[tokio::test]
#[ignore = "requires the local agentd running with http.enabled"]
async fn health_projects_create_send_subscribe() {
    let t: Arc<dyn Transport> = Arc::new(UdsTransport::new(UdsTransport::default_socket()));

    t.health().await.expect("agentd should be reachable on the UDS");

    let projects = t.list_projects().await.expect("list_projects");
    assert!(projects.iter().any(|p| p.id.as_str() == "personal"), "expected a 'personal' project");

    let conv = t.create_conversation("personal", None).await.expect("create_conversation");

    // Subscribe BEFORE sending so we catch the streamed reply.
    let mut events = t.subscribe(conv.id.as_str());
    let _mid = t.send_message(conv.id.as_str(), "Reply with the single word: pong").await.expect("send_message");

    // Await a terminal `final` event within a timeout.
    let got_final = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while let Some(ev) = events.next().await {
            let ev = ev.expect("stream event");
            println!("[event] kind={} seq={} payload={}", ev.kind, ev.seq, ev.payload);
            if ev.kind == "final" { return true; }
            if ev.kind == "error" { panic!("agentd error: {:?}", ev.payload); }
        }
        false
    }).await.expect("timed out waiting for final");
    assert!(got_final, "expected a final event from the turn");
}

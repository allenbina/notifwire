//! End-to-end loopback smoke test for the D0 walking skeleton:
//! `notifwire-send` injects → the node serves it over SSE → the consumer
//! receives and formats it. Plus a cursor-resume test proving catch-up.
//!
//! Note: the outbox is in-memory, so a *true* process restart loses the
//! buffer. The cursor-resume test exercises the catch-up mechanism (a consumer
//! reconnecting with its last cursor); durable cross-restart catch-up waits on
//! a persistent outbox in a later epic.

use notifwire_cli::{build_notification, send, SendOpts};
use notifwire_consumer::format_line;
use notifwire_core::{Notification, Priority};
use notifwire_transport::{Cursor, EventStream, MeshConsumer, SseClient, SseServer};
use std::time::Duration;
use tokio::time::timeout;

use futures::StreamExt;

async fn next_event(stream: &mut EventStream) -> (Cursor, Notification) {
    let item = timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("timed out waiting for event")
        .expect("stream ended early")
        .expect("transport error");
    (item.seq, item.notification)
}

fn opts(title: &str) -> SendOpts {
    SendOpts {
        title: title.into(),
        body: "world".into(),
        app: "demo".into(),
        priority: Some(Priority::High),
        icon: None,
        producer_node: "test".into(),
    }
}

/// Inject via notifwire-send's real (blocking) send path, off the async runtime.
async fn inject(base: &str, title: &str) -> Cursor {
    let url = base.to_string();
    let n = build_notification(&opts(title));
    tokio::task::spawn_blocking(move || send(&url, &n))
        .await
        .expect("join")
        .expect("send")
}

#[tokio::test]
async fn notifwire_send_reaches_the_consumer() {
    let server = SseServer::new(100);
    let (addr, serve) = server.bind("127.0.0.1:0").await.unwrap();
    tokio::spawn(serve);
    let base = format!("http://{addr}");

    // notifwire-send "hello" --app demo --priority high
    let seq = inject(&base, "hello").await;
    assert_eq!(seq, 1);

    // The consumer subscribes and receives exactly what was sent.
    let client = SseClient::new(base);
    let mut stream = client.subscribe(0).await.unwrap();
    let (got_seq, note) = next_event(&mut stream).await;
    assert_eq!(got_seq, 1);
    assert_eq!(note.title, "hello");
    assert_eq!(note.app_name, "demo");

    // And the stub consumer renders it.
    let line = format_line(got_seq, &note);
    assert_eq!(line, "[1] demo: hello — world (high)");
}

#[tokio::test]
async fn consumer_resumes_from_its_cursor() {
    let server = SseServer::new(100);
    let (addr, serve) = server.bind("127.0.0.1:0").await.unwrap();
    tokio::spawn(serve);
    let base = format!("http://{addr}");

    // Two notifications land while the consumer is connected.
    inject(&base, "one").await;
    inject(&base, "two").await;

    let client = SseClient::new(base.clone());
    let mut stream = client.subscribe(0).await.unwrap();
    assert_eq!(next_event(&mut stream).await.0, 1);
    assert_eq!(next_event(&mut stream).await.0, 2);

    // Consumer goes away, a third lands, then it reconnects from cursor 2.
    drop(stream);
    inject(&base, "three").await;

    let mut resumed = client.subscribe(2).await.unwrap();
    let (seq, note) = next_event(&mut resumed).await;
    assert_eq!(seq, 3, "should resume after the cursor, not replay 1 and 2");
    assert_eq!(note.title, "three");
}

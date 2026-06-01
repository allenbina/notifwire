//! Server-Sent Events implementation of the mesh transport.
//!
//! The producer serves `GET /events?since=<cursor>` (or honours a
//! `Last-Event-ID` header). It first replays everything the outbox still holds
//! after `since`, then streams new notifications live. Each SSE frame's `id:`
//! is the notification's [`Cursor`] and its `data:` is the notification JSON —
//! so a reconnecting consumer just sends its last cursor back and resumes
//! exactly where it left off.
//!
//! To avoid a gap or duplicate at the replay/live boundary, the handler
//! subscribes to the live broadcast *before* it snapshots the outbox, then
//! forwards only live events whose sequence exceeds the last replayed one.

use crate::{Cursor, MeshConsumer, MeshProducer, Outbox, Sequenced, TransportError};
use axum::{
    extract::Json,
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use futures::{Stream, StreamExt};
use notifwire_core::Notification;
use serde::Deserialize;
use std::convert::Infallible;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

const BROADCAST_CAPACITY: usize = 1024;

#[derive(Clone, Debug)]
struct ServerState {
    outbox: Arc<Mutex<Outbox>>,
    tx: broadcast::Sender<Sequenced>,
    /// When set, the outbox is snapshotted here after every publish so it
    /// survives a restart (D1-7).
    persist_path: Option<PathBuf>,
}

impl ServerState {
    /// Assign the next cursor, buffer the notification for catch-up, and fan it
    /// out to any live subscribers. The single source of truth for publishing,
    /// shared by the producer handle and the ingest endpoint.
    fn publish(&self, notification: Notification) -> Cursor {
        let seq = {
            let mut ob = self.outbox.lock().expect("outbox mutex poisoned");
            let seq = ob.append(notification.clone());
            // Snapshot under the lock so what's on disk matches in-memory.
            // Best-effort: a persistence failure (e.g. full disk) must not drop
            // the live notification, so we warn and continue.
            if let Some(path) = &self.persist_path {
                if let Err(e) = crate::save_outbox(&ob, path) {
                    eprintln!(
                        "notifwire: failed to persist outbox to {}: {e}",
                        path.display()
                    );
                }
            }
            seq
        };
        // No live subscribers is fine — the outbox still has it for catch-up.
        let _ = self.tx.send(Sequenced { seq, notification });
        seq
    }
}

/// An SSE producer: owns the outbox and serves the event stream.
#[derive(Debug)]
pub struct SseServer {
    state: ServerState,
}

/// A cheap, cloneable handle for publishing into a running [`SseServer`].
#[derive(Clone, Debug)]
pub struct SseProducer {
    state: ServerState,
}

impl SseServer {
    /// Create a server whose outbox retains at most `capacity` recent events,
    /// held in memory only (lost on restart).
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            state: ServerState {
                outbox: Arc::new(Mutex::new(Outbox::new(capacity))),
                tx,
                persist_path: None,
            },
        }
    }

    /// Like [`new`](Self::new), but durable: the outbox is loaded from `path`
    /// at startup and snapshotted back after every publish, so buffered
    /// notifications and the monotonic cursor survive a restart (D1-7).
    pub fn with_persistence(capacity: usize, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let outbox = crate::load_outbox(&path, capacity);
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            state: ServerState {
                outbox: Arc::new(Mutex::new(outbox)),
                tx,
                persist_path: Some(path),
            },
        }
    }

    /// A handle for injecting notifications (implements [`MeshProducer`]).
    pub fn producer(&self) -> SseProducer {
        SseProducer {
            state: self.state.clone(),
        }
    }

    /// The axum router: `GET /events` (subscribe) and `POST /ingest` (local
    /// injection). For D0 both share one listener; `/ingest` is meant to be
    /// bound to localhost only — splitting it onto its own loopback listener
    /// is a follow-up once the producer node proper lands (D1).
    pub fn router(&self) -> Router {
        Router::new()
            .route("/events", get(events_handler))
            .route("/ingest", post(ingest_handler))
            .with_state(self.state.clone())
    }

    /// Bind a TCP listener and return its address plus a future that serves
    /// until the process ends. Use `127.0.0.1:0` to get an ephemeral port.
    pub async fn bind(
        &self,
        addr: &str,
    ) -> std::io::Result<(std::net::SocketAddr, impl std::future::Future<Output = ()>)> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        let router = self.router();
        let serve = async move {
            let _ = axum::serve(listener, router).await;
        };
        Ok((local, serve))
    }
}

impl MeshProducer for SseProducer {
    fn publish(&self, notification: Notification) -> Cursor {
        self.state.publish(notification)
    }
}

#[derive(Deserialize)]
struct EventsQuery {
    since: Option<Cursor>,
    /// When true, skip the backlog and stream only notifications from now on.
    live: Option<bool>,
}

fn to_event(seq: Cursor, notification: &Notification) -> Event {
    let data = serde_json::to_string(notification).unwrap_or_default();
    Event::default().id(seq.to_string()).data(data)
}

type EventResult = Result<Event, Infallible>;

async fn events_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> impl IntoResponse {
    // Subscribe BEFORE snapshotting so nothing slips through the boundary.
    let mut rx = state.tx.subscribe();
    let (replay, mut last_sent) = {
        let ob = state.outbox.lock().expect("outbox mutex poisoned");
        if query.live.unwrap_or(false) {
            // Live: skip the backlog, stream only notifications from now on.
            (Vec::new(), ob.latest())
        } else {
            // Last-Event-ID (set by a reconnecting EventSource) wins over ?since=.
            let since = headers
                .get("last-event-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<Cursor>().ok())
                .or(query.since)
                .unwrap_or(0);
            let catch_up = ob.since(since);
            let last = catch_up.events.last().map(|s| s.seq).unwrap_or(since);
            (catch_up.events, last)
        }
    };

    let stream = async_stream::stream! {
        for ev in replay {
            yield Ok(to_event(ev.seq, &ev.notification));
        }
        loop {
            match rx.recv().await {
                Ok(ev) if ev.seq > last_sent => {
                    last_sent = ev.seq;
                    yield Ok(to_event(ev.seq, &ev.notification));
                }
                Ok(_) => {}                                   // already replayed
                Err(broadcast::error::RecvError::Lagged(_)) => {} // slow client; reconnect catches up
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    let boxed: Pin<Box<dyn Stream<Item = EventResult> + Send>> = Box::pin(stream);
    Sse::new(boxed).keep_alive(KeepAlive::default())
}

/// `POST /ingest` — the localhost injection point. Input plugins and
/// `notifwire-send` POST a normalized notification here; the node assigns it a
/// cursor and returns it. This is the local half of the producer: capture and
/// input plugins funnel through the same door.
async fn ingest_handler(
    State(state): State<ServerState>,
    Json(notification): Json<Notification>,
) -> impl IntoResponse {
    let seq = state.publish(notification);
    Json(serde_json::json!({ "seq": seq }))
}

/// An SSE consumer that dials a producer's `/events` endpoint.
#[derive(Clone, Debug)]
pub struct SseClient {
    base_url: String,
}

impl SseClient {
    /// `base_url` is the producer root, e.g. `http://mac.local:8787`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    /// Subscribe to only *new* notifications, skipping the producer's backlog.
    pub async fn subscribe_live(&self) -> Result<crate::EventStream, TransportError> {
        self.open("live=true").await
    }

    /// Open the `/events` stream with a raw query string (e.g. `since=4`).
    async fn open(&self, query: &str) -> Result<crate::EventStream, TransportError> {
        let url = format!("{}/events?{query}", self.base_url.trim_end_matches('/'));
        let resp = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .map_err(|e| TransportError::Connect(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(TransportError::Http(format!("status {}", resp.status())));
        }

        let stream = async_stream::stream! {
            let byte_stream = resp.bytes_stream();
            futures::pin_mut!(byte_stream);
            let mut buf = String::new();
            let mut cur_id: Option<Cursor> = None;
            let mut data = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        yield Err(TransportError::Http(e.to_string()));
                        break;
                    }
                };
                buf.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(nl) = buf.find('\n') {
                    let line = buf[..nl].trim_end_matches('\r').to_string();
                    buf.drain(..=nl);

                    if line.is_empty() {
                        // Blank line dispatches the accumulated event.
                        if !data.is_empty() {
                            match serde_json::from_str::<Notification>(&data) {
                                Ok(n) => yield Ok(Sequenced {
                                    seq: cur_id.unwrap_or(0),
                                    notification: n,
                                }),
                                Err(e) => yield Err(TransportError::Decode(e.to_string())),
                            }
                        }
                        cur_id = None;
                        data.clear();
                    } else if let Some(v) = line.strip_prefix("id:") {
                        cur_id = v.trim().parse().ok();
                    } else if let Some(v) = line.strip_prefix("data:") {
                        if !data.is_empty() {
                            data.push('\n');
                        }
                        data.push_str(v.strip_prefix(' ').unwrap_or(v));
                    }
                    // Other fields (event:, retry:) and `:` keep-alive comments are ignored.
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

impl MeshConsumer for SseClient {
    async fn subscribe(&self, since: Cursor) -> Result<crate::EventStream, TransportError> {
        self.open(&format!("since={since}")).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notifwire_core::SourcePlatform;
    use std::time::Duration;
    use tokio::time::timeout;

    fn note(title: &str) -> Notification {
        Notification::new(
            "id",
            "node",
            SourcePlatform::Plugin,
            "app",
            title,
            "body",
            "2026-05-31T00:00:00Z",
        )
    }

    async fn next_seq(stream: &mut crate::EventStream) -> Cursor {
        let item = timeout(Duration::from_secs(3), stream.next())
            .await
            .expect("timed out waiting for event")
            .expect("stream ended early")
            .expect("transport error");
        item.seq
    }

    #[tokio::test]
    async fn serve_catch_up_live_and_reconnect() {
        let server = SseServer::new(100);
        let producer = server.producer();
        let (addr, serve) = server.bind("127.0.0.1:0").await.unwrap();
        tokio::spawn(serve);

        // Publish three before any consumer connects → pure catch-up path.
        for t in ["a", "b", "c"] {
            producer.publish(note(t));
        }

        let client = SseClient::new(format!("http://{addr}"));

        // Pull from the beginning: replays 1,2,3.
        let mut stream = client.subscribe(0).await.unwrap();
        assert_eq!(next_seq(&mut stream).await, 1);
        assert_eq!(next_seq(&mut stream).await, 2);
        assert_eq!(next_seq(&mut stream).await, 3);

        // A live publish arrives on the open stream as seq 4.
        producer.publish(note("d"));
        assert_eq!(next_seq(&mut stream).await, 4);

        // Drop and reconnect from cursor 2 → replays only 3,4 (catch-up).
        drop(stream);
        let mut reconnected = client.subscribe(2).await.unwrap();
        assert_eq!(next_seq(&mut reconnected).await, 3);
        assert_eq!(next_seq(&mut reconnected).await, 4);
    }

    #[tokio::test]
    async fn live_skips_backlog() {
        let server = SseServer::new(100);
        let producer = server.producer();
        let (addr, serve) = server.bind("127.0.0.1:0").await.unwrap();
        tokio::spawn(serve);

        // Backlog before the live consumer connects.
        producer.publish(note("old1"));
        producer.publish(note("old2"));

        let client = SseClient::new(format!("http://{addr}"));
        let mut stream = client.subscribe_live().await.unwrap();

        // Let the handler register, then publish a fresh one.
        tokio::time::sleep(Duration::from_millis(100)).await;
        producer.publish(note("new"));

        // Live yields only the new event (seq 3), never the backlog (1, 2).
        assert_eq!(next_seq(&mut stream).await, 3);
    }

    #[tokio::test]
    async fn ingest_endpoint_publishes_and_is_received() {
        let server = SseServer::new(100);
        let (addr, serve) = server.bind("127.0.0.1:0").await.unwrap();
        tokio::spawn(serve);
        let base = format!("http://{addr}");

        // POST a notification to /ingest exactly as notifwire-send does.
        let resp = reqwest::Client::new()
            .post(format!("{base}/ingest"))
            .header("content-type", "application/json")
            .body(serde_json::to_string(&note("ingested")).unwrap())
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let body: serde_json::Value = serde_json::from_str(&resp.text().await.unwrap()).unwrap();
        assert_eq!(body["seq"], 1);

        // A consumer subscribing from 0 receives the ingested notification.
        let client = SseClient::new(base);
        let mut stream = client.subscribe(0).await.unwrap();
        assert_eq!(next_seq(&mut stream).await, 1);
    }

    #[tokio::test]
    async fn subscribing_current_then_receiving_live() {
        let server = SseServer::new(100);
        let producer = server.producer();
        let (addr, serve) = server.bind("127.0.0.1:0").await.unwrap();
        tokio::spawn(serve);

        let client = SseClient::new(format!("http://{addr}"));
        // Nothing buffered yet; subscribe live from 0.
        let mut stream = client.subscribe(0).await.unwrap();

        // Give the handler a moment to register its broadcast subscription,
        // then publish — it should arrive as the first (seq 1) live event.
        tokio::time::sleep(Duration::from_millis(100)).await;
        producer.publish(note("hello"));
        assert_eq!(next_seq(&mut stream).await, 1);
    }
}

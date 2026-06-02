//! notifwire consumer.
//!
//! Subscribes to a producer over the mesh transport and runs each received
//! notification through the consumer pipeline: record to history, apply the
//! rules filter, dedup, and show it via a [`NotificationSink`] (a native toast
//! on Windows, or a printer when headless).

mod history;
pub use history::History;

use anyhow::Result;
use futures::StreamExt;
use notifwire_core::{
    ConnectionState, ConsumerHealth, Deduper, Notification, NotificationSink, Priority,
    ProducerStatus, Rules, SelfChecks,
};
use notifwire_transport::{Cursor, MeshConsumer, SseClient};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Render a notification as `app: title — body (priority)`, without a cursor.
pub fn format_notification(n: &Notification) -> String {
    let mut line = format!("{}: {}", n.app_name, n.title);
    if !n.body.is_empty() {
        line.push_str(" — ");
        line.push_str(&n.body);
    }
    match n.priority() {
        Priority::High => line.push_str(" (high)"),
        Priority::Urgent => line.push_str(" (urgent)"),
        _ => {}
    }
    line
}

/// Render a received notification as a single line, prefixed with its cursor.
pub fn format_line(seq: Cursor, n: &Notification) -> String {
    format!("[{seq}] {}", format_notification(n))
}

/// The consumer pipeline: history → rules filter → dedup → display.
pub struct Pipeline {
    rules: Rules,
    deduper: Deduper,
    history: Option<History>,
    sink: Box<dyn NotificationSink>,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("rules", &self.rules)
            .field("has_history", &self.history.is_some())
            .finish_non_exhaustive()
    }
}

impl Pipeline {
    pub fn new(
        rules: Rules,
        dedup_window_ms: i64,
        history: Option<History>,
        sink: Box<dyn NotificationSink>,
    ) -> Self {
        Self {
            rules,
            deduper: Deduper::new(dedup_window_ms),
            history,
            sink,
        }
    }

    /// Process one received notification: record it to history (best-effort),
    /// then show it unless the rules suppress it or it's a recent duplicate.
    /// Returns whether it was shown.
    pub fn handle(&mut self, n: &Notification, now_ms: i64) -> Result<bool> {
        if let Some(h) = &self.history {
            if let Err(e) = h.record(n) {
                tracing::warn!(error = %e, id = %n.id, "history record failed");
            }
        }
        if !self.rules.allows(n) || self.deduper.is_duplicate(n, now_ms) {
            return Ok(false);
        }
        self.sink
            .show(n)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(true)
    }

    /// Access to the history store (e.g. to query recent notifications).
    pub fn history(&self) -> Option<&History> {
        self.history.as_ref()
    }

    /// Run the consumer's self-checks: is history readable, is the pipeline live.
    /// `pipeline_alive` is asserted by the caller (the run loop is what's alive).
    pub fn self_checks(&self, pipeline_alive: bool) -> SelfChecks {
        let (history_ok, detail) = match &self.history {
            Some(h) => match h.count() {
                Ok(_) => (true, None),
                Err(e) => (false, Some(format!("history unreadable: {e}"))),
            },
            None => (true, None), // no history configured → nothing to be wrong
        };
        SelfChecks {
            history_ok,
            pipeline_alive,
            detail,
        }
    }
}

/// Backoff/retry policy for the consumer's auto-reconnect loop.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    /// Delay before the first retry; doubles each attempt up to `max_backoff`.
    pub initial_backoff: Duration,
    /// Cap on the backoff delay.
    pub max_backoff: Duration,
    /// `None` retries forever (the runtime default); `Some(n)` bounds the number
    /// of reconnect attempts (used by tests to make the loop terminate).
    pub max_retries: Option<usize>,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            max_retries: None,
        }
    }
}

/// A cloneable handle to the live status of one producer connection. The run
/// loop writes it; the UI (and `consumer_health`) read it.
#[derive(Clone, Debug)]
pub struct StatusHandle(Arc<Mutex<ProducerStatus>>);

impl StatusHandle {
    /// A fresh handle for `url`, starting in [`Connecting`](ConnectionState::Connecting).
    pub fn new(url: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(ProducerStatus::new(url))))
    }

    /// A snapshot of the current status.
    pub fn get(&self) -> ProducerStatus {
        self.0.lock().expect("status mutex poisoned").clone()
    }

    fn update(&self, f: impl FnOnce(&mut ProducerStatus)) {
        f(&mut self.0.lock().expect("status mutex poisoned"));
    }
}

/// Build the consumer's composite health from its self-checks and the live
/// status of every producer it subscribes to.
pub fn consumer_health(self_checks: SelfChecks, producers: &[StatusHandle]) -> ConsumerHealth {
    let statuses = producers.iter().map(StatusHandle::get).collect();
    ConsumerHealth::rollup(self_checks, statuses)
}

/// Subscribe to `producer_url` and feed received notifications through
/// `pipeline`, **auto-reconnecting with backoff** if the stream drops — so a
/// transient producer restart or network blip doesn't silently kill the
/// consumer. Uses the default [`ReconnectPolicy`] (retries forever); for finer
/// control or to observe connection status, use [`run_with_reconnect`].
pub async fn run_with_pipeline(
    producer_url: &str,
    since: Cursor,
    live: bool,
    mut pipeline: Pipeline,
) -> Result<()> {
    let status = StatusHandle::new(producer_url);
    run_with_reconnect(
        producer_url,
        since,
        live,
        &mut pipeline,
        &ReconnectPolicy::default(),
        &status,
    )
    .await
}

/// The reconnecting consumer loop. Connects, probes `/health`, streams events
/// into `pipeline`, and on any disconnect updates `status` and retries with
/// exponential backoff per `policy`. On reconnect it resumes from the highest
/// cursor it has seen (not the original `since`), so the backlog isn't re-shown.
///
/// Returns `Ok(())` only when a bounded `policy.max_retries` is exhausted; with
/// the default policy it runs until the task is cancelled. Returns `Err` only if
/// the pipeline itself fails (e.g. the display sink errors).
pub async fn run_with_reconnect(
    producer_url: &str,
    since: Cursor,
    live: bool,
    pipeline: &mut Pipeline,
    policy: &ReconnectPolicy,
    status: &StatusHandle,
) -> Result<()> {
    let client = SseClient::new(producer_url);
    let mut cursor = since;
    let mut first = true;
    let mut failures: usize = 0;
    let mut backoff = policy.initial_backoff;

    loop {
        // First connection honors `live` (skip backlog); reconnects always
        // resume from the last cursor so we catch up exactly what we missed.
        let subscribe = if first && live {
            client.subscribe_live().await
        } else {
            client.subscribe(cursor).await
        };
        first = false;

        match subscribe {
            Ok(mut stream) => {
                failures = 0;
                backoff = policy.initial_backoff;
                status.update(|s| {
                    s.state = ConnectionState::Connected;
                    s.last_error = None;
                });
                tracing::info!(producer = producer_url, "connected");

                // Best-effort: enrich status with the producer's self-report.
                if let Ok(health) = client.health().await {
                    status.update(|s| s.health = Some(health));
                }

                while let Some(item) = stream.next().await {
                    match item {
                        Ok(seq_note) => {
                            cursor = seq_note.seq.max(cursor);
                            let now = now_ms();
                            status.update(|s| s.last_event_unix_ms = Some(now));
                            pipeline.handle(&seq_note.notification, now)?;
                        }
                        Err(e) => {
                            status.update(|s| s.last_error = Some(e.to_string()));
                            tracing::warn!(producer = producer_url, error = %e, "stream error");
                            break;
                        }
                    }
                }
                status.update(|s| s.state = ConnectionState::Reconnecting);
                tracing::info!(producer = producer_url, "stream ended; reconnecting");
            }
            Err(e) => {
                failures += 1;
                // A few consecutive connect failures = treat as unreachable.
                let state = if failures >= 3 {
                    ConnectionState::Unreachable
                } else {
                    ConnectionState::Reconnecting
                };
                status.update(|s| {
                    s.state = state;
                    s.last_error = Some(e.to_string());
                });
                tracing::warn!(producer = producer_url, failures, error = %e, "connect failed");
            }
        }

        if let Some(max) = policy.max_retries {
            if failures >= max {
                status.update(|s| s.state = ConnectionState::Unreachable);
                return Ok(());
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(policy.max_backoff);
    }
}

/// Current wall-clock time in unix milliseconds (the consumer's clock; the
/// dedup engine in `core` stays clock-free and takes this as input).
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notifwire_core::{DisplayError, Priority, SourcePlatform};
    use std::sync::{Arc, Mutex};

    fn note(app: &str, title: &str, body: &str) -> Notification {
        Notification::new(
            "id",
            "node",
            SourcePlatform::Plugin,
            app,
            title,
            body,
            "2026-05-31T00:00:00Z",
        )
    }

    #[test]
    fn formats_app_title_and_body() {
        let line = format_line(4, &note("rsync", "Backup complete", "42GB synced"));
        assert_eq!(line, "[4] rsync: Backup complete — 42GB synced");
    }

    #[test]
    fn omits_dash_when_body_empty() {
        assert_eq!(
            format_line(1, &note("weechat", "ping", "")),
            "[1] weechat: ping"
        );
    }

    #[test]
    fn tags_elevated_priority() {
        let mut n = note("alerts", "disk full", "");
        n.priority = Some(Priority::Urgent);
        assert!(format_line(2, &n).ends_with("(urgent)"));
    }

    #[test]
    fn pipeline_filters_dedups_and_shows() {
        let shown = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = shown.clone();
        let sink = move |n: &Notification| {
            captured.lock().unwrap().push(n.title.clone());
            Ok::<(), DisplayError>(())
        };

        let mut rules = Rules::default();
        rules.apps.insert("Spam".to_string(), false); // blocked app

        let mut p = Pipeline::new(rules, 60_000, None, Box::new(sink));
        assert!(p.handle(&note("Slack", "ping", ""), 0).unwrap()); // shown
        assert!(!p.handle(&note("Spam", "ad", ""), 0).unwrap()); // filtered
        assert!(p.handle(&note("Slack", "other", ""), 1000).unwrap()); // shown
        assert!(!p.handle(&note("Slack", "ping", ""), 2000).unwrap()); // dup → suppressed

        assert_eq!(*shown.lock().unwrap(), vec!["ping", "other"]);
    }

    #[test]
    fn pipeline_records_all_received_to_history() {
        let history = History::open_in_memory().unwrap();
        let sink = |_: &Notification| Ok::<(), DisplayError>(());
        let mut p = Pipeline::new(Rules::default(), 0, Some(history), Box::new(sink));

        // Distinct ids so history (idempotent on id) counts each.
        for i in 0..3 {
            let mut n = note("App", &format!("t{i}"), "");
            n.id = format!("id-{i}");
            p.handle(&n, 0).unwrap();
        }
        assert_eq!(p.history().unwrap().count().unwrap(), 3);
    }

    #[test]
    fn self_checks_pass_with_working_history() {
        let history = History::open_in_memory().unwrap();
        let sink = |_: &Notification| Ok::<(), DisplayError>(());
        let p = Pipeline::new(Rules::default(), 0, Some(history), Box::new(sink));
        let checks = p.self_checks(true);
        assert!(checks.history_ok && checks.pipeline_alive);
        assert_eq!(checks.status(), notifwire_core::HealthStatus::Ok);
    }

    fn noop_pipeline() -> Pipeline {
        let sink = |_: &Notification| Ok::<(), DisplayError>(());
        Pipeline::new(Rules::default(), 0, None, Box::new(sink))
    }

    #[tokio::test]
    async fn unreachable_producer_gives_up_after_bounded_retries() {
        let policy = ReconnectPolicy {
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
            max_retries: Some(2),
        };
        let status = StatusHandle::new("http://127.0.0.1:1"); // nothing listening
        let mut pipeline = noop_pipeline();

        // Bounded policy means this returns rather than looping forever.
        run_with_reconnect(
            "http://127.0.0.1:1",
            0,
            false,
            &mut pipeline,
            &policy,
            &status,
        )
        .await
        .unwrap();

        let s = status.get();
        assert_eq!(s.state, ConnectionState::Unreachable);
        assert!(s.last_error.is_some());
        assert_eq!(s.status(), notifwire_core::HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn connects_receives_and_reports_status() {
        use notifwire_transport::{MeshProducer, SseServer};

        let server = SseServer::new(100);
        let producer = server.producer();
        let (addr, serve) = server.bind("127.0.0.1:0").await.unwrap();
        tokio::spawn(serve);
        let base = format!("http://{addr}");

        // Two notifications waiting in the backlog.
        producer.publish(note("Slack", "one", ""));
        producer.publish(note("Slack", "two", ""));

        let shown = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = shown.clone();
        let sink = move |n: &Notification| {
            captured.lock().unwrap().push(n.title.clone());
            Ok::<(), DisplayError>(())
        };
        let mut pipeline = Pipeline::new(Rules::default(), 0, None, Box::new(sink));
        let status = StatusHandle::new(&base);
        let policy = ReconnectPolicy::default();

        // The loop blocks on the live stream once caught up, so bound it: by the
        // time it elapses it has connected, probed /health, and replayed both.
        let _ = tokio::time::timeout(
            Duration::from_millis(600),
            run_with_reconnect(&base, 0, false, &mut pipeline, &policy, &status),
        )
        .await;

        assert_eq!(*shown.lock().unwrap(), vec!["one", "two"]);
        let s = status.get();
        assert_eq!(s.state, ConnectionState::Connected);
        assert!(s.last_event_unix_ms.is_some());
        assert!(s.health.is_some(), "should have probed /health on connect");
        assert_eq!(s.health.unwrap().latest_cursor, 2);
    }
}

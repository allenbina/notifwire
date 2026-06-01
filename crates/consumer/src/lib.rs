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
use notifwire_core::{Deduper, Notification, NotificationSink, Priority, Rules};
use notifwire_transport::{Cursor, MeshConsumer, SseClient};

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
                eprintln!("notifwire: history record failed: {e}");
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
}

/// Subscribe to `producer_url` from `since` and feed every received
/// notification through `pipeline`. Runs until the stream ends or errors.
pub async fn run_with_pipeline(
    producer_url: &str,
    since: Cursor,
    live: bool,
    mut pipeline: Pipeline,
) -> Result<()> {
    let client = SseClient::new(producer_url);
    let subscribe = if live {
        client.subscribe_live().await
    } else {
        client.subscribe(since).await
    };
    let mut stream = subscribe.map_err(|e| anyhow::anyhow!(e.to_string()))?;
    while let Some(item) = stream.next().await {
        let seq_note = item.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        pipeline.handle(&seq_note.notification, now_ms())?;
    }
    Ok(())
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
}

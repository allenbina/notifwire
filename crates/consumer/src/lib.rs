//! notifwire consumer.
//!
//! Subscribes to a producer over the mesh transport. For the D0 walking
//! skeleton it just connects and prints what it receives — enough to prove the
//! end-to-end loop. The native OS display, filters, dedup, icons, and history
//! arrive in D2.

use anyhow::Result;
use futures::StreamExt;
use notifwire_core::Notification;
use notifwire_transport::{Cursor, MeshConsumer, SseClient};

/// Render a received notification as a single human-readable line.
pub fn format_line(seq: Cursor, n: &Notification) -> String {
    let mut line = format!("[{seq}] {}: {}", n.app_name, n.title);
    if !n.body.is_empty() {
        line.push_str(" — ");
        line.push_str(&n.body);
    }
    match n.priority() {
        notifwire_core::Priority::High => line.push_str(" (high)"),
        notifwire_core::Priority::Urgent => line.push_str(" (urgent)"),
        _ => {}
    }
    line
}

/// Subscribe to `producer_url` starting after `since`, invoking `handler` for
/// each received notification. Runs until the stream ends or errors — which,
/// for a live SSE connection, is effectively forever.
pub async fn run<F>(producer_url: &str, since: Cursor, mut handler: F) -> Result<()>
where
    F: FnMut(Cursor, &Notification),
{
    let client = SseClient::new(producer_url);
    let mut stream = client
        .subscribe(since)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    while let Some(item) = stream.next().await {
        let seq_note = item.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        handler(seq_note.seq, &seq_note.notification);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notifwire_core::{Priority, SourcePlatform};

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
        let line = format_line(1, &note("weechat", "ping", ""));
        assert_eq!(line, "[1] weechat: ping");
    }

    #[test]
    fn tags_elevated_priority() {
        let mut n = note("alerts", "disk full", "");
        n.priority = Some(Priority::Urgent);
        assert!(format_line(2, &n).ends_with("(urgent)"));
    }
}

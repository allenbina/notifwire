//! Consumer-side notification history.
//!
//! A consumer keeps its own history of what it received (the spec puts history
//! on the consumer, not the producer). Backed by SQLite via bundled rusqlite —
//! no system dependency, so it stays portable across Windows/Linux/macOS. Key
//! fields are indexed columns for querying by app/time; the full notification
//! rides along as JSON so the schema survives model changes.

use anyhow::{Context, Result};
use notifwire_core::Notification;
use rusqlite::{params, Connection};
use std::path::Path;

/// A persistent store of received notifications.
#[derive(Debug)]
pub struct History {
    conn: Connection,
}

impl History {
    /// Open (creating if needed) a history database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening history db at {}", path.display()))?;
        Self::init(conn)
    }

    /// An ephemeral in-memory history (used in tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notifications (
                 id            TEXT PRIMARY KEY,
                 app_name      TEXT NOT NULL,
                 timestamp     TEXT NOT NULL,
                 producer_node TEXT NOT NULL,
                 json          TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_app ON notifications(app_name);
             CREATE INDEX IF NOT EXISTS idx_ts  ON notifications(timestamp);",
        )?;
        Ok(Self { conn })
    }

    /// Record a received notification. Idempotent on the notification id, so
    /// re-delivery of the same event doesn't duplicate history.
    pub fn record(&self, n: &Notification) -> Result<()> {
        let json = serde_json::to_string(n)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO notifications (id, app_name, timestamp, producer_node, json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![n.id, n.app_name, n.timestamp, n.producer_node, json],
        )?;
        Ok(())
    }

    /// The most recent notifications, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<Notification>> {
        self.query(
            "SELECT json FROM notifications ORDER BY timestamp DESC, rowid DESC LIMIT ?1",
            params![limit as i64],
        )
    }

    /// The most recent notifications from a given app, newest first.
    pub fn by_app(&self, app_name: &str, limit: usize) -> Result<Vec<Notification>> {
        self.query(
            "SELECT json FROM notifications WHERE app_name = ?1
             ORDER BY timestamp DESC, rowid DESC LIMIT ?2",
            params![app_name, limit as i64],
        )
    }

    /// Total number of notifications in the history.
    pub fn count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM notifications", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Notification>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            out.push(serde_json::from_str(&json?).context("decoding stored notification")?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notifwire_core::SourcePlatform;

    fn note(id: &str, app: &str, ts: &str) -> Notification {
        Notification::new(
            id,
            "node",
            SourcePlatform::Windows,
            app,
            "title",
            "body",
            ts,
        )
    }

    #[test]
    fn records_and_lists_newest_first() {
        let h = History::open_in_memory().unwrap();
        h.record(&note("1", "Slack", "2026-06-01T10:00:00Z"))
            .unwrap();
        h.record(&note("2", "Teams", "2026-06-01T11:00:00Z"))
            .unwrap();
        h.record(&note("3", "Slack", "2026-06-01T12:00:00Z"))
            .unwrap();

        assert_eq!(h.count().unwrap(), 3);
        let recent = h.recent(10).unwrap();
        let ids: Vec<&str> = recent.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["3", "2", "1"]); // newest first
    }

    #[test]
    fn respects_limit() {
        let h = History::open_in_memory().unwrap();
        for i in 0..5 {
            h.record(&note(
                &i.to_string(),
                "App",
                &format!("2026-06-01T0{i}:00:00Z"),
            ))
            .unwrap();
        }
        assert_eq!(h.recent(2).unwrap().len(), 2);
    }

    #[test]
    fn filters_by_app() {
        let h = History::open_in_memory().unwrap();
        h.record(&note("1", "Slack", "2026-06-01T10:00:00Z"))
            .unwrap();
        h.record(&note("2", "Teams", "2026-06-01T11:00:00Z"))
            .unwrap();
        h.record(&note("3", "Slack", "2026-06-01T12:00:00Z"))
            .unwrap();

        let slack = h.by_app("Slack", 10).unwrap();
        assert_eq!(slack.len(), 2);
        assert!(slack.iter().all(|n| n.app_name == "Slack"));
        assert_eq!(slack[0].id, "3"); // newest Slack first
    }

    #[test]
    fn record_is_idempotent_on_id() {
        let h = History::open_in_memory().unwrap();
        let n = note("dup", "Slack", "2026-06-01T10:00:00Z");
        h.record(&n).unwrap();
        h.record(&n).unwrap(); // same id again
        assert_eq!(h.count().unwrap(), 1);
    }

    #[test]
    fn round_trips_full_notification() {
        let h = History::open_in_memory().unwrap();
        let mut n = note("1", "Slack", "2026-06-01T10:00:00Z");
        n.subtitle = Some("sub".into());
        n.priority = Some(notifwire_core::Priority::High);
        h.record(&n).unwrap();
        assert_eq!(h.recent(1).unwrap()[0], n);
    }
}

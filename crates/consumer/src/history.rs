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

    /// Delete notifications older than `cutoff_ms` (unix timestamp in milliseconds).
    /// Returns the number of rows deleted.
    pub fn prune_older_than_ms(&self, cutoff_ms: i64) -> Result<usize> {
        let cutoff = ms_to_iso8601(cutoff_ms);
        let deleted = self.conn.execute(
            "DELETE FROM notifications WHERE timestamp < ?1",
            params![cutoff],
        )?;
        Ok(deleted)
    }

    /// Delete the oldest rows so only `max_count` most-recent remain.
    /// Returns the number of rows deleted. No-op if count <= max_count.
    pub fn prune_to_count(&self, max_count: usize) -> Result<usize> {
        let current = self.count()?;
        if current <= max_count {
            return Ok(0);
        }
        let to_delete = current - max_count;
        let deleted = self.conn.execute(
            "DELETE FROM notifications WHERE rowid IN (
                 SELECT rowid FROM notifications ORDER BY timestamp ASC, rowid ASC LIMIT ?1
             )",
            params![to_delete as i64],
        )?;
        Ok(deleted)
    }

    /// Query notifications with optional filters, newest first.
    pub fn query_filtered(
        &self,
        app_name: Option<&str>,
        producer_node: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Notification>> {
        // Build a dynamic query based on which filters are present.
        match (app_name, producer_node) {
            (Some(app), Some(node)) => self.query_raw(
                "SELECT json FROM notifications WHERE app_name = ?1 AND producer_node = ?2
                 ORDER BY timestamp DESC, rowid DESC LIMIT ?3 OFFSET ?4",
                params![app, node, limit as i64, offset as i64],
            ),
            (Some(app), None) => self.query_raw(
                "SELECT json FROM notifications WHERE app_name = ?1
                 ORDER BY timestamp DESC, rowid DESC LIMIT ?2 OFFSET ?3",
                params![app, limit as i64, offset as i64],
            ),
            (None, Some(node)) => self.query_raw(
                "SELECT json FROM notifications WHERE producer_node = ?1
                 ORDER BY timestamp DESC, rowid DESC LIMIT ?2 OFFSET ?3",
                params![node, limit as i64, offset as i64],
            ),
            (None, None) => self.query_raw(
                "SELECT json FROM notifications ORDER BY timestamp DESC, rowid DESC LIMIT ?1 OFFSET ?2",
                params![limit as i64, offset as i64],
            ),
        }
    }

    fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Notification>> {
        self.query_raw(sql, params)
    }

    fn query_raw(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Notification>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            out.push(serde_json::from_str(&json?).context("decoding stored notification")?);
        }
        Ok(out)
    }
}

/// Convert a unix timestamp in milliseconds to an ISO 8601 UTC string
/// (`YYYY-MM-DDTHH:MM:SSZ`), suitable for comparison against the `timestamp`
/// column which is stored in this same format.
fn ms_to_iso8601(ms: i64) -> String {
    // Total seconds since epoch (floor for negative timestamps too)
    let secs = ms.div_euclid(1000);
    let days_since_epoch = secs.div_euclid(86400);
    let time_of_day = secs.rem_euclid(86400);

    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Gregorian calendar computation from Julian Day Number
    // Julian Day Number for Unix epoch (1970-01-01) = 2440588
    let jdn = days_since_epoch + 2_440_588;
    let (year, month, day) = jdn_to_ymd(jdn);

    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert a Julian Day Number to (year, month, day) using the proleptic
/// Gregorian calendar algorithm (Richards 2013).
fn jdn_to_ymd(jdn: i64) -> (i64, i64, i64) {
    let f = jdn + 1_401 + (((4 * jdn + 274_277) / 146_097) * 3) / 4 - 38;
    let e = 4 * f + 3;
    let g = (e % 1_461) / 4;
    let h = 5 * g + 2;
    let day = (h % 153) / 5 + 1;
    let month = (h / 153 + 2) % 12 + 1;
    let year = e / 1_461 - 4_716 + (12 + 2 - month) / 12;
    (year, month, day)
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

    #[test]
    fn prune_older_than_ms_removes_old_rows() {
        let h = History::open_in_memory().unwrap();
        h.record(&note("1", "App", "2026-01-01T00:00:00Z")).unwrap();
        h.record(&note("2", "App", "2026-06-01T00:00:00Z")).unwrap();
        h.record(&note("3", "App", "2026-12-01T00:00:00Z")).unwrap();

        // cutoff at 2026-06-01: only "2026-01-01" is strictly older
        // Unix ms for 2026-06-01T00:00:00Z = 1780272000000
        let cutoff_ms: i64 = 1_780_272_000_000;
        let deleted = h.prune_older_than_ms(cutoff_ms).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(h.count().unwrap(), 2);
    }

    #[test]
    fn prune_to_count_removes_oldest() {
        let h = History::open_in_memory().unwrap();
        for i in 1..=5 {
            h.record(&note(
                &i.to_string(),
                "App",
                &format!("2026-01-{i:02}T00:00:00Z"),
            ))
            .unwrap();
        }
        let deleted = h.prune_to_count(3).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(h.count().unwrap(), 3);
        // Remaining should be the 3 newest
        let remaining = h.recent(10).unwrap();
        let ids: Vec<&str> = remaining.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["5", "4", "3"]);
    }

    #[test]
    fn prune_to_count_noop_when_under_limit() {
        let h = History::open_in_memory().unwrap();
        h.record(&note("1", "App", "2026-01-01T00:00:00Z")).unwrap();
        assert_eq!(h.prune_to_count(5).unwrap(), 0);
    }

    #[test]
    fn query_filtered_by_app() {
        let h = History::open_in_memory().unwrap();
        h.record(&note("1", "Slack", "2026-01-01T10:00:00Z"))
            .unwrap();
        h.record(&note("2", "Teams", "2026-01-01T11:00:00Z"))
            .unwrap();
        h.record(&note("3", "Slack", "2026-01-01T12:00:00Z"))
            .unwrap();

        let slack = h.query_filtered(Some("Slack"), None, 10, 0).unwrap();
        assert_eq!(slack.len(), 2);
        assert!(slack.iter().all(|n| n.app_name == "Slack"));
    }

    #[test]
    fn query_filtered_with_offset() {
        let h = History::open_in_memory().unwrap();
        for i in 1..=5 {
            h.record(&note(
                &i.to_string(),
                "App",
                &format!("2026-01-{i:02}T00:00:00Z"),
            ))
            .unwrap();
        }
        let page2 = h.query_filtered(None, None, 2, 2).unwrap();
        assert_eq!(page2.len(), 2);
        // Newest-first order, offset 2 skips ids 5 and 4
        assert_eq!(page2[0].id, "3");
        assert_eq!(page2[1].id, "2");
    }

    #[test]
    fn ms_to_iso8601_known_timestamps() {
        // Unix epoch: 0 ms = 1970-01-01T00:00:00Z
        assert_eq!(ms_to_iso8601(0), "1970-01-01T00:00:00Z");
        // 2026-06-01T00:00:00Z = 1780272000000 ms
        assert_eq!(ms_to_iso8601(1_780_272_000_000), "2026-06-01T00:00:00Z");
    }
}

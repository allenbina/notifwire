//! Durable outbox: snapshot the producer [`Outbox`] to disk so a restart
//! doesn't lose buffered notifications, and a consumer that reconnects after
//! the producer bounced still catches up from its cursor.
//!
//! The store is deliberately simple — a single JSON snapshot written
//! atomically (temp file + rename) on each change. At human-notification
//! volumes the per-append cost is trivial; if write volume ever matters this
//! can become an append-only log with compaction without changing callers.

use crate::Outbox;
use std::io;
use std::path::Path;

/// Load an outbox snapshot from `path`, falling back to a fresh outbox if the
/// file is missing or unreadable/corrupt. The configured `capacity` is
/// re-applied (trimming the loaded buffer if it was larger), while the saved
/// sequence counter is preserved so cursors stay monotonic across restarts.
pub fn load_outbox(path: &Path, capacity: usize) -> Outbox {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Outbox>(&bytes) {
            Ok(mut outbox) => {
                outbox.set_capacity(capacity);
                outbox
            }
            // Corrupt snapshot: don't wedge the producer, start clean.
            Err(_) => Outbox::new(capacity),
        },
        Err(_) => Outbox::new(capacity),
    }
}

/// Write `outbox` to `path` atomically (temp file + rename), so a crash
/// mid-write can't leave a torn snapshot.
pub fn save_outbox(outbox: &Outbox, path: &Path) -> io::Result<()> {
    let bytes = serde_json::to_vec(outbox)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    // std::fs::rename replaces an existing destination on both Unix and Windows.
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Outbox;
    use notifwire_core::{Notification, SourcePlatform};
    use std::path::PathBuf;

    fn note(title: &str) -> Notification {
        Notification::new(
            "id",
            "node",
            SourcePlatform::Plugin,
            "app",
            title,
            "body",
            "2026-06-01T00:00:00Z",
        )
    }

    // Unique per-test path under the temp dir (process id keeps parallel runs
    // from colliding; distinct names keep tests in this process apart).
    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "notifwire-test-{}-{}.json",
            name,
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_yields_fresh_outbox() {
        let p = temp_path("missing");
        let _ = std::fs::remove_file(&p);
        let ob = load_outbox(&p, 10);
        assert_eq!(ob.latest(), 0);
        assert!(ob.since(0).events.is_empty());
    }

    #[test]
    fn survives_restart_preserving_cursor() {
        let p = temp_path("restart");
        let _ = std::fs::remove_file(&p);

        let mut ob = Outbox::new(100);
        for t in ["a", "b", "c"] {
            ob.append(note(t));
        }
        save_outbox(&ob, &p).unwrap();

        // "Restart": reload from disk.
        let mut reloaded = load_outbox(&p, 100);
        assert_eq!(reloaded.latest(), 3);
        let kept: Vec<_> = reloaded.since(0).events.iter().map(|s| s.seq).collect();
        assert_eq!(kept, vec![1, 2, 3]);
        // New appends continue the sequence, not restart it.
        assert_eq!(reloaded.append(note("d")), 4);

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn corrupt_snapshot_falls_back_to_fresh() {
        let p = temp_path("corrupt");
        std::fs::write(&p, b"this is not json").unwrap();
        let ob = load_outbox(&p, 10);
        assert_eq!(ob.latest(), 0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn load_reapplies_smaller_capacity() {
        let p = temp_path("capacity");
        let _ = std::fs::remove_file(&p);
        let mut ob = Outbox::new(100);
        for t in ["a", "b", "c", "d", "e"] {
            ob.append(note(t));
        }
        save_outbox(&ob, &p).unwrap();

        // Reload with a tighter capacity: keep only the most recent 2.
        let reloaded = load_outbox(&p, 2);
        let kept: Vec<_> = reloaded.since(0).events.iter().map(|s| s.seq).collect();
        assert_eq!(kept, vec![4, 5]);
        assert_eq!(reloaded.latest(), 5); // counter unchanged
        let _ = std::fs::remove_file(&p);
    }
}

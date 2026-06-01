//! Dedup window for the consumer.
//!
//! The same logical notification can arrive more than once — an app re-posts an
//! identical toast, a producer's poll re-reports it, or two producers capture
//! the same event. The [`Deduper`] suppresses an identical notification seen
//! again within a time window, so it's shown at most once per window.
//!
//! `core` stays clock-free: the caller passes the current time in (`now_ms`),
//! rather than the engine reaching for a clock.

use crate::Notification;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Content fingerprint: identical `app_name` + `title` + `body` are "the same"
/// notification for dedup purposes.
fn fingerprint(n: &Notification) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    n.app_name.hash(&mut h);
    n.title.hash(&mut h);
    n.body.hash(&mut h);
    h.finish()
}

/// Suppresses identical notifications seen again within a fixed time window
/// (measured from the first sighting, not sliding).
#[derive(Debug)]
pub struct Deduper {
    window_ms: i64,
    /// fingerprint → time it was last *shown* (in the caller's clock, ms).
    last_shown: HashMap<u64, i64>,
}

impl Deduper {
    /// Create a deduper with the given window in milliseconds. A window of `0`
    /// dedups nothing (every notification is considered new).
    pub fn new(window_ms: i64) -> Self {
        Self {
            window_ms: window_ms.max(0),
            last_shown: HashMap::new(),
        }
    }

    /// Decide whether `n` is a duplicate that should be suppressed, given the
    /// caller's current time `now_ms`. A non-duplicate is recorded as shown.
    pub fn is_duplicate(&mut self, n: &Notification, now_ms: i64) -> bool {
        if self.window_ms == 0 {
            return false;
        }
        let fp = fingerprint(n);
        let duplicate = self
            .last_shown
            .get(&fp)
            .is_some_and(|&shown| now_ms - shown < self.window_ms);

        if !duplicate {
            self.last_shown.insert(fp, now_ms);
        }
        // Drop entries older than the window so the map stays bounded.
        let window = self.window_ms;
        self.last_shown
            .retain(|_, &mut shown| now_ms - shown < window);

        duplicate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourcePlatform;

    fn note(app: &str, title: &str, body: &str) -> Notification {
        Notification::new(
            "id",
            "node",
            SourcePlatform::Windows,
            app,
            title,
            body,
            "2026-06-01T00:00:00Z",
        )
    }

    #[test]
    fn first_sighting_is_not_a_duplicate() {
        let mut d = Deduper::new(60_000);
        assert!(!d.is_duplicate(&note("Slack", "ping", ""), 0));
    }

    #[test]
    fn immediate_repeat_is_a_duplicate() {
        let mut d = Deduper::new(60_000);
        let n = note("Slack", "ping", "");
        assert!(!d.is_duplicate(&n, 1000));
        assert!(d.is_duplicate(&n, 1000));
        assert!(d.is_duplicate(&n, 30_000)); // still within the 60s window
    }

    #[test]
    fn window_is_fixed_from_first_sighting() {
        let mut d = Deduper::new(60_000);
        let n = note("alerts", "battery low", "");
        assert!(!d.is_duplicate(&n, 0)); // shown
        assert!(d.is_duplicate(&n, 59_000)); // within window → suppressed
        assert!(!d.is_duplicate(&n, 60_000)); // window elapsed → shown again
    }

    #[test]
    fn different_content_is_never_a_duplicate() {
        let mut d = Deduper::new(60_000);
        assert!(!d.is_duplicate(&note("Slack", "a", ""), 0));
        assert!(!d.is_duplicate(&note("Slack", "b", ""), 0)); // different title
        assert!(!d.is_duplicate(&note("Teams", "a", ""), 0)); // different app
    }

    #[test]
    fn zero_window_dedups_nothing() {
        let mut d = Deduper::new(0);
        let n = note("Slack", "ping", "");
        assert!(!d.is_duplicate(&n, 0));
        assert!(!d.is_duplicate(&n, 0));
    }
}

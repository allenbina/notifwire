//! The producer outbox: a bounded, monotonically-sequenced buffer of recent
//! notifications that backs offline catch-up.
//!
//! Each appended notification is assigned a monotonic [`Cursor`] (sequence
//! number) starting at 1. A consumer remembers the cursor of the last
//! notification it saw; on reconnect it asks for everything *after* that
//! cursor via [`Outbox::since`]. The buffer is bounded, so if a consumer has
//! been offline long enough that events it never saw were evicted, the
//! catch-up result flags a [`gap`](CatchUp::gap) so the consumer knows it
//! missed some — the same high-water-mark idea the SSE `id:` field carries on
//! the wire.

use notifwire_core::Notification;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A monotonic high-water mark. On the wire this is the SSE `id:` value; a
/// consumer stores one of these per producer. `0` means "from the beginning".
pub type Cursor = u64;

/// A notification paired with the cursor the producer assigned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sequenced {
    pub seq: Cursor,
    pub notification: Notification,
}

/// The result of a [`Outbox::since`] catch-up query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchUp {
    /// Notifications with `seq > cursor`, in ascending order.
    pub events: Vec<Sequenced>,
    /// True if events after the requested cursor had already been evicted —
    /// the consumer missed some and the stream it receives is not gap-free.
    pub gap: bool,
}

/// A bounded, monotonically-sequenced ring of recent notifications.
///
/// It is `Serialize`/`Deserialize` so it can be snapshotted to disk for
/// durable catch-up across restarts (see [`crate::persist`]); the next
/// sequence number is part of the snapshot, so cursors stay monotonic.
#[derive(Debug, Serialize, Deserialize)]
pub struct Outbox {
    capacity: usize,
    next_seq: Cursor,
    buf: VecDeque<Sequenced>,
}

impl Outbox {
    /// Create an outbox retaining at most `capacity` of the most recent
    /// notifications. `capacity` is clamped to at least 1.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_seq: 1,
            buf: VecDeque::new(),
        }
    }

    /// Append a notification, assign it the next cursor, and return that
    /// cursor. Evicts the oldest entry if the buffer is over capacity.
    pub fn append(&mut self, notification: Notification) -> Cursor {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.buf.push_back(Sequenced { seq, notification });
        while self.buf.len() > self.capacity {
            self.buf.pop_front();
        }
        seq
    }

    /// Re-apply a retention `capacity` (e.g. after loading a snapshot whose
    /// configured capacity differs), trimming the oldest entries if needed.
    /// Does not touch the sequence counter.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        while self.buf.len() > self.capacity {
            self.buf.pop_front();
        }
    }

    /// The highest cursor assigned so far, or `0` if nothing has been appended.
    pub fn latest(&self) -> Cursor {
        self.next_seq - 1
    }

    /// The oldest cursor still retained, or `None` if the buffer is empty.
    pub fn oldest_retained(&self) -> Option<Cursor> {
        self.buf.front().map(|s| s.seq)
    }

    /// Everything after `cursor`, plus a flag indicating whether any events in
    /// `(cursor, oldest_retained)` had already been evicted (a gap).
    pub fn since(&self, cursor: Cursor) -> CatchUp {
        let events: Vec<Sequenced> = self
            .buf
            .iter()
            .filter(|s| s.seq > cursor)
            .cloned()
            .collect();
        // A gap exists when the oldest event we still hold is more than one
        // step past the consumer's cursor — i.e. cursor+1 .. oldest-1 are gone.
        let gap = self
            .oldest_retained()
            .is_some_and(|oldest| oldest > cursor + 1);
        CatchUp { events, gap }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notifwire_core::SourcePlatform;

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

    #[test]
    fn append_assigns_monotonic_cursors_from_one() {
        let mut ob = Outbox::new(10);
        assert_eq!(ob.latest(), 0);
        assert_eq!(ob.append(note("a")), 1);
        assert_eq!(ob.append(note("b")), 2);
        assert_eq!(ob.append(note("c")), 3);
        assert_eq!(ob.latest(), 3);
        assert_eq!(ob.oldest_retained(), Some(1));
    }

    #[test]
    fn since_returns_only_events_after_cursor() {
        let mut ob = Outbox::new(10);
        for t in ["a", "b", "c"] {
            ob.append(note(t));
        }
        let all = ob.since(0);
        assert!(!all.gap);
        assert_eq!(all.events.len(), 3);
        assert_eq!(all.events[0].seq, 1);

        let tail = ob.since(2);
        assert_eq!(tail.events.len(), 1);
        assert_eq!(tail.events[0].seq, 3);

        assert!(ob.since(3).events.is_empty()); // caught up
    }

    #[test]
    fn eviction_keeps_only_the_most_recent() {
        let mut ob = Outbox::new(3);
        for t in ["a", "b", "c", "d", "e"] {
            ob.append(note(t)); // seqs 1..=5; only 3,4,5 retained
        }
        assert_eq!(ob.latest(), 5);
        assert_eq!(ob.oldest_retained(), Some(3));
        let kept: Vec<Cursor> = ob.since(0).events.iter().map(|s| s.seq).collect();
        assert_eq!(kept, vec![3, 4, 5]);
    }

    #[test]
    fn gap_is_flagged_when_unseen_events_were_evicted() {
        let mut ob = Outbox::new(3);
        for t in ["a", "b", "c", "d", "e"] {
            ob.append(note(t)); // retains seq 3,4,5
        }
        // Consumer last saw seq 1 → it never saw seq 2, which is gone: gap.
        assert!(ob.since(1).gap);
        // Consumer last saw seq 0 (fresh) → missed 1,2: gap.
        assert!(ob.since(0).gap);
        // Consumer last saw seq 2 → next is 3, which we still hold: no gap.
        assert!(!ob.since(2).gap);
        // Already at the latest: no gap.
        assert!(!ob.since(5).gap);
    }

    #[test]
    fn empty_outbox_has_no_events_and_no_gap() {
        let ob = Outbox::new(5);
        let c = ob.since(0);
        assert!(c.events.is_empty());
        assert!(!c.gap);
    }
}

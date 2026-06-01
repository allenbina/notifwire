//! The `MeshTransport` seam: the traits that decouple the notification
//! pipeline from how bytes actually move. The SSE types in [`crate::sse`] are
//! the first implementation; a WebSocket adapter can satisfy the same traits
//! later without touching callers.

use crate::{Cursor, Sequenced, TransportError};
use futures::stream::BoxStream;
use notifwire_core::Notification;
use std::future::Future;

/// A live stream of sequenced notifications from one producer.
pub type EventStream = BoxStream<'static, Result<Sequenced, TransportError>>;

/// The producer side: accepts notifications and assigns each a [`Cursor`].
pub trait MeshProducer: Send + Sync {
    /// Publish a notification to live subscribers and the catch-up buffer,
    /// returning the cursor it was assigned.
    fn publish(&self, notification: Notification) -> Cursor;
}

/// The consumer side: subscribes to a producer and receives its stream,
/// resuming from a remembered cursor.
pub trait MeshConsumer: Send + Sync {
    /// Subscribe starting *after* `since` (use `0` for "from the beginning").
    /// The returned stream first replays any buffered catch-up events, then
    /// stays open for live ones.
    fn subscribe(
        &self,
        since: Cursor,
    ) -> impl Future<Output = Result<EventStream, TransportError>> + Send;
}

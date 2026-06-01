//! notifwire transport.
//!
//! Defines the seam between the notification pipeline and how bytes move
//! between a producer and a consumer. The catch-up foundation lives here: the
//! monotonic [`Cursor`] and the bounded producer [`Outbox`] that backs offline
//! recovery. The `MeshTransport` trait and its SSE implementation (serve +
//! pull-since-cursor, reconnect/catch-up) land alongside each other next, so
//! the trait is shaped against a real implementation rather than in the
//! abstract (D0-5).

mod error;
mod mesh;
mod outbox;
mod sse;

pub use error::TransportError;
pub use mesh::{EventStream, MeshConsumer, MeshProducer};
pub use outbox::{CatchUp, Cursor, Outbox, Sequenced};
pub use sse::{SseClient, SseProducer, SseServer};

/// Crate version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }
}

//! notifwire transport.
//!
//! Defines the `MeshTransport` trait — the seam between the notification
//! pipeline and how bytes move between a producer and a consumer — and an
//! SSE implementation behind it (D0-5). A WebSocket adapter can slot in
//! later without touching callers.
//!
//! This is a stub crate scaffolded in D0-1.

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

//! notifwire core.
//!
//! Home for the OS-independent building blocks shared by every node:
//! the normalized [`Notification`] data model (D0-3), the versioned config
//! schema with apply-if-newer semantics (D0-4), and the rules engine.
//!
//! This is a stub crate scaffolded in D0-1; the types above land in their
//! own issues. Nothing here depends on a platform or a transport.

/// Crate version string, surfaced in handshakes and `--version` output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }
}

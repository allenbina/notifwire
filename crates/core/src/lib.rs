//! notifwire core.
//!
//! Home for the OS-independent building blocks shared by every node: the
//! normalized [`Notification`] data model, the versioned config schema with
//! apply-if-newer semantics (D0-4), and the rules engine. Nothing here depends
//! on a platform or a transport.

mod notification;

pub use notification::{Notification, Priority, SourcePlatform};

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

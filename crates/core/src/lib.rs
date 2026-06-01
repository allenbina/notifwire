//! notifwire core.
//!
//! Home for the OS-independent building blocks shared by every node: the
//! normalized [`Notification`] data model, the versioned config schema with
//! apply-if-newer semantics (D0-4), and the rules engine. Nothing here depends
//! on a platform or a transport.

mod capture;
mod config;
mod dedup;
mod icon;
mod notification;
mod rules;
mod sink;

pub use capture::{CaptureError, NotificationSource, SyntheticSource};
pub use config::{Config, Freshness};
pub use dedup::Deduper;
pub use icon::{classify, icon_chain, IconRef, IconStep};
pub use notification::{Notification, Priority, SourcePlatform};
pub use rules::{DefaultMode, Filter, FilterAction, MatchField, Rules, Verdict};
pub use sink::{DisplayError, NotificationSink};

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

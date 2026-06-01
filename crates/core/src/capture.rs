//! The OS-capture seam.
//!
//! Capturing notifications from a host OS is platform-specific and can't be
//! unit-tested without firing real toasts, so it lives behind the
//! [`NotificationSource`] trait. The pipeline (producer node → ingest → mesh)
//! is driven by this trait, which means it can be exercised end-to-end with a
//! [`SyntheticSource`] in tests, and the real Windows/macOS/Linux bridges just
//! provide their own implementation.
//!
//! The trait is intentionally runtime-agnostic — it declares an async method
//! via `impl Future` rather than pulling a channel type (and thus a runtime)
//! into `core`.

use crate::Notification;
use std::collections::VecDeque;
use std::future::Future;
use thiserror::Error;

/// Something went wrong capturing from the OS.
#[derive(Debug, Error)]
pub enum CaptureError {
    /// The OS hasn't granted notification-access permission (see onboarding).
    #[error("notification access not granted")]
    AccessDenied,
    /// The platform capture backend failed.
    #[error("capture backend error: {0}")]
    Backend(String),
}

/// A source of captured notifications: an OS bridge in production, or a
/// [`SyntheticSource`] in tests. Implementors normalize whatever the platform
/// hands them into a [`Notification`] before yielding it.
pub trait NotificationSource: Send {
    /// A short label for logs, e.g. `"windows-winrt"` or `"synthetic"`.
    fn name(&self) -> &str;

    /// Resolve the next captured notification, `None` once the source is
    /// exhausted (synthetic sources end; live OS sources effectively never do).
    fn next(&mut self) -> impl Future<Output = Result<Option<Notification>, CaptureError>> + Send;
}

/// A test/in-memory source that yields a fixed queue of notifications and then
/// ends. Lets the whole pipeline be exercised without any OS capture.
#[derive(Debug, Default)]
pub struct SyntheticSource {
    queue: VecDeque<Notification>,
}

impl SyntheticSource {
    /// Create a source pre-loaded with `notifications`, yielded in order.
    pub fn new(notifications: impl IntoIterator<Item = Notification>) -> Self {
        Self {
            queue: notifications.into_iter().collect(),
        }
    }

    /// Queue another notification to be yielded.
    pub fn push(&mut self, notification: Notification) {
        self.queue.push_back(notification);
    }
}

impl NotificationSource for SyntheticSource {
    fn name(&self) -> &str {
        "synthetic"
    }

    fn next(&mut self) -> impl Future<Output = Result<Option<Notification>, CaptureError>> + Send {
        // Resolve synchronously; return a ready future so no runtime is needed.
        let item = self.queue.pop_front();
        async move { Ok(item) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourcePlatform;

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

    #[tokio::test]
    async fn synthetic_yields_in_order_then_ends() {
        let mut src = SyntheticSource::new([note("a"), note("b")]);
        assert_eq!(src.name(), "synthetic");
        assert_eq!(src.next().await.unwrap().unwrap().title, "a");
        assert_eq!(src.next().await.unwrap().unwrap().title, "b");
        assert!(src.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn push_appends_to_the_queue() {
        let mut src = SyntheticSource::default();
        assert!(src.next().await.unwrap().is_none());
        src.push(note("late"));
        assert_eq!(src.next().await.unwrap().unwrap().title, "late");
    }
}

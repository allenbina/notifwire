//! The display seam.
//!
//! A consumer renders received notifications on its host — natively, as an OS
//! toast (the spec: no custom UI). [`NotificationSink`] is the seam the display
//! implementations satisfy: a WinRT toast sink on Windows, a
//! tauri-plugin-notification sink in the GUI app, or a log sink for headless
//! nodes and tests. It's the display-side mirror of
//! [`NotificationSource`](crate::NotificationSource).

use crate::Notification;
use thiserror::Error;

/// Something went wrong rendering a notification.
#[derive(Debug, Error)]
pub enum DisplayError {
    /// The platform display backend failed.
    #[error("display backend error: {0}")]
    Backend(String),
    /// No native display on this platform/build.
    #[error("native display not supported here")]
    Unsupported,
}

/// Renders a single notification for the user.
pub trait NotificationSink: Send {
    fn show(&self, notification: &Notification) -> Result<(), DisplayError>;
}

/// Any matching closure is a sink — lets the consumer wire a print/handler
/// closure (or a test spy) wherever a `NotificationSink` is expected.
impl<F> NotificationSink for F
where
    F: Fn(&Notification) -> Result<(), DisplayError> + Send,
{
    fn show(&self, notification: &Notification) -> Result<(), DisplayError> {
        self(notification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourcePlatform;
    use std::sync::Mutex;

    fn note(title: &str) -> Notification {
        Notification::new(
            "id",
            "node",
            SourcePlatform::Windows,
            "app",
            title,
            "body",
            "2026-06-01T00:00:00Z",
        )
    }

    #[test]
    fn closure_is_a_sink() {
        let seen = Mutex::new(Vec::new());
        let sink = |n: &Notification| {
            seen.lock().unwrap().push(n.title.clone());
            Ok(())
        };
        sink.show(&note("a")).unwrap();
        sink.show(&note("b")).unwrap();
        assert_eq!(*seen.lock().unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn sink_can_report_failure() {
        let sink = |_: &Notification| Err(DisplayError::Unsupported);
        assert!(sink.show(&note("x")).is_err());
    }
}

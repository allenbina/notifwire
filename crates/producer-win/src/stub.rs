//! Non-Windows stub so the crate (and the workspace) builds cross-platform.
//! Windows notification capture is, unsurprisingly, Windows-only.

use notifwire_core::{CaptureError, Notification, NotificationSource};

/// Placeholder on non-Windows targets: construction fails with a clear error.
#[derive(Debug)]
pub struct WindowsNotificationSource {
    _private: (),
}

impl WindowsNotificationSource {
    /// Always errors off Windows.
    pub fn start(_producer_node: impl Into<String>) -> Result<Self, CaptureError> {
        Err(CaptureError::Backend(
            "Windows notification capture is only available on Windows".to_owned(),
        ))
    }
}

impl NotificationSource for WindowsNotificationSource {
    fn name(&self) -> &str {
        "windows-winrt (unavailable)"
    }

    async fn next(&mut self) -> Result<Option<Notification>, CaptureError> {
        Ok(None)
    }
}

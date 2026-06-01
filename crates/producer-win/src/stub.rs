//! Non-Windows stub so the crate (and the workspace) builds cross-platform.
//! Windows notification capture is, unsurprisingly, Windows-only.

use crate::AccessState;
use notifwire_core::{CaptureError, Notification, NotificationSource};

/// Always errors off Windows.
pub fn request_access() -> Result<AccessState, CaptureError> {
    Err(CaptureError::Backend(
        "Windows notification capture is only available on Windows".to_owned(),
    ))
}

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

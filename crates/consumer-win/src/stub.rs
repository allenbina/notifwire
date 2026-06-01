//! Non-Windows stub so the crate (and workspace) builds cross-platform.

use notifwire_core::{DisplayError, Notification, NotificationSink};

/// Placeholder on non-Windows targets.
#[derive(Debug)]
pub struct WindowsToastSink {
    _private: (),
}

impl WindowsToastSink {
    /// Always errors off Windows.
    pub fn new(
        _aumid: impl AsRef<str>,
        _display_name: impl AsRef<str>,
    ) -> Result<Self, DisplayError> {
        Err(DisplayError::Unsupported)
    }
}

impl NotificationSink for WindowsToastSink {
    fn show(&self, _notification: &Notification) -> Result<(), DisplayError> {
        Err(DisplayError::Unsupported)
    }
}

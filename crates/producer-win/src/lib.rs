//! Windows notification capture via the WinRT `UserNotificationListener`.
//!
//! WinRT objects are apartment-bound and not `Send`/await-friendly, so the
//! actual listening runs on a dedicated worker thread that polls the listener
//! and sends normalized [`Notification`](notifwire_core::Notification)s over a
//! channel. [`WindowsNotificationSource`] holds the (`Send`) receiving end and
//! satisfies [`NotificationSource`](notifwire_core::NotificationSource).
//!
//! ## Runtime requirements (see `docs/windows-notification-capture.md`)
//!
//! `UserNotificationListener` requires **package identity** (a sparse package)
//! and a **user-granted notification-access permission**. Without identity the
//! API fails at runtime. This crate compiles (and CI compiles it on a real
//! Windows runner, validating the bindings), but live capture is validated in
//! D1-6 once the sparse package + permission grant are in place.

/// Result of requesting notification-access permission from Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessState {
    /// The user has granted access; capture will work.
    Granted,
    /// The user denied access; capture yields nothing until re-granted in
    /// Settings → Privacy & security → Notifications.
    Denied,
    /// Not yet decided (no prompt answered).
    Unspecified,
}

#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::{request_access, WindowsNotificationSource};

#[cfg(not(windows))]
mod stub;
#[cfg(not(windows))]
pub use stub::{request_access, WindowsNotificationSource};

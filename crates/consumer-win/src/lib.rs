//! Windows native display: render received notifications as WinRT toasts.
//!
//! Implements [`NotificationSink`](notifwire_core::NotificationSink) by building
//! a ToastGeneric XML payload and showing it through `ToastNotificationManager`.
//!
//! ## Runtime note
//!
//! Windows shows a toast only for an app with a registered **AppUserModelID**.
//! [`WindowsToastSink::new`] registers one the lightweight way — sets the
//! process AUMID and writes the `HKCU\Software\Classes\AppUserModelId\<id>`
//! display metadata — which is the modern unpackaged path. If toasts don't
//! appear on a given Windows build we may also need a Start Menu shortcut
//! carrying the AUMID; this compiles and CI validates the bindings, but whether
//! a toast visually pops is confirmed live (D2-6).

#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::WindowsToastSink;

#[cfg(not(windows))]
mod stub;
#[cfg(not(windows))]
pub use stub::WindowsToastSink;

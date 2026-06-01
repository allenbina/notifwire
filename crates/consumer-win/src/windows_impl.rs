//! WinRT toast display (Windows only).

use notifwire_core::{DisplayError, Notification, NotificationSink};
use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

/// Shows notifications as native Windows toasts under a registered AppUserModelID.
pub struct WindowsToastSink {
    aumid: HSTRING,
}

impl WindowsToastSink {
    /// Register the AUMID (process id + display metadata) and build a sink that
    /// shows toasts under it. `aumid` is a stable id like
    /// `"uk.allenbina.notifwire"`; `display_name` is what the toast is labeled.
    pub fn new(
        aumid: impl AsRef<str>,
        display_name: impl AsRef<str>,
    ) -> Result<Self, DisplayError> {
        let aumid = aumid.as_ref();
        register_aumid(aumid, display_name.as_ref())?;
        Ok(Self {
            aumid: HSTRING::from(aumid),
        })
    }
}

impl NotificationSink for WindowsToastSink {
    fn show(&self, notification: &Notification) -> Result<(), DisplayError> {
        let doc = XmlDocument::new().map_err(winrt_err)?;
        doc.LoadXml(&HSTRING::from(build_toast_xml(notification)))
            .map_err(winrt_err)?;
        let toast = ToastNotification::CreateToastNotification(&doc).map_err(winrt_err)?;
        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&self.aumid).map_err(winrt_err)?;
        notifier.Show(&toast).map_err(winrt_err)
    }
}

fn winrt_err(e: windows::core::Error) -> DisplayError {
    DisplayError::Backend(e.to_string())
}

/// Tell the process which AUMID its toasts belong to and register the AUMID's
/// display metadata under HKCU so Windows shows the right name.
fn register_aumid(aumid: &str, display_name: &str) -> Result<(), DisplayError> {
    // SAFETY: a documented WinRT/Win32 call; the HSTRING outlives the call.
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(aumid)) }.map_err(|e| {
        DisplayError::Backend(format!("SetCurrentProcessExplicitAppUserModelID: {e}"))
    })?;

    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(format!("Software\\Classes\\AppUserModelId\\{aumid}"))
        .map_err(|e| DisplayError::Backend(format!("registering AUMID: {e}")))?;
    key.set_value("DisplayName", &display_name.to_string())
        .map_err(|e| DisplayError::Backend(format!("setting AUMID DisplayName: {e}")))?;
    Ok(())
}

fn build_toast_xml(n: &Notification) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(&n.title),
        xml_escape(&n.body),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use notifwire_core::SourcePlatform;

    #[test]
    fn escapes_xml_metacharacters() {
        assert_eq!(
            xml_escape("a & b <x> \"q\""),
            "a &amp; b &lt;x&gt; &quot;q&quot;"
        );
    }

    #[test]
    fn toast_xml_is_well_formed_and_escaped() {
        let n = Notification::new(
            "id",
            "node",
            SourcePlatform::Windows,
            "app",
            "Deploy <ok>",
            "build & ship",
            "2026-06-01T00:00:00Z",
        );
        let xml = build_toast_xml(&n);
        assert!(xml.starts_with("<toast>") && xml.ends_with("</toast>"));
        assert!(xml.contains("Deploy &lt;ok&gt;"));
        assert!(xml.contains("build &amp; ship"));
        assert!(xml.contains(r#"template="ToastGeneric""#));
    }
}

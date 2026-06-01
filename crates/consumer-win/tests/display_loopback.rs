//! OS-integration self-test: show a toast, then capture it back through the
//! notification listener and assert it matches — so native display can be
//! verified WITHOUT a human watching the screen. notifwire tests its own
//! display because it also captures.
//!
//! Ignored by default: it needs a real Windows desktop session with the
//! notification platform (a headless CI runner may not have one). Run it on a
//! real machine on demand:
//!
//!   cargo test -p notifwire-consumer-win --test display_loopback -- --ignored --nocapture

#![cfg(windows)]

use notifwire_consumer_win::WindowsToastSink;
use notifwire_core::{Notification, NotificationSink, NotificationSource, SourcePlatform};
use notifwire_producer_win::WindowsNotificationSource;
use std::time::Duration;

const TEST_AUMID: &str = "notifwire-selftest";

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires a real Windows desktop session; run with -- --ignored"]
async fn displayed_toast_is_captured_back() {
    // A unique title so we recognize our own toast amid whatever else is in the
    // Action Center.
    let nonce = format!("nonce-{}-loopback", std::process::id());

    // Show a toast under a throwaway app identity.
    let sink = WindowsToastSink::new(TEST_AUMID, TEST_AUMID).expect("create toast sink");
    let note = Notification::new(
        "selftest",
        "node",
        SourcePlatform::Windows,
        TEST_AUMID,
        &nonce,
        "loopback body",
        "2026-06-01T00:00:00Z",
    );
    sink.show(&note).expect("show toast");

    // Capture it back from the Action Center via the real listener.
    let mut source = WindowsNotificationSource::start("selftest").expect("start capture");
    let found = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match source.next().await {
                Ok(Some(cap)) if cap.title == nonce => return true,
                Ok(Some(_)) => continue, // some other toast; keep looking
                _ => return false,       // source ended / errored
            }
        }
    })
    .await
    .unwrap_or(false);

    cleanup_test_aumid();
    assert!(
        found,
        "the displayed toast '{nonce}' was not captured back from the Action Center \
         — native display from the consumer path is not working"
    );
}

/// Remove the throwaway AUMID registry key the test registered.
fn cleanup_test_aumid() {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(format!("Software\\Classes\\AppUserModelId\\{TEST_AUMID}"));
}

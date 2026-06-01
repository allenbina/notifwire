//! Fire a single native toast to confirm Windows display works.
//!
//! `cargo run -p notifwire-consumer-win --example show_toast`
//!
//! Registers the `notifwire` AppUserModelID (an HKCU key) and
//! shows one toast. If it pops top-right / lands in the Action Center, native
//! display works unpackaged — the same question we answered for capture.

use notifwire_consumer_win::WindowsToastSink;
use notifwire_core::{Notification, NotificationSink, SourcePlatform};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sink = WindowsToastSink::new("notifwire", "notifwire")?;
    let n = Notification::new(
        "test-1",
        "notifwire",
        SourcePlatform::Windows,
        "notifwire",
        "Test toast",
        "If you can see this, native display works.",
        "2026-06-01T00:00:00Z",
    );
    sink.show(&n)?;
    println!("Toast fired — check the top-right of your screen / Action Center.");
    Ok(())
}

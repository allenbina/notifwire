//! WinRT capture implementation (Windows only).

use notifwire_core::{CaptureError, Notification, NotificationSource, SourcePlatform};
use std::collections::HashSet;
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};

/// How often the worker polls the listener for new notifications.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Bound on in-flight captured notifications buffered toward the consumer.
const CHANNEL_CAPACITY: usize = 256;

/// Captures live Windows toast notifications and yields them as normalized
/// [`Notification`]s. Construct with [`start`](Self::start).
pub struct WindowsNotificationSource {
    rx: mpsc::Receiver<Notification>,
    _worker: thread::JoinHandle<()>,
}

impl WindowsNotificationSource {
    /// Spawn the WinRT capture worker. `producer_node` is stamped on every
    /// captured notification as its originating node id.
    ///
    /// Returns once the worker thread is launched; permission is requested on
    /// the worker (if not granted, it logs and the stream simply yields
    /// nothing). Onboarding/permission status is handled separately in D1-5.
    pub fn start(producer_node: impl Into<String>) -> Result<Self, CaptureError> {
        let node = producer_node.into();
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let worker = thread::Builder::new()
            .name("notifwire-winrt-capture".to_owned())
            .spawn(move || run_capture(&node, &tx))
            .map_err(|e| CaptureError::Backend(format!("spawning capture thread: {e}")))?;
        Ok(Self {
            rx,
            _worker: worker,
        })
    }
}

impl NotificationSource for WindowsNotificationSource {
    fn name(&self) -> &str {
        "windows-winrt"
    }

    async fn next(&mut self) -> Result<Option<Notification>, CaptureError> {
        Ok(self.rx.recv().await)
    }
}

/// Worker-thread entry point: acquire the listener, request access, then poll.
fn run_capture(node: &str, tx: &mpsc::Sender<Notification>) {
    let listener = match UserNotificationListener::Current() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("notifwire: UserNotificationListener unavailable: {e}");
            return;
        }
    };

    match request_access(&listener) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("notifwire: notification access not granted; capturing nothing");
            return;
        }
        Err(e) => {
            eprintln!("notifwire: requesting notification access failed: {e}");
            return;
        }
    }

    let mut seen: HashSet<u32> = HashSet::new();
    loop {
        match poll_once(&listener, node, &mut seen) {
            Ok(batch) => {
                for n in batch {
                    if tx.blocking_send(n).is_err() {
                        return; // consumer dropped the receiver
                    }
                }
            }
            Err(e) => eprintln!("notifwire: capture poll error: {e}"),
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn request_access(listener: &UserNotificationListener) -> windows::core::Result<bool> {
    let status = listener.RequestAccessAsync()?.get()?;
    Ok(status == UserNotificationListenerAccessStatus::Allowed)
}

/// Read current toast notifications, returning the ones not seen before.
fn poll_once(
    listener: &UserNotificationListener,
    node: &str,
    seen: &mut HashSet<u32>,
) -> windows::core::Result<Vec<Notification>> {
    let view = listener
        .GetNotificationsAsync(NotificationKinds::Toast)?
        .get()?;
    let mut out = Vec::new();
    for i in 0..view.Size()? {
        let un = view.GetAt(i)?;
        let id = un.Id()?;
        if !seen.insert(id) {
            continue;
        }
        if let Some(n) = extract(&un, node) {
            out.push(n);
        }
    }
    Ok(out)
}

/// Normalize a WinRT `UserNotification` into our model. Returns `None` if it
/// isn't a parseable generic toast.
fn extract(un: &UserNotification, node: &str) -> Option<Notification> {
    let app_name = un
        .AppInfo()
        .ok()?
        .DisplayInfo()
        .ok()?
        .DisplayName()
        .ok()?
        .to_string();

    let visual = un.Notification().ok()?.Visual().ok()?;
    let binding = visual
        .GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?)
        .ok()?;
    let texts = binding.GetTextElements().ok()?;

    let mut lines: Vec<String> = Vec::new();
    for i in 0..texts.Size().ok()? {
        if let Ok(t) = texts.GetAt(i) {
            if let Ok(s) = t.Text() {
                lines.push(s.to_string());
            }
        }
    }
    let (title, body) = match lines.split_first() {
        Some((first, rest)) => (first.clone(), rest.join("\n")),
        None => (String::new(), String::new()),
    };

    Some(Notification::new(
        uuid::Uuid::new_v4().to_string(),
        node.to_owned(),
        SourcePlatform::Windows,
        app_name,
        title,
        body,
        chrono::Utc::now().to_rfc3339(),
    ))
}

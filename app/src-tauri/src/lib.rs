//! notifwire Tauri app — backend entry point.
//!
//! Wires the consumer pipeline (reconnecting SSE subscriber) into the Tauri
//! shell so the app can:
//!   - Connect to a producer URL via the `connect` command
//!   - Show WinRT toasts for each received notification
//!   - Emit a `notification` event to the frontend window
//!   - Report live connection health via `get_health`

use notifwire_consumer::{
    consumer_health, Pipeline, ReconnectPolicy, StatusHandle,
};
use notifwire_consumer_win::WindowsToastSink;
use notifwire_core::{
    ConsumerHealth, DisplayError, Notification, NotificationSink, Rules, SelfChecks,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

// ---------------------------------------------------------------------------
// TauriSink
// ---------------------------------------------------------------------------

/// Payload emitted to the frontend for every notification that passes the
/// pipeline filter.
#[derive(Debug, Clone, Serialize)]
struct NotificationPayload {
    title: String,
    body: String,
    app_name: String,
    timestamp_ms: i64,
}

/// A [`NotificationSink`] that:
/// 1. Fires a WinRT toast via [`WindowsToastSink`].
/// 2. Emits a `notification` Tauri event to all frontend windows.
///
/// `WindowsToastSink` is `Send` but not `Sync`, so we gate it behind a
/// `Mutex` so the struct can be `Send + Sync` (required by `AppHandle`).
struct TauriSink {
    toast: Mutex<WindowsToastSink>,
    app: AppHandle,
}

impl std::fmt::Debug for TauriSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TauriSink").finish_non_exhaustive()
    }
}

impl NotificationSink for TauriSink {
    fn show(&self, n: &Notification) -> Result<(), DisplayError> {
        // Best-effort toast: log if it fails but don't abort the pipeline.
        if let Err(e) = self.toast.lock().expect("toast mutex poisoned").show(n) {
            log::warn!("WinRT toast failed: {e}");
        }

        let now_ms = {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0)
        };

        let payload = NotificationPayload {
            title: n.title.clone(),
            body: n.body.clone(),
            app_name: n.app_name.clone(),
            timestamp_ms: now_ms,
        };

        if let Err(e) = self.app.emit("notification", &payload) {
            log::warn!("Tauri event emit failed: {e}");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Managed state shared across Tauri commands.
struct AppState {
    /// Handle to the currently-running consumer task, if any.
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// Status of the current producer connection (updated by the run loop).
    /// `None` when not connected.
    status: Mutex<Option<StatusHandle>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl AppState {
    fn new() -> Self {
        Self {
            task: Mutex::new(None),
            status: Mutex::new(None),
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Connect to a producer.  Aborts any existing task, then starts a new
/// reconnecting consumer loop in the background.
#[tauri::command]
fn connect(
    app: AppHandle,
    state: State<Arc<AppState>>,
    producer_url: String,
) -> Result<(), String> {
    // Abort any existing task first.
    abort_existing(&state);

    // Build the toast sink; surface init errors to the UI immediately.
    let toast = WindowsToastSink::new("com.notifwire.app", "notifwire")
        .map_err(|e| format!("toast sink init failed: {e}"))?;

    let sink = TauriSink {
        toast: Mutex::new(toast),
        app: app.clone(),
    };

    let pipeline = Pipeline::new(
        Rules::default(),
        5_000, // 5 s dedup window
        None,  // no history for now
        Box::new(sink),
    );

    let status = StatusHandle::new(&producer_url);

    // Store the status handle so get_health can read it.
    *state.status.lock().expect("status mutex poisoned") = Some(status.clone());

    let url = producer_url.clone();
    let state_clone = Arc::clone(&state);

    let handle = tauri::async_runtime::spawn(async move {
        let mut pipeline = pipeline;
        let result = notifwire_consumer::run_with_reconnect(
            &url,
            0,    // since: start at beginning / live
            true, // live: only-new mode
            &mut pipeline,
            &ReconnectPolicy::default(),
            &status,
        )
        .await;

        if let Err(e) = result {
            log::error!("consumer loop exited with error: {e}");
        }

        // Clear the task slot when the loop exits.
        *state_clone.task.lock().expect("task mutex poisoned") = None;
    });

    *state.task.lock().expect("task mutex poisoned") = Some(handle);
    Ok(())
}

/// Disconnect from the current producer.
#[tauri::command]
fn disconnect(state: State<Arc<AppState>>) -> Result<(), String> {
    abort_existing(&state);
    *state.status.lock().expect("status mutex poisoned") = None;
    Ok(())
}

/// Return the current consumer health (connection state + producer status).
#[tauri::command]
fn get_health(state: State<Arc<AppState>>) -> Result<ConsumerHealth, String> {
    let status_guard = state.status.lock().expect("status mutex poisoned");
    match &*status_guard {
        None => {
            // Not connected: return a trivially healthy consumer with no producers.
            let self_checks = SelfChecks {
                history_ok: true,
                pipeline_alive: false,
                detail: None,
            };
            Ok(consumer_health(self_checks, &[]))
        }
        Some(handle) => {
            let task_alive = state
                .task
                .lock()
                .expect("task mutex poisoned")
                .is_some();
            let self_checks = SelfChecks {
                history_ok: true,
                pipeline_alive: task_alive,
                detail: None,
            };
            Ok(consumer_health(self_checks, std::slice::from_ref(handle)))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn abort_existing(state: &State<Arc<AppState>>) {
    if let Some(handle) = state
        .task
        .lock()
        .expect("task mutex poisoned")
        .take()
    {
        handle.abort();
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = Arc::new(AppState::new());

    tauri::Builder::default()
        .manage(state)
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![connect, disconnect, get_health])
        .run(tauri::generate_context!())
        .expect("notifwire app failed to start");
}

//! notifwire Tauri app — backend entry point.
//!
//! Slice 3B: persistent producers list.
//!
//! - Loads `config.json` from `app_config_dir` on startup.
//! - Auto-connects all enabled producers.
//! - Exposes Tauri commands for CRUD on the producer list.
//! - `AppState` holds a per-URL map of (JoinHandle, StatusHandle).

use notifwire_consumer::{Pipeline, ReconnectPolicy, StatusHandle};
use notifwire_consumer_win::WindowsToastSink;
use notifwire_core::{DisplayError, Notification, NotificationSink, ProducerStatus, Rules};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// Config schema
// ---------------------------------------------------------------------------

/// One configured producer (persisted to `config.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProducerEntry {
    pub url: String,
    /// Optional friendly display name shown in the UI.
    pub label: Option<String>,
    pub enabled: bool,
}

/// Root of `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppConfig {
    producers: Vec<ProducerEntry>,
}

// ---------------------------------------------------------------------------
// Config I/O helpers
// ---------------------------------------------------------------------------

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("config.json"))
        .map_err(|e| format!("could not resolve app_config_dir: {e}"))
}

fn load_config(app: &AppHandle) -> Vec<ProducerEntry> {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("config path error: {e}");
            return vec![];
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<AppConfig>(&text)
            .map(|c| c.producers)
            .unwrap_or_else(|e| {
                log::warn!("config parse error (starting fresh): {e}");
                vec![]
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => {
            log::warn!("config read error: {e}");
            vec![]
        }
    }
}

fn save_config(app: &AppHandle, producers: &[ProducerEntry]) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create config dir failed: {e}"))?;
    }
    let cfg = AppConfig {
        producers: producers.to_vec(),
    };
    let text =
        serde_json::to_string_pretty(&cfg).map_err(|e| format!("config serialize failed: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("config write failed: {e}"))
}

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

/// Per-connection live data.
struct Connection {
    handle: tauri::async_runtime::JoinHandle<()>,
    status: StatusHandle,
}

/// Managed state shared across Tauri commands.
/// Keyed by producer URL.
#[derive(Default)]
struct AppState {
    connections: Mutex<HashMap<String, Connection>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Internal connect / disconnect helpers
// ---------------------------------------------------------------------------

/// Start a consumer task for `url`, replacing any existing one.
/// Returns an error string if the toast sink can't be initialised.
fn connect_one(app: &AppHandle, state: &Arc<AppState>, url: &str) -> Result<(), String> {
    // Abort any existing task for this URL.
    disconnect_one(state, url);

    let toast = WindowsToastSink::new("com.notifwire.app", "notifwire")
        .map_err(|e| format!("toast sink init failed: {e}"))?;

    let sink = TauriSink {
        toast: Mutex::new(toast),
        app: app.clone(),
    };

    let pipeline = Pipeline::new(Rules::default(), 5_000, None, Box::new(sink));
    let status = StatusHandle::new(url);
    let url_owned = url.to_owned();
    let state_clone = Arc::clone(state);
    let status_clone = status.clone();

    let handle = tauri::async_runtime::spawn(async move {
        let result = notifwire_consumer::run_with_reconnect(
            &url_owned,
            0,
            true,
            &mut { pipeline },
            &ReconnectPolicy::default(),
            &status_clone,
        )
        .await;

        if let Err(e) = result {
            log::error!("consumer loop ({url_owned}) exited with error: {e}");
        }

        // Remove ourselves from the map when the loop exits.
        state_clone
            .connections
            .lock()
            .expect("connections mutex poisoned")
            .remove(&url_owned);
    });

    state
        .connections
        .lock()
        .expect("connections mutex poisoned")
        .insert(url.to_owned(), Connection { handle, status });

    Ok(())
}

/// Abort and remove the task for `url` (no-op if not connected).
fn disconnect_one(state: &Arc<AppState>, url: &str) {
    if let Some(conn) = state
        .connections
        .lock()
        .expect("connections mutex poisoned")
        .remove(url)
    {
        conn.handle.abort();
    }
}

/// Connect all enabled producers from config at startup.
fn connect_all(app: &AppHandle, state: &Arc<AppState>) {
    for entry in load_config(app) {
        if entry.enabled {
            if let Err(e) = connect_one(app, state, &entry.url) {
                log::warn!("startup connect failed for {}: {e}", entry.url);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return all configured producers (from config file).
#[tauri::command]
fn get_producers(app: AppHandle) -> Vec<ProducerEntry> {
    load_config(&app)
}

/// Append a new producer, save config, and connect if enabled.
#[tauri::command]
fn add_producer(
    app: AppHandle,
    state: State<Arc<AppState>>,
    url: String,
    label: Option<String>,
) -> Result<(), String> {
    let mut producers = load_config(&app);
    let url = url.trim().to_owned();
    if url.is_empty() {
        return Err("URL must not be empty".into());
    }
    if producers.iter().any(|p| p.url == url) {
        return Err(format!("producer '{url}' already exists"));
    }
    producers.push(ProducerEntry {
        url: url.clone(),
        label,
        enabled: true,
    });
    save_config(&app, &producers)?;
    connect_one(&app, &state, &url)?;
    Ok(())
}

/// Remove a producer, save config, and disconnect it.
#[tauri::command]
fn remove_producer(app: AppHandle, state: State<Arc<AppState>>, url: String) -> Result<(), String> {
    let mut producers = load_config(&app);
    producers.retain(|p| p.url != url);
    save_config(&app, &producers)?;
    disconnect_one(&state, &url);
    Ok(())
}

/// Enable or disable a producer, save config, and connect/disconnect.
#[tauri::command]
fn set_producer_enabled(
    app: AppHandle,
    state: State<Arc<AppState>>,
    url: String,
    enabled: bool,
) -> Result<(), String> {
    let mut producers = load_config(&app);
    let entry = producers
        .iter_mut()
        .find(|p| p.url == url)
        .ok_or_else(|| format!("producer '{url}' not found"))?;
    entry.enabled = enabled;
    save_config(&app, &producers)?;
    if enabled {
        connect_one(&app, &state, &url)?;
    } else {
        disconnect_one(&state, &url);
    }
    Ok(())
}

/// Return the live status of every active connection.
/// The `url` field lets the frontend match entries back to config rows.
#[tauri::command]
fn get_health(state: State<Arc<AppState>>) -> Result<Vec<ProducerStatus>, String> {
    let connections = state
        .connections
        .lock()
        .expect("connections mutex poisoned");
    let statuses = connections.values().map(|c| c.status.get()).collect();
    Ok(statuses)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state: Arc<AppState> = Arc::new(AppState::default());
    let state_for_setup = Arc::clone(&state);

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            // Auto-connect all enabled producers from saved config.
            connect_all(app.handle(), &state_for_setup);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_producers,
            add_producer,
            remove_producer,
            set_producer_enabled,
            get_health,
        ])
        .run(tauri::generate_context!())
        .expect("notifwire app failed to start");
}

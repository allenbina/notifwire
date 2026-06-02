//! notifwire Tauri app — backend entry point.
//!
//! Slice 3C: rules persistence, seen-apps tracking, filters UI.
//!
//! - Loads `config.json` from `app_config_dir` on startup.
//! - Auto-connects all enabled producers with persisted rules.
//! - Exposes Tauri commands for CRUD on the producer list.
//! - Exposes Tauri commands for rules management.
//! - `AppState` holds a per-URL map of (JoinHandle, StatusHandle).

use notifwire_consumer::{Pipeline, ReconnectPolicy, StatusHandle};
use notifwire_consumer_win::WindowsToastSink;
use notifwire_core::{
    DefaultMode, DisplayError, Filter, FilterAction, MatchField, Notification, NotificationSink,
    ProducerStatus, Rules,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
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
    #[serde(default)]
    rules: Rules,
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

fn load_config(app: &AppHandle) -> AppConfig {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("config path error: {e}");
            return AppConfig::default();
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<AppConfig>(&text).unwrap_or_else(|e| {
            log::warn!("config parse error (starting fresh): {e}");
            AppConfig::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
        Err(e) => {
            log::warn!("config read error: {e}");
            AppConfig::default()
        }
    }
}

fn save_config(app: &AppHandle, cfg: &AppConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create config dir failed: {e}"))?;
    }
    let text =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("config serialize failed: {e}"))?;
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
/// 3. Records the `app_name` in the shared seen-apps set.
struct TauriSink {
    toast: Mutex<WindowsToastSink>,
    app: AppHandle,
    seen_apps: Arc<Mutex<BTreeSet<String>>>,
}

impl std::fmt::Debug for TauriSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TauriSink").finish_non_exhaustive()
    }
}

impl NotificationSink for TauriSink {
    fn show(&self, n: &Notification) -> Result<(), DisplayError> {
        // Track seen app names.
        self.seen_apps
            .lock()
            .expect("seen_apps mutex poisoned")
            .insert(n.app_name.clone());

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
struct AppState {
    connections: Mutex<HashMap<String, Connection>>,
    seen_apps: Arc<Mutex<BTreeSet<String>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            seen_apps: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }
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
fn connect_one(
    app: &AppHandle,
    state: &Arc<AppState>,
    url: &str,
    rules: Rules,
) -> Result<(), String> {
    // Abort any existing task for this URL.
    disconnect_one(state, url);

    let toast = WindowsToastSink::new("com.notifwire.app", "notifwire")
        .map_err(|e| format!("toast sink init failed: {e}"))?;

    let sink = TauriSink {
        toast: Mutex::new(toast),
        app: app.clone(),
        seen_apps: Arc::clone(&state.seen_apps),
    };

    let pipeline = Pipeline::new(rules, 5_000, None, Box::new(sink));
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
    let cfg = load_config(app);
    for entry in &cfg.producers {
        if entry.enabled {
            if let Err(e) = connect_one(app, state, &entry.url, cfg.rules.clone()) {
                log::warn!("startup connect failed for {}: {e}", entry.url);
            }
        }
    }
}

/// Disconnect and reconnect every enabled producer with the given rules.
/// Used after any rules change so the pipeline picks up the new configuration.
fn restart_all_connections(app: &AppHandle, state: &Arc<AppState>, rules: &Rules) {
    let cfg = load_config(app);
    for entry in &cfg.producers {
        if entry.enabled {
            if let Err(e) = connect_one(app, state, &entry.url, rules.clone()) {
                log::warn!("rules-restart connect failed for {}: {e}", entry.url);
            }
        } else {
            disconnect_one(state, &entry.url);
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return all configured producers (from config file).
#[tauri::command]
fn get_producers(app: AppHandle) -> Vec<ProducerEntry> {
    load_config(&app).producers
}

/// Append a new producer, save config, and connect if enabled.
#[tauri::command]
fn add_producer(
    app: AppHandle,
    state: State<Arc<AppState>>,
    url: String,
    label: Option<String>,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    let url = url.trim().to_owned();
    if url.is_empty() {
        return Err("URL must not be empty".into());
    }
    if cfg.producers.iter().any(|p| p.url == url) {
        return Err(format!("producer '{url}' already exists"));
    }
    cfg.producers.push(ProducerEntry {
        url: url.clone(),
        label,
        enabled: true,
    });
    let rules = cfg.rules.clone();
    save_config(&app, &cfg)?;
    connect_one(&app, &state, &url, rules)?;
    Ok(())
}

/// Remove a producer, save config, and disconnect it.
#[tauri::command]
fn remove_producer(app: AppHandle, state: State<Arc<AppState>>, url: String) -> Result<(), String> {
    let mut cfg = load_config(&app);
    cfg.producers.retain(|p| p.url != url);
    save_config(&app, &cfg)?;
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
    let mut cfg = load_config(&app);
    let entry = cfg
        .producers
        .iter_mut()
        .find(|p| p.url == url)
        .ok_or_else(|| format!("producer '{url}' not found"))?;
    entry.enabled = enabled;
    let rules = cfg.rules.clone();
    save_config(&app, &cfg)?;
    if enabled {
        connect_one(&app, &state, &url, rules)?;
    } else {
        disconnect_one(&state, &url);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rules commands
// ---------------------------------------------------------------------------

/// Return the current rules from config.
#[tauri::command]
fn get_rules(app: AppHandle) -> Rules {
    load_config(&app).rules
}

/// Return every app name seen in the current session (sorted).
#[tauri::command]
fn get_seen_apps(state: State<Arc<AppState>>) -> Vec<String> {
    state
        .seen_apps
        .lock()
        .expect("seen_apps mutex poisoned")
        .iter()
        .cloned()
        .collect()
}

/// Set the default mode: `"allow"` or `"block"`.
#[tauri::command]
fn set_default_mode(
    app: AppHandle,
    state: State<Arc<AppState>>,
    mode: String,
) -> Result<(), String> {
    let default_mode = parse_default_mode(&mode)?;
    let mut cfg = load_config(&app);
    cfg.rules.default_mode = default_mode;
    save_config(&app, &cfg)?;
    restart_all_connections(&app, &state, &cfg.rules);
    Ok(())
}

/// Set per-app allow (`enabled=true`) or block (`enabled=false`).
#[tauri::command]
fn set_app_rule(
    app: AppHandle,
    state: State<Arc<AppState>>,
    app_name: String,
    enabled: bool,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    cfg.rules.apps.insert(app_name, enabled);
    save_config(&app, &cfg)?;
    restart_all_connections(&app, &state, &cfg.rules);
    Ok(())
}

/// Remove the explicit per-app rule (falls back to default mode).
#[tauri::command]
fn remove_app_rule(
    app: AppHandle,
    state: State<Arc<AppState>>,
    app_name: String,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    cfg.rules.apps.remove(&app_name);
    save_config(&app, &cfg)?;
    restart_all_connections(&app, &state, &cfg.rules);
    Ok(())
}

/// Append a new keyword filter.
#[tauri::command]
fn add_filter(
    app: AppHandle,
    state: State<Arc<AppState>>,
    field: String,
    contains: String,
    action: String,
) -> Result<(), String> {
    let field = parse_match_field(&field)?;
    let action = parse_filter_action(&action)?;
    let contains = contains.trim().to_owned();
    if contains.is_empty() {
        return Err("filter keyword must not be empty".into());
    }
    let mut cfg = load_config(&app);
    cfg.rules.filters.push(Filter {
        field,
        contains,
        action,
    });
    save_config(&app, &cfg)?;
    restart_all_connections(&app, &state, &cfg.rules);
    Ok(())
}

/// Remove a keyword filter by index.
#[tauri::command]
fn remove_filter(app: AppHandle, state: State<Arc<AppState>>, index: usize) -> Result<(), String> {
    let mut cfg = load_config(&app);
    if index >= cfg.rules.filters.len() {
        return Err(format!(
            "filter index {index} out of range (len={})",
            cfg.rules.filters.len()
        ));
    }
    cfg.rules.filters.remove(index);
    save_config(&app, &cfg)?;
    restart_all_connections(&app, &state, &cfg.rules);
    Ok(())
}

// ---------------------------------------------------------------------------
// Enum parsing helpers
// ---------------------------------------------------------------------------

fn parse_default_mode(s: &str) -> Result<DefaultMode, String> {
    match s {
        "allow" => Ok(DefaultMode::Allow),
        "block" => Ok(DefaultMode::Block),
        other => Err(format!(
            "unknown default_mode '{other}'; expected allow|block"
        )),
    }
}

fn parse_match_field(s: &str) -> Result<MatchField, String> {
    match s {
        "title" => Ok(MatchField::Title),
        "body" => Ok(MatchField::Body),
        "appname" => Ok(MatchField::AppName),
        "any" => Ok(MatchField::Any),
        other => Err(format!(
            "unknown field '{other}'; expected title|body|appname|any"
        )),
    }
}

fn parse_filter_action(s: &str) -> Result<FilterAction, String> {
    match s {
        "allow" => Ok(FilterAction::Allow),
        "block" => Ok(FilterAction::Block),
        other => Err(format!("unknown action '{other}'; expected allow|block")),
    }
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
            get_rules,
            get_seen_apps,
            set_default_mode,
            set_app_rule,
            remove_app_rule,
            add_filter,
            remove_filter,
        ])
        .run(tauri::generate_context!())
        .expect("notifwire app failed to start");
}

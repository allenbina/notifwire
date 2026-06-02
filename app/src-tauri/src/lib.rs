//! notifwire Tauri app — backend entry point.
//!
//! Slice 3F: Focus Schedules — time-based automatic focus switching.
//!
//! - Loads `config.json` from `app_config_dir` on startup.
//! - Auto-connects all enabled producers with persisted rules.
//! - Each pipeline opens its own History connection to the shared DB file.
//! - Exposes Tauri commands for CRUD on the producer list.
//! - Exposes Tauri commands for rules management.
//! - Exposes Tauri commands for history queries and retention settings.
//! - Exposes Tauri commands for focus CRUD and active-focus management.
//! - Exposes Tauri commands for schedule CRUD and schedule evaluation.
//! - `AppState` holds a per-URL map of (JoinHandle, StatusHandle).

use notifwire_consumer::{History, Pipeline, ReconnectPolicy, StatusHandle};
use notifwire_consumer_win::WindowsToastSink;
use notifwire_core::{
    DefaultMode, DisplayError, Filter, FilterAction, MatchField, Notification, NotificationSink,
    ProducerStatus, Rules,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
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

/// Retention configuration: how long to keep notifications per-global and per-producer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// Default retention in days. Default: 30.
    pub default_days: u32,
    /// Optional global cap on number of stored notifications.
    pub max_count: Option<u32>,
    /// Per-producer URL overrides: producer URL → retention days.
    pub per_producer: HashMap<String, u32>,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            default_days: 30,
            max_count: None,
            per_producer: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Focus types
// ---------------------------------------------------------------------------

/// Day-of-week enum for schedule matching.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

/// A half-open time range in HHMM notation (local time).
/// If `end_hhmm < start_hhmm` the range wraps midnight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    /// e.g. 2200 = 22:00
    pub start_hhmm: u16,
    /// e.g. 700 = 07:00
    pub end_hhmm: u16,
}

/// A saved schedule: activate `focus_id` on `days` during `time_range`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusSchedule {
    pub id: String,
    /// ID of the [`Focus`] to activate.
    pub focus_id: String,
    /// Days this schedule applies to.
    pub days: Vec<Weekday>,
    pub time_range: TimeRange,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Schedule helpers (pure, not Tauri commands)
// ---------------------------------------------------------------------------

/// Parse a weekday string (same variants as [`Weekday`] serde) into the enum.
fn parse_weekday(s: &str) -> Result<Weekday, String> {
    match s {
        "mon" => Ok(Weekday::Mon),
        "tue" => Ok(Weekday::Tue),
        "wed" => Ok(Weekday::Wed),
        "thu" => Ok(Weekday::Thu),
        "fri" => Ok(Weekday::Fri),
        "sat" => Ok(Weekday::Sat),
        "sun" => Ok(Weekday::Sun),
        other => Err(format!(
            "unknown weekday '{other}'; expected mon|tue|wed|thu|fri|sat|sun"
        )),
    }
}

/// Return true if `hhmm` (e.g. 2230 = 22:30) falls inside `range`.
/// Handles midnight-wrap when `end < start`.
fn hhmm_in_range(hhmm: u16, range: &TimeRange) -> bool {
    let s = range.start_hhmm;
    let e = range.end_hhmm;
    if e < s {
        // Wraps midnight: in range if >= start OR < end
        hhmm >= s || hhmm < e
    } else {
        hhmm >= s && hhmm < e
    }
}

/// Find the first enabled schedule whose day + time range matches the supplied
/// local weekday + HHMM, and return its `focus_id`.  Returns `None` if nothing
/// matches.  The frontend calls this every minute so we keep it allocation-light.
pub fn active_scheduled_focus(
    schedules: &[FocusSchedule],
    _focuses: &[Focus],
    weekday: &Weekday,
    hhmm: u16,
) -> Option<String> {
    schedules
        .iter()
        .filter(|s| s.enabled && s.days.contains(weekday) && hhmm_in_range(hhmm, &s.time_range))
        .map(|s| s.focus_id.clone())
        .next()
}

/// Generate a stable local ID from a monotonic counter mixed with a
/// fixed multiplier. No external crate needed — good enough for local IDs.
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    format!(
        "f{:016x}",
        n.wrapping_mul(0x9e3779b97f4a7c15)
            .wrapping_add(0xdead_beef_cafe_0000)
    )
}

/// A named filter profile.  The "All" focus is synthetic — the frontend
/// adds it; only custom focuses are stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Focus {
    /// Stable local ID produced by [`new_id`].
    pub id: String,
    pub name: String,
    /// Emoji or short decorative string shown next to the name.
    pub icon: Option<String>,
    /// Rules that gate which notifications this focus shows.
    pub rules: Rules,
    /// Lower value → higher in the list.
    pub sort_order: u32,
}

/// Root of `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppConfig {
    producers: Vec<ProducerEntry>,
    #[serde(default)]
    rules: Rules,
    #[serde(default)]
    retention: RetentionConfig,
    #[serde(default)]
    focuses: Vec<Focus>,
    /// `None` means the built-in "All" focus is active.
    #[serde(default)]
    active_focus_id: Option<String>,
    #[serde(default)]
    schedules: Vec<FocusSchedule>,
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
    /// Path to the shared history SQLite database.
    /// Each pipeline opens its own connection to this file.
    history_db_path: Mutex<Option<PathBuf>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            seen_apps: Arc::new(Mutex::new(BTreeSet::new())),
            history_db_path: Mutex::new(None),
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

    // Open a per-pipeline History connection to the shared DB file.
    // SQLite supports multiple connections to the same WAL-mode file.
    let history = state
        .history_db_path
        .lock()
        .expect("history_db_path mutex poisoned")
        .as_ref()
        .and_then(|p| {
            History::open(p)
                .map_err(|e| log::warn!("history open failed for {url}: {e}"))
                .ok()
        });

    let pipeline = Pipeline::new(rules, 5_000, history, Box::new(sink));
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

/// Resolve and initialise the history DB path in `AppState`. Called once at startup.
fn init_history_db(app: &AppHandle, state: &Arc<AppState>) {
    let db_path = match app
        .path()
        .app_data_dir()
        .map(|d| d.join("history.db"))
        .map_err(|e| format!("app_data_dir error: {e}"))
    {
        Ok(p) => p,
        Err(e) => {
            log::warn!("history DB path unavailable: {e}");
            return;
        }
    };

    if let Some(parent) = db_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("could not create data dir for history DB: {e}");
            return;
        }
    }

    *state
        .history_db_path
        .lock()
        .expect("history_db_path mutex poisoned") = Some(db_path);
}

/// Run initial pruning according to retention config. Call after `init_history_db`.
fn prune_with_config(state: &Arc<AppState>, retention: &RetentionConfig) -> usize {
    let path = state
        .history_db_path
        .lock()
        .expect("history_db_path mutex poisoned")
        .clone();
    let path = match path {
        Some(p) => p,
        None => return 0,
    };

    let history = match History::open(&path) {
        Ok(h) => h,
        Err(e) => {
            log::warn!("prune: could not open history DB: {e}");
            return 0;
        }
    };

    let mut total = 0usize;

    // Use the minimum configured days across default + all per-producer overrides
    // as a safe global cutoff for now.
    let min_days = retention
        .per_producer
        .values()
        .copied()
        .fold(retention.default_days, |a, b| a.min(b));

    let now_ms = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };
    let cutoff_ms = now_ms - (min_days as i64) * 86_400 * 1_000;

    match history.prune_older_than_ms(cutoff_ms) {
        Ok(n) => total += n,
        Err(e) => log::warn!("prune_older_than_ms failed: {e}"),
    }

    if let Some(max) = retention.max_count {
        match history.prune_to_count(max as usize) {
            Ok(n) => total += n,
            Err(e) => log::warn!("prune_to_count failed: {e}"),
        }
    }

    total
}

/// Connect all enabled producers from config at startup.
fn connect_all(app: &AppHandle, state: &Arc<AppState>) {
    let cfg = load_config(app);

    // Initialize history DB and run initial prune before connecting.
    init_history_db(app, state);
    let pruned = prune_with_config(state, &cfg.retention);
    if pruned > 0 {
        log::info!("startup prune removed {pruned} history rows");
    }

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
// History commands
// ---------------------------------------------------------------------------

/// Query the history DB. Opens a fresh read connection to the shared file.
#[tauri::command]
fn get_history(
    state: State<Arc<AppState>>,
    app_name: Option<String>,
    limit: usize,
    offset: usize,
) -> Result<Vec<Notification>, String> {
    let path = state
        .history_db_path
        .lock()
        .expect("history_db_path mutex poisoned")
        .clone();
    let path = match path {
        Some(p) => p,
        None => return Ok(vec![]),
    };
    let history = History::open(&path).map_err(|e| format!("history open failed: {e}"))?;
    history
        .query_filtered(app_name.as_deref(), None, limit, offset)
        .map_err(|e| format!("history query failed: {e}"))
}

/// Return the current retention config.
#[tauri::command]
fn get_retention(app: AppHandle) -> RetentionConfig {
    load_config(&app).retention
}

/// Update the global default retention days, save config, and re-prune.
#[tauri::command]
fn set_retention_default(
    app: AppHandle,
    state: State<Arc<AppState>>,
    days: u32,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    cfg.retention.default_days = days;
    save_config(&app, &cfg)?;
    prune_with_config(&state, &cfg.retention);
    Ok(())
}

/// Update the max_count cap, save config, and re-prune.
#[tauri::command]
fn set_retention_max_count(
    app: AppHandle,
    state: State<Arc<AppState>>,
    max_count: Option<u32>,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    cfg.retention.max_count = max_count;
    save_config(&app, &cfg)?;
    prune_with_config(&state, &cfg.retention);
    Ok(())
}

/// Set a per-producer override retention days, save config, and re-prune.
#[tauri::command]
fn set_retention_producer(
    app: AppHandle,
    state: State<Arc<AppState>>,
    producer_url: String,
    days: u32,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    cfg.retention.per_producer.insert(producer_url, days);
    save_config(&app, &cfg)?;
    prune_with_config(&state, &cfg.retention);
    Ok(())
}

/// Remove a per-producer override, save config.
#[tauri::command]
fn remove_retention_producer(
    app: AppHandle,
    _state: State<Arc<AppState>>,
    producer_url: String,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    cfg.retention.per_producer.remove(&producer_url);
    save_config(&app, &cfg)?;
    Ok(())
}

/// Manually trigger a prune. Returns total rows deleted.
#[tauri::command]
fn prune_now(app: AppHandle, state: State<Arc<AppState>>) -> Result<usize, String> {
    let cfg = load_config(&app);
    Ok(prune_with_config(&state, &cfg.retention))
}

// ---------------------------------------------------------------------------
// Focus commands
// ---------------------------------------------------------------------------

/// Return all stored custom focuses (the "All" built-in is added by the frontend).
#[tauri::command]
fn get_focuses(app: AppHandle) -> Vec<Focus> {
    load_config(&app).focuses
}

/// Return the currently active focus id (`None` = built-in "All").
#[tauri::command]
fn get_active_focus(app: AppHandle) -> Option<String> {
    load_config(&app).active_focus_id
}

/// Create a new focus with empty rules, append it to the list, save, and
/// return the new [`Focus`].
#[tauri::command]
fn add_focus(app: AppHandle, name: String, icon: Option<String>) -> Result<Focus, String> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err("focus name must not be empty".into());
    }
    let mut cfg = load_config(&app);
    let sort_order = cfg.focuses.len() as u32;
    let focus = Focus {
        id: new_id(),
        name,
        icon,
        rules: Rules::default(),
        sort_order,
    };
    cfg.focuses.push(focus.clone());
    save_config(&app, &cfg)?;
    Ok(focus)
}

/// Rename or re-icon an existing focus.
#[tauri::command]
fn update_focus(
    app: AppHandle,
    id: String,
    name: Option<String>,
    icon: Option<String>,
) -> Result<(), String> {
    let mut cfg = load_config(&app);
    let focus = cfg
        .focuses
        .iter_mut()
        .find(|f| f.id == id)
        .ok_or_else(|| format!("focus '{id}' not found"))?;
    if let Some(n) = name {
        let n = n.trim().to_owned();
        if n.is_empty() {
            return Err("focus name must not be empty".into());
        }
        focus.name = n;
    }
    // A `None` icon argument means "clear the icon"; use a sentinel Option<Option<String>>
    // would complicate the API.  Instead we always update if the caller passes icon.
    // To keep it simple: icon=None here means "don't touch it".  Frontend passes
    // `Some("")` to clear.
    if let Some(ic) = icon {
        focus.icon = if ic.trim().is_empty() { None } else { Some(ic) };
    }
    save_config(&app, &cfg)
}

/// Deep-copy a focus with a new id and " (copy)" appended to the name.
#[tauri::command]
fn clone_focus(app: AppHandle, id: String) -> Result<Focus, String> {
    let mut cfg = load_config(&app);
    let src = cfg
        .focuses
        .iter()
        .find(|f| f.id == id)
        .ok_or_else(|| format!("focus '{id}' not found"))?
        .clone();
    let sort_order = cfg.focuses.len() as u32;
    let cloned = Focus {
        id: new_id(),
        name: format!("{} (copy)", src.name),
        icon: src.icon.clone(),
        rules: src.rules.clone(),
        sort_order,
    };
    cfg.focuses.push(cloned.clone());
    save_config(&app, &cfg)?;
    Ok(cloned)
}

/// Delete a focus.  If it was active, reset active_focus_id to None ("All").
#[tauri::command]
fn remove_focus(app: AppHandle, id: String) -> Result<(), String> {
    let mut cfg = load_config(&app);
    let before = cfg.focuses.len();
    cfg.focuses.retain(|f| f.id != id);
    if cfg.focuses.len() == before {
        return Err(format!("focus '{id}' not found"));
    }
    if cfg.active_focus_id.as_deref() == Some(&id) {
        cfg.active_focus_id = None;
    }
    save_config(&app, &cfg)
}

/// Replace the rules for a focus.
#[tauri::command]
fn set_focus_rules(app: AppHandle, id: String, rules: Rules) -> Result<(), String> {
    let mut cfg = load_config(&app);
    let focus = cfg
        .focuses
        .iter_mut()
        .find(|f| f.id == id)
        .ok_or_else(|| format!("focus '{id}' not found"))?;
    focus.rules = rules;
    save_config(&app, &cfg)
}

/// Set the active focus.  `None` activates the built-in "All" focus.
#[tauri::command]
fn set_active_focus(app: AppHandle, id: Option<String>) -> Result<(), String> {
    let mut cfg = load_config(&app);
    if let Some(ref focus_id) = id {
        if !cfg.focuses.iter().any(|f| &f.id == focus_id) {
            return Err(format!("focus '{focus_id}' not found"));
        }
    }
    cfg.active_focus_id = id;
    save_config(&app, &cfg)
}

/// Rewrite sort_order for all focuses based on the supplied ordered id list.
/// Focuses not in `ids` keep their existing sort_order values (they appear after).
#[tauri::command]
fn reorder_focuses(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    let mut cfg = load_config(&app);
    for (pos, id) in ids.iter().enumerate() {
        if let Some(f) = cfg.focuses.iter_mut().find(|f| &f.id == id) {
            f.sort_order = pos as u32;
        }
    }
    // Re-sort the stored vec for consistency.
    cfg.focuses.sort_by_key(|f| f.sort_order);
    save_config(&app, &cfg)
}

// ---------------------------------------------------------------------------
// Schedule commands
// ---------------------------------------------------------------------------

/// Return all saved focus schedules.
#[tauri::command]
fn get_schedules(app: AppHandle) -> Vec<FocusSchedule> {
    load_config(&app).schedules
}

/// Create a new schedule.  `days` is a list of strings like `["mon","fri"]`.
#[tauri::command]
fn add_schedule(
    app: AppHandle,
    focus_id: String,
    days: Vec<String>,
    start_hhmm: u16,
    end_hhmm: u16,
) -> Result<FocusSchedule, String> {
    if focus_id.is_empty() {
        return Err("focus_id must not be empty".into());
    }
    let parsed_days = days
        .iter()
        .map(|d| parse_weekday(d))
        .collect::<Result<Vec<_>, _>>()?;
    if parsed_days.is_empty() {
        return Err("days must not be empty".into());
    }
    let mut cfg = load_config(&app);
    if !cfg.focuses.iter().any(|f| f.id == focus_id) {
        return Err(format!("focus '{focus_id}' not found"));
    }
    let schedule = FocusSchedule {
        id: new_id(),
        focus_id,
        days: parsed_days,
        time_range: TimeRange {
            start_hhmm,
            end_hhmm,
        },
        enabled: true,
    };
    cfg.schedules.push(schedule.clone());
    save_config(&app, &cfg)?;
    Ok(schedule)
}

/// Update all mutable fields of an existing schedule.
#[tauri::command]
fn update_schedule(
    app: AppHandle,
    id: String,
    focus_id: String,
    days: Vec<String>,
    start_hhmm: u16,
    end_hhmm: u16,
    enabled: bool,
) -> Result<(), String> {
    let parsed_days = days
        .iter()
        .map(|d| parse_weekday(d))
        .collect::<Result<Vec<_>, _>>()?;
    if parsed_days.is_empty() {
        return Err("days must not be empty".into());
    }
    let mut cfg = load_config(&app);
    if !focus_id.is_empty() && !cfg.focuses.iter().any(|f| f.id == focus_id) {
        return Err(format!("focus '{focus_id}' not found"));
    }
    let sched = cfg
        .schedules
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("schedule '{id}' not found"))?;
    sched.focus_id = focus_id;
    sched.days = parsed_days;
    sched.time_range = TimeRange {
        start_hhmm,
        end_hhmm,
    };
    sched.enabled = enabled;
    save_config(&app, &cfg)
}

/// Delete a schedule by id.
#[tauri::command]
fn remove_schedule(app: AppHandle, id: String) -> Result<(), String> {
    let mut cfg = load_config(&app);
    let before = cfg.schedules.len();
    cfg.schedules.retain(|s| s.id != id);
    if cfg.schedules.len() == before {
        return Err(format!("schedule '{id}' not found"));
    }
    save_config(&app, &cfg)
}

/// Evaluate schedules against the caller-supplied local weekday + HHMM.
/// Returns the matching `focus_id`, or `null` if nothing matches.
/// Frontend calls this on mount and every 60 seconds.
#[tauri::command]
fn get_scheduled_focus(app: AppHandle, weekday: String, hhmm: u16) -> Option<String> {
    let weekday = parse_weekday(&weekday).ok()?;
    let cfg = load_config(&app);
    active_scheduled_focus(&cfg.schedules, &cfg.focuses, &weekday, hhmm)
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
            get_history,
            get_retention,
            set_retention_default,
            set_retention_max_count,
            set_retention_producer,
            remove_retention_producer,
            prune_now,
            get_focuses,
            get_active_focus,
            add_focus,
            update_focus,
            clone_focus,
            remove_focus,
            set_focus_rules,
            set_active_focus,
            reorder_focuses,
            get_schedules,
            add_schedule,
            update_schedule,
            remove_schedule,
            get_scheduled_focus,
        ])
        .run(tauri::generate_context!())
        .expect("notifwire app failed to start");
}

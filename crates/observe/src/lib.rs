//! Shared observability for notifwire.
//!
//! One logging facade — [`tracing`] — across every crate, initialized once per
//! binary by [`init`]. Library crates just call the `tracing::{info,warn,error}`
//! macros; they make no decision about where logs go. Each binary (and, later,
//! the Tauri app) calls [`init`] at startup, which fans events out to **stderr**
//! and a **rotating daily file** under [`log_dir`], filterable via `RUST_LOG`.
//!
//! ```no_run
//! // Hold the guard for the whole program; dropping it flushes buffered logs.
//! let _log = notifwire_observe::init("producer");
//! ```
//!
//! The file output is what the in-app log viewer tails, and [`log_dir`] is the
//! single source of truth for "where are the logs" across the codebase.

use std::path::PathBuf;

use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

/// Keeps the background log-writer thread alive. Hold it for the lifetime of the
/// program — dropping it flushes any buffered log lines to disk.
pub struct LogGuard {
    _file: tracing_appender::non_blocking::WorkerGuard,
}

impl std::fmt::Debug for LogGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The inner WorkerGuard isn't Debug; nothing useful to show anyway.
        f.debug_struct("LogGuard").finish_non_exhaustive()
    }
}

/// Initialize logging for a binary: stderr + a rotating daily file
/// (`<component>.log` in [`log_dir`]). `component` names the binary (e.g.
/// `"producer"`, `"consumer"`) and becomes the log filename prefix.
///
/// Level is controlled by `RUST_LOG` (e.g. `RUST_LOG=debug`,
/// `RUST_LOG=notifwire_transport=trace`), defaulting to `info`. Calling this more
/// than once in a process is a no-op for the global subscriber (the first call
/// wins); the returned guard is still valid to hold.
pub fn init(component: &str) -> LogGuard {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let dir = log_dir();
    // Best-effort: if we can't create the log dir, the file appender will fail
    // to write but stderr logging still works — we never want logging setup to
    // take down the program.
    let _ = std::fs::create_dir_all(&dir);
    let appender = tracing_appender::rolling::daily(&dir, format!("{component}.log"));
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let stderr_layer = fmt::layer().with_target(false).with_writer(std::io::stderr);
    let file_layer = fmt::layer().with_ansi(false).with_writer(writer);

    let installed = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init()
        .is_ok();

    if installed {
        tracing::info!(component, log_dir = %dir.display(), "logging initialized");
    }
    LogGuard { _file: guard }
}

/// Directory where notifwire writes its rotating log files. Per-OS data dir
/// (e.g. `%LOCALAPPDATA%\notifwire\data\logs` on Windows,
/// `~/.local/share/notifwire/logs` on Linux), falling back to a temp dir if the
/// platform dirs can't be resolved. The log viewer and disk/health checks read
/// from here, so this is the one place that defines the location.
pub fn log_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "notifwire")
        .map(|d| d.data_local_dir().join("logs"))
        .unwrap_or_else(|| std::env::temp_dir().join("notifwire").join("logs"))
}

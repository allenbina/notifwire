//! notifwire-consumer — subscribe to a producer, filter, dedup, and display.
//!
//! ```text
//! notifwire-consumer --producer http://127.0.0.1:8787                 # print
//! notifwire-consumer --producer http://127.0.0.1:8787 --display-windows --history hist.db
//! ```

use anyhow::Result;
use clap::Parser;
use notifwire_consumer::{format_notification, run_with_pipeline, History, Pipeline};
use notifwire_consumer_win::WindowsToastSink;
use notifwire_core::{DisplayError, Notification, NotificationSink, Rules};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "notifwire-consumer",
    version,
    about = "Subscribe to a notifwire producer; filter, dedup, and display"
)]
struct Cli {
    /// Producer base URL.
    #[arg(long)]
    producer: String,

    /// Resume from this cursor (0 = from the start of the producer's buffer).
    #[arg(long, default_value_t = 0)]
    since: u64,

    /// Show only NEW notifications, skipping the producer's backlog. Recommended
    /// for display so the consumer doesn't re-toast everything on connect.
    #[arg(long)]
    live: bool,

    /// Show notifications as native Windows toasts (default: print to stdout).
    #[arg(long)]
    display_windows: bool,

    /// Persist received notifications to this SQLite history file.
    #[arg(long)]
    history: Option<PathBuf>,

    /// Dedup window in ms (identical notifications shown at most once per window).
    #[arg(long, default_value_t = 60_000)]
    dedup_window_ms: i64,

    /// App name to suppress (repeatable). Handy to block notifwire's own toasts
    /// when capturing and displaying on the same machine.
    #[arg(long = "block-app")]
    block_apps: Vec<String>,
}

// Single-threaded runtime so the WinRT display calls stay on one thread.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut rules = Rules::default();
    for app in &cli.block_apps {
        rules.apps.insert(app.clone(), false);
    }

    let history = match &cli.history {
        Some(path) => Some(History::open(path)?),
        None => None,
    };

    let sink: Box<dyn NotificationSink> = if cli.display_windows {
        Box::new(
            WindowsToastSink::new("notifwire", "notifwire")
                .map_err(|e| anyhow::anyhow!("starting Windows toast display: {e}"))?,
        )
    } else {
        Box::new(|n: &Notification| {
            println!("{}", format_notification(n));
            Ok::<(), DisplayError>(())
        })
    };

    println!(
        "subscribing to {} from cursor {} (display: {})",
        cli.producer,
        cli.since,
        if cli.display_windows {
            "windows toasts"
        } else {
            "stdout"
        }
    );

    let pipeline = Pipeline::new(rules, cli.dedup_window_ms, history, sink);
    run_with_pipeline(&cli.producer, cli.since, cli.live, pipeline).await
}

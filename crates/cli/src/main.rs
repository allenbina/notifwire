//! `notifwire-send` — inject a notification into a local notifwire node over
//! its localhost ingest API, with no OS-capture code in the path. The harness
//! the whole pipeline is tested against, and a handy CLI in its own right:
//!
//! ```text
//! notifwire-send "Backup complete" --app rsync --priority high --icon rsync
//! notifwire-send --json ./notification.json
//! ```

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use notifwire_cli::{build_notification, send, SendOpts};
use notifwire_core::{Notification, Priority};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "notifwire-send",
    version,
    about = "Inject a notification into a local notifwire node"
)]
struct Cli {
    /// Notification title (omit when using --json).
    title: Option<String>,

    /// Notification body.
    #[arg(long, default_value = "")]
    body: String,

    /// App name the notification is attributed to.
    #[arg(long, default_value = "notifwire-send")]
    app: String,

    /// Delivery priority.
    #[arg(long, value_enum)]
    priority: Option<PriorityArg>,

    /// Icon reference.
    #[arg(long)]
    icon: Option<String>,

    /// Producer node id stamped on the notification.
    #[arg(long, default_value = "notifwire-send")]
    from: String,

    /// Node ingest base URL.
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    node: String,

    /// Read a full normalized Notification JSON from this file instead of
    /// building one from the flags above.
    #[arg(long, conflicts_with_all = ["title", "body", "app", "priority", "icon", "from"])]
    json: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum PriorityArg {
    Low,
    Normal,
    High,
    Urgent,
}

impl From<PriorityArg> for Priority {
    fn from(p: PriorityArg) -> Self {
        match p {
            PriorityArg::Low => Priority::Low,
            PriorityArg::Normal => Priority::Normal,
            PriorityArg::High => Priority::High,
            PriorityArg::Urgent => Priority::Urgent,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let notification = if let Some(path) = &cli.json {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str::<Notification>(&raw)
            .with_context(|| format!("parsing {} as a Notification", path.display()))?
    } else {
        let title = cli
            .title
            .clone()
            .context("a title is required (or pass --json <file>)")?;
        build_notification(&SendOpts {
            title,
            body: cli.body.clone(),
            app: cli.app.clone(),
            priority: cli.priority.map(Into::into),
            icon: cli.icon.clone(),
            producer_node: cli.from.clone(),
        })
    };

    let seq = send(&cli.node, &notification)?;
    println!("sent seq={seq} to {}", cli.node);
    Ok(())
}

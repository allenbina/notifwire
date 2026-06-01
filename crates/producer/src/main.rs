//! notifwire-producer — run a producer node.
//!
//! Serves the mesh transport (SSE `GET /events`) and the localhost injection
//! endpoint (`POST /ingest`). For D1-1 there is no OS capture yet, so the
//! outbox starts empty and is fed by `notifwire-send` (or, later, the Windows
//! capture bridge). This is what turns the in-process server into a real node
//! you can run and point consumers at.
//!
//! ```text
//! notifwire-producer --bind 127.0.0.1:8787
//! # then, elsewhere:
//! notifwire-send "hello" --node http://127.0.0.1:8787
//! notifwire-consumer --producer http://127.0.0.1:8787
//! ```

use anyhow::Result;
use clap::Parser;
use notifwire_core::NotificationSource;
use notifwire_producer_win::WindowsNotificationSource;
use notifwire_transport::{MeshProducer, SseServer};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "notifwire-producer",
    version,
    about = "Run a notifwire producer node (SSE events + localhost ingest)"
)]
struct Cli {
    /// Address to bind, host:port. Keep it on 127.0.0.1 unless you intend to
    /// expose the node; how it's reached (Tailscale, tunnel, port-forward) is
    /// up to the operator.
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,

    /// Max notifications retained in the catch-up outbox.
    #[arg(long, default_value_t = 1000)]
    capacity: usize,

    /// Persist the outbox to this file so buffered notifications and the
    /// catch-up cursor survive a restart. Omit for in-memory only.
    #[arg(long)]
    persist: Option<PathBuf>,

    /// Capture live Windows toast notifications into this node (WinRT). Requires
    /// the packaged build with notification-access granted — see
    /// docs/windows-notification-capture.md.
    #[arg(long)]
    capture_windows: bool,

    /// Node id stamped on captured notifications.
    #[arg(long, default_value = "windows")]
    node_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let server = match &cli.persist {
        Some(path) => SseServer::with_persistence(cli.capacity, path),
        None => SseServer::new(cli.capacity),
    };
    let (addr, serve) = server.bind(&cli.bind).await?;

    println!(
        "notifwire-producer listening on http://{addr} (outbox capacity {})",
        cli.capacity
    );
    if let Some(path) = &cli.persist {
        println!("  persisting outbox to {}", path.display());
    }
    println!("  subscribe : GET  http://{addr}/events?since=<cursor>");
    println!("  ingest    : POST http://{addr}/ingest");
    println!("  e.g.      : notifwire-send \"hello\" --node http://{addr}");

    if cli.capture_windows {
        // Pump captured Windows toasts into this node's outbox/stream.
        let mut source = WindowsNotificationSource::start(cli.node_id.clone())
            .map_err(|e| anyhow::anyhow!("starting Windows capture: {e}"))?;
        let producer = server.producer();
        println!(
            "  capturing : Windows toasts via {} (node id: {})",
            source.name(),
            cli.node_id
        );
        tokio::spawn(async move {
            loop {
                match source.next().await {
                    Ok(Some(n)) => {
                        producer.publish(n);
                    }
                    Ok(None) => break, // capture source ended (e.g. access not granted)
                    Err(e) => {
                        eprintln!("notifwire: capture error: {e}");
                        break;
                    }
                }
            }
        });
    }

    serve.await;
    Ok(())
}

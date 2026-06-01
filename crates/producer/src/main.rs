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
use notifwire_transport::SseServer;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let server = SseServer::new(cli.capacity);
    let (addr, serve) = server.bind(&cli.bind).await?;

    println!(
        "notifwire-producer listening on http://{addr} (outbox capacity {})",
        cli.capacity
    );
    println!("  subscribe : GET  http://{addr}/events?since=<cursor>");
    println!("  ingest    : POST http://{addr}/ingest");
    println!("  e.g.      : notifwire-send \"hello\" --node http://{addr}");

    serve.await;
    Ok(())
}

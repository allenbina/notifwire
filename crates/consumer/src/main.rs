//! notifwire-consumer — subscribe to a producer and print what it sends.
//!
//! ```text
//! notifwire-consumer --producer http://mac.allenbina.uk:8787
//! notifwire-consumer --producer http://127.0.0.1:8787 --since 42
//! ```

use anyhow::Result;
use clap::Parser;
use notifwire_consumer::{format_line, run};

#[derive(Parser, Debug)]
#[command(
    name = "notifwire-consumer",
    version,
    about = "Subscribe to a notifwire producer and print what it sends"
)]
struct Cli {
    /// Producer base URL.
    #[arg(long)]
    producer: String,

    /// Resume from this cursor (0 = from the start of the producer's buffer).
    #[arg(long, default_value_t = 0)]
    since: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    println!("subscribing to {} from cursor {}", cli.producer, cli.since);
    run(&cli.producer, cli.since, |seq, n| {
        println!("{}", format_line(seq, n));
    })
    .await
}

//! `notifwire-send` — inject a notification into a local node over the
//! localhost ingest API, with no OS-capture code in the path.
//!
//! This is the harness the whole pipeline is tested against. Flag parsing,
//! JSON-on-stdin, and the ingest POST land in D0-6; this is a D0-1 stub.

fn main() -> anyhow::Result<()> {
    println!(
        "notifwire-send {} (stub — see D0-6)",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

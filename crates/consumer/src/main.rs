//! notifwire consumer.
//!
//! Subscribes to one or more producers over the mesh transport. For the D0
//! walking skeleton it just connects and prints what it receives, which is
//! enough to prove the end-to-end loop (D0-7). The native display, filters,
//! dedup, icons, and history come in D2.
//!
//! This is a D0-1 stub.

fn main() -> anyhow::Result<()> {
    println!(
        "notifwire-consumer {} (stub — see D0-7)",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

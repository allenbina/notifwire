# notifwire

**A self-hosted, peer-to-peer notification mesh for every device you own.**

notifwire captures native OS notifications from any device and delivers them
natively to any other device — with full control over filtering, grouping,
history, and encryption. No cloud. No subscriptions. No separate server to
maintain: every install is a node.

A companion project to chatwire. Same philosophy, different data stream. Where
chatwire taps the iMessage database, notifwire taps the live notification layer
across your entire device ecosystem.

You host it yourself. That's the point.

## Status

Early design stage. The full technical design lives in **[SPEC.md](SPEC.md)** —
architecture, data model, per-platform notification handling, the icon
resolution system, plugin contract, `notifwire-send` CLI, rules engine, and the
v1→v3 roadmap.

## At a glance

- **Mesh, not client/server** — every install is a node; any always-on node can
  act as the hub. Many producers → hub → many consumers.
- **Native capture & delivery** — macOS (AXObserver), Windows (WinRT), Linux
  (D-Bus), Android (`NotificationListenerService`); consumers render via native
  OS notification APIs.
- **Built on Tauri v2** — Rust backend, web UI, ~5–10MB binary, one codebase
  across desktop + Android.
- **Outputs beyond the OS** — MQTT, HTTP webhook, Apprise (100+ services), and
  an MCP server for querying notification history.
- **Self-hosted & private** — TLS everywhere, opt-in end-to-end encryption,
  configurable retention.

See [SPEC.md](SPEC.md) for the complete design.

## License

MIT — see [LICENSE](LICENSE).

# notifwire

<p align="center">
  <img src="docs/assets/logo.png" alt="notifwire" width="200">
</p>

<p align="center">
  <strong>Self-hosted, peer-to-peer notification mesh for every device you own.</strong>
</p>

<p align="center">
  <a href="https://github.com/allenbina/notifwire/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  <a href="https://github.com/allenbina/notifwire/stargazers"><img src="https://img.shields.io/github/stars/allenbina/notifwire" alt="Stars"></a>
  <a href="https://github.com/allenbina/notifwire/releases"><img src="https://img.shields.io/github/v/release/allenbina/notifwire" alt="Release"></a>
</p>

---

notifwire captures native OS notifications from any device and delivers them
natively to any other device — with full control over filtering, grouping,
history, and encryption. No cloud. No subscriptions. No separate server to
maintain: every install is a node.

A companion project to chatwire. Same philosophy, different data stream. Where
chatwire taps the iMessage database, notifwire taps the live notification layer
across your entire device ecosystem.

## Why notifwire

Some of the most important alerts you get only ever appear on your phone — 2FA
codes, app-only push, transaction alerts. If your phone is in the other room,
you miss them. notifwire frees them, and turns every notification from a thing
you glance at into a signal you can route, store, search, and act on.

- **Mesh, not client/server** — every install is a node; any always-on node is the hub
- **Native in, native out** — captured from real OS APIs, delivered as real OS notifications
- **Routable** — also HTTP webhook, MQTT, and Apprise (100+ services)
- **Queryable** — searchable history plus an MCP server
- **Private** — TLS everywhere, opt-in end-to-end encryption, you host it yourself

Read the full case in **[docs/why.md](docs/why.md)**.

## Quick start

> **Status: active development.** The Windows core — toast capture and native
> display — works end-to-end; the desktop GUI is being built next. Not yet
> released. ⭐ Star the repo to follow along.

Once v1 ships, getting running will look like:

```text
1. Install notifwire on each device  (.dmg / .exe / AppImage)
2. Designate one always-on node as the hub
3. Pair your other devices to it
4. Notifications flow from every producer to every consumer, automatically
```

See [docs/setup.md](docs/setup.md) for the full guide.

## Documentation

- [Setup guide](docs/setup.md)
- [Architecture](docs/architecture.md)
- [Plugin development](docs/plugins.md)
- [Use cases / why](docs/why.md)
- [Full spec](docs/SPEC.md)
- [FAQ](docs/FAQ.md)
- [Philosophy](docs/PHILOSOPHY.md)
- [Changelog](docs/CHANGELOG.md)

## Philosophy

notifwire exists because your notifications are yours. They shouldn't be trapped
on one device or routed through someone else's cloud. notifwire keeps them on
hardware you control and lets you do whatever you want with them.

Read the full [philosophy](docs/PHILOSOPHY.md).

## Contributing

Contributions welcome! See [CONTRIBUTING.md](docs/CONTRIBUTING.md) and our
[Code of Conduct](docs/CodeOfConduct.md).

## License

MIT — see [LICENSE](LICENSE). Use it, fork it, make it yours.

---

<p align="center">
  <sub>Built by <a href="https://github.com/allenbina">allenbina</a> · part of the <em>wire</em> family</sub>
</p>

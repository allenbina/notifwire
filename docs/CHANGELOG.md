# Changelog

All notable changes to notifwire are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Public spec (v3) and project documentation; project scaffolding (README, docs
  site, contribution guides)
- Rust cargo workspace + Tauri v2 + SvelteKit skeleton; GitHub Actions CI
  (Windows): `cargo fmt --check`, `clippy -D warnings`, `cargo test`, frontend build
- `core`: normalized Notification data model and versioned config schema
  (apply-if-newer); rules engine, dedup, and icon resolution chain
- `transport`: SSE mesh transport behind a `MeshTransport` trait — serve +
  cursor catch-up + reconnect, plus a durable (restart-surviving) producer outbox
- `notifwire-send` CLI: inject notifications over a node's localhost ingest API
  (the test harness for the whole pipeline)
- Windows **producer**: live toast capture via WinRT `UserNotificationListener`
  — works unpackaged (no MSIX/sparse package/signing on Windows 11)
- Windows **consumer**: native WinRT toast display, SQLite history, and the full
  capture → filter → dedup → history → display pipeline; `--live` (only-new) mode
- Observability: `tracing`-based logging to stderr + a rotating daily file, and a
  producer `GET /health` self-report endpoint

notifwire is in **active development**; the Windows core (capture + native
display) works end-to-end. Not yet released — see the [spec](SPEC.md) for the
v1 → v3 roadmap and [BUILD_PLAN](BUILD_PLAN.md) for epic status.

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

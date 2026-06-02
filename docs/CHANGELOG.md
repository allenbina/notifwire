# Changelog

All notable changes to notifwire are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

#### GUI / Desktop App (D3)
- App-as-consumer: Tauri app subscribes to producers, runs consumer pipeline,
  shows WinRT toasts and in-app notification list
- Producers settings: persistent producer list (JSON config), add/remove/
  enable/disable, auto-connect on startup
- Filters: per-app allow/block rules, keyword filters (field/contains/action),
  default-mode toggle — persisted and applied to pipeline
- History: SQLite history view with pagination and app filter; retention
  settings (global default + per-producer override); age-based pruning with
  optional max-count cap
- Focuses: named filter profiles with icon/rules; "All" built-in; sidebar
  switcher; CRUD in settings; per-focus rules editor
- Focus schedules: time-based automatic focus switching; day+time-range per
  schedule; manual override preserved until next transition
- System tray: hide-to-tray on window close; tray menu (Open / Settings /
  History / Quit); left-click toggles visibility; panel navigation events
- Import/Export: full config round-trip via JSON copy-paste
- Custom CSS theming: user-supplied CSS injected into webview with live preview

#### Foundation (D0–D2)
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
display) works end-to-end, and the desktop GUI (D3) is complete. Not yet
released — see the [spec](SPEC.md) for the v1 → v3 roadmap and
[BUILD_PLAN](BUILD_PLAN.md) for epic status.

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

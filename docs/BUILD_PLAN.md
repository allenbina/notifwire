# notifwire — Build Plan

Master plan for building notifwire. Tracked in the open: **Milestones = epics
(D0–D5)**, **Issues = tasks**. This doc is the human-readable map; GitHub
Issues are the source of truth for status.

Architecture reference: `docs/SPEC.md` (being reconciled to the hub-less direct
mesh — see D0). The canonical design notes live in the private `notifwire-notes`
repo.

## Principles

- **Vertical slice before horizontal layers.** Build a walking skeleton
  (one producer → transport → one consumer, on one machine) before widening to
  every platform.
- **`notifwire-send` is the test harness.** It injects notifications with no
  OS-capture code, so the whole pipeline is testable before the WinRT bridge
  exists.
- **OS capture behind a trait.** The macro pipeline stays testable with
  synthetic events; platform code is swappable.
- **CI green is the merge gate.** Foundation is serial; fan out only once D0 +
  CI exist.
- **Local-first on Windows.** Develop natively on Windows (real toast capture +
  native display); WSL2/Docker for the Linux/Docker pieces; mbair/plinux over
  Tailscale for the other platforms later.

## Stack

- **Backend:** Rust, Tauri v2. Cargo workspace: `core` (model/config/rules/dedup/
  icons/health), `observe` (logging), `transport` (SSE mesh + outbox), `cli`
  (`notifwire-send`), `producer` + `producer-win` (Windows capture), `consumer` +
  `consumer-win` (Windows display), `app/src-tauri` (desktop app); `plugins/*` later.
- **Frontend:** SvelteKit (Tauri web UI).
- **Transport:** SSE for v1, behind a `MeshTransport` trait (WebSocket adapter
  later).
- **CI:** GitHub Actions — `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`, frontend lint/build. Windows runner first; matrix later.

## Epics (Milestones)

| Epic | Title | Depends on | Done when |
|------|-------|-----------|-----------|
| **D0** ✅ | Foundation / walking skeleton | — | inject via `notifwire-send` → SSE loopback → stub consumer prints it |
| **D1** ✅ | Windows producer | D0 | real Windows toasts captured → served → caught by test consumer; offline catch-up via cursor works |
| **D2** ✅ | Windows consumer (native display) | D0 (D1 for real data) | full loopback: notifications mirrored, filtered, deduped, icons + history |
| **D3** ✅ | Observability foundation + Settings UI + Focuses | D1, D2 | logging + health/auto-reconnect, then focuses tree (add/copy/schedule/default) + per-device toggles + config import/export |
| **D4** | Headless + Docker + config sync + output plugins | D2, D3 | Docker consumer pulls file config, subscribes over Tailscale, re-exports to MQTT |
| **D5** | Encryption (opt-in) | D2 | E2E encrypt-to-pubkey → decrypt; `key_required`/`key_mismatch` codes fire |
| **Later (v2+)** | macOS producer (mbair), Linux producer (plinux), Android, plugin registry, MCP server, Clearbit/favicon, WS adapter | — | per roadmap |

> **D3 complete:** all GUI slices shipped (3A–3I).

> **Build-infra relocation is post-RC1 (Windows).** Containerizing the build and
> moving it off the dev laptop onto the k3s/Kubernetes cluster (in containers or
> VMs) is a **D4-era** task that happens *after* the Windows app reaches a
> release candidate — it does not gate the Windows product. The eventual test
> matrix is ≥5 producers and ≥6 consumers (macOS/iOS, Windows, KDE, GNOME,
> Docker, RSS, MQTT, HTTP).

## D0 task breakdown

1. Scaffold cargo workspace + Tauri v2 shell + SvelteKit skeleton
2. CI (GitHub Actions, Windows runner): fmt, clippy, test, frontend build
3. `core`: normalized Notification data model (+ serde, tests)
4. `core`: versioned config schema + apply-if-newer compare (+ tests)
5. `transport`: `MeshTransport` trait + SSE impl (serve + pull-since-cursor,
   reconnect/catch-up) (+ integration tests)
6. `cli`: `notifwire-send` (flags + JSON stdin) + localhost ingest HTTP API
7. Stub consumer (prints received notifications) + E2E loopback smoke test
8. Reconcile public docs (`SPEC.md`/`architecture.md`/`plugins.md`) to the
   hub-less direct mesh

**D0 acceptance:** `notifwire-send "hello"` on the local machine is received
over SSE by the stub consumer and printed, with the catch-up cursor proven by a
producer-restart test.

# notifwire — Development, Build & Test

How to set up a dev environment, build notifwire, and run the tests. notifwire
is a **Tauri v2** app: a Rust backend (cargo workspace) with a **SvelteKit**
web UI. See `docs/BUILD_PLAN.md` for the epic/issue roadmap and `docs/SPEC.md`
for the architecture.

> Status: D0–D2 complete and CI-green (foundation, transport/capture, Windows
> consumer with native display). D3 (the desktop GUI + observability) is in
> progress. Epic-tagged sections below reflect the feature they landed with.

## Prerequisites

| Tool | Purpose | Install |
|------|---------|---------|
| **Rust (stable, MSVC)** | backend | `winget install Rustlang.Rustup` then `rustup default stable-x86_64-pc-windows-msvc` |
| **MSVC C++ Build Tools** | linker (`link.exe`) for the MSVC Rust target | Visual Studio Build Tools 2022 with the **"Desktop development with C++"** workload |
| **WebView2 runtime** | Tauri rendering | Ships with Windows 11; otherwise the Evergreen installer from Microsoft |
| **Node.js + npm** | SvelteKit frontend | https://nodejs.org (v20+; v24 known-good) |
| **Tauri CLI** | build/dev driver | `cargo install tauri-cli` (or `npm i -D @tauri-apps/cli`) |
| git, gh | source control / GitHub | https://git-scm.com , https://cli.github.com |

Per-user installs (rustup, cargo-installed tools, global npm) need **no admin**.
Adding the C++ workload to Build Tools **does** need admin (VS Installer + UAC).

### Known-good Windows dev environment (reference snapshot)

Captured on the primary dev laptop, 2026-05-31:

- Windows 11 Home 26200
- Visual Studio **Build Tools 2022** installed
- WebView2 runtime **148.x**
- Node **v24.15.0**, npm **11.12.1**
- Rust: installed via rustup, MSVC target

If a build fails with a linker error (`link.exe not found`), the
"Desktop development with C++" workload is missing from Build Tools — add it via
the Visual Studio Installer.

### Per-platform notes

- **Windows** — primary dev + test target (real toast capture + native display).
- **Linux / Docker** — use WSL2 + Docker Desktop, or a real Linux host.
- **macOS / Android** — later epics; build on real hardware (notification capture
  does not work reliably in a VM).

## Build _(D0)_

```
# from repo root, once the cargo workspace exists
cargo build                 # all crates
cargo tauri dev             # run the desktop app with hot reload
cargo tauri build           # production bundle
```

Frontend (SvelteKit) is built by the Tauri CLI; for frontend-only work:

```
cd <frontend-dir>
npm install
npm run dev
```

## Run / dev loop _(D0)_

The fastest inner loop uses `notifwire-send` as a synthetic producer (no OS
capture needed):

```
# terminal 1: start a producer node
# terminal 2: start a consumer subscribed to it
notifwire-send "hello from the CLI"     # should appear on the consumer
```

## Windows capture (manual) _(D1)_

Capture live Windows toasts with a plain (unpackaged) build — verified working
on Windows 11 26200; no MSIX/sparse package or signing required (see
`docs/windows-notification-capture.md`):

```
# check / grant notification access (first run shows the consent prompt)
notifwire-producer --check-access            # prints "notification access: Granted"

# terminal 1: producer capturing Windows toasts
notifwire-producer --bind 127.0.0.1:8787 --capture-windows

# terminal 2: consumer — existing Action Center toasts replay, new ones stream live
notifwire-consumer --producer http://127.0.0.1:8787
```

`GetNotificationsAsync` returns the current Action Center contents, so the
consumer sees a catch-up batch immediately, then new toasts as they fire.

## Logging & diagnostics _(D3)_

Every binary logs through the `tracing` facade, initialized once at startup by
the shared `notifwire-observe` crate. Output goes two places:

- **stderr** — human-readable, in whatever terminal you launched the node in.
- **a rotating daily file** — `<component>.log.<date>` (e.g.
  `producer.log.2026-06-01`) under the per-OS data dir:
  - Windows: `%LOCALAPPDATA%\notifwire\data\logs`
  - Linux: `~/.local/share/notifwire/logs`
  - macOS: `~/Library/Application Support/notifwire/logs`

The in-app log viewer (a later D3 slice) tails these files. Control verbosity
with `RUST_LOG` (default `info`):

```
RUST_LOG=debug notifwire-producer --capture-windows
RUST_LOG=notifwire_transport=trace notifwire-consumer --producer http://127.0.0.1:8787
```

Program **output** stays on stdout — the consumer's notification lines and
`notifwire-send`'s `sent seq=…` confirmation are data, not logs. Status,
warnings, and errors are diagnostics and go through `tracing`.

## Test

Strategy: most logic is OS-independent and unit-tested; **OS capture is isolated
behind a trait** so the pipeline is testable with synthetic events
(`notifwire-send`), and only the capture bridge needs manual testing.

```
cargo test                  # unit + integration tests, all crates
cargo fmt --all -- --check  # formatting gate
cargo clippy --all-targets --all-features -- -D warnings   # lint gate
```

### Test inventory

**Unit (Rust)**
- Notification data-model normalization (round-trips, optional fields)
- Rules engine: whitelist/blacklist, priority mapping, dedup window, grouping
- Cursor / sequence-number math
- Icon resolution-chain ordering
- Config version comparison (apply-if-newer)

**Integration**
- SSE transport: serve → pull-since-cursor → reconnect/catch-up
- Auth handshake → each HTTP error code (`auth_required`, `auth_invalid`,
  `key_required`, `key_mismatch`, `version_unsupported`, `rate_limited`)
- Outbox bounded-by-time/size eviction
- Config-sync apply-if-newer

**E2E smoke (loopback)**
- `notifwire-send` inject → received + printed by stub consumer
- producer restart → consumer catches up from its cursor

**Manual / semi-automated**
- WinRT capture (fire a real toast, assert it lands)
- Native display rendering
- menubar UX

## CI

GitHub Actions, Windows runner first (matrix to macOS/Linux as those producers
land). Gates on every PR: `cargo fmt --check`, `cargo clippy -D warnings`,
`cargo test`, frontend lint/build. **Green CI is the merge gate.**

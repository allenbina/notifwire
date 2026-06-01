# notifwire

**A self-hosted, peer-to-peer notification mesh for every device you own.**

notifwire captures native OS notifications from any device and delivers them
natively to any other device — with full control over filtering, grouping,
history, and encryption.  No cloud.  No subscriptions.  No central server.
Every install is a node, and consumers talk to producers directly.

A companion project to chatwire.  Same philosophy, different data stream.
Where chatwire taps the iMessage database, notifwire taps the live notification
layer across your entire device ecosystem.

You host it yourself.  That's the point.

---

## Core Concept

There is no central server and **no relay node**.  Every notifwire install is a
node: a **producer** (capturing notifications from its host OS), a **consumer**
(displaying incoming notifications natively or re-exporting them), or both
simultaneously.

Consumers connect **directly** to producers.  A consumer chooses which
producers to subscribe to, authenticates to each, and receives their
notification streams — filtered and routed per the consumer's own rules.  No
node sits in the middle.  If a consumer is offline, the producer it subscribes
to holds a short buffer (see Offline Behavior); nothing else in the mesh is
affected.

```
        Producers                          Consumers
   (capture & serve)                   (subscribe & display)

  ┌──────────────┐                    ┌──────────────┐
  │ Mac producer │◀───────────────────│ Windows      │  native display
  └──────────────┘         ┌──────────│ consumer     │
  ┌──────────────┐         │          └──────────────┘
  │ Android      │◀────────┤          ┌──────────────┐
  │ producer     │◀────────┼──────────│ Linux/Docker │──▶ MQTT
  └──────────────┘         │          │ consumer     │──▶ Apprise
  ┌──────────────┐         │          │ (headless,   │──▶ webhook
  │ Linux        │◀────────┘          │  re-export)  │
  │ producer     │                    └──────────────┘
  └──────────────┘
        ▲
        │ localhost HTTP
  ┌──────────────┐
  │ input plugins│  RSS, notifwire-send, …
  └──────────────┘

  Arrows = a consumer subscribing directly to a producer.
  No central relay.  Sinks (MQTT/Apprise/webhook) hang off a
  consumer; nodes never relay to other nodes.
```

**Many producers, many consumers, direct connections.**  A consumer subscribes
to the producers it cares about and receives their notifications at once,
filtered and routed per its rules.  Consumers never subscribe to other
consumers — there is no node-to-node relaying (see Roles).

**Reachability is the operator's choice, and notifwire stays out of it.**  A
producer listens on an address and port; you reach it however you like — LAN
IP, hostname, port-forward, Tailscale, or Cloudflare Tunnel.  No special
topology, no port forwarding required unless you want it.

---

## Roles

### Producer
A node that captures notifications from its host OS (or from input plugins) and
**serves** them to subscribed consumers.  It maintains the list of apps it has
seen (apps are discovered, not declared — see Focuses) and a short outbox for
catch-up.  A Mac running notifwire is a producer.  A cron job calling
`notifwire-send` is feeding a producer node.

### Consumer
A node that subscribes to one or more producers and either displays
notifications natively on its host OS **or** re-exports them to an external
destination (MQTT, webhook, Apprise) via an output plugin.  A consumer holds
its own configuration: filters, focuses, history, and optional E2E keys.  A
Windows PC running notifwire is a consumer.  A headless Linux/Docker box
re-exporting to MQTT is a consumer.

### Always-on consumer (optional, not a special type)
Any consumer deployed somewhere that never sleeps — typically a Linux/Docker
box.  Useful as a persistent bridge to MQTT/Apprise/Home Assistant, or as a
single subscription point for many producers.  **Nothing is forced through
it**; if it's down, every other node still works directly.

### No node-to-node relaying
A notifwire consumer **always** subscribes to producers directly, **never** to
another consumer.  Sinks fan out *outward* to non-notifwire systems (MQTT, HA,
scripts); nodes never relay *inward* to peers.  This rule keeps an always-on
consumer from quietly becoming a relay (routing tiers, cross-hop dedup, cursor
ambiguity).

### A node can hold multiple roles
Your Mac is typically producer + consumer.  Your Windows PC is consumer only.
Your Android phone is producer + consumer.  Roles are configured per node.

---

## Platform

Built with **Tauri v2** (Rust backend, web UI frontend).  One codebase for
macOS, Windows, and Linux desktop.  Android via Tauri v2's native Android
support with Kotlin bridges for OS-specific capture APIs.

Why Tauri:
- Background tray / menubar agent is the primary use case — Tauri is built
  for exactly this
- Binary size ~5–10MB vs Electron's ~150MB
- Minimal memory footprint for a process running 24/7 on every device
- First-class system tray and menubar support built in
- Native notification sending via `tauri-plugin-notification`
- chatwire alignment — shared toolchain, shared knowledge

Every node — producer, consumer, GUI or headless — runs the **same backend
skeleton** (web UI + config server + sync client).  The only difference is that
a headless node reads its config from JSON and does not serve the web settings
UI.

OS-specific notification capture requires native bridges regardless of
framework.  These are implemented as Tauri plugins:

| Platform | Capture API | Bridge |
|---|---|---|
| macOS | AXObserver (Accessibility API) | Swift/Rust Tauri plugin |
| Windows | WinRT `Windows.UI.Notifications` | Rust WinRT bindings |
| Linux | D-Bus `org.freedesktop.Notifications` | Rust zbus |
| Android | `NotificationListenerService` | Kotlin Tauri plugin |

**Distribution:**
- macOS — `.dmg`, menubar app, Accessibility permission granted once in
  System Settings
- Windows — installer or portable `.exe`, no admin required for basic use
- Linux — AppImage (universal), also `.deb` / `.rpm`, **plus a headless Docker
  image** for servers and output-plugin nodes (no GUI, JSON config)
- Android — `.apk` sideload or Play Store (future)

---

## Transport & Connection

The producer↔consumer mesh and the local ingest path are two distinct
surfaces — don't conflate them.

### Local ingest (inside one node)
Input plugins and `notifwire-send` POST normalized notification JSON to the
node's **localhost HTTP API** (`localhost:PORT`).  This is loopback, so
plaintext is fine.  Simple request/response.

### Mesh transport (producer ↔ consumer, across the network)
- **Transport: Server-Sent Events (SSE) for v1**, sitting behind a small
  `MeshTransport` interface so a WebSocket adapter (or anything else) can be
  added later without touching the rest of the app.  SSE is plain HTTP: the
  producer holds one long response open and writes each notification as a
  `data:` line with an `id:` — and that `id` **is** the catch-up cursor (the
  client's auto-reconnect resends it as `Last-Event-ID`).  Auth rides in the
  connect request; the producer's app list is sent as the first event on the
  stream.  Traverses proxies and CF Tunnel with no special config.
- **Consumer dials the producer** (it's the subscriber).  The producer listens;
  the operator routes reachability however they like.
- A single connection handles **both** live push and catch-up replay (see
  Offline Behavior).

### Handshake / error codes
Standard HTTP status codes, with a short machine-readable `code` in the JSON
body only where the status alone is ambiguous:

| Status | `code` | Meaning |
|---|---|---|
| 200 | — | connected, SSE stream opens |
| 401 | `auth_required` | producer needs a password, none sent |
| 401 | `auth_invalid` | wrong password |
| 403 | `key_required` | E2E is on, consumer has no key configured |
| 403 | `key_mismatch` | key / public key doesn't match, can't decrypt |
| 426 | `version_unsupported` | protocol version mismatch |
| 429 | `rate_limited` | optional, for internet-exposed producers |

Body shape: `{ "code": "auth_invalid", "message": "wrong password" }` — `code`
drives client logic, `message` is shown in the connect dialog.  This shares one
error vocabulary with `notifwire-send`'s exit codes.

---

## Notification Data Model

Every notification in notifwire is normalized to a common schema regardless
of origin — OS capture or input plugin.  Fields beyond the lowest common
denominator are carried as optional extras and rendered if the destination
platform supports them.

### Lowest Common Denominator (guaranteed on all platforms)

| Field | Type | Notes |
|---|---|---|
| `id` | string | UUID generated at capture |
| `producer_node` | string | Node ID that captured it |
| `source_platform` | enum | macos / windows / linux / android / plugin |
| `plugin_id` | string | Set when source_platform is plugin |
| `app_name` | string | Always present |
| `title` | string | Always present |
| `body` | string | Always present |
| `timestamp` | ISO 8601 | Captured at receive time |
| `app_icon` | PNG, 48x48 | Normalized at the consumer — see Icon System |

### Extended fields (carried if available)

| Field | Platforms | Notes |
|---|---|---|
| `subtitle` | macOS, iOS | Middle line between title and body |
| `image` | Android, Windows | Inline or hero image |
| `category` | macOS, Android | App-defined notification category |
| `thread_id` | macOS, Android | For grouping related notifications |
| `icon_resolution` | All | Source resolution before normalization |
| `is_synthetic` | All | True for battery events, plugin-generated |

---

## Notification Styles by Platform

Consumers always render using native OS APIs — no custom UI, no electron
windows, no webviews.  notifwire maps the normalized payload to whatever the
destination OS supports.

### macOS
Banner (auto-dismisses ~5s) or Alert (persists until dismissed).  User
controls per-app in System Settings → Notifications.  App icon always shown.
Title, subtitle, body, inline image.  Action buttons on Alerts only.

### iOS (via Continuity on Mac)
iOS notifications mirror to a paired Mac automatically via Continuity — no
same-network requirement, works over the internet.  All paired iPhones and
iPads mirror simultaneously.  notifwire sees these as macOS-sourced
notifications with the iOS app name.  Which iOS device originated a
notification (iPhone vs iPad) cannot be distinguished — this is a Continuity
limitation, not a notifwire one.

### Android
Richest notification system of all platforms.  Heads-up banner, expandable
shade (BigText, BigPicture, Inbox style), ongoing persistent notifications.
Up to 3 action buttons, inline reply via RemoteInput.  Small icon (monochrome,
24dp, required) plus optional large icon (64dp).  notifwire captures via
`NotificationListenerService`.

### Windows
Toast notifications with Action Center persistence.  Title, body, optional
hero image (full-width), optional inline image.  Up to 5 action buttons,
inline reply text input.  App icon 44x44px square or circular.  Flexible
XML-based adaptive template system.

### GNOME (Linux)
Floating bubble (top center) with notification list.  App name, title, body,
app icon (32–48px PNG).  Up to 3 action buttons, inconsistent across apps.
No inline reply natively.  No image attachments in standard libnotify.
Protocol: D-Bus `org.freedesktop.Notifications`.

### KDE Plasma (Linux)
Same D-Bus protocol as GNOME with KDE extensions.  Better action button
support, better notification history UI in system tray.  Optional image via
pixmap over D-Bus.  Icons 32–48px.

---

## Icon System

Icons are resolved, normalized, cached, and self-improved by the **consumer**.
Plugin and script authors do not need to solve the icon problem — the consumer
handles it.  Producers send the best icon they happen to have; each consumer
caches and upgrades from there.

### How the cache works

Each consumer maintains an icon cache keyed by `app_name`, always storing the
highest-resolution version seen so far.  When a higher-resolution copy
arrives (e.g. a macOS bundle icon at 512px arriving after an earlier Android
capture at 64px), the cache entry upgrades automatically.  `icon_resolution`
in the notification schema tells the consumer whether to upgrade or keep what
it has.

All icons are normalized to **48x48px PNG** as the transit and storage format.
SVGs from Simple Icons are rasterized at the highest useful resolution before
normalization.

### Source quality by platform

| Source | Max resolution | Notes |
|---|---|---|
| macOS app bundle | Up to 1024x1024 | Best source in the mesh by far |
| iOS via Continuity | Same as macOS | Same bundle quality |
| Windows .ico | Up to 256px | Multi-resolution, good quality |
| Android large icon | ~64dp | Acceptable, supplements well |
| Linux .desktop | 32–48px | Lowest quality native source |
| Plugin-provided URL | Variable | Consumer fetches and caches |
| Plugin manifest icon | 48–128px | Generic fallback for that plugin |

### Resolution chain (evaluated in order, stops at first hit)

```
1. Consumer icon cache  — best version seen from any source so far
2. Simple Icons         — vector SVG for ~3000 major brands, perfect at any size
3. Clearbit Logo API    — user-supplied API key, domain-matched
4. DuckDuckGo favicon   — no key required, ~32px, last resort
5. Google favicon       — no key required, 64px max, last resort
6. Placeholder          — app_name initial on a generated colored background
```

### Simple Icons

Simple Icons (`simpleicons.org`) is the MVP of the icon system:
- ~3000 brand SVGs: Pinterest, Coinbase, GitHub, Discord, Slack, all major
  banks, crypto exchanges, developer tools, and consumer apps
- Vector format — infinitely scalable, zero resolution loss
- MIT / CC licensed — safe to bundle and redistribute
- No API key, no rate limit, no account
- Ships bundled with notifwire — works offline

When `app_name` or `--icon` matches a Simple Icons slug (case-insensitive,
fuzzy matched), the SVG is used and the resolution chain stops there.

### Clearbit Logo API

For apps not covered by Simple Icons, Clearbit serves brand logos by domain.
Requires a user-supplied API key (free tier is generous for personal use).
notifwire ships a community-maintained `app_domains.json` mapping
(`"Coinbase" → "coinbase.com"`, `"Pinterest" → "pinterest.com"`, etc.) that
grows with each release, so most lookups are automatic.

### API key management

Keys are stored locally in the node config.  Never transmitted to other nodes
or any service beyond their intended API call.  notifwire never ships embedded
keys and never proxies lookups through any notifwire-operated service.

Settings → API Keys:
- Clearbit Logo API key
- (future) additional icon services as community demand warrants

---

## Producers

A producer captures notifications from its host OS and serves them to
subscribed consumers over the mesh transport.  It keeps a discovered-apps list
(sent to consumers on connect, refreshed as new apps appear) and a short
outbox for catch-up.

### macOS producer
- Captures all macOS notifications including iOS/iPadOS Continuity mirrors
- Requires Accessibility permission — one-time grant in System Settings
- Extracts highest-resolution icon available from app bundle (.icns)
- Configurable: whitelist/blacklist, DND schedule, battery events

### Windows producer
- Captures Windows toast notifications via WinRT notification queue
- No special permissions required beyond standard user
- Extracts icon from .ico at best available resolution

### Linux producer
- Listens on D-Bus `org.freedesktop.Notifications`
- Works on GNOME and KDE Plasma; other DEs vary
- Icon quality limited — consumer icon lookup recommended

### Android producer
- Captures via `NotificationListenerService`
- Requires Notification Access permission — one-time in Android Settings
- Always-on even when screen is off — Android push infrastructure keeps the
  node alive.  Makes Android the most reliable always-on producer in the mesh,
  especially useful when desktop machines are sleeping.

---

## Consumers

### Native OS display
Notifications delivered via the host OS's native notification API.  Looks and
behaves exactly like a local notification — because it is one.  No custom UI.

### Output plugins (re-export)
External destinations are handled by **output plugins** — microservice
processes, one per sink technology, each consuming normalized JSON from its
consumer host and delivering it to a single sink.  Anyone can write one (see
Plugins).  Official output plugins:

- **MQTT** (`mqtt-out`) — publish notification JSON to a broker topic; fan out
  to n8n, Home Assistant, custom scripts, anything with a MQTT client.
  Topic structure: `notifwire/{producer_node}/{app_name}`
- **HTTP Webhook** (`http-out`) — POST JSON to any URL.  Triggers n8n, IFTTT,
  Zapier, Home Assistant automations, or any custom endpoint.  Per-app or
  global.
- **Apprise** (`apprise-out`) — forward to any of Apprise's 100+ services:
  Telegram, Discord, Slack, Pushover, ntfy, email, Signal, Matrix, and more.
  One integration, the entire Apprise catalog.

notifwire is what Apprise has always needed on the receiving end.  Apprise
sends notifications out to services.  notifwire captures them from OS sources,
routes them, stores history, and uses Apprise as one of its output adapters.

### MCP Server
Every notifwire node exposes an MCP server.  Enables Claude and any other
MCP client to query notification history, search by app or keyword, get
recent notifications, and subscribe to live events.

Example queries:
- "Did my backup job finish last night?"
- "What Coinbase alerts did I get this week?"
- "What time did that download finish on my Windows machine?"
- "Show me all missed calls in the last 7 days"
- "Did any of my nodes go offline overnight?"

---

## Plugins

Plugins extend notifwire on both sides.  They follow the same architecture as
chatwire: official plugins, GitHub-hosted community plugins (auto-update), and
ZIP upload (no auto-update).

- **Input plugins** (producer side) inject non-OS notification sources — RSS,
  etc. — into a producer.
- **Output plugins** (consumer side) re-export notifications to a single sink
  technology — `mqtt-out`, `http-out`, `apprise-out`.

### Plugin contract

A plugin is a **process**, not a library.  This is intentional:

- No SDK required.  Any language that can make an HTTP POST (input) or receive
  one (output) works.
- notifwire core (Rust/Tauri) does not need the plugin's runtime installed.
- **Input:** the plugin POSTs normalized notification JSON to the local
  producer node's HTTP API (`localhost:PORT`).  The node handles dedup,
  filtering, routing, icon resolution — the plugin just sends the event.
- **Output:** the plugin receives normalized JSON from its consumer host and
  delivers it to its sink.  Typically packaged as one Docker container per sink
  technology; can run headless (JSON config, no web UI).
- The plugin process is started, stopped, and updated by notifwire (or, for
  containerized output plugins, deployed by the operator).

### Plugin manifest (`notifwire-plugin.json`)

```json
{
  "id": "notifwire-rss",
  "name": "RSS / Atom Feed Reader",
  "version": "1.0.0",
  "author": "notifwire",
  "kind": "input",
  "entrypoint": {
    "macos": "bin/notifwire-rss-macos",
    "windows": "bin/notifwire-rss-windows.exe",
    "linux": "bin/notifwire-rss-linux"
  },
  "config_schema": { },
  "update_url": "https://github.com/allenbina/notifwire-rss/releases"
}
```

notifwire renders the `config_schema` as a settings UI automatically.  Plugin
authors define fields; notifwire handles the form.  A plugin running headless
reads the same fields from a JSON config instead of serving the form.

### Plugin tiers

**Official** — maintained in the notifwire org.  Bundled or one-click install.
Auto-update via GitHub releases.  Held to the same quality bar as core.

**GitHub** — paste a GitHub repo URL.  notifwire fetches the manifest, installs
the correct binary for the current OS, and checks for updates on a configurable
schedule.  Same trust model as chatwire community plugins.

**ZIP upload** — drag in a zip containing the manifest and binaries.  No
auto-update.  For private, internal, or offline plugins.

### Official input plugin: RSS / Atom (v1 of the plugin)

The first official input plugin.

- Multiple feed URLs, each independently configured
- Poll interval per feed (or global default)
- `app_name` mapped per feed for correct grouping and icon lookup
- Optional keyword filter per feed (notify only if title contains X)
- Priority per feed
- Dedup by GUID — never re-notifies items already seen
- Persists seen GUIDs locally across restarts
- Feed-provided image or favicon used as icon source; falls through to the
  consumer resolution chain

---

## `notifwire-send` CLI

A small standalone binary, ships with every notifwire install, lives in PATH.
Allows any script, cron job, CI pipeline, or scheduled task to inject a
notification into the local producer node.  The node handles everything else.

### Simple usage

```bash
# Minimal
notifwire-send "Backup complete"

# Common flags
notifwire-send "Backup complete" --app "rsync" --priority high

# With icon — app name, brand name, URL, domain, or file path
notifwire-send "Build finished" --app "Jenkins" --icon "jenkins"
notifwire-send "Download done" --app "aria2" --icon "https://example.com/icon.png"
notifwire-send "Deploy complete" --app "Ansible" --icon "/path/to/icon.png"
```

### Rich usage (JSON)

```bash
# Pipe JSON body
echo '{
  "title": "Backup complete",
  "body": "42GB synced in 4m 12s",
  "app_name": "rsync",
  "priority": "high",
  "icon": "rsync"
}' | notifwire-send

# Pass a JSON file
notifwire-send --json ./notification.json
```

### `--icon` resolution

The `--icon` value is interpreted intelligently:

| Value | Behavior |
|---|---|
| Brand / app name | Fuzzy matched against Simple Icons slugs first, then Clearbit |
| Domain | Favicon lookup via DuckDuckGo / Google |
| URL | Consumer fetches, caches, normalizes |
| File path | Binary reads, base64 encodes, sends inline |
| Omitted | Consumer uses `--app` value as lookup key |

`--app` doubles as the icon lookup key when `--icon` is not specified.
`--app "Coinbase"` with no `--icon` will automatically resolve the Coinbase
icon via Simple Icons.

### Environment and CI behavior

```bash
# Send to a remote producer instead of localhost
NOTIFWIRE_HOST=mac.local notifwire-send "Remote job done"

# Timeout is fast (default 2s) — scripts never block on notifwire
# Exit codes are meaningful — CI-friendly
notifwire-send "Build failed" --priority urgent
echo "Exit: $?"  # 0 success, 1 node unreachable, 2 invalid args
```

notifwire being unreachable never fails a script.  The binary exits cleanly
with a non-zero code and your pipeline continues.

### Windows / PowerShell

```powershell
notifwire-send.exe "Build finished" --app "MSBuild" --priority normal

# Or pipe JSON
'{"title":"Done","app_name":"MSBuild"}' | notifwire-send.exe
```

Same binary, same flags, same behavior on every OS.

---

## Rules Engine

Evaluated per notification at the **consumer**, in order, before display or
re-export.  (Producers may also apply a coarse send-side filter so they never
ship muted apps over the wire at all.)

### Filtering
- **Whitelist mode** — forward only listed apps (recommended for low noise)
- **Blacklist mode** — forward everything except listed apps
- Per-app toggle
- Keyword filters on title and/or body (include or exclude)
- The whitelist/blacklist default also governs **not-yet-seen apps**: in
  blacklist mode a brand-new app flows through until muted; in whitelist mode
  it's silent until allowed (see Focuses → App discovery)
- iMessage and SMS excluded by default — handled by chatwire

### Priority levels
Map any app or plugin to: Silent / Low / Normal / High / Urgent

Controls: sound on consumer, banner vs alert style, DND override behavior,
offline queue behavior.

### Grouping
- Mirror source OS grouping behavior per app
- Manual group key per app or plugin
- Configurable max group size before collapsing to summary
- Group timeout — ungroup after N minutes of inactivity

### Deduplication
Continuity sometimes fires the same notification on both iPhone and Mac.
notifwire fingerprints each notification (app + title + body) within a
configurable time window (default: 5 seconds) and drops duplicates.  For
Continuity duplicates this happens on the **Mac producer**, where both copies
originate.

### Scheduling / DND
- Per-node DND windows (e.g. 22:00–07:00)
- Urgent priority overrides DND
- Per-app DND override

---

## Settings, Focuses & UI

### Producer settings

**Network tab** — deliberately minimal:
- **Listen on all interfaces** (toggle) — on = `0.0.0.0`; off = bind to
  loopback / a specific interface for the lock-it-down crowd
- **Port** — editable, with a sane default
- **Public address** (optional) — `notifwire.domain.com` or an IP; the
  advertised address handed out for pairing/connect.  Blank = consumers use
  whatever they dialed.
- **TLS** — `off (plaintext) · self-signed · own cert · upstream-terminated`;
  see Encryption.  `off` is a legitimate selection, not a warning state.

**Security tab** (separate from Network):
- Password protection on the producer.  This is the whole producer-side
  security surface for now.

**Encryption** — a single checkbox, off by default (see Encryption).

### Consumer settings (per consumer device)
Each toggle is on/off **per consumer device**:
- Show notification count.
- Menubar-hover shows the active focus name — *or* show the focus name in the
  menubar directly.
- If possible, set the menubar icon to the icon chosen for the active focus.
  *(Feasibility unknown — platform dependent.)*

**Menubar click** → picker for: **Settings · Mute · Pick Focus**.

**Import / Export** — all settings, so a config built on one device loads onto
others.  The exported artifact is the **same JSON** a headless container reads
(see Config Sync) — one schema, two ways to feed it.

### Filters everywhere
Filters appear at **both** the top level and the drill-down for each
service / app / producer.

### Focuses
A "focus" is a named profile (think Apple Focus modes).  Each focus owns its
own full tree of producers / apps / filters.

Per-focus controls (next to each focus): **rename · change icon · add · copy ·
remove · expand**
- **Add** → a new focus with the **tree structure only** (producers / apps /
  filter slots); settings start clean.  Same skeleton, fresh config.
- **Copy** → a full duplicate of the focus **including its settings** (toggle
  states, filter contents, per-producer config) — a true clone you then tweak.
- **Switch schedule** — per focus, times to switch *to* and *from* it.
- **Default** — the last focus in the list is the default, used when no focus
  is actively picked.

#### Focus tree structure

```
Focus: All
├─ Filters for All (applies to every producer)
└─ Producers
   ├─ windows1   [edit · expand]
   │  ├─ Filters for windows1
   │  └─ Apps
   │     ├─ app1   [✓ on/off · + add filter]
   │     │  ├─ filter1   [remove]
   │     │  └─ filter2   [remove]
   │     └─ app2
   │        └─ filters for app2   [remove]
   ├─ ios1        [edit · expand]
   └─ windows2    [edit · expand]
Focus: focus2    [rename · change icon · add · copy · remove · expand]
Focus: default   ← last = default when none selected
```

- **edit** per producer opens that producer's settings.  *(Popup menu vs.
  separate tab — undecided.)*
- Checkmark next to each app toggles it on/off; apps and filters can be added
  or removed inline.

#### App discovery
There is no API that lists the apps installed on a producer — an app only
becomes knowable when it fires its **first notification**.  So apps are
**discovered, not declared**:
- Each producer keeps the list of apps it has ever seen and sends it to
  authenticated consumers (on connect, refreshed as new apps appear).
- A newly discovered app appears as a new leaf in **all** focuses at once, in
  each focus's default-mode state — so a new banking app is never silently
  missing from a focus just because it first fired while another was active.
- What happens to an app before you configure it is governed by the focus's
  (or producer's) default whitelist/blacklist mode (see Rules Engine).

---

## Config Sync

Config sync is a **shared module in the common backend skeleton** — GUI
consumers and headless containers sync via the *same code*, so they never
drift.  The only difference is the GUI also edits; the container only reads.

- **`ConfigSource`** = where config lives: a mounted file/volume, a git repo, a
  Dropbox folder, an HTTP(S) URL.  Pluggable behind an interface; ship the
  **mounted-file/volume** source first, add git/Dropbox/HTTP later without
  touching anything else.
- **Sync client** pulls on a timer (+ on file-change), validates the JSON, and
  **hot-reloads** with no restart.
- **One writer, many pull-only readers** — containers are always pull-only and
  never edit.  A GUI consumer may publish edits up to the source.  Keep it
  **single-writer-by-convention for v1** (one device is the source of truth)
  with the version stamp as the guardrail; true multi-editor merge is a later
  problem.
- **Version stamp lives inside the JSON** — the file is self-describing, so a
  mounted file, a git blob, and a Dropbox copy each carry their own truth with
  no external index:

```json
{
  "config_version": 47,
  "updated_at": "2026-05-31T18:04:00Z",
  "updated_by": "allen-macbook",
  "producers": [ ... ],
  "focuses": [ ... ]
}
```

The sync client compares `config_version` / `updated_at` and applies only what
is newer — the same high-water-mark logic as the notification cursor, so a
container that's been offline catches its config up the same way a consumer
catches up notifications.

### Container config
- **One writable config file per container** (mqtt-out's broker creds,
  http-out's URLs — per-deployment anyway).
- A shared source of truth (producer list + focus tree) may be mounted
  **read-only** into multiple containers.  Read-only = no contention; the trap
  is multiple containers *writing* the same file, never multiple reading.
- Bind-mount the JSON from the host; the container watches the file (or reloads
  on SIGHUP) so edits apply without a restart.

---

## Offline Behavior

There is no central queue.  When a consumer is unreachable, the **producer**
holds the buffer:

- Each producer assigns a **monotonic sequence number** to its own stream
  (producer-local — no global sequencer needed in a direct mesh).
- Each consumer stores **one cursor per producer** (`windows1: last_seq 4417`,
  `mac1: last_seq 982`, …).
- On reconnect, the consumer presents its cursor and the producer **replays
  everything after it from the outbox, then resumes live** — the same SSE
  connection handles catch-up and live delivery.

The outbox is **bounded by both time and size** (default: ~2h, also capped by
count) so a chatty app can't blow up memory during a long outage.  Per-consumer
policy on what to retain as the buffer ages:

| Mode | Behavior |
|---|---|
| **Queue and deliver** | Hold all notifications, deliver in order on reconnect |
| **Queue with summary** | Single summary on reconnect ("14 notifications while offline") |
| **Urgent only** | As the time bound trims the queue, keep High/Urgent longer |
| **Drop** | Discard while offline, no catchup |

This is distinct from the on-device **ring buffer** (display history, below) —
the outbox is the delivery queue, the ring buffer is local history.  Don't
conflate them, or you'll re-deliver the whole ring buffer on every reconnect.

Android nodes remain online even with the screen off — Android push
infrastructure keeps them alive.  An old Android phone or tablet running
notifwire makes an ideal always-on producer for a mesh where desktop machines
sleep.

---

## Battery Monitoring

Each node monitors its own device battery and emits synthetic notifications
to its subscribers when configured thresholds are crossed.

- Configurable threshold per node (default: 20%)
- Emits as Normal priority by default, configurable up to Urgent
- Useful for: Android tablet left unplugged, laptop on battery during a long
  session, any node in the mesh running low
- Implemented via Tauri system info APIs — no external dependency

---

## History

### Consumer-side
- Full log of what the consumer received: app, title, body, producer node,
  source platform, plugin ID, timestamp
- Searchable and filterable via the consumer's web UI
- Configurable retention per app or globally (default: 30 days or indefinite)
- Export: JSON, CSV, plain text

### On-device ring buffer
- Each node maintains a local ring buffer (configurable, default: last 500)
- Available offline
- This is **display history**, separate from the producer outbox (delivery
  queue) and from full consumer-side history

---

## Encryption

Encryption is **optional** and **off by default** — your data, your network,
your choice.  notifwire never forces it and never refuses a plaintext
connection.  Someone running laptop-to-laptop on their own LAN gets plaintext
if they want it.

### Transport encryption (operator's choice)
Whether the wire is encrypted is up to the operator, and most real deployments
get it for free without thinking about it:
- **Tailscale / WireGuard** encrypts node-to-node at the network layer — run
  plaintext inside the tunnel and it's already encrypted.  Zero certs.
- **Cloudflare Tunnel** terminates real, auto-renewing TLS at the edge; the
  producer runs plain HTTP behind it.  Zero certs.
- **Reverse proxy (Caddy)** auto-provisions Let's Encrypt.  Zero certs.
- **Raw direct exposure** is the only case that needs the producer's own cert —
  and even then it can **auto-generate a self-signed cert with trust-on-first-
  use at pairing** (like SSH's first-connect prompt), so it stays invisible.

The producer TLS field — `off · self-signed · own cert · upstream-terminated` —
includes an **upstream-terminated / trusted-transport** mode so notifwire
doesn't refuse a plaintext connection when WireGuard or a proxy is already
doing the encrypting.

### End-to-end encryption (opt-in)
For content that shouldn't be readable in transit regardless of transport —
2FA codes, voicemail transcripts, bank alerts — notifwire offers **app-layer
E2E** as a **single checkbox, off by default**.  When enabled, it expands inline
to the key workflow: generate keys → show / copy / export the key → add it to
the consumer.

- Built on a **vetted crypto library — never hand-rolled**.  The leaning is
  **`age`**: short, copy-pasteable keys, no certificate infrastructure, no CA,
  no expiry, Rust-native.
- **Public-key model preferred:** each consumer generates a keypair on first
  run and shares only its **public** key; the producer encrypts *to* it; the
  private (decryption) key never leaves the consumer.  Nothing secret travels.
- A **symmetric pre-shared key** is the simpler alternative (one key copied to
  both ends) if chosen — at the cost of the secret having to travel.
- App-layer, so it's **independent of transport** — works over plaintext LAN,
  Tailscale, or CF Tunnel alike.

### Informing, not enforcing
notifwire **informs, the operator decides.**
- **Documentation** — a "Securing your mesh" guide lays out every option
  (Tailscale / CF Tunnel / reverse proxy / own cert / plaintext / E2E) with
  honest tradeoffs, including that plaintext means 2FA codes travel in the
  clear.
- **Warning** — a **one-time, dismissable** in-app notice when running plaintext
  on a **non-loopback** bind (actually exposed, not just localhost).
  Non-blocking; never nags after dismissal.  Makes the choice a choice, not an
  accident.

---

## Notable Use Cases

### Voicemail transcripts
iOS Visual Voicemail includes the full transcript in the notification banner.
AXObserver captures it verbatim.  Full transcript on your PC, logged to
history, searchable.  No phone required.

### Missed calls
Contact name or number forwarded immediately.  Logged with timestamp.
Effective persistent call log across your entire mesh.

### 2FA codes
Authenticator codes that appear in notification banners forwarded to your PC
the moment they appear.  See the code without touching your phone.  Pairs well
with short retention and opt-in E2E encryption.

### Finance and crypto alerts
Coinbase, banking apps, brokerage notifications delivered natively to Windows
or Linux.  No dedicated Windows client needed for any of them.

### Cross-device job completion
Any script on any machine calls `notifwire-send` when a job finishes.  The
notification routes to every consumer subscribed to that producer.  Chain jobs
across machines by combining with the HTTP webhook or MQTT output plugins.
notifwire is the signal layer.

### RSS as a notification source
Follow release feeds, status pages, blogs, or any RSS/Atom source.  New items
arrive as native OS notifications on every consumer.  Keyword filtering keeps
noise down.

### Android as always-on producer
An Android device stays online while desktops sleep.  As a producer node it
captures and buffers notifications while desktop machines are off, serving
them on reconnect.  As a consumer node it receives notifications even at 3am.

---

## Relationship to chatwire

| | chatwire | notifwire |
|---|---|---|
| Data source | `chat.db` (SQLite, persistent) | AXObserver + OS APIs (ephemeral) |
| Scope | iMessage + SMS/MMS | All OS notifications + plugins |
| Architecture | Mac producer | Direct peer nodes, any OS, no relay |
| Platform | Python / pipx | Tauri v2 (Rust + web) |
| History | Full, permanent | Configurable retention |
| Plugin system | Yes | Yes — input + output |
| iMessage handling | Yes, primary purpose | Excluded by default |

Same deployment philosophy.  Different stack.  notifwire blacklists the
Messages app by default and defers entirely to chatwire for iMessage and SMS.

---

## Known Limitations

- **iOS device disambiguation:** Continuity mirrors all paired iPhones and
  iPads to the Mac simultaneously.  notifwire cannot distinguish which iOS
  device originated a notification — filtering is by app name only.  This is
  a Continuity limitation.
- **Sleeping desktop nodes:** macOS, Windows, and Linux nodes do not capture or
  serve notifications while the machine is asleep.  Android nodes remain active.
  This is an OS constraint.
- **Notification actions:** One-directional only.  notifwire forwards
  notification content; it does not support responding to or acting on
  notifications remotely.  The round-trip timing across a network hop is not
  reliable enough to be useful.
- **AXObserver banner timing:** Banners that appear and dismiss in under ~1
  second (rare) may be missed.  Alert-style notifications are always captured.
- **Linux icon quality:** Linux .desktop icons are typically 32–48px.  Consumer
  icon lookup via Simple Icons or Clearbit is strongly recommended for Linux
  producer nodes.

---

## Out of Scope

- iMessage / SMS — chatwire
- Email notifications — use a real email client
- GitHub notifications — email with more context, or the GitHub app
- Social media — blacklisted by default in recommended config
- Notification action responses / bidirectional control
- **Node-to-node relaying / consumer chaining** — by design; sinks fan out,
  nodes don't relay to peers
- Enterprise integration (Kafka, Avro, etc.) — add your own HTTP middleware
- Cloud hosting — you host it yourself, that's the point
- Android TV / Fire TV — out of scope

---

## Versioning Roadmap

### v1 — macOS + Windows core
- macOS producer (AXObserver, Tauri menubar)
- Windows producer (WinRT notification capture, Tauri tray)
- Direct consumer↔producer mesh: pairing, auth, **SSE transport** behind a
  `MeshTransport` interface; standard HTTP handshake/error codes
- Rules engine (whitelist/blacklist, priority, dedup, DND, grouping)
- **Focuses** (per-focus trees, add vs. copy, switch schedule, default focus)
- App discovery (producer-held seen-apps list)
- History (SQLite, search, export JSON/CSV/TXT)
- Web UI (settings, focuses, history, node management, plugin management) +
  menubar agent
- Consumers: native OS display
- `notifwire-send` CLI binary (simple flags + JSON, all OSes)
- Battery monitoring
- Offline (producer outbox + per-producer cursor; queue modes)
- Simple Icons bundled (offline icon resolution)
- Consumer icon cache with auto-upgrade
- Config import / export

### v2 — Linux + Android + plugins + encryption + icon intelligence
- Linux producer (D-Bus, AppImage + .deb/.rpm)
- Android producer (NotificationListenerService, Kotlin/Tauri)
- **Headless Docker consumer image**
- **Output plugins** (`mqtt-out`, `http-out`, `apprise-out`)
- **Config sync** (`ConfigSource`: mounted file first, then git/Dropbox/HTTP;
  version-stamped, single-writer/many-readers)
- **Opt-in E2E encryption** (age, public-key preferred; expand-on-check UX)
- MCP server
- Clearbit Logo API integration (user-supplied key)
- Favicon fallback chain
- Settings → API Keys UI
- RSS / Atom input plugin (first official plugin)
- Plugin registry (GitHub install, ZIP upload, auto-update)
- Community `app_domains.json` for Clearbit domain mapping
- **WebSocket transport adapter** (alternative to SSE, behind the same
  interface) — only if a real need appears

### v3 — Ecosystem
- Always-on consumer patterns (the optional always-on node)
- Postgres history backend option
- Community plugin ecosystem (input + output)
- Notification search API (for external tooling)

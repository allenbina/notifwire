# NotifWire

**A self-hosted, peer-to-peer notification mesh for every device you own.**

NotifWire captures native OS notifications from any device and delivers them
natively to any other device — with full control over filtering, grouping,
history, and encryption.  No cloud.  No subscriptions.  No server to maintain
separately: every install is a node.

A companion project to Chatwire.  Same philosophy, different data stream.
Where Chatwire taps the iMessage database, NotifWire taps the live notification
layer across your entire device ecosystem.

You host it yourself.  That's the point.

---

## Core Concept

There is no central server.  Every NotifWire install is a node.  A node can
be a **producer** (capturing notifications from its host OS), a **consumer**
(displaying incoming notifications natively), or both simultaneously.

You install NotifWire on your Mac.  It becomes a node.  You install it on your
Windows machine.  It becomes a node.  You pair both to the hub.  Notifications
flow from every producer to every consumer automatically.

```
┌──────────────────────────────────────────────────────────────┐
│                        NotifWire Mesh                         │
│                                                               │
│   Mac Node          Android Node       Linux Node             │
│  ┌──────────┐       ┌──────────┐      ┌──────────┐           │
│  │ Producer │──┐ ┌──│ Producer │   ┌──│ Producer │           │
│  │ Consumer │  │ │  └──────────┘   │  └──────────┘           │
│  └──────────┘  │ │                 │                          │
│                ▼ ▼                 ▼                          │
│             ┌─────────────────────────┐                       │
│             │          Hub            │                       │
│             │  (any always-on node)   │                       │
│             └────────────┬────────────┘                       │
│               ┌──────────┼──────────┐                        │
│               ▼          ▼          ▼                         │
│         ┌──────────┐ ┌───────┐ ┌────────────┐                │
│         │ Windows  │ │ MQTT  │ │  Apprise   │                │
│         │ Consumer │ │ topic │ │ (100+ svcs)│                │
│         └──────────┘ └───────┘ └────────────┘                │
└──────────────────────────────────────────────────────────────┘
```

**Many producers → hub → many consumers.**  A consumer subscribes to the hub
and receives notifications from all producers at once, filtered and routed per
your rules.  You do not wire consumers to producers directly.

One node acts as the hub — typically your always-on machine or a lightweight
VPS.  Others peer to it.  Cloudflare Tunnel or similar handles external access
with no port forwarding required.

---

## Roles

### Producer
A node that captures notifications from its host OS (or from producer plugins)
and forwards them to the hub.  A Mac running NotifWire is a producer.  An
Android phone running NotifWire is a producer.  A cron job calling
`notifwire-send` on a Linux server is feeding a producer node.

### Consumer
A node that receives notifications from the hub and displays them natively on
its host OS, or forwards them to an external destination (MQTT, webhook,
Apprise).  A Windows PC running NotifWire is a consumer.  A Linux desktop is
a consumer.  MQTT is a consumer.  Apprise is a consumer.

### Hub
The hub is not a special installation — it is any node designated as the
central relay.  It aggregates from all producers, applies the rules engine,
maintains history, and fans out to all consumers.  A node can be a producer,
consumer, and hub simultaneously.

### A node can hold multiple roles
Your Mac is typically producer + consumer + hub.  Your Windows PC is consumer
only.  Your Android phone is producer + consumer.  Roles are configured per
node in the web UI.

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
- Chatwire alignment — shared toolchain, shared knowledge

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
- Linux — AppImage (universal), also `.deb` / `.rpm`
- Android — `.apk` sideload or Play Store (future)

---

## Notification Data Model

Every notification in NotifWire is normalized to a common schema regardless
of origin — OS capture or producer plugin.  Fields beyond the lowest common
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
| `app_icon` | PNG, 48x48 | Normalized at hub — see Icon System |

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
windows, no webviews.  NotifWire maps the normalized payload to whatever the
destination OS supports.

### macOS
Banner (auto-dismisses ~5s) or Alert (persists until dismissed).  User
controls per-app in System Settings → Notifications.  App icon always shown.
Title, subtitle, body, inline image.  Action buttons on Alerts only.

### iOS (via Continuity on Mac)
iOS notifications mirror to a paired Mac automatically via Continuity — no
same-network requirement, works over the internet.  All paired iPhones and
iPads mirror simultaneously.  NotifWire sees these as macOS-sourced
notifications with the iOS app name.  Which iOS device originated a
notification (iPhone vs iPad) cannot be distinguished — this is a Continuity
limitation, not a NotifWire one.

### Android
Richest notification system of all platforms.  Heads-up banner, expandable
shade (BigText, BigPicture, Inbox style), ongoing persistent notifications.
Up to 3 action buttons, inline reply via RemoteInput.  Small icon (monochrome,
24dp, required) plus optional large icon (64dp).  NotifWire captures via
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

Icons are resolved, normalized, cached, and self-improved by the hub.
Plugin and script authors do not need to solve the icon problem — the hub
handles it.

### How the cache works

The hub maintains an icon cache keyed by `app_name`, always storing the
highest-resolution version seen so far.  When a higher-resolution copy
arrives (e.g. a macOS bundle icon at 512px arriving after an earlier Android
capture at 64px), the cache entry upgrades automatically.  `icon_resolution`
in the notification schema tells the hub whether to upgrade or keep what it
has.

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
| Plugin-provided URL | Variable | Hub fetches and caches |
| Plugin manifest icon | 48–128px | Generic fallback for that plugin |

### Resolution chain (evaluated in order, stops at first hit)

```
1. Hub icon cache       — best version seen from any source so far
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
- Ships bundled with NotifWire — works offline

When `app_name` or `--icon` matches a Simple Icons slug (case-insensitive,
fuzzy matched), the SVG is used and the resolution chain stops there.

### Clearbit Logo API

For apps not covered by Simple Icons, Clearbit serves brand logos by domain.
Requires a user-supplied API key (free tier is generous for personal use).
NotifWire ships a community-maintained `app_domains.json` mapping
(`"Coinbase" → "coinbase.com"`, `"Pinterest" → "pinterest.com"`, etc.) that
grows with each release, so most lookups are automatic.

### API key management

Keys are stored locally in the node config.  Never transmitted to other nodes
or any service beyond their intended API call.  NotifWire never ships embedded
keys and never proxies lookups through any NotifWire-operated service.

Settings → API Keys:
- Clearbit Logo API key
- (future) additional icon services as community demand warrants

---

## Producers

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
- Icon quality limited — hub icon lookup recommended

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

### MQTT
Publish notification payload as JSON to a configured broker topic.  Fan out
to any number of MQTT subscribers simultaneously — n8n, Home Assistant,
custom scripts, anything with a MQTT client.

Topic structure: `notifwire/{producer_node}/{app_name}`

### HTTP Webhook
POST notification payload as JSON to any URL.  Triggers n8n, IFTTT, Zapier,
Home Assistant automations, or any custom endpoint.  Configurable per-app
or global.

### Apprise
Forward to any of Apprise's 100+ supported notification services: Telegram,
Discord, Slack, Pushover, ntfy, email, Signal, Matrix, and more.  One
integration, the entire Apprise catalog.

NotifWire is what Apprise has always needed on the receiving end.  Apprise
sends notifications out to services.  NotifWire captures them from OS sources,
routes them, stores history, and uses Apprise as one of its output adapters.
They complement each other perfectly.

### MCP Server
Every NotifWire node exposes an MCP server.  Enables Claude and any other
MCP client to query notification history, search by app or keyword, get
recent notifications, and subscribe to live events.

Example queries:
- "Did my backup job finish last night?"
- "What Coinbase alerts did I get this week?"
- "What time did that download finish on my Windows machine?"
- "Show me all missed calls in the last 7 days"
- "Did any of my nodes go offline overnight?"

---

## Producer Plugins

Producer plugins extend the producer side with non-OS notification sources.
They follow the same plugin architecture as Chatwire: official plugins,
GitHub-hosted community plugins (auto-update), and ZIP upload (no auto-update).

### Plugin contract

A plugin is a **process**, not a library.  This is intentional:

- No SDK required.  Any language that can make an HTTP POST works.
- NotifWire core (Rust/Tauri) does not need the plugin's runtime installed.
- The plugin POSTs normalized notification JSON to the local producer node's
  HTTP API (localhost:PORT).  The node handles dedup, filtering, routing,
  icon resolution — the plugin just sends the event.
- The plugin process is started, stopped, and updated by NotifWire.

### Plugin manifest (`notifwire-plugin.json`)

```json
{
  "id": "notifwire-rss",
  "name": "RSS / Atom Feed Reader",
  "version": "1.0.0",
  "author": "notifwire",
  "entrypoint": {
    "macos": "bin/notifwire-rss-macos",
    "windows": "bin/notifwire-rss-windows.exe",
    "linux": "bin/notifwire-rss-linux"
  },
  "config_schema": { },
  "update_url": "https://github.com/allenbina/notifwire-rss/releases"
}
```

NotifWire renders the `config_schema` as a settings UI automatically.  Plugin
authors define fields; NotifWire handles the form.

### Plugin tiers

**Official** — maintained in the NotifWire org.  Bundled or one-click install.
Auto-update via GitHub releases.  Held to the same quality bar as core.

**GitHub** — paste a GitHub repo URL.  NotifWire fetches the manifest, installs
the correct binary for the current OS, and checks for updates on a configurable
schedule.  Same trust model as Chatwire community plugins.

**ZIP upload** — drag in a zip containing the manifest and binaries.  No
auto-update.  For private, internal, or offline plugins.

### Official plugin: RSS / Atom (v1)

The first official producer plugin.

- Multiple feed URLs, each independently configured
- Poll interval per feed (or global default)
- `app_name` mapped per feed for correct grouping and icon lookup
- Optional keyword filter per feed (notify only if title contains X)
- Priority per feed
- Dedup by GUID — never re-notifies items already seen
- Persists seen GUIDs locally across restarts
- Feed-provided image or favicon used as icon source; falls through to hub
  resolution chain

---

## `notifwire-send` CLI

A small standalone binary, ships with every NotifWire install, lives in PATH.
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
| URL | Hub fetches, caches, normalizes |
| File path | Binary reads, base64 encodes, sends inline |
| Omitted | Hub uses `--app` value as lookup key |

`--app` doubles as the icon lookup key when `--icon` is not specified.
`--app "Coinbase"` with no `--icon` will automatically resolve the Coinbase
icon via Simple Icons.

### Environment and CI behavior

```bash
# Send to a remote hub instead of localhost
NOTIFWIRE_HOST=hub.allenbina.uk notifwire-send "Remote job done"

# Timeout is fast (default 2s) — scripts never block on NotifWire
# Exit codes are meaningful — CI-friendly
notifwire-send "Build failed" --priority urgent
echo "Exit: $?"  # 0 success, 1 node unreachable, 2 invalid args
```

NotifWire being unreachable never fails a script.  The binary exits cleanly
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

Evaluated per notification at the hub, in order, before routing to consumers.

### Filtering
- **Whitelist mode** — forward only listed apps (recommended for low noise)
- **Blacklist mode** — forward everything except listed apps
- Per-app toggle
- Keyword filters on title and/or body (include or exclude)
- iMessage and SMS excluded by default — handled by Chatwire

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
NotifWire fingerprints each notification (app + title + body) within a
configurable time window (default: 5 seconds) and drops duplicates.

### Scheduling / DND
- Per-node DND windows (e.g. 22:00–07:00)
- Urgent priority overrides DND
- Per-app DND override

---

## Offline Behavior

When a consumer node is unreachable, behavior is configurable per consumer:

| Mode | Behavior |
|---|---|
| **Queue and deliver** | Hold all notifications, deliver in order on reconnect |
| **Queue with summary** | Single summary on reconnect ("14 notifications while offline") |
| **Urgent only** | Queue High/Urgent only, drop the rest |
| **Drop** | Discard while offline, no catchup |

Android nodes remain online even with the screen off — Android push
infrastructure keeps them alive.  An old Android phone or tablet running
NotifWire makes an ideal always-on relay node for a mesh where desktop
machines sleep.

---

## Battery Monitoring

Each node monitors its own device battery and emits synthetic notifications
to the mesh when configured thresholds are crossed.

- Configurable threshold per node (default: 20%)
- Emits as Normal priority by default, configurable up to Urgent
- Useful for: Android tablet left unplugged, laptop on battery during a long
  session, any node in the mesh running low
- Implemented via Tauri system info APIs — no external dependency

---

## History

### Hub-side
- Full log: app, title, body, producer node, source platform, plugin ID,
  timestamp, delivery status per consumer
- Searchable and filterable via web UI
- Configurable retention per app or globally (default: 30 days or indefinite)
- Export: JSON, CSV, plain text

### On-device
- Each node maintains a local ring buffer (configurable, default: last 500)
- Available offline
- Syncs with hub on reconnect

---

## Encryption

Notification content regularly includes sensitive material: 2FA codes,
voicemail transcripts, bank alerts.  Encryption is a first-class feature.

- **Transit:** TLS mandatory everywhere.  No plaintext HTTP between nodes.
- **End-to-end:** Payloads encrypted on the producer node, decrypted only on
  consumer nodes.  Hub stores and routes ciphertext only in E2E mode.
- **Key management:** Device keypairs generated locally on first run.  Public
  keys exchanged during pairing.  Hub never sees plaintext in E2E mode.
- E2E is opt-in but strongly recommended for any mesh handling 2FA or
  financial notifications.

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
with short retention and E2E encryption.

### Finance and crypto alerts
Coinbase, banking apps, brokerage notifications delivered natively to Windows
or Linux.  No dedicated Windows client needed for any of them.

### Cross-device job completion
Any script on any machine calls `notifwire-send` when a job finishes.  The
notification routes to every consumer in the mesh.  Chain jobs across machines
by combining with HTTP webhook or MQTT.  NotifWire is the signal layer.

### RSS as a notification source
Follow release feeds, status pages, blogs, or any RSS/Atom source.  New items
arrive as native OS notifications on every consumer.  Keyword filtering keeps
noise down.

### Android as always-on relay
An Android device stays online while desktops sleep.  As a producer node it
captures and queues notifications while desktop machines are off, forwarding
them on wake.  As a consumer node it receives notifications even at 3am.

---

## Relationship to Chatwire

| | Chatwire | NotifWire |
|---|---|---|
| Data source | `chat.db` (SQLite, persistent) | AXObserver + OS APIs (ephemeral) |
| Scope | iMessage + SMS/MMS | All OS notifications + plugins |
| Architecture | Mac producer + hub | Peer nodes, any OS |
| Platform | Python / pipx | Tauri v2 (Rust + web) |
| History | Full, permanent | Configurable retention |
| Plugin system | Yes | Yes — producer side |
| iMessage handling | Yes, primary purpose | Excluded by default |

Same deployment philosophy.  Different stack.  NotifWire blacklists the
Messages app by default and defers entirely to Chatwire for iMessage and SMS.

---

## Known Limitations

- **iOS device disambiguation:** Continuity mirrors all paired iPhones and
  iPads to the Mac simultaneously.  NotifWire cannot distinguish which iOS
  device originated a notification — filtering is by app name only.  This is
  a Continuity limitation.
- **Sleeping desktop nodes:** macOS, Windows, and Linux nodes do not forward
  notifications while the machine is asleep.  Android nodes remain active.
  This is an OS constraint.
- **Notification actions:** One-directional only.  NotifWire forwards
  notification content; it does not support responding to or acting on
  notifications remotely.  The round-trip timing across a network hop is not
  reliable enough to be useful.
- **AXObserver banner timing:** Banners that appear and dismiss in under ~1
  second (rare) may be missed.  Alert-style notifications are always captured.
- **Linux icon quality:** Linux .desktop icons are typically 32–48px.  Hub
  icon lookup via Simple Icons or Clearbit is strongly recommended for Linux
  producer nodes.

---

## Out of Scope

- iMessage / SMS — Chatwire
- Email notifications — use a real email client
- GitHub notifications — email with more context, or the GitHub app
- Social media — blacklisted by default in recommended config
- Notification action responses / bidirectional control
- Enterprise integration (Kafka, Avro, etc.) — add your own HTTP middleware
- Cloud hosting — you host it yourself, that's the point
- Android TV / Fire TV — out of scope

---

## Versioning Roadmap

### v1 — macOS + Windows core
- macOS producer (AXObserver, Tauri menubar)
- Windows producer (WinRT notification capture, Tauri tray)
- Node pairing and mesh protocol (WebSocket, TLS)
- Rules engine (whitelist/blacklist, priority, dedup, DND, grouping)
- History (SQLite, search, export JSON/CSV/TXT)
- Web UI (settings, history, node management, plugin management)
- Consumers: native OS display, HTTP webhook, Apprise
- `notifwire-send` CLI binary (simple flags + JSON, all OSes)
- Battery monitoring
- Offline queue modes
- Simple Icons bundled (offline icon resolution)
- Hub icon cache with auto-upgrade

### v2 — Linux + Android + encryption + icon intelligence
- Linux producer (D-Bus, AppImage + .deb/.rpm)
- Android producer (NotificationListenerService, Kotlin/Tauri)
- End-to-end encryption
- MQTT consumer
- MCP server
- Clearbit Logo API integration (user-supplied key)
- Favicon fallback chain
- Settings → API Keys UI
- RSS / Atom producer plugin (first official plugin)
- Plugin registry (GitHub install, ZIP upload, auto-update)
- Community `app_domains.json` for Clearbit domain mapping

### v3 — Ecosystem
- Android as always-on relay node (producer + hub mode)
- Postgres history backend option
- Community producer plugin ecosystem
- Notification search API (for external tooling)

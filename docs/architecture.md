# Architecture

How notifwire works under the hood. For the complete technical reference, see
the [full spec](SPEC.md).

## Overview

notifwire is a mesh of nodes. There is no central server. Each node can play
three roles — **producer**, **consumer**, and **hub** — alone or in combination.

```
   Producers                  Hub                    Consumers
┌──────────────┐                                  ┌──────────────┐
│ macOS        │──┐                            ┌──│ Native OS    │
│ Windows      │  │      ┌──────────────┐      │  │ display      │
│ Linux        │──┼────▶ │  Rules +      │ ────┼──│ HTTP webhook │
│ Android      │  │      │  history +    │      │  │ MQTT         │
│ Plugins / CLI│──┘      │  icon cache   │      └──│ Apprise (100+)│
└──────────────┘         └──────────────┘         └──────────────┘
        many produce  →  one hub relays  →  many consume
```

- **Producer** — captures notifications from its host OS (or from plugins / the
  CLI) and forwards them to the hub.
- **Consumer** — receives from the hub and displays natively, or forwards to an
  external destination (webhook, MQTT, Apprise).
- **Hub** — any node designated as the central relay. Aggregates from all
  producers, runs the rules engine, keeps history, fans out to all consumers.

Many producers → hub → many consumers. Consumers subscribe to the hub; you do
not wire consumers to producers directly.

## Capture (producers)

Notification capture requires native OS bridges, implemented as Tauri plugins:

| Platform | Capture API |
|---|---|
| macOS | AXObserver (Accessibility) — includes iOS via Continuity |
| Windows | WinRT `Windows.UI.Notifications` |
| Linux | D-Bus `org.freedesktop.Notifications` (GNOME/KDE) |
| Android | `NotificationListenerService` |

Every captured notification is normalized to a common schema (app, title, body,
icon, timestamp, and optional extras) before it enters the mesh.

## The hub

The hub is the heart of the mesh:

- Runs the **rules engine** — filtering, priority, grouping, deduplication, DND
- Maintains **history** — searchable, with configurable retention and export
- Manages the **icon cache** — resolves and upgrades app icons (see the spec's
  Icon System), so plugin and script authors never have to solve icons
- **Fans out** to all consumers, honoring per-consumer offline behavior

In end-to-end encryption mode, the hub stores and routes ciphertext only — it
never sees plaintext.

## Delivery (consumers)

Consumers render via the host OS's native notification API — it looks and
behaves exactly like a local notification, because it is one. Beyond native
display, a consumer can forward to an HTTP webhook, an MQTT topic, or any of
Apprise's 100+ services. Every node also exposes an MCP server for querying
notification history.

## Platform

Built with **Tauri v2** (Rust backend, web UI). One codebase across macOS,
Windows, Linux, and Android. Binary size ~5–10MB, minimal footprint for a
process that runs 24/7 on every device.

## Data flow

1. A notification appears on a producer device
2. The native bridge captures it; the node normalizes it
3. The node forwards it to the hub (encrypted, if E2E is on)
4. The hub applies rules, resolves the icon, logs it to history
5. The hub fans out to every subscribed consumer
6. Each consumer displays it natively or forwards it onward

## Storage

History lives on the hub (SQLite by default; Postgres planned). Each node also
keeps a small local ring buffer that syncs with the hub on reconnect.

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

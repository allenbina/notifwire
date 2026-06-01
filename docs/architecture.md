# Architecture

How notifwire works under the hood. For the complete technical reference, see
the [full spec](SPEC.md).

## Overview

notifwire is a mesh of nodes. There is no central server and no relay node.
Each node plays one or both roles — **producer** and **consumer** — and
consumers connect directly to producers.

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

  Arrows = consumer subscribing directly to a producer.
  No central relay. Sinks hang off a consumer; nodes never relay to nodes.
```

- **Producer** — captures notifications from its host OS (or from input plugins
  / the CLI) and serves them to subscribed consumers. Keeps a short outbox for
  offline catch-up.
- **Consumer** — subscribes directly to one or more producers and either
  displays natively or forwards to an external destination (webhook, MQTT,
  Apprise). Holds its own rules engine, history, and cursor per producer.

Consumers never subscribe to other consumers — there is no node-to-node
relaying. Sinks (MQTT, Apprise, webhooks) are external outputs of a consumer,
not nodes in the mesh.

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

## Transport

The mesh uses **Server-Sent Events (SSE)** for v1, behind a `MeshTransport`
interface. The consumer dials the producer and holds a single long-lived HTTP
connection open. The producer writes each notification as a `data:` line; the
SSE `id:` field doubles as the catch-up cursor, so auto-reconnect sends
`Last-Event-ID` and replay is handled automatically.

Auth rides in the connect request. The producer's discovered-apps list is sent
as the first event on the stream. The same connection handles live push and
catch-up replay — there is no separate sync endpoint.

## Rules engine (consumers)

The rules engine runs at the **consumer**, per notification, before display or
re-export:

- Filtering — whitelist or blacklist mode, per-app toggles, keyword filters
- Priority — Silent / Low / Normal / High / Urgent
- Grouping — mirror source OS grouping, or manual group key
- Deduplication — fingerprint within a configurable time window
- DND — per-node windows; Urgent overrides

Producers may apply a coarse send-side filter so they never ship muted apps
over the wire at all.

## Offline catch-up

When a consumer is unreachable, the **producer** holds the buffer:

- Each producer assigns a monotonic sequence number to its own stream.
- Each consumer stores one cursor per producer.
- On reconnect, the consumer presents its cursor and the producer replays
  everything since that point, then resumes live.

The outbox is bounded by time and size (default: ~2 hours) so a chatty app
can't blow up memory during a long outage.

## Delivery (consumers)

Consumers render via the host OS's native notification API — it looks and
behaves exactly like a local notification, because it is one. Beyond native
display, a consumer can forward to an HTTP webhook, an MQTT topic, or any of
Apprise's 100+ services via output plugins. Every node also exposes an MCP
server for querying notification history.

## Icon system

Icons are resolved, normalized, and cached by the **consumer**. Each consumer
maintains an icon cache keyed by `app_name`, always keeping the
highest-resolution copy seen from any source. The resolution chain (evaluated
in order, stops at first hit):

1. Consumer icon cache
2. Simple Icons (~3000 brand SVGs, bundled, works offline)
3. Clearbit Logo API (user-supplied key)
4. DuckDuckGo / Google favicon (last resort)
5. Placeholder (app_name initial on a colored background)

Plugin and script authors set `app_name`; the consumer handles icons.

## Platform

Built with **Tauri v2** (Rust backend, web UI). One codebase across macOS,
Windows, Linux, and Android. Binary size ~5–10MB, minimal footprint for a
process that runs 24/7 on every device.

## Data flow

1. A notification appears on a producer device
2. The native bridge captures it; the node normalizes it
3. The producer assigns a sequence number and adds it to the outbox
4. Subscribed consumers receive it over SSE (or pull it on reconnect via cursor)
5. Each consumer's rules engine evaluates it
6. The consumer displays it natively or forwards it via an output plugin

## Storage

History lives on the **consumer** (SQLite by default; Postgres planned). Each
node also keeps a small local ring buffer available offline. The producer
maintains a bounded outbox for catch-up only — it is not a history store.

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

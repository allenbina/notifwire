# Setup Guide

> **Status: active development.** notifwire is not yet released. This guide
> describes how setup will work for v1 (macOS + Windows core); the Windows core
> (capture + native display) already works. It will be kept current as the build
> progresses. ⭐ Star the repo to follow along.

notifwire is a mesh: every install is a node, and one always-on node acts as the
**hub**. Getting running means installing on each device, picking a hub, and
pairing.

## Prerequisites

- One or more devices: macOS, Windows, Linux (GNOME/KDE), or Android
- One device that stays on, to act as the hub (a desktop, a Pi, a spare Android
  device, or a small VPS)
- (Optional) Cloudflare Tunnel or similar for external access without port
  forwarding

## Install

Download the build for each device:

- **macOS** — `.dmg`, runs as a menubar app
- **Windows** — installer or portable `.exe`, runs in the tray
- **Linux** — AppImage (universal), or `.deb` / `.rpm`
- **Android** — `.apk` sideload (Play Store later)

The `notifwire-send` CLI is bundled with every install and added to your PATH.

## Grant capture permissions

Capturing notifications needs a one-time OS permission on producer nodes:

- **macOS** — Accessibility permission, in **System Settings → Privacy &
  Security → Accessibility**. This lets notifwire observe notification banners
  (including iOS/iPadOS mirrors via Continuity).
- **Android** — Notification Access, in **Settings → Notifications →
  Notification access**.
- **Windows / Linux** — no special permission required for basic use.

## Designate the hub

Pick your always-on device and enable **hub** mode in its web UI. The hub
aggregates from all producers, applies the rules engine, keeps history, and fans
out to all consumers. A node can be producer, consumer, and hub at once.

## Pair your devices

On each other node, pair to the hub from the web UI. Device keypairs are
generated locally on first run and public keys are exchanged during pairing.
Once paired, notifications flow from every producer to every consumer
automatically — you don't wire consumers to producers directly.

## Send from scripts

Any script, cron job, or CI pipeline can inject a notification:

```bash
notifwire-send "Backup complete" --app "rsync" --priority high
```

```bash
# Send to a remote hub instead of localhost
NOTIFWIRE_HOST=hub.example.com notifwire-send "Remote job done"
```

notifwire being unreachable never fails your script — it exits with a non-zero
code and your pipeline continues.

## Configuration

Each node is configured from its web UI: roles, filtering (whitelist/blacklist),
priority mapping, grouping, deduplication, DND windows, offline behavior,
history retention, and consumers (native, webhook, MQTT, Apprise).

## Troubleshooting

### Notifications aren't being captured (macOS)
Accessibility permission isn't granted, or the app needs a restart after
granting it. Revisit **Privacy & Security → Accessibility**.

### Notifications aren't being captured (Android)
Check **Notification access** is enabled for notifwire.

### A consumer isn't receiving
Confirm it's paired to the hub and online. Check its offline-queue mode — it may
be set to drop rather than queue while disconnected.

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

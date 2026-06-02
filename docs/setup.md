# Setup Guide

> **Status: active development.** notifwire is not yet released. This guide
> describes how setup will work for v1 (macOS + Windows core); the Windows core
> (capture + native display) already works. It will be kept current as the build
> progresses. ⭐ Star the repo to follow along.

notifwire is a mesh: every node connects directly to every other node it needs
to reach. Getting running means installing on each device and pairing consumers
to producers.

## Prerequisites

- One or more devices: macOS, Windows, Linux (GNOME/KDE), or Android
- At least one always-on device to act as a producer (a desktop, a Pi, a spare
  Android device, or a small VPS) if you want persistent capture
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

## Pair your devices

On each consumer node, pair directly to each producer from the web UI. Device
keypairs are generated locally on first run and public keys are exchanged during
pairing. Once paired, each consumer subscribes directly to its producers — there
is no relay or forwarding node. A node can be both a producer and a consumer at
the same time.

## Send from scripts

Any script, cron job, or CI pipeline can inject a notification:

```bash
notifwire-send "Backup complete" --app "rsync" --priority high
```

```bash
# Send to a remote producer instead of localhost
NOTIFWIRE_HOST=producer.example.com notifwire-send "Remote job done"
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
Confirm it's paired directly to the relevant producer and that the producer is
online. Check the consumer's offline-queue mode — it may be set to drop rather
than queue while disconnected.

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

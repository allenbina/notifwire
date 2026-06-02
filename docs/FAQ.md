# Frequently Asked Questions

## General

### What is notifwire?
notifwire is a self-hosted, peer-to-peer notification mesh. It captures native
OS notifications from your devices and delivers them natively to any other
device — with filtering, grouping, history, encryption, and routing to webhooks,
MQTT, and Apprise.

### Is it free?
Yes. notifwire is MIT licensed and free to use, modify, and distribute.

### Does my data go to the cloud?
No. notifwire runs entirely on your own devices. There is no central notifwire
server. Notifications never leave your hardware unless you explicitly configure a
consumer (webhook, MQTT, Apprise) to send them somewhere.

### Is it ready to use?
Not yet — but it's actively being built. The full spec is published and v1
(macOS + Windows core) is in development; the Windows core (toast capture +
native display) already works end-to-end. Star the repo to follow along.

### How does it relate to chatwire?
Same philosophy, different data stream. chatwire bridges your iMessages;
notifwire bridges your OS notifications. notifwire excludes the Messages app by
default and defers to chatwire for iMessage/SMS.

## Architecture

### Is there a server I have to run?
No separate server. Every install is a node. You designate one always-on node
(a desktop, a Pi, a spare Android device, a small VPS) as the **hub**, and your
other devices peer to it. The hub is just a node with a role, not a separate
product.

### What platforms are supported?
macOS, Windows, and Linux (GNOME/KDE) as producers and consumers, plus Android.
iOS notifications are captured via Continuity through a paired Mac. See the
[architecture guide](architecture.md).

### Do I need port forwarding?
No. Cloudflare Tunnel (or similar) handles external access with no port
forwarding required.

## Privacy & Security

### Does notifwire collect telemetry?
No. Zero telemetry, zero analytics, zero phone-home.

### Is my data encrypted?
Transport is TLS everywhere — no plaintext between nodes. End-to-end encryption
is opt-in and strongly recommended for sensitive content: payloads are encrypted
on the producer and decrypted only on consumers, so the hub routes ciphertext it
cannot read.

### Can I forward 2FA codes safely?
Yes — that's a flagship use case. Pair it with end-to-end encryption and short
retention. See [why notifwire](why.md).

## Plugins & CLI

### What can I send from a script?
Anything. The `notifwire-send` CLI ships with every install and lets any script,
cron job, or CI pipeline inject a notification into the mesh. It never blocks or
fails your script.

### What are producer plugins?
Plugins add non-OS notification sources (RSS, custom services). A plugin is a
**process**, not a library — any language that can make an HTTP POST works. No
SDK required. See the [plugin guide](plugins.md).

### Are plugins sandboxed?
Plugins run as separate processes that POST to the local node's API. Only
install plugins you trust. See the [plugin guide](plugins.md).

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

# Why notifwire

> This page is the "why" / "what for" of notifwire — the case for the product,
> in plain language. It is the source material for the website's *Why* and
> *What* pages. The full technical design lives in [SPEC.md](SPEC.md).

A notification today is something you *glance at* and forget. notifwire turns it
into a **signal you can route, store, search, and act on** — across every device
you own, with no cloud and no central server.

---

## The big one: notifications that are trapped on your phone

Some of the most important alerts you get **only ever appear on your phone**.
There is no email, no web dashboard, no desktop equivalent. If your phone is in
the other room — or you're heads-down on your laptop — you miss them, or you
stop what you're doing and go get your phone.

notifwire frees these. It captures them at the source and delivers them natively
to whatever device you're actually using.

### 2FA / one-time login codes — the killer example

A six-digit code lands on your phone. You're working on your Windows or Linux
machine. Today that means reaching for your phone, unlocking it, reading the
code, typing it back. **notifwire forwards the code to the device you're already
on, the instant it appears.** (Pairs naturally with short retention and
end-to-end encryption — these are sensitive by nature.)

### App-only push

Plenty of apps notify *only* inside their mobile app — no email digest, no web
view, no desktop client. The event happens, your phone buzzes, and that's the
only place it exists. notifwire is the only way to see those events anywhere
else.

### Other phone-only alerts notifwire liberates

- **Bank / card "transaction posted"** — usually push-only, never emailed.
- **Ride-share & delivery** — "your driver is arriving," "order delivered,"
  "food is on its way." Time-sensitive and push-only.
- **Package delivery** — carrier-app "Delivered" alerts.
- **Airline app** — gate change, boarding, delay. You want these on your laptop
  in the lounge, not buried in your pocket.
- **Find My / AirTag** — separation and location alerts.
- **Voicemail transcripts & missed calls** — inherently phone-bound; notifwire
  surfaces the full transcript and a searchable call log on every device.

The throughline: if it only lives on your phone, notifwire is the bridge to
everywhere else.

---

## Notifications you can act on

Because every alert can be routed to a webhook, MQTT, or a script via
`notifwire-send`, a notification stops being a dead-end. It becomes the trigger
for the *next* thing.

- **A download finishes (e.g. FileZilla) → kick off a script that processes the
  file.** The toast that used to just say "done" now *starts* the next step.
- **The washing machine finishes → n8n → flash or recolor the smart lights.**
  A push from one ecosystem drives an automation in another.
- **Your Apple Watch finishes charging → a toast pops on your Linux laptop.**
  An event you'd normally only ever see on Apple hardware, surfaced anywhere.
- **A CI / build job finishes on a remote server → native toast on whichever
  desktop you're sitting at.** One `notifwire-send` line in the pipeline.
- **A finance or crypto alert (Coinbase, bank, brokerage) → delivered natively
  to Windows/Linux and logged to searchable history.** No phone-grabbing, and a
  permanent record.
- **An RSS release feed posts a new version → native notification on every
  device.** Status pages, release feeds, blogs — all become push.
- **Any node's battery drops below threshold → the whole mesh hears about it.**
  The tablet left unplugged in the kitchen tells you before it dies.
- **Doorbell or camera motion → MQTT → Home Assistant automation.**
- **An overnight backup finishes → a tidy summary waiting on your desktop in the
  morning**, instead of nothing or a buried log line.

---

## What makes this possible

- **No central server.** Every install is a node; any always-on node is the hub.
  You own the whole pipeline.
- **Native in, native out.** Captured from real OS notification APIs, delivered
  as real OS notifications — it looks and behaves like a local notification,
  because it is one.
- **Routable.** Beyond native delivery: HTTP webhook, MQTT, and Apprise's 100+
  services (Slack, Discord, ntfy, Telegram, email, and more).
- **Queryable.** Full searchable history, plus an MCP server so you can ask
  questions like "did my backup finish last night?"

See [SPEC.md](SPEC.md) for how all of this works under the hood.

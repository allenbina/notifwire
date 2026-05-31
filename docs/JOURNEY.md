# The notifwire Journey

The story behind the project.

## The itch

It started with a familiar frustration: the notification I needed was on the
wrong device. A 2FA code lands on the phone while I'm heads-down on a laptop. A
job finishes on a server and tells no one. A bank alert buzzes a pocket and
vanishes. The signals about my own life were scattered across devices and
trapped inside apps.

The information was right there — every OS has a notification layer. But there
was no clean way to gather it, route it, and act on it across everything I own.
So I started building one.

## The principle

The same principle that anchors chatwire anchors notifwire: this only works if
the data never leaves your control. The moment you route personal alerts through
someone else's cloud, you've recreated the exact problem you were trying to
escape.

So notifwire is self-hosted by design, with no central server. Every install is
a node; any always-on node is the hub. Not as a feature — as a foundation.

## The build

notifwire is built on Tauri v2 — one small Rust + web codebase across macOS,
Windows, Linux, and Android. Capture happens through native OS bridges;
delivery happens through native OS notifications. In between sits a hub that
filters, groups, dedups, stores, and routes.

Each piece follows the same rule as the rest of the family: small, sharp,
composable. The `notifwire-send` CLI is one binary any script can call. Plugins
are plain processes that speak HTTP. No SDK, no lock-in, no accounts.

## The family

notifwire is the second of the *wire* family — self-hosted tools that give you
control over your own data streams. Where chatwire taps the iMessage database,
notifwire taps the live notification layer. Same philosophy, different data.

## Where it's going

The roadmap goes macOS + Windows first, then Linux + Android + end-to-end
encryption + the icon intelligence and plugin ecosystem, then Android-as-relay
and external tooling. Sharper, not bigger — without betraying the founding
principle. Your notifications stay yours.

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

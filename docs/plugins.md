# Plugin Development

notifwire plugins extend notifwire on both sides of the mesh — **input plugins**
add non-OS notification sources to a producer (RSS feeds, custom services,
anything that can generate an event), while **output plugins** deliver
notifications from a consumer to an external sink.

## Plugin model

A notifwire plugin is a **process, not a library**. This is intentional:

- No SDK required. Any language that can make an HTTP POST (input) or receive
  one (output) works.
- notifwire core (Rust/Tauri) does not need the plugin's runtime installed.
- **Input plugins** POST normalized notification JSON to the local producer
  node's HTTP API (`localhost:PORT`). The node handles dedup, filtering,
  routing, and icon resolution — the plugin just sends the event.
- **Output plugins** receive normalized JSON from their consumer host and
  deliver it to a single sink. Typically packaged as one Docker container per
  sink technology; can run headless from a JSON config.
- notifwire starts, stops, and updates the plugin process for you (or, for
  containerized output plugins, the operator deploys them).

## Plugin tiers

notifwire supports three tiers of plugins:

1. **Official** — maintained in the notifwire org, held to the core quality bar,
   auto-updated via GitHub releases
2. **Community (GitHub)** — installed from a GitHub repo URL, auto-updated on a
   configurable schedule
3. **Local (ZIP)** — drag in a zip with the manifest and binaries, no
   auto-update, for private/offline use

## A minimal input plugin

Send a notification by POSTing JSON to the local producer node. In any language:

```bash
curl -s localhost:$NOTIFWIRE_PORT/api/notify -d '{
  "title": "New release: v2.1.0",
  "body": "tauri-apps/tauri published a release",
  "app_name": "GitHub",
  "priority": "normal",
  "icon": "github"
}'
```

The producer normalizes the payload, resolves the `github` icon, assigns a
sequence number, and streams it to every subscribed consumer.

## The manifest

Every plugin needs a `notifwire-plugin.json` manifest:

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

The `kind` field is `"input"` for producer-side plugins and `"output"` for
consumer-side sink plugins. notifwire renders the `config_schema` as a settings
UI automatically. Plugin authors define the fields; notifwire handles the form.
A plugin running headless reads the same fields from a JSON config file.

## Icons

You don't need to solve the icon problem. Set `app_name` (or send an `icon`
hint — a brand name, domain, URL, or file path) and each consumer's icon cache
and resolution chain take it from there. See the Icon System in the
[full spec](SPEC.md).

## Official input plugin: RSS / Atom

The first official input plugin polls RSS/Atom feeds and emits new items as
notifications — multiple feeds, per-feed poll interval, keyword filters,
priority, and GUID-based dedup so items never re-notify. It's the reference
implementation of the input plugin contract.

## Best practices

- Keep it simple — a plugin only needs to POST events; the node does the rest
- Handle errors gracefully — don't crash; fail soft like `notifwire-send` does
- Persist your own dedup state (e.g. seen GUIDs) across restarts
- Respect privacy — don't exfiltrate notification data without consent
- Document your `config_schema` fields

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>

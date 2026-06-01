# Windows notification capture — mechanism & packaging decision (D1-3 spike)

## The question

How does a notifwire Windows producer capture *other apps'* toast
notifications, and what does that force on how we package/distribute it?

## Findings

The supported API is **`Windows.UI.Notifications.Management.UserNotificationListener`**
(`UserNotificationListener.Current`, `RequestAccessAsync`, `GetNotificationsAsync`,
and the `NotificationChanged` event). There is no other first-class Windows API
for reading the notifications of arbitrary apps.

Two hard requirements:

1. **Package identity.** `UserNotificationListener` requires the calling process
   to have package identity. A plain (unpackaged) Win32 app — which is what a
   default Tauri build is — hits `APPMODEL_ERROR_NO_PACKAGE` /
   `E_ILLEGAL_METHOD_CALL` when it touches these APIs.
2. **The `userNotificationListener` restricted capability** must be declared in
   the package manifest, **and** the user must grant access at runtime via
   `RequestAccessAsync` (a first-run consent prompt). Without the grant the
   listener returns nothing.

So the capture API is reachable only from a process with package identity + the
declared capability + a runtime user grant. This is a real fork in how the
Windows producer ships.

## Options

| Option | What it means | Friction |
|---|---|---|
| **A. Sparse package / "packaging with external location"** | Keep the normal Tauri `.exe`/installer, add a small MSIX *sparse* package that grants identity and declares the capability. No switch to full MSIX, installer unchanged. | Must be **code-signed** with a cert trusted on the machine. Build adds a manifest + signing step. Lowest-friction path that actually works. |
| **B. Full MSIX packaging** | Ship the whole app as an MSIX. Tauri's default Windows bundlers are NSIS/WiX-MSI, not MSIX, so this fights the toolchain. | Highest friction; also needs signing; changes distribution model. |
| **C. Don't use `UserNotificationListener`** | Avoid the API. | No supported alternative exists for capturing arbitrary apps' notifications on Windows. Not viable for the Windows *producer's* core purpose. |

### The signing wrinkle (matters for a self-hosted tool)

Any identity path (A or B) requires the package to be **code-signed**, and the
signing cert must be trusted on the user's machine. For an open-source,
self-hosted tool that means either: ship a signed package (we hold a cert), or
document how a self-hoster trusts a bundled self-signed cert. This is the same
"operators handle their own trust" posture we already took for transport
encryption — but it's a real install-time step on Windows, not optional.

## Recommendation

**Option A — sparse package (packaging with external location).** It's the only
low-friction route that keeps Tauri's normal installer while unlocking the
capture API. The producer flow becomes: app has identity (sparse pkg) → declares
`userNotificationListener` → on first run calls `RequestAccessAsync` (D1-5
onboarding) → listens via `UserNotificationListener` (D1-4).

## What this gates

- **D1-4 (capture bridge)** and **D1-5 (onboarding)** depend on this choice —
  hold them until it's confirmed.
- A runnable capture proof can't exist until identity is set up (an unpackaged
  build can't even call the API), so the proof lands *with* D1-4, not before.
- **D1-7 (durable outbox)** is independent of this decision and can proceed.

## Decision

> **Pending** — surfaced to the maintainer. Default recommendation: Option A.

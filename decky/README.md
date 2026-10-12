# XREAL (Decky Loader plugin)

A game-mode control panel for the installed [xreal-linux](../) runtime. It drives the runtime through `systemctl --user`
and the runtime's own CLI **as your session user** — the panel never speaks the glasses' protocol itself, so only the
requests the runtime already makes ever reach the glasses.

## Modes

| Mode | What it does |
| --- | --- |
| **Off** | `xreal-linux.socket` and `xreal-headlook.service` are both disabled. The glasses stay an ordinary monitor. |
| **Head mouse** | `xreal-headlook.service` is enabled; the socket is disabled. Head motion drives a virtual mouse, no SteamVR. |
| **SteamVR** | `xreal-linux.socket` is enabled; the head-look service is disabled. The background service waits to present a SteamVR session. |

The panel shows the units' **real** state (enabled/active), not the last button pressed, so a reboot or a manual
`systemctl` change is reflected when you next open it.

If you switch away from **SteamVR** while a SteamVR session is running, the panel asks for confirmation first, because
that stops the presenter and takes the glasses' display away mid-session.

## Runtime

If the runtime is not installed, the panel shows a **Runtime missing** section with an install button. It also reports
when an installed runtime is **too old** to describe itself to the panel.

Install/update downloads the release that matches the plugin's own version: it fetches the `SHA256SUMS` for that tag,
downloads `xreal-linux-x86_64.AppImage`, verifies the published checksum, and only then places the file and runs
`setup --yes --no-enable`. That places the runtime and **all** the user units but enables none of them, so a fresh install
is **Off** — the panel's mode switch is what turns head mouse or SteamVR on. A mismatch (no asset for the tag, or a bad
checksum) installs nothing. SteamVR does not need to be installed first: the runtime and its units are placed either way,
and the install only reports the driver registration as a note until SteamVR is present.

Uninstalling the plugin **never** removes the runtime, your units or your settings — that stays yours.

## Development

    pnpm install
    pnpm run build      # writes dist/index.js (what a release bundles)
    pnpm test           # python3 -m unittest over the backend's pure helpers

The release workflow (`.github/workflows/release.yml`) builds this directory and publishes the plugin bundle alongside
`SHA256SUMS`, next to the AppImage.

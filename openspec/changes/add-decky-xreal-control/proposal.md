# Proposal

## Why

The two working tiers — the head-look input device (`xreal-presenter headlook`) and the SteamVR background service
(`xreal-linux.socket`/`.service`) — can only be started from a terminal, and the project explicitly does not promise
the Deck's game mode today. A Decky Loader plugin puts one control for both tiers in the Steam Deck's Quick Access
Menu, so the glasses' head-tracked and SteamVR behaviour can be turned on and off where a game is actually launched.

## What Changes

- **New Decky plugin** (a new `decky/` directory, built with `pnpm`, not shipped inside the AppImage): a generic
  **XREAL** control panel for game mode. Its central control is a single mutually-exclusive **mode selector** —
  `Off` / `Head mouse` / `SteamVR` — plus a status readout (glasses reachable, unit state, installed version). The
  1S is the only device profile now; the plugin renders controls from the device's capabilities so other XREAL
  glasses can be added later.
- **New user unit `xreal-headlook.service`**, written by the existing `setup` (opt-in, same "Managed by
  xreal-linux" marker, removed by `uninstall`), so head mouse can be enabled/disabled as a service rather than a
  foreground process. `setup`/`uninstall` remain the only writer of user unit files.
- **Runtime reports the device and its capabilities** in a machine-readable form (for example `status --json`),
  including the connected XREAL model and the controls it supports, so the generic plugin can render the panel and
  the mode selector.
- **The plugin installs and updates the runtime.** The release carries the AppImage, a checksum file and the Decky
  plugin bundle from the same tag; the plugin detects whether the runtime is installed and offers a button that
  downloads its matching AppImage, verifies the checksum, and runs `setup --yes` as the session user.
- **Nothing else changes** in the driver, the presenter's SteamVR path, the IMU parser/fusion, the glasses'
  request allowlist, or the flat-stereo (`sbs`) path.

## Verified on hardware versus assumed

Verified on the Deck (existing, unchanged by this change): the `headlook` subcommand drives a rootless `uinput`
mouse (`uaccess` ACL) and the mouse is the device that works; `setup` writes and enables `xreal-linux.socket` and
`uninstall` removes it; the 1S is identified by EDID (`MRG` `0x4102`) and the installed paths are
`~/.local/share/xreal-linux/` and `~/.config/systemd/user/`.

Assumed, each with the experiment that settles it:

| Assumption | Experiment |
|---|---|
| A Decky plugin backend can run a user-session command as the logged-in user (`systemctl --user`, and the AppImage that needs `WAYLAND_DISPLAY`/`DBUS`/`/dev/uinput`) while the backend itself runs as root | Build a minimal plugin that reports `systemctl --user is-active xreal-linux.socket` and runs `xreal-presenter headlook` (Phase 1) |
| A Decky plugin may download and execute the released AppImage (store policy and the root backend) | Local install from a zip, then read the submission policy for binary-downloading plugins (Phase 1) |
| Enabling/disabling `xreal-headlook.service` from the panel persists across a reboot and the service reconnects to the IMU on its own | Enable from the panel, reboot, check the unit and the virtual device (Phase 1) |
| The runtime can report the connected model and a per-model capability set; today only the EDID is known | Read the model from the existing allowlisted getters (`GetConfig` and friends) and record what is available; if nothing does, report the capability set by EDID-derived model (Phase 0) |
| The plugin can map its version to the matching AppImage on the release and verify it | Dry-run the download against a real tagged release and check the checksum (Phase 1) |

## Non-goals

- Supporting XREAL glasses other than the 1S. The plugin is architected generically, but only the 1S capability
  profile is implemented.
- Glasses functions beyond the mode selector — brightness, display modes, electrochromic dimming, volume. Each is a
  new request to the glasses and, under the project's rules, is allowlisted, byte-for-byte approved and
  hardware-verified in its own change.
- Flat stereo (`sbs`) as a mode-selector entry. It is the obvious fourth mode but needs its own unit and is out of
  scope here.
- Submitting the plugin to the Decky plugin store, and any change to the IMU/fusion, the SteamVR driver, the
  presenter's VR path, or the Eye camera.

## Capabilities

### New Capabilities

- `decky-control`: the game-mode XREAL panel — the mutually-exclusive mode selector (off / head mouse / SteamVR),
  the status readout, exclusivity between the two tiers, running actions as the session user, and requiring the
  runtime to be installed.
- `runtime-provisioning`: installing, detecting and updating the runtime from a release — the download, checksum
  verification, version coupling and running `setup` as the session user.
- `device-capabilities`: the runtime reporting the connected XREAL device and the controls it supports in a
  machine-readable form, so a generic front-end can render itself.

### Modified Capabilities

- `head-look-input`: adds the opt-in user unit (`xreal-headlook.service`) with enable/disable/start/stop and the
  service lifecycle, beyond the current foreground-only behaviour.
- `installable-package`: the release is no longer a single file — it also carries the Decky plugin bundle and a
  checksum file from the same commit; `setup` offers the head-look unit and `uninstall` removes it.

## Impact

- `decky/` (new): `plugin.json`, `main.py`, `src/index.tsx`, `package.json`, `rollup.config.js`, `LICENSE`, built
  with `pnpm`; not part of the AppImage.
- `presenter/src/setup/` : a third unit (`units.rs`), offered by `setup` (`install.rs`, `guided-setup`) and removed
  by `uninstall`; the runtime version and device report (`status`/`check` or a new command).
- `presenter/src/headlook.rs`: optionally a `--service` mode suited to the unit (no terminal, restart on IMU loss
  already handled by `tracking::run`).
- `packaging/` and `.github/workflows/release.yml`: build and attach the plugin bundle and a `SHA256SUMS` file
  alongside the AppImage from the same tag.
- `docs/`: `README.md`, `presenter/README.md`, `handoff.md`, `findings.md`/`open-questions.md`, and the new
  `decky/README.md`.
- No change to the driver, the glasses' message allowlist, the SteamVR path, or any SteamVR setting.

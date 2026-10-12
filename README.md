# xreal-linux

Use **XREAL 1S** glasses as a **SteamVR headset** on Linux (3DoF: head rotation from the glasses' own sensors). Verified on a **Steam Deck**
(Bazzite, Plasma on Wayland, AMD GPU). Other AMD PCs with Plasma on Wayland should work the same way; NVIDIA and Intel GPUs are
**not** supported yet (untested), and the Eye camera (6DoF) is not used.

Two ways to use it:

- **AppImage** — install once from the desktop, drive everything from a terminal. Good for desktop-mode use and for SteamVR through Steam itself.
- **Decky plugin** — a Quick Access Menu panel in game mode: pick **Off / Head mouse / SteamVR** with a few clicks, no terminal.

Both drive the same runtime; you can use either or both.

## Install with the AppImage

You need: the glasses plugged in over USB-C, Steam with **SteamVR** installed (Steam app 250820) and started **once** so it has written its
settings, and a second display enabled (the Deck's own screen) so desktop windows have somewhere to go besides the glasses.

1. Download `xreal-linux-x86_64.AppImage` from the [latest release](https://github.com/kevindurb/xreal-linux/releases/latest) and make it
   executable (`chmod +x`, or *Properties > Permissions* in the file manager).
2. Run it (double-click, or `./xreal-linux-x86_64.AppImage`). Answer the questions: it installs for your user only (no root), registers the
   driver with SteamVR, offers to enable a background service, offers the opt-in head-look service (head mouse, off unless you say yes), and
   offers to change four SteamVR settings (each is shown and asked about, and a backup is kept). `--dry-run` shows what it would do without
   changing anything.
   *If the AppImage does not start because FUSE is missing, run it as `./xreal-linux-x86_64.AppImage --appimage-extract-and-run`.*
3. On the glasses, set **Follow mode** (not Anchor), **Stabilizer off** and **auto sleep off** in their own menu (double-click the X button,
   then Display, ...). The host can neither read nor set these.
4. Start SteamVR from Steam. The glasses switch to full side-by-side by themselves and the headset appears; when SteamVR quits they go back
   to the mode they were in. Each switch re-plugs the glasses' display for about two seconds, so the desktop may rearrange windows.
   Nothing is done to the glasses while SteamVR is not running, so you can use them as a normal monitor.

### AppImage commands

Afterwards, from the installed copy (`~/.local/share/xreal-linux/xreal-linux.AppImage`) or the downloaded file:

| Command | What it does |
|---|---|
| `check` | Reports what is in place and what is not (changes nothing); exits non-zero on a failure |
| `fix` | Sets the SteamVR settings the setup needs, asking for each (SteamVR must not be running) |
| `status` | Shows the installed version, the service, the glasses' display mode and the last session's log |
| `uninstall` | Undoes everything `setup` did: the service, the driver registration, the changed settings, the installed files |
| `restore-display` | Puts the glasses back in the 2D mode they were in before a session, if a restore is still pending |

The presenter's log is in the journal: `journalctl --user -u xreal-linux`. Options for the service go in
`~/.config/xreal-linux/service.env` (`XREAL_REPROJECT=0` turns reprojection off, `XREAL_EXTRA_ARGS="..."` adds presenter flags).

**Head mouse as a service (opt-in).** Answering yes to the head-look question in `setup` writes and enables the user unit
`xreal-headlook.service`, which runs `xreal-presenter headlook --service` with no terminal (head motion drives a virtual mouse). Enable/Disable
it as `systemctl --user enable/disable --now xreal-headlook.service`. Its options go in `~/.config/xreal-linux/headlook.env`
(`XREAL_HEADLOOK_DEVICE=mouse`, `XREAL_HEADLOOK_MODEL=stick`, `XREAL_HEADLOOK_ARGS="--sensitivity 1.5 --range 45"`). `uninstall` stops and
removes it. The glasses still need Follow mode, Stabilizer off and auto sleep off.

## Use with the Decky plugin (game mode)

A [Decky Loader](https://github.com/SteamDeckHomebrew/decky-loader) plugin puts the controls in the Quick Access Menu. The plugin ships in the
same release as the AppImage, verified by a published `SHA256SUMS`.

1. Install [Decky Loader](https://github.com/SteamDeckHomebrew/decky-loader), then install this plugin from its bundle
   (see [decky/README.md](decky/README.md)).
2. Open the panel in the Quick Access Menu. If the runtime is not installed yet, use the **Install runtime** button: it downloads the
   matching release, verifies the `SHA256SUMS` checksum, places the files and every user unit — but enables none of them, so a fresh install
   is **Off** until you pick a mode. (SteamVR does not need to be installed first; the driver registration is only reported as a note until
   it is.)
3. Pick a mode:

| Mode | What it does |
| --- | --- |
| **Off** | Everything disabled; the glasses stay an ordinary monitor. |
| **Head mouse** | Head motion drives a virtual mouse, no SteamVR. |
| **SteamVR** | The background service waits to present a SteamVR session. Start SteamVR and the headset appears. |

The panel shows the units' **real** state (enabled/active), not the last button pressed, so a reboot or a manual `systemctl` change is
reflected when you next open it. Switching away from **SteamVR** while a session runs asks for confirmation first, because that stops the
presenter mid-session.

The plugin is a thin front end: it runs `systemctl --user` and the runtime's own CLI as your session user and never touches the glasses'
protocol itself. Uninstalling the plugin never removes the runtime, your units or your settings. The glasses still need **Follow mode,
Stabilizer off, auto sleep off** in every mode. *Using the background service from game mode is not yet verified — if it misbehaves, install
with the AppImage once from desktop mode.*

## Troubleshooting

- *SteamVR says "Headset Not Detected (108)" or "Some Add-ons Blocked"* — SteamVR's safe mode disabled the driver after a crash or a slow
  start. Quit SteamVR, run `check` (it reports `driver_xreal.blocked_by_safe_mode` as a failure) and then `fix`, which clears the block after
  asking.
- *AppImage does not start (no FUSE)* — run `./xreal-linux-x86_64.AppImage --appimage-extract-and-run`.
- *Headset never appears* — run `check`, confirm SteamVR is installed and has been started once, and confirm the glasses are in
  **Follow mode** with the stabilizer and auto sleep off.

**Not promised:** Flatpak Steam, and GPUs other than AMD.

## Development and research notes

The rest of the repo and `docs/` are engineering notes: how the pieces work and what was measured. Project state at a glance —
video, audio, 3DoF tracking and the SteamVR integration work; install is a guided AppImage plus the Decky panel; the Eye camera
(6DoF/SLAM) is parked. Details in [docs/findings.md](docs/findings.md) and unverified items in [docs/open-questions.md](docs/open-questions.md).

- `tools/decode_imu.py` — read the IMU stream from the glasses and print gyro/accel.
- `tools/decode_camera_frame.py` — split a captured camera frame into its parts and save PNGs.
- `tools/imu_web/` — a browser page that streams live IMU graphs and walks through direction tests (yaw / pitch / roll) to pin down
  which gyro axis and sign each head motion is. Run it on the Deck with `python3 ~/xreal-linux/tools/imu_web/server.py` (reads
  169.254.1.1:52998), typically over `ssh -L 8765:localhost:8765 steamdeck`. Press **Calibrate**, hold still 3 s, then **Run all**;
  **Save** writes `captures/axis-map-*.json`. Tests: `node tools/imu_web/test_core.mjs` and
  `python3 -m unittest tools/imu_web/test_server.py`. The server binds to 127.0.0.1 by default; `--bind 0.0.0.0` exposes an
  unauthenticated API that can write into `captures/`, so avoid it on untrusted networks.
- Without the glasses: `--source replay:captures/imu_yaw.bin` replays a capture, `--source sim` generates labelled synthetic data.

The scripts above only read from the glasses. The presenter's only writes are the display input mode: full side-by-side while SteamVR
uses the glasses, and back to the previous 2D mode afterwards (see `docs/handoff.md` for the rules).

**Privacy note.** Camera captures show the inside of a home, so raw captures and rendered images are deliberately not in this repo
(`captures/` is git-ignored).

## Development workflow (OpenSpec)

Changes are planned with [OpenSpec](https://github.com/Fission-AI/OpenSpec): `openspec/specs/` holds what the system does today
(`imu-tracking`, `presenter-display`, `steamvr-driver`, `glasses-setup`), `openspec/changes/` holds proposed changes as deltas against
those specs, and `openspec/config.yaml` gives the AI the project context (architecture, hard facts about the glasses and SteamVR,
build and test environment). In Claude Code, `/opsx:propose "idea"` creates a proposal, specs delta, design and tasks, `/opsx:apply`
implements them and `/opsx:archive` folds them into the specs. `openspec list` and `openspec validate --all` check the state.
The specs describe behaviour; known bugs and unverified items live in `docs/open-questions.md`.

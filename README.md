# xreal-linux

## Quick start

Use **XREAL 1S** glasses as a **SteamVR headset** on Linux (3DoF: head rotation from the glasses' own sensors). Verified on a **Steam Deck**
(Bazzite, desktop mode, Plasma on Wayland, AMD GPU). Other AMD PCs with Plasma on Wayland should work the same way; NVIDIA and Intel GPUs are
**not** supported yet (untested), and the Eye camera (6DoF) is not used.

You need: the glasses plugged in over USB-C, Steam with **SteamVR** installed (Steam app 250820) and started **once** so it has written its
settings, and a second display enabled (the Deck's own screen) so desktop windows have somewhere to go besides the glasses.

1. Download `xreal-linux-x86_64.AppImage` from the [latest release](https://github.com/kevindurb/xreal-linux/releases/latest) and make it
   executable (`chmod +x`, or *Properties > Permissions* in the file manager).
2. Run it (double-click, or `./xreal-linux-x86_64.AppImage`). Answer the questions: it installs for your user only (no root), registers the
   driver with SteamVR, offers to enable a background service, and offers to change four SteamVR settings (each is shown and asked about, and
   a backup is kept). `--dry-run` shows what it would do without changing anything.
   *If the AppImage does not start because FUSE is missing, run it as `./xreal-linux-x86_64.AppImage --appimage-extract-and-run`.*
3. On the glasses, set **Follow mode** (not Anchor), **Stabilizer off** and **auto sleep off** in their own menu (double-click the X button,
   then Display, ...). The host can neither read nor set these.
4. Start SteamVR from Steam. The glasses switch to full side-by-side by themselves and the headset appears; when SteamVR quits they go back
   to the mode they were in. Each switch re-plugs the glasses' display for about two seconds, so the desktop may rearrange windows.
   Nothing is done to the glasses while SteamVR is not running, so you can use them as a normal monitor.

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

**Not promised:** the Deck's *game mode* (whether the background service runs there is not known yet), Flatpak Steam, and GPUs other than AMD.

## Status

The rest of this file and `docs/` are the engineering notes: how the pieces work and what was measured.

| Piece | Status |
|---|---|
| Video (DisplayPort over USB-C) | Works as a plain monitor in several aspect modes; full side-by-side (3840x1080) for VR, set automatically |
| Audio, buttons | Work (standard USB audio class and HID) |
| Head tracking, 3DoF (IMU) | Works; the yaw drifts (the magnetometer is not usable) |
| SteamVR integration | Works: a driver-direct-mode driver plus a presenter that owns the glasses' display |
| Install | One AppImage with a guided setup, a socket-activated user service, undo |
| Eye camera, 6DoF / SLAM | Stream partly decoded; a host-started camera sends only four frames; parked |

## Goal

Make the glasses usable as a SteamVR-style headset on Linux, starting on a Steam Deck and working on any Linux gaming PC.
It should work in two tiers, so people without the XREAL Eye still benefit:

- **3DoF (rotation only), IMU only.** Works with the glasses alone.
- **6DoF (rotation and position), IMU plus the Eye camera.** An optional upgrade when the camera is present.

See [docs/findings.md](docs/findings.md) for the details and [docs/open-questions.md](docs/open-questions.md)
for what is unverified.

## Tools

- `tools/decode_imu.py` - read the IMU stream from the glasses and print gyro/accel.
- `tools/decode_camera_frame.py` - split a captured camera frame into its parts and save PNGs.

- `tools/imu_web/` - a browser page that streams live IMU graphs and walks you through direction tests
  (yaw / pitch / roll) to pin down which gyro axis and sign each head motion is. See below.

The scripts above only read from the glasses. The presenter's only writes are the display input mode: full side-by-side while SteamVR uses the glasses, and back to the previous 2D mode afterwards (see `docs/handoff.md` for the rules).

### IMU axis check page

Standard-library Python server plus a static page, no installs needed.

```sh
# on the Deck (after tools/imu_web/deploy.sh copied it over)
python3 ~/xreal-linux/tools/imu_web/server.py            # reads 169.254.1.1:52998
# from your own machine
ssh -L 8765:localhost:8765 steamdeck                     # then open http://localhost:8765/
```

1. Press **Calibrate** and hold still for 3 s (gyro bias is measured and removed).
2. Press **Run all** (or **Run** on one row). For each test you move to the pose and hold still; the page measures the
   angle by integrating the gyro, cross-checks it against the accelerometer's gravity direction, then asks you to
   return to centre and checks that you got back to where you started. Direction and target angle are editable per row.
3. The result table says, for example, "yaw left -> gyro -Y". **Save** writes `captures/axis-map-*.json`.

Other sources for developing without the glasses: `--source replay:captures/imu_yaw.bin` replays a capture, and
`--source sim` generates clearly labelled synthetic data (its axis conventions are invented).

Tests: `node tools/imu_web/test_core.mjs` and `python3 -m unittest tools/imu_web/test_server.py`.
The server binds to 127.0.0.1 by default; `--bind 0.0.0.0` exposes an unauthenticated API that can write into
`captures/`, so avoid it on untrusted networks.

## Privacy note

Camera captures show the inside of a home, so raw captures and rendered images are deliberately not in this repo
(`captures/` is git-ignored).

## Development workflow (OpenSpec)

Changes are planned with [OpenSpec](https://github.com/Fission-AI/OpenSpec): `openspec/specs/` holds what the system does today
(`imu-tracking`, `presenter-display`, `steamvr-driver`, `glasses-setup`), `openspec/changes/` holds proposed changes as deltas against
those specs, and `openspec/config.yaml` gives the AI the project context (architecture, hard facts about the glasses and SteamVR,
build and test environment). In Claude Code, `/opsx:propose "idea"` creates a proposal, specs delta, design and tasks, `/opsx:apply`
implements them and `/opsx:archive` folds them into the specs. `openspec list` and `openspec validate --all` check the state.
The specs describe behaviour; known bugs and unverified items live in `docs/open-questions.md`.

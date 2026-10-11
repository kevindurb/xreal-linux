# Design

## Context

See `proposal.md` for motivation and the verified/assumed table. The pieces this builds on already exist:

- `presenter/src/setup/` is the package's user tool. `dispatch` routes `setup`/`check`/`fix`/`status`/`uninstall`/
  `restore-display`/`headlook`/`sbs`; `units.rs` writes and marks the two user units (`xreal-linux.socket`,
  `xreal-linux.service`) and `install.rs`/`uninstall` place and remove files. `env.rs` resolves
  `~/.local/share/xreal-linux/`, the unit directory and `~/.config/xreal-linux/`.
- `headlook.rs` already runs as a standalone service, reconnects on its own through `tracking::run`, and creates a
  rootless `uinput` device under `uaccess`; it is a foreground command with no unit today.
- `glasses.rs` is the only path that talks to the glasses and holds the allowlist; the plugin must not replace it.
- `.github/workflows/release.yml` builds the AppImage in the pinned sniper SDK and attaches it to a tag;
  `packaging/` holds the build, checksum and content checks.

The constraints that shape this design: the plugin's backend runs with more privilege than the desktop user while
every real action here belongs to that user; the project's rule that only allowlisted requests may reach the
glasses must not be widened; the release is built from one commit and the runtime and plugin must stay in step.

## Goals / Non-Goals

**Goals (design-level):**

- A game-mode panel whose actions are a thin, auditable translation of `systemctl --user` and the existing CLI, with
  no reimplemented behaviour and no duplicated unit files.
- One written owner of user units (`setup`/`uninstall`), extended with the head-look unit.
- A runtime capability report rich enough for a generic front-end, and an extension point for other XREAL models
  and their functions.
- Plugin and runtime published from one tag, with the plugin able to install/update the runtime it expects.

**Non-Goals (design-level):**

- A compiled plugin backend, a plugin-side device protocol, or any direct glasses I/O in the plugin.
- Adding controls beyond the mode selector; the capability model is the seam for them, not an implementation now.
- Flat stereo in the selector; its `sbs on` supervision needs its own unit and is deferred.
- Decky plugin store submission and any change to the SteamVR/settings path.

## Decisions

**1. The plugin is a new top-level `decky/` tree, not part of the AppImage.**
A Decky plugin is `plugin.json`, a Python `main.py`, and a bundled `dist/index.js` built from `src/index.tsx` with
`pnpm`; it installs under the Decky plugin directory and is loaded by Decky, not by the AppImage. Keeping it out of
the AppImage lets the game-mode front-end version and update independently while still being released from the same
tag. Alternative: embedding the bundle in the AppImage and copying it out during `setup` — rejected in favour of
the plugin-first direction; it would give the runtime a second artifact to manage and a directory it does not own.

**2. Thin backend: the Python plugin drives the CLI and `systemctl --user`, never the glasses.**
Every mode action is `systemctl --user enable/disable --now` on a unit; status is read from `systemctl --user
show` plus the runtime's report. Rationale: the CLI already encapsulates the allowlist, paths and lifecycle, and is
verified on hardware; duplicating any of it would drift. Alternative: a fat plugin that owns its own units and
launches the presenter itself — rejected (two writers, two code paths, and the panel could then bypass the
allowlist).

**3. The backend runs its actions as the session user, not as root.**
The Decky backend process is more privileged than the desktop session, but the targets — `systemctl --user`,
`/dev/uinput` via `uaccess`, the Wayland session for the AppImage — are the user's. The plugin resolves the session
user (Decky exposes the runtime user; otherwise the owner of the active seat) and runs each action through a single
helper that sets `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS` and `WAYLAND_DISPLAY` and drops privilege. Rationale:
one narrow, testable boundary instead of scattered `sudo`/`runuser` calls; mirrors the package's "one user" rule.
Alternative: run as root and rely on root's access to `/dev/uinput` and systemd — rejected (wrong owner for the
units, and it would not match `setup`).

**4. The mode selector maps directly onto mutually-exclusive unit state.**
`Head mouse` = `enable --now xreal-headlook.service` + `disable --now xreal-linux.socket`; `SteamVR` = the
reverse; `Off` = both disabled. Reading the units' real state on open makes the selector honest after any external
change. Rationale: exclusivity is a property of the state, so it needs no separate arbitration logic and the
archived "non-combined use" note in the head-look design is respected by construction. Alternative: two independent
toggles plus arbitration — rejected as more states to get wrong.

**5. A third user unit, `xreal-headlook.service`, written by `setup` like the others.**
It runs `xreal-presenter headlook` under the user, marked "Managed by xreal-linux", disabled by default, with its
options in `~/.config/xreal-linux/headlook.env` (device `mouse` default, model, tuning), and removed by
`uninstall`. `Restart=on-failure` is enough: `headlook` already keeps running through an IMU loss and reconnects, so
a dropped link must not restart the process. Rationale: `setup` is the single, already-tested writer of units and
`uninstall` already stops and removes "the units it installed". Alternative: a transient `systemd-run` owned by the
plugin — rejected once the user chose persistent enable/disable, and it would make head mouse unusable outside game
mode.

**6. The runtime grows a machine-readable report; the human output stays.**
A structured report (JSON) names the connected device, its model, the runtime version, the units' state and the
controls the model supports. It is built from read-only, already-allowlisted information (EDID `MRG` `0x4102` plus
the existing getters); producing it sends no state-changing request. The capability set is a small table keyed by
model with the 1S entry, which is the seam for other XREAL glasses and future controls. Rationale: a generic
front-end cannot parse free text, and a capability table keeps device-specific knowledge in the runtime where the
protocol lives. Alternatives: the plugin detecting the device itself — rejected (it has no business speaking the
protocol); parsing `status` text — rejected as brittle.

**7. The plugin installs and updates the runtime from its own release.**
The plugin's release version maps to a release tag; it requests the AppImage asset from that tag, verifies it
against the published checksum, places it, and runs `setup --yes` as the session user. On first install there is no
CLI yet, so the plugin performs the download, checksum check, `chmod` and `setup` run itself using only shell tools
and the AppImage. It refuses an artifact whose checksum does not match and reports when the expected version is not
on the release. Rationale: one installer per artifact, gapless version coupling, and the runtime stays independently
installable. Alternative: the AppImage installing the plugin — rejected (two installers, and the plugin directory is
Decky's).

### Deck-specific versus portable

- **Deck/Bazzite-specific:** Decky Loader and the Quick Access Menu, game mode/gamescope, and the `deck` user the
  plugin runs actions as. The front-end only exists there; the plugin's tests use the same helper against a normal
  session.
- **Portable (must hold elsewhere):** the runtime, the user units, the head-look service, the structured report and
  the whole install/update path. None of these touches AMD, RADV, Wayland or Plasma specifically; they behave the
  same on an Intel/NVIDIA desktop-distro machine.
- No part of this design uses the Steam Deck's screen, the glasses' resolution or the GPU beyond what already
  exists.

### Boundaries the design touches

- **Pressure vessel:** nothing. The plugin never opens the driver↔presenter abstract socket and never enters the
  Steam Runtime container; it manages user units and runs the CLI on the host.
- **SteamVR settings:** nothing is written by the plugin. `setup`/`fix` remain the only writers, unchanged.
- **Glasses menu settings the user must still set:** Follow mode, **Stabilizer off**, auto-sleep off. The panel
  cannot read or set them (the control channel cannot), so the plugin documents them and the status does not claim
  to verify them.
- **Glasses requests:** none new. The only setter remains the allowlisted display-mode request used by the existing
  paths; the report is read-only.

## Risks / Trade-offs

- [A Decky plugin downloading and executing a binary may be disallowed by the plugin store or unwelcome] → keep the
  downloader an explicit action, verify the checksum before use, document the behaviour, and allow a manual install
  that the plugin simply detects; the store policy is checked before submission (Phase 1).
- [Running user actions from a more-privileged backend is a sharp edge] → a single run-as-user helper with the three
  session variables, unit-tested and exercised on the Deck first; no shell string interpolation of user input.
- [Version skew between plugin and runtime] → the plugin requests the artifact for its own tag and reports a
  mismatch instead of installing a different version; the runtime's report states its version.
- [Head mouse and a user-launched SteamVR session can still overlap] → the selector prevents it at toggle time; a
  session the user starts outside the panel is warned about, not force-stopped (killing a live session is worse
  than the overlap, which is only two IMU readers).
- [A boot-persistent head-look service keeps a virtual input device present] → it emits nothing while still and
  nothing while the IMU is unavailable, and `Off` (or disabling the unit) removes it; the trade-off is accepted for
  symmetric enable/disable.
- [The expected AppImage may not exist for a given plugin build] → the plugin reports the mismatch and installs
  nothing, rather than falling back to an arbitrary version.
- [First-run bootstrap runs `setup` with `--yes`] → `setup` is already safe to re-run, refuses root, and supports
  its dry run; the plugin surfaces the setup output so a failure is visible.

## Migration Plan

- Opt-in and additive: a new plugin artifact and a new user unit that is disabled unless asked for. Existing
  installs are unaffected until the user opts in.
- Release ordering: build and attach the AppImage, its checksum and the plugin bundle from the same tag, so a
  plugin build always has a matching AppImage. The plugin's own store/zip release points at that tag.
- Rollback: disable/remove the plugin and, if wanted, `uninstall` the runtime; disabling the head-look unit removes
  the virtual device. No data migration.
- `setup` remains backward compatible: an older installed runtime without the head-look unit is reported as such by
  the panel's status, and the install/update action brings it forward.

## Open Questions

- Whether the panel later surfaces `check`/`fix` results and the SteamVR settings the runtime already manages; the
  capability model and the report can carry them without changing these specs.
- The exact Decky API surface and minimum Decky version the plugin targets (confirmed against the current plugin
  template when the code is written, not a spec decision).
- How the capability table is extended for the next XREAL model (a new table entry and its control set); deferred
  until a second model is in hand.

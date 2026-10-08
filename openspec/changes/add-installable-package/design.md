# Design

## Context

Everything runs from a checkout at a fixed path: `tools/vr_session.sh` and `tools/doctor.sh` hard-code `~/xreal-linux` and `/tmp/presenter.log`, the driver is registered by hand with `vrpathreg.sh adddriver <repo>/driver/xreal`, and builds happen in podman containers on the Deck (fedora:44 for both pieces). See proposal.md for motivation and the verified/assumed split.

Facts that constrain the packaging:
- vrserver and vrcompositor run inside Steam's pressure-vessel container. The driver is loaded into vrserver, so it must be built against a glibc no newer than that container's, and be readable from inside it. The presenter is a normal host process.
- The driver and presenter talk over an abstract unix socket, which is per network namespace: any sandbox for the presenter must share the host network namespace (the default for Flatpak with network access).
- The presenter must open the glasses' output fullscreen, use the same GPU as SteamVR, and reach `169.254.x.1` over the glasses' USB network.
- The Deck's host is immutable (Bazzite): installs have to live under the home directory.

## Goals / Non-Goals

**Goals:**
- A user installs, sets up, runs and removes the project without a checkout, a compiler or root.
- Both the driver and the presenter in a release are reproducible from a commit.
- Setup never changes the user's SteamVR configuration without showing it and keeping an undo.

**Non-Goals:**
- A GUI, other GPU vendors, camera tracking or any behaviour change in the driver and presenter (separate changes; only a protocol version field is added to both).

## Decisions

### 1. Two delivery stages: archive first, Flatpak second

Stage one is a tarball with `install.sh` that installs under `$HOME` and works on the Deck and on ordinary distros. Stage two wraps the app side (presenter and setup command) in a Flatpak once the sandbox questions are answered. The driver is not in the Flatpak: it has to be a plain file Steam can read, so the Flatpak's `xreal-setup` installs it into the data directory the same way `install.sh` does.

Why: the archive has the fewest unknowns and proves the install paths; the Flatpak adds the sandbox questions (GPU, Wayland output choice, Steam as a Flatpak) which are experiments in their own right. Alternatives considered: an AppImage (the Vulkan and Wayland stack is better taken from the host or a runtime than bundled); distro packages (can wrap the archive later); a Rust-only static presenter (Vulkan loading needs the host's driver anyway).

### 2. Build on an old-glibc baseline in a fixed container

Driver and presenter are built in a container image pinned in the repo (the Steam Runtime SDK or an older distro image, chosen by the first task by checking which loads in SteamVR and which runs the presenter on the Deck and one other distro). A build step lists the driver's glibc symbol versions and fails if one exceeds the baseline. This replaces `-fno-math-errno` as the only guard against `GLIBC_2.43`-style breaks (the flag stays).

### 3. Driver location: a stable path under the data directory

The driver is installed to `$XDG_DATA_HOME/xreal-linux/driver/xreal` and registered by that path with SteamVR's own `vrpathreg`. An update replaces the files in place; the path never changes, so no re-registration. If Steam is the Flatpak, its sandbox must be able to read the path (an override or a path under its data directory) — the Flatpak Steam case is its own early experiment and the setup command reports it plainly if it cannot be done.

### 4. `xreal-setup`: one command, check / fix / start / stop

A small Rust command (it shares code with the presenter's parsing of DRM modes and the IMU probe) replaces `doctor.sh` and `vr_session.sh` for installed users; the scripts remain for checkouts and call the same logic where practical. `fix` is the only part that writes, and only to: the driver registration (through `vrpathreg`) and `steamvr.vrsettings`.

Why a command and not more shell: the checks need structured output, tests and a stable surface for a later GUI.

### 5. Safe edits to the SteamVR settings

`steamvr.vrsettings` is rewritten by SteamVR on exit, so `fix` refuses while SteamVR runs. It writes a timestamped backup before the first change, records which keys it changed and their previous values in a small manifest under the state directory, and `uninstall` uses the manifest (not the whole backup) to put back only those keys.

### 6. Protocol version in the driver link

The first message each side sends carries a protocol version (a small integer bumped on message changes). A mismatch is reported by the presenter and by `check`. This is the one behaviour change in the driver and presenter, and it is what makes a half-updated install diagnosable.

### 7. State and logs

Logs go under `$XDG_STATE_HOME/xreal-linux/` (presenter log, session log), the settings backup and change manifest live there too. The `/tmp/presenter.log` path in the scripts moves with it.

### 8. CI builds the release

A workflow builds the release archive from a tag in the pinned container and uploads it; the same container runs the symbol-version check. No secrets are involved.

## Risks / Trade-offs

- [The old-glibc driver still does not load in a future Steam runtime] → The symbol check and the "activated" log line are tested on the Deck on every release; `check` reports a driver that did not activate.
- [Flatpak Steam cannot read the installed driver, or the presenter cannot get a fullscreen window on the glasses from a Flatpak] → Stage two is gated on those experiments; stage one ships regardless and the README says which Steam setups are supported.
- [Editing the SteamVR settings corrupts them] → Refuse while SteamVR runs, back up first, write atomically, keep a manifest for undo.
- [`vrpathreg` is missing or changes] → `check` reports it; the fallback edits are not attempted silently.
- [Hard-coded Deck assumptions (output name `DP-1`, `card1`) in the scripts] → The port to `xreal-setup` finds the glasses' output from the DRM modes instead; the Deck is the first test, another distro the second.
- [AMD/Wayland/Bazzite specific] → Install paths and the setup logic are distro-neutral; the verified GPU is AMD only, which the README says.

## Migration Plan

Existing checkout users keep `tools/*.sh`; nothing is removed. The first release is a tag with the archive. Rollback is `install.sh --uninstall` followed by the checkout workflow. Settings the user must keep are unchanged: full SBS, Follow mode, Stabilizer off, auto-sleep off, and the SteamVR settings `fix` manages.

## Open Questions

- Naming and version scheme for releases: does not affect the specs or the tasks beyond a string.

# Design

## Context

See `proposal.md` for motivation. The pieces this builds on already exist and are verified on the Deck:

- `presenter/src/tracking.rs` reads the IMU (TCP 52998) and produces both an absolute orientation (Mahony-style fusion, gravity-corrected pitch/roll, drifting yaw) and the world-frame angular velocity (`PoseState.q`, `PoseState.omega`). Its parser and filter are unit-tested.
- `presenter/src/glasses.rs` holds the control-port client and the full-SBS setter/restore (`set_sbs_once`, `restore_previous_mode`), and already records the previous mode.
- `presenter/src/setup/mod.rs::dispatch` routes subcommands (`setup`/`check`/`fix`/`status`/`uninstall`/`restore-display`/`serve`) before the presenter's Vulkan path runs. `main.rs` also accepts one-shot flags (`--set-sbs-only`, `--restore-display`).
- The presenter crate is a single binary; the head-look and flat-stereo paths do not need Vulkan or winit.

The constraints that shape the approach: the glasses' IMU is read-only from the host; only the allowlisted `NRDpSetInputMode` setter may ever be sent; yaw drifts because there is no usable magnetometer; and the target distro already grants the active session user read/write on `/dev/uinput` and `/dev/uhid` via `uaccess`, so no root or udev rule is needed.

## Goals / Non-Goals

**Goals:**

- A standalone head-look service and a flat-stereo command that reuse the existing IMU reader, fusion and control-port client with no refactor of the SteamVR path.
- A mapping core that is pure and unit-testable offline against recorded captures, so the look behaviour is validated without a wearer or the Deck.
- No root, no SteamVR, no driver, no new request to the glasses.

**Non-Goals (design-level):**

- Replacing or re-scoping the SteamVR path; the two tiers coexist and are not combined here.
- Automatic per-game detection or a game database.
- Lens-distortion correction, positional (6DoF) tracking, or stereo re-projection — flat stereo is the game's own SBS output shown on the glasses.
- Setup/AppImage integration (offering the tiers from `setup`), systemd units, or a GUI.
- Any new message to the glasses.

## Decisions

**1. New subcommands in the existing `xreal-presenter` binary, not a new crate/binary.**
Route `headlook` and `sbs` through `setup::dispatch`. Rationale: the modules to reuse (`tracking`, `glasses`) live in this crate; the installed AppImage already ships the binary; the paths never call into the Vulkan/`winit` code. Alternatives: a separate crate would need either a shared library refactor or duplicated IMU code, and adds build/packaging churn for no behavioural gain.

**2. Reuse `tracking.rs` unchanged; add a thin consumer.**
The service consumes `PoseState` (`q` for stick/absolute, `omega` for rate). `tracking::run` already reconnects, applies the factory IMU matrices and estimates bias; no change to `imu-tracking` behaviour is needed. Alternative: a second IMU reader — rejected as duplication of the parser and the tested filter.

**3. Virtual device via `uinput`, implemented with raw `libc` ioctls (no new dependency).**
The mapping core is pure (`map(model, tuning, pose) -> Output`); a small module turns `Output` into uinput events. Rationale: the Deck is used offline and the cargo cache would not contain a new crate, so adding a dependency (for example the `evdev` crate's `uinput`) risks breaking the offline build for marginal benefit; the project already hand-rolls low-level code and depends on `libc`. Alternative considered: an external uinput crate (safer ABI, one more dependency, offline-build risk) and a Python helper (its `evdev`/`uinput` binding is not installed). Two device kinds are exposed: a **mouse** (default: SDL/Steam pass a pointer through unchanged, so flat games' mouse-look reads it directly) and a **gamepad** (for stick-driven titles; Steam's SDL maps only `ABS_X`/`ABS_Y` for an unrecognised pad, so it drives the left stick and needs a Steam Input remap — see `docs/findings.md`, task 5.1).

**4. Three mapping models sharing one set of knobs.**

- **rate**: output ∝ `omega` (world yaw/pitch), deadzone on the rate. Drift-tolerant because only the rate is used.
- **stick**: axis = clamp((angle − neutral) / range), deadzone and curve applied. Self-centring: returning the head to neutral returns the axis to zero.
- **absolute**: the stick mapping at 1:1 gain with a fixed range (a TrackIR-like "look").

All three reset their reference on **recenter**. Rationale: the HCI literature splits cleanly — rate control for free-look and unlimited range, position/1:1 control for precision within a limited range — and the flat-screen case is unstudied, so all three ship and the wearer A/B (Phase 2) picks the default; that default is configuration, not a spec change. Alternative: ship one model — rejected because the choice is empirical.

**5. Recenter as a signal, with a button as a later option.**
Baseline: `SIGUSR1` recenters (also settable at start). Rationale: no dependency on reading another input device, and it is testable offline; a controller-button trigger needs evdev reading and can be added without changing the specs. Alternative: read a chosen button from a gamepad now — deferred.

**6. Flat stereo wraps the existing setter/restore.**
`sbs on|off|status` calls `glasses::set_sbs_once` / `restore_previous_mode` and reports the mode; it refuses while a SteamVR session owns the glasses (the presenter owns the setter then) and re-sets a bounded number of times if the glasses drop to 2D. Rationale: the setter, the allowlist and the mode record already exist; only the standalone lifecycle and the SteamVR-conflict guard are new. Alternative: document the existing `--set-sbs-only`/`--restore-display` flags — rejected because a named subcommand can carry `status` and the conflict guard.

**7. Offline mapping simulator as a first-class test.**
A harness feeds the recorded captures (`captures/imu_yaw.bin`, `imu_move.bin`, `xreal_52998_still.bin`) through the same fusion and the pure mapping core, and asserts model behaviour and measures drift. Rationale: it makes Phase 0 run without the Deck or a wearer, matching the project's "verify by measurement" rule.

### Deck-specific versus portable

- **Deck/Bazzite-specific:** the `uaccess` ACL that makes uinput rootless is a logind feature common to desktop Linux but must be confirmed per distro; Steam's controller interception (whether it hides the virtual device) is Steam-on-Deck-specific and is checked in Phase 1.
- **Portable (must hold elsewhere):** the IMU parsing, fusion, mapping maths, the control-port client and the flat-stereo lifecycle are GPU-, compositor- and vendor-independent. Nothing here uses Wayland, X11, AMD, RADV or Vulkan, so the change should behave the same on Intel/NVIDIA and other distros; only the uinput permission check is environment-dependent.
- No part depends on the Steam Deck's screen size or the glasses' resolution beyond the existing full-SBS mode.

### Boundaries the design touches

- **Pressure vessel:** nothing. The head-look service and the flat-stereo command run on the host; they never open the driver↔presenter abstract socket and never enter the Steam Runtime container.
- **SteamVR settings:** nothing is written. Flat stereo merely refuses to run while SteamVR owns the glasses, so it cannot fight the presenter over the display mode.
- **Glasses menu settings the user must still set:** Follow mode, **Stabilizer off**, and auto sleep off. Stabilizer off matters even more here than for SteamVR, because the glasses' own stabiliser would fight the look mapping. The design cannot read or set these.

## Risks / Trade-offs

- [uinput ABI bugs in hand-rolled ioctls] → keep the module thin and the mapping core pure and unit-tested; exercise the real device on the Deck first thing in Phase 1; take the ioctl/struct definitions from the kernel ABI.
- [Steam hides the virtual device from a game] → fall back to the game's own evdev; the `/dev/uhid` DualSense route (also permitted by `uaccess`) is a documented later option.
- [A game's SBS output assumes 16:9 and stretches on 32:9] → measure the captured halves in Phase 1; record which titles work rather than promising all of them.
- [Yaw drift makes stick/absolute wander] → the rate model is unaffected by integrated yaw; the deadzone removes constant bias; recenter is available; drift is measured in Phase 0 and Phase 3.
- [Running head-look beside a SteamVR session reads the IMU twice] → documented as a non-combined use; the IMU stream already accepts the existing tools' connections, so this is likely safe but is not a goal and is left untested.
- [Added latency from TCP + uinput] → measured in Phase 0/2; no smoothing is added unless jitter requires it, because smoothing costs latency.
- [No new dependency keeps offline builds working] → the raw-`libc` choice deliberately avoids a crate the cargo cache lacks.

## Migration Plan

- Opt-in only: adding the `headlook`/`sbs` subcommands changes nothing until a user runs them. No data migration; the display-mode record format is unchanged.
- Rollback: stop running the commands; the glasses' mode restore already exists (`restore-display`). Removing the subcommands reverts the change.
- Packaging: the same AppImage gains the subcommands; no new files, units, or settings.

## Open Questions

- The **default** model is set from the Phase 2 A/B; it is configuration and does not change the specs or tasks.
- The **recenter trigger** ergonomics (signal now, controller button later) can be refined without changing the specs.
- Which titles accept a **virtual mouse versus a gamepad** is empirical and recorded per title.

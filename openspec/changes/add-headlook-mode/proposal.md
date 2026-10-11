# Proposal

## Why

Playing ordinary (non-VR) games on the glasses as a large 2D screen already works with **no SteamVR at all**. The project's SteamVR path is powerful but heavy and fragile — driver direct mode, the pressure-vessel abstract socket, safe mode, vendor-specific GPU assumptions — and it is only needed for true VR content. Two cheap additions cover most of what a flat-game player wants: (1) map the glasses' IMU to a **virtual input device** so head motion drives a game's look controls, and (2) set the glasses' **full side-by-side mode** for games that render SBS 3D themselves, giving stereo without SteamVR. Both reuse code that is already verified on hardware (the IMU reader/fusion and the control-port SBS setter), need no driver and no SteamVR, and need **no root**: the session user can already write `/dev/uinput` on the target distro.

## What We Know and What We Assume

Verified on hardware (Steam Deck, Bazzite, game mode):

- The glasses expose the IMU on TCP 52998 and the control port on 52999; the host can set full SBS with one allowlisted request (`NRDpSetInputMode` = 1, id 10274).
- The IMU reader, record parsing, sensor-to-body conversion, orientation fusion and the control-port client already exist and are unit-tested (`presenter/src/tracking.rs`, `presenter/src/glasses.rs`).
- `/dev/uinput` and `/dev/uhid` are writable by the active session user via `TAG+="uaccess"` (`getfacl /dev/uinput` shows `user:<user>:rw-`), so a userspace virtual input device needs no root, no udev rule and no kernel module beyond `uinput`.
- With SteamVR not running the glasses behave as an ordinary monitor; full SBS presents a single 3840x1080 mode.

Assumed, not yet verified. Each names the experiment that settles it:

| Assumption | Experiment |
|---|---|
| A game binds and uses a uinput virtual gamepad and/or mouse for head-look | Emit a scripted virtual device (no glasses) and confirm it appears to `libinput`/`evtest`/`jstest` and in a game's binding menu (Phase 1) |
| A game's own side-by-side output maps cleanly to the glasses' 3840x1080, each half close to 16:9 and not stretched | Launch a game in its SBS mode fullscreen on the glasses, capture the framebuffer and measure the two halves (Phase 1) |
| Rate or stick control feels better for seated play (the HCI literature favours rate control for free-look and 1:1 position control for precision; the flat-screen case is unstudied) | A wearer A/B of the models with a fixed questionnaire and logged recenters/drift (Phase 2) |
| Residual yaw bias is small enough that an occasional recenter suffices | Measure drift on a recorded still capture (Phase 0) and across a long session (Phase 3) |
| Steam does not hide the virtual device from the game (or the game's own evdev still works if it does) | Phase 1, run with Steam active |
| Head-look latency through uinput is low enough to feel direct | Instrument IMU-to-event latency in the simulator and live (Phase 0/2) |

## What Changes

- **New head-look service.** Reads the glasses' IMU (reusing the existing reader and fusion) and drives a userspace virtual input device — a gamepad and/or a mouse — so head motion controls a game's look. Offers **rate**, **stick** and **absolute** mapping models, deadzone/sensitivity/curve tuning, a **recenter** action, and a status line. Runs without SteamVR, the driver or the presenter window, and needs no root.
- **New flat-stereo command.** Sets, reports and restores the glasses' full side-by-side mode without SteamVR, for games that render side-by-side 3D themselves. Reuses the existing allowlisted setter and the mode record/restore.
- **Offline tooling.** A mapping simulator and unit tests over recorded IMU captures (no hardware), plus a small latency/status report.
- **Docs.** Findings and open-questions entries, a README section, and the offline-first test plan (Phases 0-3).
- Not changing: the driver, the presenter's SteamVR path, the IMU parser/fusion behaviour, or the glasses' message allowlist.

## Capabilities

### New Capabilities

- `head-look-input`: the glasses' head orientation exposed as a virtual input device (mapping models, tuning, recenter, status) for games outside SteamVR.
- `flat-stereo-mode`: setting, reporting and restoring the glasses' full side-by-side mode outside SteamVR.

### Modified Capabilities

None. The change adds two capabilities and reuses existing behaviour (the IMU reader/fusion and the SBS setter) without changing any existing requirement.

## Impact

- `presenter/` (Rust): new subcommands (`headlook`, `sbs`) dispatched alongside the existing `setup`/`check`/`serve` commands; a new module for the orientation-to-input mapping and a new module for the virtual device; reuse of `tracking.rs` and `glasses.rs`. One new dependency for a userspace input device (a small uinput crate, or raw `libc` ioctls to add none).
- `tools/`: a mapping simulator/replay and the test plan.
- `docs/`: `findings.md`, `open-questions.md`, `README.md`, `handoff.md`.
- No driver, protocol, package, unit-file or SteamVR-settings change. No root. The only request ever sent to the glasses stays the allowlisted SBS setter; the IMU path is read-only.
- This change also reframes the project's top line (the glasses as a SteamVR headset *or* a head-tracked big screen); that is recorded, not re-scoped here.

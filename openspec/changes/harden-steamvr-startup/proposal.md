# Proposal

## Why

On 2026-10-08 SteamVR failed silently on the Steam Deck right after a reboot. The presenter service started before Plasma had put `WAYLAND_DISPLAY` into the user manager's environment, so it panicked while opening its window and hit its start limit. The driver waited past SteamVR's 20 s watchdog, SteamVR's safe mode then set `driver_xreal.blocked_by_safe_mode`, and every later launch showed "Headset Not Detected (108)" with the driver never loading. `xreal-linux check` reported no failures throughout, and `fix` did not know about the flag, so the only remedy was hand-editing `steamvr.vrsettings`.

## What Changes

- The presenter waits a bounded time for a display (`WAYLAND_DISPLAY` or `DISPLAY`) when it is started by the service without one, taking them from the user manager's environment when they appear there, instead of panicking at once.
- `check` reports a safe-mode block on the driver (and the "presenter keeps failing to start" state) as a failure with the remedy.
- `fix` clears `driver_xreal.blocked_by_safe_mode` with the same consent, backup and "SteamVR must be stopped" rules as its other settings changes.
- Not changing: the driver's own connect and watchdog behaviour (see design.md, Non-Goals).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `session-service`: the presenter tolerates a late-arriving display environment at start.
- `guided-setup`: `check` detects a SteamVR safe-mode block of the driver, and `fix` can clear it.

## Impact

- `presenter/src/setup/check.rs` (`judge_settings` and its facts), `presenter/src/setup/settings.rs` (`wanted`, `plan`), `presenter/src/main.rs` and the `serve` path (display wait).
- Unit tests beside each; `docs/` quick start and handoff notes mention the new check.
- No driver, protocol or unit-file format change; no new dependency.

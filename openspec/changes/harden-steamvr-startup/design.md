# Design

## Context

Observed 2026-10-08 on the Deck (see proposal.md): at 15:00:11 the service started the presenter three times in the same second, each panicking in winit with "neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set", hit `StartLimitBurst`, and left the socket dead. vrserver's `load_drivers` then ran 21.8 s and tripped SteamVR's watchdog; safe mode wrote `driver_xreal.blocked_by_safe_mode: true`. By the time of the next check the user manager did have `WAYLAND_DISPLAY`, so `check` was green. `check` already reads the manager's environment (`check.rs`), and `fix` already edits keys through `settings::wanted()/plan()` with a backup and a manifest of previous values.

## Goals / Non-Goals

**Goals:**
- A launch shortly after login works without the user doing anything.
- A safe-mode block is visible in `check` and removable with `fix`.

**Non-Goals:**
- The driver's behaviour when the presenter is dead. It still waits on the socket; changing that touches the driver protocol and is a separate change. The display wait is kept shorter than the 20 s watchdog so the common case never reaches it.
- Preventing SteamVR's safe mode from ever engaging, or editing it other than this one key.
- Auto-clearing the flag at session start (hides real crashes; `fix` stays an explicit, consented step).

## Decisions

- **Wait inside the presenter, not in the unit.** `serve` reads `WAYLAND_DISPLAY`/`DISPLAY` from `systemctl --user show-environment` when its own environment has neither, polling every 200 ms for up to 10 s, and sets them for itself. Alternatives: `ExecStartPre` polling the manager (systemd may not rebuild the service environment after the pre-step, which I could not confirm, and the unit format would change for installed users); `After=graphical-session.target` (the Deck's Plasma session does not reliably reach it before a Steam launch, and it would block unrelated use). A presenter-side wait is testable and needs no unit change.
- **Treat the block as a settings item in the existing machinery.** Add `driver_xreal.blocked_by_safe_mode` as a "wanted: absent" key in `wanted()`, so `plan`, consent, backup, manifest and uninstall restore all apply unchanged. `judge_settings` reports it as a failure (unlike the other, warning-level items) because the headset cannot work while it is set.
- **Fail with words.** On timeout the presenter prints a single message and exits non-zero; the unit's start limit then stops a loop as today.

## Risks / Trade-offs

- [`show-environment` is slow or absent when the manager is down] → treat any error as "not found" and keep polling to the deadline.
- [10 s is too short for a very slow login] → the failure message names the cause, and `fix` still clears a resulting block; the value is a constant that is easy to raise.
- [The "absent" key semantics: restoring a previously-true flag on uninstall would re-block the driver] → record it in the manifest like the others, but verify in a test that restore of a removed-then-true value is the intended behaviour, and say so in the uninstall output.

# Tasks

## 1. Display wait in the presenter

- [x] 1.1 Add a function that finds `WAYLAND_DISPLAY`/`DISPLAY` in the process environment or the user manager's, with the polling deadline injectable; unit-test present, appears-late, never-appears and manager-query-error cases (`cargo test` in `presenter/`).
- [x] 1.2 Call it at the start of the service path before the window is created; on timeout print the message and exit non-zero. Verify by running `serve` with the display variables unset and a stub manager query (test) and confirm no winit panic.
- [ ] 1.3 On the Deck: reboot, launch SteamVR within seconds of login, and confirm in `journalctl --user -u xreal-linux` that the presenter started and `vrserver.txt` shows `Present` frames with no watchdog line.

## 2. Safe-mode block in check and fix

- [x] 2.1 Make `judge_settings` fail on `driver_xreal.blocked_by_safe_mode == true` with the remedy text; add a test beside the existing settings tests and confirm `check` exits non-zero.
- [x] 2.2 Add the key to `settings::wanted()` as absent so `plan`/`fix` clear it with consent, backup and manifest; add tests for apply, decline, running-SteamVR refusal and the uninstall restore (`cargo test`).
- [ ] 2.3 On the Deck: set the flag by hand, run `check` (fails), `fix` (clears), `check` (passes), and launch SteamVR.

## 3. Documentation

- [x] 3.1 Add the symptom ("Headset Not Detected (108)" / "Some Add-ons Blocked") and the `fix` remedy to the quick start and handoff notes, and confirm the text matches the check's wording.

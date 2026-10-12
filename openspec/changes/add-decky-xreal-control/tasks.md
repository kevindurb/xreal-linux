# Tasks

## 1. Runtime: structured device and capability report

- [x] 1.1 Add a per-model capability table (the 1S entry only) and a structured report (`status --json` or a `describe --json` subcommand) naming the connected device, its model, the supported controls, the runtime version and the units' state. Verify with unit tests that parse the output and assert every field, and that the existing human-readable `status` output is unchanged.
- [x] 1.2 Populate the report from read-only, already-allowlisted information only (the EDID `MRG` `0x4102` plus existing getters), reporting "no device" and a non-zero exit when none is reachable. Verify on the Deck that it names the 1S while connected, and with `tools/watch_control.py` running that producing the report sends no state-changing request and leaves the display mode unchanged.
- [x] 1.3 Document the structured report and the capability table in `presenter/README.md`. Verify the documented command runs as written and its output parses as JSON.

## 2. Runtime: opt-in head-look user unit

- [x] 2.1 Add `xreal-headlook.service` to `presenter/src/setup/units.rs` (the "Managed by xreal-linux" marker, disabled by default, `Restart=on-failure`, `EnvironmentFile=-%E/xreal-linux/headlook.env`), offer it in `setup` (`install.rs`, guided setup) and remove it in `uninstall`. Verify unit tests for the unit text, the marker and the setup/uninstall decision, and that `check` reports its presence correctly.
- [x] 2.2 Add `headlook.env` handling (device `mouse` default, model, tuning) and any `--service` mode the unit needs. Verify with `cargo test` in `presenter/` and that the unit's `ExecStart` matches the documented flags.
- [x] 2.3 Document the head-look service (opt-in, `headlook.env`, enable/disable) in `presenter/README.md` and the quick start in `README.md`. Verify the documented commands run as written and the README names the unit.
- [x] 2.4 Deck verification: enable the unit with no terminal and confirm the virtual input device appears, disable it and confirm the device is gone, confirm `uninstall` removes it, and reboot with it enabled to confirm persistence and IMU reconnect. Record the observations in `docs/findings.md`.

## 3. Plugin: scaffold and the session-user action helper

- [x] 3.1 Create the `decky/` plugin from the current Decky template (`plugin.json`, `main.py`, `src/index.tsx`, `package.json`, `rollup.config.js`, `tsconfig.json`, `LICENSE`) with the generic "XREAL" identity, and build it with `pnpm`. Verify `pnpm run build` produces `dist/index.js` and the built plugin loads locally.
- [x] 3.2 Implement the single run-as-session-user helper (resolve the session user; set `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS` and `WAYLAND_DISPLAY`; drop privilege) and one backend method that runs `systemctl --user is-active <unit>`. Verify a unit test over the helper's argument/env construction and, on the Deck, that the method reports a real unit state while the backend runs more privileged.
- [x] 3.3 Implement the backend `get_state()` returning the units' state and the runtime's structured report, and expose it to the front end. Verify a test over a mocked report and, on the Deck, that the panel data matches `systemctl --user show` and the runtime report.

## 4. Plugin: mode selector, status and exclusivity

- [x] 4.1 Implement `set_mode(off|head_mouse|steamvr)` as `systemctl --user enable/disable --now` on `xreal-headlook.service` and `xreal-linux.socket`, mutually exclusive, executed through the run-as-user helper. Verify tests asserting the exact command set for each mode and each starting state.
- [x] 4.2 Build the Quick Access Menu panel: a mode control (Off / Head mouse / SteamVR), status rows (glasses reachable, each unit active/enabled, runtime version) and a "runtime missing" state that points at the install action. Verify it renders in game mode, reflects the units' real state on open after an external change, and leaves the glasses' display mode unchanged on a status read.
- [ ] 4.3 Guard switching away from SteamVR while a SteamVR session is running (refuse or require explicit confirmation). Verify a test for the guard and, on the Deck, that selecting Off during a live SteamVR session does not stop it without confirmation.
- [x] 4.4 Document the panel (modes, status, exclusivity, the runtime requirement, the guard) in `decky/README.md`. Verify the text matches the panel's actual behaviour.

## 5. Plugin: runtime detection, install and update

- [x] 5.1 Implement runtime detection and version reporting from the structured report, including the "not installed" and "runtime too old" cases. Verify tests for each case over mocked reports.
- [x] 5.2 Implement the install/update action: download the AppImage, verify it against the published checksum, refuse and report on mismatch, place it and run `setup --yes --no-enable` as the session user (placing every user unit but enabling none, so the panel's switch is what selects a mode). Verify tests for checksum-refusal and command construction, and a Deck dry run (or a scratch user) that a verified artifact installs and an unverified one does not.
- [x] 5.3 Implement version coupling: request the asset for the plugin's own release tag and report a mismatch instead of installing a different version. Verify a test for the matching, missing and mismatched cases.
- [x] 5.4 Deck verification: on a machine with no runtime, install from the panel and confirm `check` passes and the units exist; then run the update path from an older runtime and confirm in-place replacement and the reported version. Record the results in `docs/findings.md`.

## 6. Release and CI artifacts

- [x] 6.1 Build the Decky plugin bundle in `.github/workflows/release.yml` (Node/pnpm) and attach it plus a `SHA256SUMS` file for the downloadable artifacts to the same tag as the AppImage. Verify the workflow run (or a local run of the build steps) produces the AppImage, the plugin bundle and the checksum from one commit.
- [x] 6.2 Extend `packaging/tests/` to cover the new release artifacts and their checksums. Verify `python3 packaging/tests/test_check_contents.py` and the symbol/content checks pass.
- [x] 6.3 Update `README.md`, `docs/handoff.md` and `docs/open-questions.md` for the game-mode plugin and the head-look service. Verify `openspec validate --all` passes and the docs name both new pieces.

## 7. Integration verification on the Deck

- [ ] 7.1 End to end with the glasses attached: install the runtime from the panel, select Head mouse and confirm the virtual device appears, select SteamVR and confirm the socket is enabled and the other unit stopped, select Off and confirm both are gone, then reboot and confirm the selected mode persists. Record the run in `docs/findings.md`.
- [x] 7.2 Confirm the boundaries: the plugin opens no driver↔presenter socket and enters no pressure-vessel container, makes no new request to the glasses, changes no SteamVR setting, and leaves the glasses' display mode unchanged when only reading status (checked with `tools/watch_control.py` and the presenter log). Record the result.

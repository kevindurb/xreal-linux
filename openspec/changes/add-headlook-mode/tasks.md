# Tasks

## 1. Offline mapping core and simulator (Phase 0)

- [x] 1.1 Add a pure mapping module: given a pose (orientation and world-frame angular velocity), a model, a tuning set (deadzone, sensitivity, curve, range) and the current neutral reference, return the gamepad-axis output and the mouse-delta output. Verify with unit tests for each model's response to a known pose and to recenter (`cargo test` in `presenter/`).
- [x] 1.2 Add a simulator harness that parses `captures/imu_yaw.bin`, `captures/imu_move.bin` and `captures/xreal_52998_still.bin`, runs them through `tracking::Fusion` and the mapping core, and prints a per-model trace. Verify it runs on this machine with no glasses attached and prints a trace for each capture.
- [x] 1.3 Assert model behaviour on the captures: the rate output tracks `omega`, the stick/absolute output tracks head angle and clamps, the deadzone suppresses a still head, and recenter zeroes the output. Verify with tests that run over the recorded captures with no hardware.
- [x] 1.4 Measure residual drift on the still capture for each model and write the numbers (deg/min, with the capture and model named) to `docs/findings.md`; verify the simulator reproduces the documented numbers.
- [x] 1.5 Write the offline-first test plan (Phases 0–3, what each proves, the A/B protocol and questionnaire) to `docs/headlook-test-plan.md`; verify it matches the tasks below.

## 2. Virtual input device (uinput, no new dependency)

- [x] 2.1 Implement the uinput device module with raw `libc` ioctls: create a virtual gamepad and a mouse variant, give each a recognisable name, and remove it when dropped. Verify the event and struct encoding with unit tests and that `cargo build` succeeds with no added `Cargo.toml` dependency.
- [x] 2.2 On the Deck, create both device kinds and confirm `libinput debug-events`/`evtest`/`jstest` list them and that they disappear on exit; record the device names and IDs in `docs/findings.md`.
- [x] 2.3 Document the device name, the two device kinds and the rootless requirement (the `uaccess` ACL) in `presenter/README.md`; verify the documented name matches what the code creates.

## 3. Head-look service and `headlook` subcommand

- [x] 3.1 Add the `headlook` subcommand routed through `setup::dispatch`, with options for model (rate/stick/absolute), device kind (gamepad/mouse), deadzone, sensitivity, curve and recenter, and refuse an unknown model by naming the supported ones. Verify argument-parsing unit tests, including the refusal.
- [x] 3.2 Wire the service to `tracking::run`'s `PoseState`: consume the orientation and `omega`, run the mapping core, and emit device events. Verify with a test that feeds a synthetic `PoseState` sequence into the mapping step and checks the emitted events without a device.
- [x] 3.3 Implement recenter on start and on `SIGUSR1`. Verify a test that recenter makes the output zero for the current pose.
- [x] 3.4 Implement the status line (device kind, model, IMU connected, last recenter) and stop emitting when the IMU is unavailable. Verify the running service prints the status, and that pointing the IMU host at a closed port reports the loss and emits no head-motion events.
- [x] 3.5 Document `headlook` usage (options, recenter, and the glasses' Follow / Stabiliser-off / auto-sleep-off requirement) in `presenter/README.md`; verify the documented commands run as written for the argument-parsing path.

## 4. `sbs` subcommand (flat stereo)

- [x] 4.1 Add `sbs on|off|status` wrapping `glasses::set_sbs_once` and `glasses::restore_previous_mode`. Verify unit tests for the state transitions (already-SBS, recorded-2D restore, nothing recorded) and that `status` reports the mode and exits non-zero when the control port is unreachable.
- [x] 4.2 Refuse to change the mode while a SteamVR session owns the glasses and say why. Verify with the detection stubbed that `sbs on` reports the conflict and sends no setter.
- [x] 4.3 Re-set full SBS after a bounded number of 2D drops while active and report when the bound is reached. Verify with a test that drives the drop counter.
- [x] 4.4 Document flat stereo in `presenter/README.md` (what it is for, `on`/`off`/`status`, the restore, the SteamVR conflict) and verify the text matches the command's actual output.

## 5. Phase 1 — Deck verification, no wearer

- [ ] 5.1 On the Deck, run `headlook` driven by a recorded capture and confirm a game binds and reacts to the virtual device; record in `docs/findings.md` whether Steam holds the device or leaves it to the game.
  - Done in `docs/findings.md`: Steam's SDL drops `ABS_RX`/`ABS_RY` and maps only the gamepad's left stick; the **mouse** is the working device (default), and with the live IMU it drives the desktop pointer correctly (two device bugs fixed: fractional mouse counts, and missing mouse buttons for udev `ID_INPUT_MOUSE`). Still to check: a specific game's camera reacting through the mouse.
- [x] 5.2 On the Deck, measure end-to-end IMU-to-event latency of the headlook path and record p50/p95 in `docs/findings.md`.
- [ ] 5.3 On the Deck, set full SBS with `sbs on`, launch a game in its own side-by-side mode fullscreen, capture the framebuffer and measure that each half is close to 16:9 and not stretched; record the result and the title in `docs/findings.md`, then confirm `sbs off` restores the 2D mode.
- [ ] 5.4 On the Deck, confirm `headlook` leaves the glasses' display mode unchanged and that `sbs` re-sets full SBS after a 2D drop; record the observations in `docs/findings.md`.

## 6. Phase 2 — wearer A/B (user check)

- [ ] 6.1 Run the counterbalanced A/B of rate, stick and absolute: fixed game and scenario, 10 minutes each with washout breaks, the fixed questionnaire, and objective logging (recenter count, drift, clamp saturation). Record the raw results outside the repo.
- [ ] 6.2 Write the preference and the chosen default, with the numbers behind it, to `docs/findings.md`, and set the configurable default; verify a fresh `headlook` run reports the chosen model.

## 7. Phase 3 — endurance and reconciliation

- [ ] 7.1 Run a 45–60 minute session in the chosen model and confirm comfort, recenter cadence, drift over time and SBS stability across a 2D drop; record the results in `docs/findings.md`.
- [ ] 7.2 Update `README.md`, `docs/handoff.md` and `docs/open-questions.md` with the new head-look and flat-stereo tier and what remains unverified; verify `openspec validate --all` passes and the docs name both commands.

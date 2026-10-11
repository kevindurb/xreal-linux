# Head-look test plan (`add-headlook-mode`)

How the head-look tier is validated. Written to be run offline: no network, no package installs (no OpenTrack, no Flatpak), and only things already on the Deck. Phases 0 needs neither the Deck nor a wearer and runs on a dev machine; the later phases queue until the Deck is reachable and the wearer is available.

## What the tests decide

| Id | Question | Phase |
|---|---|---|
| D1 | Does an injected device work, and does Steam hold or hide it? | 1 |
| D2 | Rate vs stick vs absolute, on feel | 2 |
| D3 | Does a game's own side-by-side map cleanly to 3840x1080? | 1 |
| D4 | Drift rate and recenter cadence | 0, 3 |
| D5 | IMU-to-device latency | 0, 2 |
| D6 | Flat-stereo lifecycle: set, hold, restore, survive a 2D drop | 1, 3 |

## Constraints

- **Offline:** no installs, no Steam downloads, no OpenTrack. The virtual device uses `/dev/uinput`, which the active session user can already write on Bazzite via `uaccess` (no root, no udev rule).
- **The wearer is scarce:** only Phases 2 and 3 need a person; Phase 0 and most of Phase 1 do not.
- **The glasses' menu settings still apply:** Follow mode, Stabiliser off, auto sleep off. Stabiliser off matters especially here because the glasses' own stabiliser fights external head tracking.

## Phase 0 — no Deck, no wearer (runs on a dev machine)

Validate the mapping maths and get drift numbers before any hardware.

- **Inputs:** the recorded captures `captures/imu_yaw.bin`, `captures/imu_move.bin`, `captures/xreal_52998_still.bin`.
- **Method:** run each capture through the presenter's `Fusion` and the pure mapping core (`presenter/src/headlook.rs`).
- **Checks:** rate output follows the angular velocity and leaves the other channel alone on a pure-yaw capture; stick/absolute follow head angle and clamp at the range; the deadzone holds a still head; recenter zeroes the output; every axis stays in -1..1.
- **Drift:** the fused yaw change on the still capture, in deg/min, for each model.
- **Evidence:** the trace and the test run (`cd presenter && cargo test --offline headlook`), written into `docs/findings.md`.
- **Pass:** all mapping tests pass; the still-capture deadzone emits no output; the drift number is recorded.

## Phase 1 — Deck, glasses on the desk, no wearer

- **1.1 Injection:** create the virtual gamepad and mouse, drive a scripted motion, confirm `libinput debug-events` / `evtest` / `jstest` list them, and confirm they vanish on exit. Record whether, with Steam running, Steam holds the device or leaves it to the game.
- **1.2 Latency:** instrument IMU-sample to device-event time; record p50/p95.
- **1.3 SBS mapping:** with `sbs on`, launch a game in its own side-by-side mode fullscreen on the glasses, capture the framebuffer, and measure that each half is close to 16:9 and not stretched. Record the title. Confirm `sbs off` restores the 2D mode.
- **1.4 No side effects:** confirm `headlook` leaves the glasses' display mode unchanged, and that `sbs` re-sets full SBS after a 2D drop.
- **Pass:** the device is usable and visible to a game; latency is low enough to feel direct (target &lt; ~10 ms device-side p95); SBS halves are the right aspect.

## Phase 2 — wearer A/B (one evening, user check)

- **Design:** three conditions (rate, stick, absolute), within-subject, **counterbalanced** order, relabelled A/B/C to reduce expectation bias. Same game and fixed scenario, 10 minutes each, ~5 minutes washout between. The mapping code is identical across conditions, so only the model changes.
- **Objective logging:** recenter count per minute, saturation fraction (time at the clamp), yaw drift deg/min on still stretches, IMU-to-device latency.
- **Questionnaire (1-7, plus free text):** comfort; neck-shoulder fatigue; "did I have to recenter more than I wanted?"; "did the view fight me or feel laggy?"; could I reach what I wanted (range); HUD/menu precision; overall ranking of the three.
- **Caveat:** n=1, so this is exploratory. Repeat with a second wearer if a strong signal appears (the project already wanted a second wearer for the IMU axes).
- **Pass:** a condition is comfortably better than the others on comfort **and** precision; its objective numbers (recenters, saturation) agree.
- **Record:** raw results outside the repo; the preference and the chosen default in `docs/findings.md`.

## Phase 3 — endurance and integration (wearer)

- 45-60 minutes in the chosen model: comfort, recenter cadence, drift over time, and SBS stability across a known 2D drop (and whether re-setting needs the glasses power-cycled).
- Reconcile: the chosen default in code, and `README.md` / `docs/handoff.md` / `docs/open-questions.md` updated with what remains unverified.

## Risks

- Steam may hide the virtual device from a game: fall back to the game's own evdev, or the `/dev/uhid` route (also permitted).
- A uinput virtual mouse moves the whole desktop: keep it gated and capped.
- A game's SBS may assume 16:9 and letterbox/stretch on 32:9: measure and record which titles work.
- Some games only enumerate joysticks at launch: a restart may be needed.
- Rate-mode creep depends on still-detection the glasses never truly give: the deadzone and recenter are the mitigation; measure it.
- The offline captures are a single session each and the still capture is only 5 s: treat the drift number as provisional until Phase 3.

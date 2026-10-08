# Tasks

## 1. Decode the Eye stream and put it on the IMU clock (research first; everything else depends on it)

- [ ] 1.1 Record 60 s of ports 52997, 52996 and 52998 together while the glasses sit still, are shaken, and are rotated slowly. Store under a capture directory outside the repo with a documented format. Verify: the capture replays frame by frame and a size/count summary matches the live rates (about 60 fps, 120 records/s, IMU rate).
- [ ] 1.2 Settle the image geometry and pixel packing: show a printed grid at known angles and fit the unpacking so lines are straight and the aspect is right; record the result (including why the left half is interleaved) in `docs/findings.md`. Verify: a decoded frame of the grid has straight lines and the correct aspect, saved as a PNG (kept out of the repo).
- [ ] 1.3 Identify the frame timestamp field (header bytes 23-26 and the 52996 records) and measure the camera-to-IMU lag with the shake capture (gyro magnitude against optical-flow magnitude). Verify: the measured lag and its spread are written to `docs/findings.md`, and the within-5-ms requirement is met or the shortfall is stated.
- [ ] 1.4 Write the C++ reader/decoder for the camera stream (frame sync, resync after a short frame, reconnect, absent-camera handling) with unit tests on recorded frames. Verify: unit tests pass in the fedora:44 container, and a live run on the Deck prints about 60 decoded frames/s.
- [ ] 1.5 Record/replay tool that feeds a capture through the same reader. Verify: replaying a capture yields byte-identical decoded frames to the live run that produced it.

## 2. Calibration

- [ ] 2.1 Intrinsics tool: capture checkerboard views, fit pinhole and equidistant models with OpenCV, pick by reprojection error. Verify: reports error per model; a good run is under the documented threshold.
- [ ] 2.2 Camera-IMU capture session and conversion for Kalibr (extrinsics and time offset). Verify: Kalibr runs on the converted capture in its container and prints a solution with a stated residual.
- [ ] 2.3 Calibration file (JSON keyed by glasses serial), writer that refuses poor results, loader that refuses mismatched devices. Verify: unit tests for write/refuse/mismatch; a good file loads and prints its date and error.
- [ ] 2.4 Offline scoring of a capture against a calibration. Verify: scoring the calibration capture reproduces the reprojection error the tool reported.
- [ ] 2.5 Document the calibration procedure in a README (printed target, motions, thresholds). Verify: someone following only the README produces an accepted calibration (user check).

## 3. Offline visual-inertial run (go/no-go)

- [ ] 3.1 Build OpenVINS's ROS-free core in the fedora:44 container with a thin adapter that takes our images, IMU records and calibration. Verify: the adapter builds and runs on a replayed capture without crashing.
- [ ] 3.2 Record walks of known shape (a 1 m square, a 30 cm lean and back) in a textured room. Verify: captures replay and the path lengths are tape-measured and noted.
- [ ] 3.3 Offline scoring: end-point error and maximum deviation against the measured path, plus a still-for-60-s drift number. Verify: the tool prints the numbers for the recordings; the spec thresholds (5 cm lean, 2 cm still) are met or the gap is written in `docs/findings.md`.
- [ ] 3.4 CPU budget on the Deck: run the replay at real time beside SteamVR Home and measure CPU per core and the added camera-to-position latency. Verify: numbers in `docs/findings.md`; if SteamVR drops below 60 new frames/s or latency exceeds 50 ms, stop here and record why (this is the go/no-go for the rest of the change).

## 4. Live tracker and fallback tiers

- [ ] 4.1 `xreal-tracker` process: live camera and forwarded IMU in, position and tier out, status file, settings (off by default), niceness/affinity. Verify: starts and stops cleanly, the status file reflects the tier, and `tools/vr_session.sh` can start it when enabled.
- [ ] 4.2 Presenter: forward raw IMU records to the tracker, receive position, apply the yaw offset and lever arm, and send the new position message to the driver. Verify: a unit test of the alignment maths; the presenter log shows tier changes and the position rate.
- [ ] 4.3 Tier logic: 3DoF without camera or calibration, 6DoF when tracking, 6DoF-lost within 1 s of losing tracking, hold the last trusted position, recover without a restart. Verify: replay captures with the camera covered and with a blank wall; the tier and position behave as the spec scenarios say.
- [ ] 4.4 `tools/doctor.sh` reports the tier, calibration and tracker status, warning (not failing) when calibration is missing. Verify: run doctor with and without a calibration file and with no Eye.

## 5. Driver and SteamVR

- [ ] 5.1 Driver: new message type, report position and linear velocity, head model off while position is trusted, fixed height otherwise, no jump on loss. Verify: with a simulated position stream (`--sim-pose` extended to move) the driver log and SteamVR's head pose follow it; unplugging the stream returns to the fixed height.
- [ ] 5.2 Existing specs and READMEs updated: `driver/README.md` (new setting and message), `presenter/README.md`, the specs in this change synced. Verify: `openspec validate` passes and the READMEs describe the 3DoF fallback.
- [ ] 5.3 Hardware check with a wearer: lean, crouch, step, cover the camera, look at a blank wall, plus a long session. Verify: a user check against the spec scenarios, with SteamVR at 60 new frames/s, recorded in `docs/findings.md`.
- [ ] 5.4 Decide the default (off, or on when calibrated) from 5.3 and update the setting, README and `openspec/config.yaml` project notes. Verify: a fresh session on the Deck comes up in the chosen tier with the doctor output matching.

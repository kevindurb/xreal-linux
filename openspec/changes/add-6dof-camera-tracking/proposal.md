# Proposal

## Why

The glasses track rotation only (3DoF). Leaning, crouching or stepping does not move the view, which breaks immersion and causes discomfort in any room-scale or seated-lean content. The XREAL Eye camera plus the IMU we already read is the only source of position on Linux: XREAL's own 6DoF is documented for Android hosts only, and no stream we have seen carries a pose. The camera stream is partly decoded already, so this is the next tier the project promised, built so the IMU-only tier keeps working for anyone without an Eye.

## What We Know and What We Assume

Verified on the hardware (`docs/findings.md`):
- The Eye's frames arrive on TCP port 52997 at about 60 fps, each 193,862 bytes, starting `27 48 00 02`, header bytes 23-26 changing per frame.
- The image payload is 189 rows of 1024 bytes: the right half is a clean 512 x 189 grayscale image; the left half interleaves two 256 x 189 renderings (darker on even columns, brighter on odd).
- Port 52996 carries timestamps on the same clock as the IMU and camera, two records per camera frame.
- There is no stereo baseline between the views: one viewpoint.
- The glasses' control port (TCP 52999) takes the vendor SDK's request ids in a transaction-id frame (`docs/xreal-link-messages.md`, section 13). One read-only `GetConfig` request returned the glasses' **factory calibration** as JSON, among it the SLAM camera: radial lens model, 504 x 378, focal length about 238.8 px, principal point about (253.3, 190.4), the camera's position and rotation relative to the IMU (`imu_p_cam`, `imu_q_cam`), a rolling-shutter time of 1.79 ms, and the IMU's biases, calibration matrices, noise figures and a temperature table. It does not contain the camera-to-IMU time offset.
- A recorded frame, with the right half stretched to twice its height, is a natural 4:3 view of the room, which matches the 504 x 378 in the config. So the stream carries every second sensor row (189 of 378); a guess that two image rows share each payload row scored worse (adjacent-row correlation 0.53 against 0.88) and is dropped.

Assumed, not yet verified (each names the experiment that settles it):

| Assumption | Experiment |
|---|---|
| Camera frame header bytes 23-26 are a timestamp on the IMU's clock | Record both streams while shaking the glasses; correlate gyro magnitude with optical-flow magnitude and read the lag |
| Each stream pixel covers two sensor rows, so the factory vertical focal length and principal point halve (about 119 px and 95 px) and the first 504 of the 512 columns are the image (inferred from the 4:3 look and the 504 x 378 in the config) | Show a printed grid at known angles and fit the unpacking so lines are straight; check the checkerboard reprojection error with the vertical values halved against unscaled |
| The factory intrinsics (a radial model, not fisheye) and the camera-to-IMU transform are accurate enough to use as they are | Capture a checkerboard and score the reprojection error; score the extrinsics on a shake capture. Fit our own with OpenCV and Kalibr only if either exceeds its threshold |
| A host request can start the camera in Follow mode with the Stabilizer off (the control port takes the SDK's camera Create and Start ids, 10047 and 10053, the latter inferred) | With the wearer's approval, send the documented request, watch port 52997 for frames and port 52999 for the start event (10002), then send Stop (10054) |
| A single camera with the IMU gives usable position (metric scale from the accelerometer) | Run a monocular visual-inertial estimator offline on a recorded walk-around and compare with a tape-measured path |
| The 60 fps stream is enough for head motion | Measure feature-track survival across fast turns in recorded data |

## What Changes

- Decode the Eye stream into usable images (resolving the geometry and the interleaved halves) with timestamps on the IMU clock.
- Calibrate the camera from the glasses' own factory calibration (read-only request on the control port, cached per serial), measure only the camera-to-IMU time offset with a short shake capture, and check the factory intrinsics and extrinsics against a checkerboard. A full fit with OpenCV and Kalibr stays as a fallback if the checks fail.
- Estimate head position with a monocular visual-inertial estimator, fused with the existing IMU orientation, in a process separate from the presenter.
- Publish position (and linear velocity) alongside orientation to the driver, which reports it to SteamVR in place of the fixed head height.
- Keep 3DoF as the fallback tier: no camera, no calibration or lost tracking falls back to orientation with the fixed head height, and says so.
- Tools to capture, replay and score tracking runs offline, since nobody should have to wear the glasses to test it.

## Capabilities

### New Capabilities
- `eye-camera-stream`: reading, decoding and timestamping the Eye camera frames from port 52997.
- `camera-calibration`: intrinsics, camera-to-IMU extrinsics and time offset, where they come from (the glasses' factory calibration, plus a measured time offset), and how they are cached, checked and loaded.
- `position-tracking`: the visual-inertial position estimate, its tier selection and fallback, and the data it publishes.

### Modified Capabilities
- `imu-tracking`: "Publish the pose to the driver" gains position, linear velocity and a tracking tier, with 3DoF behaviour unchanged when no position is available.
- `steamvr-driver`: "Report the tracked pose" reports the position it receives instead of the fixed head height, and keeps the fixed height for the 3DoF tier.

## Impact

- New code: a tracker process (language to be decided in design), camera decoding, calibration tooling and a replay/scoring tool under `tools/`.
- `presenter/src/tracking.rs` and the driver link message gain position fields; `driver/src/xreal_driver.cpp` reports them.
- A small control-port client (read-only `GetConfig`, with a timeout, tested against a recorded response), shared with the presenter's later use of the same config.
- New dependency: a visual-inertial estimator (OpenVINS, VINS-Mono or similar) or an in-house filter; a calibration target and a capture session on the Deck.
- CPU load on the Deck (the GPU is already busy with SteamVR), which is a real constraint.
- Recorded camera data shows the inside of a home: captures stay out of the repo, as today.

## Non-goals

- Controllers or hand tracking.
- Loop closure, relocalisation across sessions or a persistent map (visual-inertial odometry only at first).
- Room setup, boundary or floor detection beyond a user-set floor height.
- Replacing the 3DoF tier or making the camera required.
- Supporting XREAL glasses other than the 1S with the Eye.
- Using any XREAL SDK or proprietary binary.

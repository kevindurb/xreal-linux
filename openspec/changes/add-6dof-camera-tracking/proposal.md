# Proposal

## Why

The glasses track rotation only (3DoF). Leaning, crouching or stepping does not move the view, which breaks immersion and causes discomfort in any room-scale or seated-lean content. The XREAL Eye camera plus the IMU we already read is the only source of position on Linux: XREAL's own 6DoF is documented for Android hosts only, and no stream we have seen carries a pose. The camera stream is partly decoded already, so this is the next tier the project promised, built so the IMU-only tier keeps working for anyone without an Eye.

## What We Know and What We Assume

Verified on the hardware (`docs/findings.md`):
- The Eye's frames arrive on TCP port 52997 at about 60 fps, each 193,862 bytes, starting `27 48 00 02`, header bytes 23-26 changing per frame.
- The image payload is 189 rows of 1024 bytes: the right half is a clean 512 x 189 grayscale image; the left half interleaves two 256 x 189 renderings (darker on even columns, brighter on odd).
- Port 52996 carries timestamps on the same clock as the IMU and camera, two records per camera frame.
- There is no stereo baseline between the views: one viewpoint.

Assumed, not yet verified (each names the experiment that settles it):

| Assumption | Experiment |
|---|---|
| Camera frame header bytes 23-26 are a timestamp on the IMU's clock | Record both streams while shaking the glasses; correlate gyro magnitude with optical-flow magnitude and read the lag |
| The image geometry is wrong at 189 rows (looks squashed) and the true aspect and pixel packing differ | Show a calibration target at known angles and measure; try plausible unpackings |
| The lens is wide-angle (fisheye) | Calibrate with a checkerboard and compare pinhole against equidistant/Kannala-Brandt fits |
| A single camera with the IMU gives usable position (metric scale from the accelerometer) | Run a monocular visual-inertial estimator offline on a recorded walk-around and compare with a tape-measured path |
| The 60 fps stream is enough for head motion | Measure feature-track survival across fast turns in recorded data |

## What Changes

- Decode the Eye stream into usable images (resolving the geometry and the interleaved halves) with timestamps on the IMU clock.
- Calibrate the camera: intrinsics, camera-to-IMU extrinsics and time offset, stored per device and loaded at start.
- Estimate head position with a monocular visual-inertial estimator, fused with the existing IMU orientation, in a process separate from the presenter.
- Publish position (and linear velocity) alongside orientation to the driver, which reports it to SteamVR in place of the fixed head height.
- Keep 3DoF as the fallback tier: no camera, no calibration or lost tracking falls back to orientation with the fixed head height, and says so.
- Tools to capture, replay and score tracking runs offline, since nobody should have to wear the glasses to test it.

## Capabilities

### New Capabilities
- `eye-camera-stream`: reading, decoding and timestamping the Eye camera frames from port 52997.
- `camera-calibration`: intrinsics, camera-to-IMU extrinsics and time offset, how they are measured, stored and loaded.
- `position-tracking`: the visual-inertial position estimate, its tier selection and fallback, and the data it publishes.

### Modified Capabilities
- `imu-tracking`: "Publish the pose to the driver" gains position, linear velocity and a tracking tier, with 3DoF behaviour unchanged when no position is available.
- `steamvr-driver`: "Report the tracked pose" reports the position it receives instead of the fixed head height, and keeps the fixed height for the 3DoF tier.

## Impact

- New code: a tracker process (language to be decided in design), camera decoding, calibration tooling and a replay/scoring tool under `tools/`.
- `presenter/src/tracking.rs` and the driver link message gain position fields; `driver/src/xreal_driver.cpp` reports them.
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

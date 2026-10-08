# Design

## Context

The presenter (Rust) reads the IMU over TCP 52998, fuses orientation and sends it to the driver over an abstract unix socket every ~2 ms (`presenter/src/tracking.rs`, driver `kMsgPose`). The driver reports that orientation at a fixed head height. The pose message is a fixed 16-word packet whose words 0-12 are used. The Eye's frames are on TCP 52997 and 52996 carries timestamps on the IMU's clock, but nothing reads either in the product code today. The glasses' control port (TCP 52999) answers the vendor SDK's request ids and returns the factory calibration on request. See proposal.md for what is verified and what is assumed.

Constraints that shape the approach:
- The Steam Deck's GPU is busy with SteamVR and the presenter; the CPU has headroom but is a 4-core/8-thread Zen 2 shared with SteamVR. The tracker must be CPU-only and light.
- The Deck host has no Rust and no libstdc++-static: builds happen in podman containers (fedora:44), as for the driver and presenter.
- vrserver runs inside Steam's pressure-vessel container; anything in the tracker path stays on the host, outside it.

## Goals / Non-Goals

**Goals:**
- Position with the orientation path's latency and robustness unchanged: the 2 ms IMU rotation path must not wait on the camera.
- A tracker that can be developed, tested and scored from recordings with no wearer.
- Clean fallback: losing the camera costs position, never rotation.

**Non-Goals:**
- GPU use by the tracker.
- Any change to how rotation is fused, other than optionally using the camera to bound yaw drift later.
- A second implementation of the estimator in this change.

## Decisions

### 1. A separate tracker process, not code inside the presenter

`xreal-tracker` is its own process. The presenter stays the only reader of the IMU port and forwards the raw IMU records (with the glasses' timestamps) to the tracker over a local unix socket; the tracker reads the camera port itself and returns position updates over the same socket. The presenter merges position into the pose it already sends.

Why: a tracker crash, stall or CPU spike must not touch display or rotation; a separate process can be niced and pinned to cores; the estimator is a C++ library and the presenter is Rust. Why the presenter stays the IMU reader: whether the glasses accept two clients on port 52998 is unverified, and a single reader keeps one timeline.

Alternatives: in-process in the presenter (couples failure and language, rejected); the tracker reads the IMU too (risk of a second client on the port, and two copies of the parsing).

### 2. Use an existing monocular visual-inertial estimator: OpenVINS

OpenVINS (an EKF/MSCKF estimator with a ROS-independent core, monocular plus IMU, online refinement of the camera-IMU extrinsics and time offset) is wrapped in a thin binary with our camera and IMU adapters.

Why: it is built for a single camera with an IMU, is filter-based so its CPU cost is low and predictable, propagates state at the IMU rate (which gives the low-latency position we want), and can refine our calibration online. Alternatives: VINS-Mono (optimisation-based, ROS-oriented, heavier), ORB-SLAM3 mono-inertial (heavier, map-centric, more than the non-goals ask for), an in-house MSCKF (large effort, deferred). The choice is revisited in task group 3 if the offline run is not good enough; the interfaces in decisions 1, 4 and 5 do not depend on it.

### 3. Decode the camera in the tracker, in C++

The tracker owns frame reading and decoding so images go straight to the estimator without crossing a process boundary. `tools/decode_camera_frame.py` stays as the research tool and the reference for the unpacking until the format is settled in task group 1, which is done first because everything else depends on it.

### 4. Position is published separately from the 16-word pose message

Words 13-15 are all that is free in the existing message, which cannot hold position, velocity and a tier. The presenter instead sends a new message type to the driver (position, linear velocity, tier, age), and the driver combines it with the latest orientation. The orientation message and its timing are untouched, so the 3DoF tier is byte-for-byte what it is today.

### 5. Frames and alignment

The estimator's world frame is gravity-aligned like ours, with an arbitrary yaw. The tracker reports position in its own frame; the presenter applies the yaw offset between the tracker's orientation and the fused IMU orientation (estimated continuously), and a fixed lever arm from the camera to the head origin. Position is relative to where tracking starts; height comes from a user floor height, not from the visual estimate.

### 6. Calibration starts from the glasses' factory calibration

The tracker asks the glasses for their configuration (read-only `GetConfig`, request id 10015 on port 52999, `docs/xreal-link-messages.md` section 13) and uses its SLAM-camera intrinsics (radial model) and camera-to-IMU transform (`imu_p_cam`, `imu_q_cam`) as the starting calibration. The response is cached per serial in the user's config directory, never in the repo (it contains the serial), and a cached copy is used when the glasses cannot be asked. The camera-to-IMU time offset is not in the file: it is measured with a short shake capture (task 1.3) and stored beside the cache.

The stream carries half the sensor rows (inferred, see the proposal), so the vertical focal length and principal point are halved and only the first 504 columns are used until a printed grid shows otherwise. OpenVINS's online refinement of the extrinsics and time offset then starts from these values.

Checks before trust: a checkerboard capture is scored for reprojection error and a shake capture for the extrinsics, each against a documented threshold. Only if a check fails do we fall back to the original plan: fit intrinsics with OpenCV on a printed checkerboard and the camera-IMU extrinsics with Kalibr in a container.

Alternatives: always fitting our own calibration (the extra work is only needed if the factory values are not good enough); relying only on online calibration (no ground truth for a first fix, risky); an in-house camera-IMU solver (later).

Dependence on the Deck: none; the request is the same on any Linux host with the glasses on their link-local network. The cached file must be per unit, since the calibration differs between pairs of glasses.

### 7. Status and tiers

The tracker writes a small status file under `$XDG_RUNTIME_DIR` (tier, quality, calibration date, last error) that `tools/doctor.sh` and the presenter's report read. Tier changes are logged with their cause.

### 8. Recording and scoring come first

A capture format (frames, IMU, timestamps) and a replay path that runs the same code as live are built before the live integration, so every later step is tested offline. Scoring uses tape-measured paths and, where available, a second reference.

## Risks / Trade-offs

- [The Eye does not stream in Follow mode with the Stabilizer off] → Observed on hardware: the camera stream is idle outside the glasses' own anchor mode. Task group 0 finds out whether a host request can start it: the control port takes the SDK's request ids (8 of 8 checked ids match the public `one-xr` library), so `NRGrayscaleCameraCreate` (10047) and Start (10053, inferred) are candidates, with the field layouts in `docs/xreal-link-messages.md` section 8. Every state-changing request needs the wearer's explicit approval and is shown byte for byte first. Nothing below group 0 can be verified on hardware until this is settled; if no request works, the change is re-scoped.
- [The Deck's CPU cannot run the estimator at 60 fps beside SteamVR] → Measure in task group 3 before integrating; reduce features/resolution, pin and nice the process, or drop to every second frame. This is the main go/no-go.
- [The camera's field of view or exposure is poor for tracking] → The offline run on real recordings in a textured room decides; if poor, the change stops at the decoder and calibration.
- [The factory calibration is less accurate than a fit of our own] → The checks in decision 6 score it before use, and the fitting tasks stay as the fallback.
- [Time offset between camera and IMU is not constant or not recoverable from header bytes] → Estimate it in calibration and refine online; the 5 ms shake-test requirement catches it.
- [Monocular scale and initialisation need motion] → Tell the user at start; the tier stays 3DoF until initialised.
- [Position drift or jumps feel worse than no position] → The fallback requirement holds position when trust is lost, and the tier makes it visible; an off switch (`position_tracking` false) is part of the tracker's settings.
- [Packaging an OpenVINS-based binary for a Flatpak later] → Dependencies are Eigen and OpenCV; containerised builds keep this tractable.
- [AMD/Wayland/Bazzite specific] → Nothing here is GPU or compositor specific; the tracker is CPU-only, so other Linux PCs work. The Deck-specific part is the CPU budget and the container builds.

## Migration Plan

Behind a setting off by default (`driver_xreal.use_position` and the tracker only starts when asked), so the 3DoF tier is unchanged until it is proven. Rollback is stopping the tracker: the driver falls back to the fixed height on its own.

Settings the user must keep: Stabilizer off, Follow mode and full SBS as today; plus a floor height setting and a one-time calibration with a printed target. Nothing touches the pressure-vessel boundary.

## Open Questions

- Whether the "darker and brighter" halves are two exposures that help tracking: answered while decoding, and only affects which image the estimator is given.
- Whether the missing rows are skipped or binned, which changes the effective vertical focal length: the grid check decides.
- Whether the tracker can later also bound yaw drift for the presenter: a possible follow-up once position works.

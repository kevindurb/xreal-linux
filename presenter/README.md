# Presenter

A fullscreen Vulkan presenter for the glasses (Rust: `ash`, `winit`, `ash-window`). It draws a side-by-side stereo **test pattern** until the SteamVR driver connects, then presents SteamVR's frames. The
test pattern so we can check that the glasses show a different image to each eye in their SBS mode. It will
grow into the process that imports SteamVR's per-eye textures from the driver and presents them (with lens correction and
late-latching reprojection).

    xreal-presenter [--monitor NAME] [--reproject]      # default DP-1; Esc quits
    options: --no-imu-calibration  --mag-yaw (experimental)  --set-sbs (sends one setter, see below)  --print-calibration (print and exit)

Left half = left eye (red tint), right half = right eye (blue tint). A green square slides across each half with a
24 px offset between the eyes: in a working stereo mode it should appear to float in front of the frame. White borders
and a centre line mark the exact edges.

Build (the Deck's host has no Rust, so use a container; the result runs on the host because it only needs Vulkan, Wayland
and xkbcommon, all loaded at runtime):

    podman run --rm -v "$PWD":/src:Z -v xreal-cargo-cache:/root/.cargo:Z -w /src registry.fedoraproject.org/fedora:44 \
      bash -c "dnf -y install rust cargo gcc && cargo build --release"

Notes for the real thing, from ALVR's Linux driver (MIT): SteamVR's swap textures are exported as fds that must be imported
with Vulkan's *opaque-fd* external memory type using the same image parameters, on the same GPU. That is why this has to
be a Vulkan program and not a plain Wayland dma-buf hand-off.

## What it does with SteamVR

The driver sends each swap-texture set's fds over the abstract socket `@xreal-presenter-<uid>` and a small message per
presented frame. The presenter imports the three textures per eye into its own Vulkan device (opaque-fd external memory,
matching SteamVR's image parameters, same GPU) and blits each eye's latest image into its half of the swapchain. There is
no head tracking, lens correction or reprojection yet, and no GPU synchronisation with the compositor's writes
(expect occasional tearing). Needs the glasses in full SBS (a single `3840x1080` mode), follow mode, Stabilizer off.

## Reprojection (`--reproject`; `tools/vr_session.sh` turns it on by default)

SteamVR's compositor does no async reprojection for a direct-mode driver, so when the app misses a frame the old one is simply
repeated. With `--reproject` the presenter warps each eye by how far the head has turned since SteamVR rendered it: the driver
sends the pose SteamVR rendered the frame for (the layer's `mHmdPose`), the presenter compares it with its latest tracked pose, and
a fragment shader (`shaders/warp.frag`, mirrored by the unit-tested `src/warp.rs`) samples the rendered frame along each pixel's current
line of sight. Rotation only, no position. It assumes SteamVR's universe origin has no rotation (no recentre), because the render pose
is in SteamVR's space and the tracked pose is in the driver's; a recentre would offset the two. Shaders are precompiled
(`glslc warp.vert -o warp.vert.spv`, same for the fragment shader) and committed.

The presenter also keeps itself on the glasses' output: if the compositor drops the window on another output while the glasses re-plug
(mode change, sleep and wake), it moves it back.

## Control port (`src/glasses.rs`)

The presenter connects to the glasses' control port (TCP 52999), reads the factory calibration (`NRGlassesGetConfig`, read-only), derives the field of view, IPD and IMU matrices, sends the field of view and IPD to the driver (message type 7) and logs the glasses' events. The reply is cached under `~/.cache/xreal-presenter/` (never in the repo; it contains the serial number). `--set-sbs` additionally reads the input mode and, if it is 0, sends `NRDpSetInputMode` = 1; test it only with the wearer's approval of those exact bytes. `--mag-yaw` is experimental: rotate the glasses slowly through many directions for about 30 s after start so the hard-iron fit settles.

## Factory display distortion and the test grid (`--factory-distortion`, `--test-grid`)

The glasses' config holds a 61 x 39 grid per eye (`display_distortion`): panel pixel (x, y), 32 pixels apart, to the position where its light is seen in the ideal picture (corners about 22-26 px outward, centre under 1 px). With `--factory-distortion` the warp shader moves each output pixel through that grid first, assuming each eye's 1080-row picture is centred in the panel's 1200 rows; `--factory-distortion-reversed` applies the opposite displacement. Both are off by default because the direction is not verified on hardware. `--test-grid` draws straight lines (120 picture pixels apart, a frame and a centre cross) through the same pass, with or without SteamVR, for judging the correction. Wearer check, on the glasses in full SBS: run `xreal-presenter --monitor DP-1 --test-grid` and note how the frame and lines bend near the edges, then `--test-grid --factory-distortion` and then `--test-grid --factory-distortion-reversed`. The right one shows straight lines everywhere in the picture; the others look worse (more curved) than no correction. With `--dump DIR` and `touch DIR/trigger` the centre crop can be captured without SteamVR.

## Magnetometer calibration (`--mag-calibrate`, `--mag-report`, `--mag-yaw`)

`xreal-presenter --mag-calibrate` (no SteamVR needed) prints one live line (samples, directions visited of 26, fitted centre, radius, per-axis scale, residual) while you turn the glasses slowly through every direction: look up, down, left, right, tilt each shoulder, a full turn each way. When coverage and residual pass it saves a per-unit calibration under `~/.cache/xreal-presenter/` and exits. `--mag-yaw` then uses it instead of learning. `xreal-presenter --mag-report` holds still for 60 s (glasses on a table) and prints the yaw drift with and without the correction. Not validated on real data; the glasses' field at rest was 81 uT in one session against 50 uT earlier, so recalibrate when the surroundings change.

## Display-mode log

The presenter logs `[display +T s] modes: ...` whenever the connector's mode list changes, on the same clock as `[control +T s] event ...`. After a session: `python3 tools/analyze_control_events.py /tmp/presenter.log` lists, for each drop from full SBS to 2D, the events in the 30 s before it.

# Presenter

A fullscreen Vulkan presenter for the glasses (Rust: `ash`, `winit`, `ash-window`). It draws a side-by-side stereo **test pattern** until the SteamVR driver connects, then presents SteamVR's frames. The
test pattern so we can check that the glasses show a different image to each eye in their SBS mode. It will
grow into the process that imports SteamVR's per-eye textures from the driver and presents them (with lens correction and
late-latching reprojection).

    xreal-presenter [--monitor NAME] [--reproject]      # default DP-1; Esc quits
    options: --no-set-sbs | --set-sbs-only (set SBS if needed, then exit)  --no-imu-calibration  --eye-rotation | --eye-rotation-reversed (experimental)  --test-grid  --print-calibration (print and exit)

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

The presenter connects to the glasses' control port (TCP 52999), reads the factory calibration (`NRGlassesGetConfig`, read-only), derives the field of view, IPD, IMU matrices and display orientations, sends the field of view and IPD to the driver (message type 7) and logs the glasses' events; a quiet connection is kept, and it reconnects only when the connection really ends. The reply is cached under `~/.cache/xreal-presenter/` (never in the repo; it contains the serial number).

**It sets full side-by-side itself:** it reads the input mode (`NRDpGetInputMode`) and, only if it is 0, sends `NRDpSetInputMode` = 1 (`28 22 00 00 00 08 80 00 00 03 1a 02 08 01`), at most once per connection and three times per run; a rejected or unanswered setter is logged and not retried. `--no-set-sbs` turns that off, and `--print-calibration` never sends it. Every test run on glasses you do not want switched needs `--no-set-sbs`. When the output changes mode the window is moved back onto it by leaving fullscreen and re-entering it.

## Test grid (`--test-grid`)

`xreal-presenter --monitor DP-1 --test-grid` draws straight lines every 120 picture pixels, a frame, a centre cross and, in the four corners, a ruler with a tick every 20 rows from the top and bottom edges, instead of the eye images (also without SteamVR). Use it to judge the visible area. On the tested unit, in full SBS: the lines looked straight, there were no black bars above and below, and the bottom border was hidden by the lens's curved edge. The factory display-distortion grid made it worse in both directions and its code was removed (see `docs/findings.md`).

## Per-eye display rotation (`--eye-rotation`, experimental)

The config gives each display's orientation relative to the IMU. On the tested unit they differ by 0.885 degrees, almost all of it about the vertical axis (0.87 degrees, about 38 px at the panel's focal length of 2490 px): with parallel eye cameras, as SteamVR renders them, infinity would be seen at about 4.2 m. `--eye-rotation` rotates each eye's sampling rays by its display's orientation relative to the pair's mean (about 19 px per eye, opposite ways), in the warp pass (also without `--reproject`). The sign convention of the factory quaternions is not verified; `--eye-rotation-reversed` applies the opposite one. Wearer check: in SteamVR Home look at something far away (the sky, the far wall) for a minute with each flag and without; the correct sign makes distant things feel farther away and the reversed one makes the picture strain to fuse. Stop at once if it hurts. The shift opens a strip of about 19 px of black at one edge of each eye's picture.

## Display-mode log

The presenter logs `[display +T s] modes: ...` whenever the connector's mode list changes, on the same clock as `[control +T s] event ...`. After a session: `python3 tools/analyze_control_events.py ~/.local/state/xreal-linux/presenter.log` lists, for each drop from full SBS to 2D, the events in the 30 s before it.

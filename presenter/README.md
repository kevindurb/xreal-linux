# Presenter

A fullscreen Vulkan presenter for the glasses (Rust: `ash`, `winit`, `ash-window`). It draws a side-by-side stereo **test pattern** until the SteamVR driver connects, then presents SteamVR's frames. The
test pattern so we can check that the glasses show a different image to each eye in their SBS mode. It will
grow into the process that imports SteamVR's per-eye textures from the driver and presents them (with lens correction and
late-latching reprojection).

    xreal-presenter [--monitor NAME] [--reproject]      # default DP-1; Esc quits

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

## Reprojection (`--reproject`, experimental)

SteamVR's compositor does no async reprojection for a direct-mode driver, so when the app misses a frame the old one is simply
repeated. With `--reproject` the presenter warps each eye by how far the head has turned since SteamVR rendered it: the driver
sends the pose SteamVR rendered the frame for (the layer's `mHmdPose`), the presenter compares it with its latest tracked pose, and
a fragment shader (`shaders/warp.frag`, mirrored by the unit-tested `src/warp.rs`) samples the rendered frame along each pixel's current
line of sight. Rotation only, no position. It assumes SteamVR's universe origin has no rotation (no recentre), because the render pose
is in SteamVR's space and the tracked pose is in the driver's; a recentre would offset the two. Shaders are precompiled
(`glslc warp.vert -o warp.vert.spv`, same for the fragment shader) and committed.

The presenter also keeps itself on the glasses' output: if the compositor drops the window on another output while the glasses re-plug
(mode change, sleep and wake), it moves it back.

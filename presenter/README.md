# Presenter

A fullscreen Vulkan presenter for the glasses (Rust: `ash`, `winit`, `ash-window`). Today it only draws a side-by-side
stereo **test pattern** so we can check that the glasses show a different image to each eye in their SBS mode. It will
grow into the process that imports SteamVR's per-eye textures from the driver and presents them (with lens correction and
late-latching reprojection).

    xreal-presenter [--monitor NAME]      # default DP-1; Esc quits

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

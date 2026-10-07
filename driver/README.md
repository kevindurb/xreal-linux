# SteamVR driver (prototype)

A minimal SteamVR driver that presents itself as an HMD using **driver direct mode**, so SteamVR's compositor does not
need a DRM lease of the display. It asks SteamVR for the per-eye swap textures (dma-bufs) and counts the frames SteamVR
presents. It forwards the swap-texture fds and a message per presented frame to the presenter (see `presenter/`) over an
abstract unix socket. It still reports a fixed head pose.

Status: verified on SteamVR 2.17.10 on a Steam Deck (Plasma 6.7 Wayland). The compositor logs
`Headset is using driver direct mode`, SteamVR Home renders into the driver's textures, and `Present` ticks at the
configured 60 Hz. See `docs/findings.md`.

The texture handling follows ALVR's Linux driver (MIT licensed),
`alvr/server_openvr/cpp/platform/linux/OvrDirectModeComponent.cpp`.

## Build

`./build.sh` fetches Valve's OpenVR headers into `.openvr/` and builds `xreal/bin/linux64/driver_xreal.so` with a static
C++ runtime (SteamVR's runtime has an older libstdc++). It needs `libstdc++-static`, so on an immutable host build in a
container, for example:

    podman run --rm -v "$PWD":/src:Z -w /src registry.fedoraproject.org/fedora:44 \
      bash -c "dnf -y install gcc-c++ libstdc++-static binutils git && ./build.sh"

The result links against glibc up to 2.38, fine for a current distro; a build for wider distribution should use an older
base image.

## Register with SteamVR

    ~/.local/share/Steam/steamapps/common/SteamVR/bin/vrpathreg.sh adddriver /path/to/driver/xreal

and in `config/steamvr.vrsettings` set `steamvr.forcedDriver` to `xreal`, `requireHmd` to true and disable `driver_null`.
Undo with `vrpathreg.sh removedriver /path/to/driver/xreal`.

## Why an abstract socket

SteamVR's `vrserver` runs inside Steam's pressure-vessel container with its own mount namespace, so it cannot see a socket
file under `/run/user/<uid>`. An abstract unix socket (`@xreal-presenter-<uid>`) lives in the network namespace and is
reachable from inside the container. The presenter checks `SO_PEERCRED` and only accepts the same user.

## SteamVR settings that matter for this headset (config/steamvr.vrsettings)

Found while tuning on a Steam Deck. All are plain SteamVR settings, not driver code:

- `power.turnOffScreensTimeout` (default **5 s**) and `power.pauseCompositorOnStandby` (default true): the glasses have no
  proximity sensor, so SteamVR decides the user has left after a few seconds without head movement, enters standby and
  pauses the compositor. That showed up as slow presents (`layers=0`) and a stuttery feel. Set the timeout very large and
  `pauseCompositorOnStandby` to false.
- `driver_xreal.render_width` / `render_height` (per eye): the driver's recommended render size, 1280x720 by default. The
  Deck's GPU was pinned at 100% at 1920x1080 per eye with SteamVR Home; 1280x720 leaves headroom.
- `driver_xreal.hold_after_present` (default true): holds SteamVR in `PostPresent` until the next running start (2 ms before
  the glasses' vblank). Set false to compare.
- `driver_xreal.seconds_from_vsync_to_photons`: overrides the advertised latency (default: the running start plus one refresh;
  the panel's own latency is not measured).
- `steamvr.enableHomeApp`: Home is heavy; turning it off lets SteamVR idle, but then there is little to look at.
- `driver_xreal.head_height` (metres, default 1.5): the driver reports the head at this height in standing space, or the
  wearer appears to be in the floor.
- Display layout: with the glasses as the only active display, every desktop window (Steam, SteamVR dialogs) lands on them
  and, in full SBS, shows up in one eye. Keep the Deck's own panel enabled and primary
  (`kscreen-doctor output.eDP-1.enable output.eDP-1.priority.1 output.DP-1.priority.2`) so windows open there.

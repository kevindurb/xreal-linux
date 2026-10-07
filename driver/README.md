# SteamVR driver (prototype)

A minimal SteamVR driver that presents itself as an HMD using **driver direct mode**, so SteamVR's compositor does not
need a DRM lease of the display. It asks SteamVR for the per-eye swap textures (dma-bufs) and counts the frames SteamVR
presents. It does not display anything yet and reports a fixed head pose.

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

# Proposal

## Why

Today the project only works for someone who can build it: a Rust presenter and a C++ driver built in podman containers, an `rsync` to the Deck, `vrpathreg.sh adddriver` by hand, hand-edited `steamvr.vrsettings`, and shell scripts that assume the repo lives at `~/xreal-linux`. The goal is an easy download that works on a Steam Deck and on any Linux gaming PC, so other XREAL 1S owners can use it. The pacing work is done and verified, so the setup is stable enough to package.

## What We Know and What We Assume

Verified here (`docs/findings.md`, `driver/README.md`, the specs):
- The driver loads inside SteamVR's container when it is built with a static C++ runtime and no libraries beyond libc/libm/libpthread. A Fedora 44 build needed `-fno-math-errno` to avoid a `GLIBC_2.43` symbol, so the build environment's glibc matters.
- The presenter needs only Vulkan, Wayland and xkbcommon, all loaded at runtime, and runs on the Deck host from a container build.
- The driver and presenter talk over an abstract unix socket (`@xreal-presenter-<uid>`) because vrserver cannot see `/run/user`.
- NetworkManager brings up the glasses' USB network on its own on Bazzite.
- The glasses' display mode (full SBS) can be read from the DRM mode list; Follow mode, Stabilizer and auto-sleep cannot be read, and nothing is ever written to the glasses.
- `tools/doctor.sh` and `tools/vr_session.sh` already hold most of the setup and run logic.

Assumed, with the experiment that settles each:

| Assumption | Experiment |
|---|---|
| A driver built on an older-glibc base (the Steam Runtime SDK, or an older distro container) still loads in the container and is faster to distribute than a Fedora build | Build the driver there, check symbol versions, run SteamVR on the Deck |
| SteamVR (native Steam) loads a driver registered from `$XDG_DATA_HOME` instead of the repo | Install to the data directory, register it, start SteamVR, check the activation log line |
| The presenter can run inside a Flatpak (Vulkan, a fullscreen Wayland window on a chosen output, the abstract socket, link-local TCP) | Build a Flatpak and run it on the Deck against a running SteamVR |
| Flatpak Steam can load a driver and talk to the presenter | Test with the Flatpak Steam on a second machine or the Deck |
| Registering the driver and editing the SteamVR settings can be made safe (backup, SteamVR stopped, undoable) | Install, change, uninstall on a clean user; diff the settings before and after |
| Other distros' glibc and Vulkan stacks run the same presenter build | Run the release on one more distro in a container with GPU access |

## What Changes

- A reproducible release build: driver and presenter built on a fixed older-glibc base, with a check that the driver's symbol versions stay within the runtime's.
- A release archive and an install script that needs no root: installs the presenter, the driver (to `$XDG_DATA_HOME`), the commands and a version file; `--uninstall` reverses it.
- A single setup command, `xreal-setup`, that does what `doctor.sh` and `vr_session.sh` do today and can also apply the safe fixes with consent: register the driver, set the SteamVR settings with a backup, and say exactly which glasses menu settings the user has to set by hand.
- A version check between driver and presenter, so a mismatched pair is reported instead of misbehaving.
- A Flatpak of the app side as a second stage, after the archive works.
- A quick-start README for users, separate from the engineering notes.

## Capabilities

### New Capabilities
- `installable-package`: the release artifacts, installing, updating and uninstalling them without root, and version agreement between the driver and the presenter.
- `guided-setup`: checking the machine, applying the safe fixes with consent and a backup, and telling the user what only they can set.

### Modified Capabilities
- `steamvr-driver`: "Loadable inside SteamVR" gains the build-baseline requirement and location independence.
- `glasses-setup`: "Session control" and "Read-only diagnostics" become parts of the shipped command instead of repo scripts.

## Impact

- New: release build files, install/uninstall scripts, `xreal-setup`, packaging metadata (Flatpak manifest), a user quick start.
- Changed: `tools/doctor.sh` and `tools/vr_session.sh` become thin wrappers or are replaced; the hard-coded `~/xreal-linux` and `/tmp/presenter.log` paths go; the driver and presenter share a protocol version.
- Dependencies: a container image for the build baseline; CI to build releases on tags.
- Touches the pressure-vessel boundary (where the driver lives and how it is registered) and the user's SteamVR settings file, so both need a backup and an undo.

## Non-goals

- A graphical interface. `xreal-setup` is a command; a GUI (Qt or other) is a later change on top of it.
- Support for non-AMD GPUs, covered by its own change.
- Camera tracking (its own change) or any new feature in the driver or presenter.
- Writing to the glasses, or setting their menu options.
- A distro package (rpm, deb, AUR). The archive and the Flatpak come first; distro packages can wrap them.
- Auto-start at login.

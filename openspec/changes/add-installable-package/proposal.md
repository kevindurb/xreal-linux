# Proposal

## Why

Today the project only works for someone who can build it: a Rust presenter and a C++ driver built in podman containers, an `rsync` to the Deck, `vrpathreg.sh adddriver` by hand, hand-edited `steamvr.vrsettings`, and shell scripts that assume the repo lives at `~/xreal-linux`. The goal is an easy download that works on a Steam Deck and on any Linux gaming PC, so other XREAL 1S owners can use it: download one file, run it, say yes to a few questions, then start SteamVR. The pacing work is done and verified, and the presenter already sets full SBS itself, so the setup is stable enough to package.

Many owners also use the glasses outside SteamVR, as a plain virtual screen in a 2D mode, so the package must stay out of the way until SteamVR actually uses them.

## What We Know and What We Assume

Verified here (`docs/findings.md`, `docs/handoff.md`, `driver/README.md`, the specs):
- The driver loads inside SteamVR's container when it is built with a static C++ runtime and no libraries beyond libc/libm/libpthread. A Fedora 44 build needed `-fno-math-errno` to avoid a `GLIBC_2.43` symbol, so the build environment's glibc matters.
- The presenter needs only Vulkan, Wayland and xkbcommon, all loaded at runtime, and runs on the Deck host from a container build.
- The driver and presenter talk over an abstract unix socket (`@xreal-presenter-<uid>`) because vrserver cannot see `/run/user`. The driver already connects without blocking, retries every 500 ms and re-sends everything it created on each new connection. The presenter's socket listener is already separate from its window.
- The glasses answer host requests on the control port (52999). The presenter already reads the config and the input mode, and sets full SBS with one request (`presenter/src/glasses.rs`, limited to once per connection and three per run). `tools/vr_session.sh start` already confirms the single 3840x1080 mode before starting.
- A mode switch re-plugs the glasses' display for about 1.8 s; after a replug or sleep the glasses come back in 2D, and why they sometimes drop to 2D mid-session is not yet understood.
- NetworkManager brings up the glasses' USB network on its own on Bazzite.
- Follow mode, Stabilizer and auto-sleep cannot be read or set by the host.
- `tools/doctor.sh` and `tools/vr_session.sh` already hold most of the setup and run logic.

Assumed, with the experiment that settles each:

| Assumption | Experiment |
|---|---|
| A driver built on an older-glibc base (the Steam Runtime SDK, or an older distro container) still loads in the container | Build it there, check symbol versions, run SteamVR on the Deck (task 1.1) |
| SteamVR (native Steam) loads a driver registered from `$XDG_DATA_HOME` instead of the repo | Install to the data directory, register it, start SteamVR, check the activation log line (task 5.3) |
| The presenter works as an AppImage (host Vulkan and Wayland, FUSE or the extract fallback) | Build it and run it on the Deck and one other distro (task 5.1) |
| A socket-activated user service starts fast enough for the driver's retry loop to absorb | Measure the cold start including an AppImage mount and a 2D to SBS switch (task 3.5) |
| The user unit sees `WAYLAND_DISPLAY` and gets a window on the glasses' output | Check on a Plasma session on the Deck (tasks 3.3, 3.4, 5.5) |
| Setting 2D again (value 0) restores the user's previous mode, including its aspect and refresh rate | Compare the DRM mode list before and after (tasks 4.3, 7.1) |
| Registering the driver and editing the SteamVR settings can be made safe (backup, SteamVR stopped, undoable) | Install, change, uninstall on a clean user; diff the settings before and after (tasks 5.7, 5.10) |
| User units are active and usable in the Deck's game mode | Record-only experiment (task 7.2) |
| Flatpak Steam can read the driver from the data directory | Record-only experiment (task 7.3) |

## What Changes

- A reproducible release build on a fixed older-glibc base, with a check that the driver's symbol versions stay within the runtime's, and CI that builds the release on a tag.
- **One AppImage** attached to the release. Run it and it checks the machine, asks (through `kdialog`, `zenity` or the terminal) before installing, copies itself and the driver to stable paths under `$XDG_DATA_HOME`, registers the driver, fixes the SteamVR settings with a backup and undo, and enables two systemd user units.
- **A socket-activated service.** The socket unit owns the driver link's socket; the presenter runs only while SteamVR's driver is connected and exits afterwards. Nothing runs, and the glasses are not touched, while SteamVR is not running, so the glasses stay usable as a virtual screen.
- **A handshake** between driver and presenter carrying a protocol version and whether the glasses are present; the driver reports no headset until the glasses are confirmed.
- **Display mode in and out.** The presenter switches the glasses to full SBS when SteamVR starts using them (building on the existing setter) and restores their previous 2D mode when it stops, including after a crash.
- **The presenter follows the glasses' output**, identified by EDID, and never fullscreens on another output.
- A quick-start README for users, separate from the engineering notes.

## Capabilities

### New Capabilities
- `installable-package`: the single AppImage release, self-install and update without root, uninstall, and version agreement between driver and presenter.
- `guided-setup`: dialogs through kdialog/zenity/terminal, checking the machine, applying the safe fixes with consent and a backup, telling the user what will happen to the glasses and what only they can set.
- `session-service`: the socket-activated presenter service, its lifecycle, the glasses-present handshake and collisions with manual runs.

### Modified Capabilities
- `steamvr-driver`: the headset is reported only once the presenter confirms the glasses; "Loadable inside SteamVR" gains the build-baseline requirement and location independence.
- `glasses-setup`: full SBS is set and restored by the presenter around a SteamVR session; session control and diagnostics become parts of the shipped package.
- `presenter-display`: the window follows the glasses' output (found by EDID) and exists only during a session; the driver link can be a socket handed over by systemd.

## Impact

- New: release build files, the AppImage build, the setup logic, the systemd unit files, a user quick start, a CI workflow.
- Changed: `presenter/src/glasses.rs` (restore and allowlist), `presenter/src/main.rs` (socket handoff, lazy window, EDID output detection), `driver/src/xreal_driver.cpp` (handshake, no HMD until confirmed), `tools/doctor.sh` and `tools/vr_session.sh` (thin wrappers, stop the units first); the hard-coded `~/xreal-linux` and `/tmp/presenter.log` paths go.
- Dependencies: a container image for the build baseline; CI to build releases on tags.
- Touches the pressure-vessel boundary (where the driver lives and how it is registered), the user's SteamVR settings file, the user's systemd units, and the glasses' display mode, so each needs consent, a record and an undo.

## Non-goals

- A graphical interface. The commands use `kdialog`/`zenity`/the terminal; a GUI (Qt or other) is a later change on top.
- Support for non-AMD GPUs, covered by its own change.
- Camera tracking (its own change, parked).
- Any other write to the glasses: camera or sensor starts and the Follow mode, Stabilizer and auto-sleep settings stay with the user. The only writes are the display input mode 0/1 around a SteamVR session.
- Presenting through `VK_KHR_display` or another compositor bypass. It needs DRM master on a connector the compositor will not lease (`non-desktop = 0`) and would not help under gamescope; not investigated here.
- A Flatpak or a distro package (rpm, deb, AUR); either can wrap the AppImage later.
- Promising game-mode support; it is only recorded by an experiment.
- Explaining why the glasses drop to 2D mid-session (`tools/analyze_control_events.py` is for that).

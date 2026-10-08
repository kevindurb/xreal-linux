# Design

## Context

Everything runs from a checkout at a fixed path: `tools/vr_session.sh` and `tools/doctor.sh` hard-code `~/xreal-linux` and `/tmp/presenter.log`, the driver is registered by hand with `vrpathreg.sh adddriver <repo>/driver/xreal`, and builds happen in podman containers on the Deck (fedora:44 for both pieces). See proposal.md for motivation and the verified/assumed split.

Facts that constrain the packaging:
- vrserver and vrcompositor run inside Steam's pressure-vessel container. The driver is loaded into vrserver, so it must be built against a glibc no newer than that container's, and be readable from inside it. The presenter is a normal host process.
- The driver and presenter talk over an abstract unix socket, which is per network namespace: the presenter must run in the host network namespace. An AppImage and a systemd user unit both do.
- The driver already connects without blocking, retries every 500 ms and re-sends everything it created on each new connection, so a presenter that appears after the driver is a case it handles. The presenter's socket listener (`link_thread`) is already independent of its window, which is created later.
- The presenter must open the glasses' output fullscreen, use the same GPU as SteamVR, and reach `169.254.x.1` over the glasses' USB network.
- The Deck's host is immutable (Bazzite): installs have to live under the home directory.
- The glasses are also used outside SteamVR, as a plain monitor in any 2D mode. With SteamVR not running, nothing we ship may touch the display or the glasses.
- The glasses can be switched from 2D to full SBS from the host with one request on the control port, and the presenter already does so (`glasses.rs`); they come back in 2D after a replug (`docs/handoff.md`). A switch re-plugs the display for about 1.8 s.

## Goals / Non-Goals

**Goals:**
- A user downloads one file, runs it, and gets a working SteamVR headset; no checkout, compiler or root.
- Nothing runs and nothing is grabbed until SteamVR's driver actually connects.
- The driver and presenter in a release are reproducible from a commit.
- Setup never changes the user's SteamVR configuration without showing it and keeping an undo.
- The glasses go to full SBS when SteamVR starts using them and back to the user's previous mode when it stops, with no menu step.

**Non-Goals:**
- Any other write to the glasses: camera or sensor starts, and the Follow mode, Stabilizer and auto-sleep settings stay with the user.
- A GUI, other GPU vendors, camera tracking, or any behaviour change in the driver and presenter beyond the handshake (decision 6) and the display-mode control (decision 7).
- Presenting through `VK_KHR_display` or any other compositor bypass. It needs DRM master on a connector the compositor will not lease (`non-desktop = 0`, see `docs/findings.md`), and would not help under gamescope. Not investigated here.
- A Flatpak or distro packages (rpm, deb, AUR). They can wrap the AppImage later.

## Decisions

### 1. One AppImage, built by CI, attached to the release

The release is a single AppImage containing the presenter, the driver files and the setup logic. It is built in GitHub Actions in the pinned container (decision 2) and uploaded to the release. The Vulkan, Wayland and xkbcommon stack is taken from the host (the presenter already loads them at runtime), not bundled.

The AppImage needs FUSE2 to mount. If it is missing, `--appimage-extract-and-run` is the documented fallback and `setup` detects the case and says so.

Alternatives considered: a tarball with `install.sh` (an extra manual step for no benefit now that the app can install itself); Flatpak (sandbox questions about GPU, output choice and Steam-as-Flatpak, each an experiment, and no gain); distro packages (can wrap the AppImage later).

### 2. Build on an old-glibc baseline in a fixed container

Driver and presenter are built in a container image pinned in the repo (the Steam Runtime SDK or an older distro image, chosen by the first task by checking which loads in SteamVR and which runs the presenter on the Deck and one other distro). A build step lists the driver's glibc symbol versions and fails if one exceeds the baseline. This replaces `-fno-math-errno` as the only guard against `GLIBC_2.43`-style breaks (the flag stays).

### 3. Self-install to stable paths under the data directory

An AppImage mounts at an ephemeral `/tmp/.mount_*` path, and Steam and systemd both need paths that persist. `setup` therefore copies:
- the AppImage to `$XDG_DATA_HOME/xreal-linux/xreal-linux.AppImage` (the user can delete the download afterwards);
- the driver to `$XDG_DATA_HOME/xreal-linux/driver/xreal`, registered by that path with SteamVR's own `vrpathreg`.

Running a newer AppImage compares versions and offers to update both copies in place; the paths never change, so there is no re-registration. If Steam is the Flatpak, its sandbox must be able to read the driver path; this is an early experiment, and `check` reports it plainly if it cannot be done.

### 4. Interactive `setup`, headless service

All consent happens in `setup` and the other commands (`check`, `fix`, `status`, `uninstall`). It asks through `kdialog`, then `zenity`, then the terminal, whichever is present and usable, so a double-clicked AppImage with no terminal still works. The background service never prompts: in game mode nobody could see a dialog. After setup it only acts within what was approved.

Why this split and not a GUI: the checks need structured output and tests, and a GUI is a later change on top of the same logic.

### 5. Socket activation instead of a resident daemon

`setup` installs two systemd user units in `~/.config/systemd/user/`:
- `xreal-linux.socket` listens on the abstract socket `@xreal-presenter-%U` as `ListenSequentialPacket`, always bound, with no process behind it;
- `xreal-linux.service` runs the presenter from the stable AppImage path and is started when the driver connects.

```
 driver connects --> backlog of xreal-linux.socket --> systemd starts the service
                                                          presenter takes the fd (LISTEN_FDS),
                                                          accepts, opens the window,
                                                          exits shortly after the driver disconnects
```

The presenter reads `LISTEN_PID` and `LISTEN_FDS` itself (no libsystemd) and keeps its own `bind` path for runs from a checkout. The `SO_PEERCRED` check stays. Because the unit is a user unit it exists in desktop and game mode alike (to be confirmed, see Open Questions).

Consequences:
- Nothing resident, no autostart entry, no process scan for vrserver and no watching for the glasses: the driver connecting is the trigger. When SteamVR is not running the glasses are untouched, so virtual-screen use is unaffected.
- A manual presenter (`tools/vr_session.sh`, the checkout workflow) fails with `EADDRINUSE` while the socket unit is enabled. The scripts stop the units first or the presenter takes a `--socket-name`.
- The service has a start limit and an idle exit with a short grace, so a crash loop or a SteamVR restart does not spin it.
- The service needs `WAYLAND_DISPLAY` in the user manager's environment. Plasma normally imports it at login; `check` verifies it instead of assuming.
- The first connect pays Vulkan init plus an AppImage mount. The driver's 500 ms retry absorbs this, but the cold-start time is measured, not assumed.

### 6. Handshake: protocol version and `glasses_present`

The driver's first message carries a protocol version; the presenter's reply carries the same plus `glasses_present`. A version mismatch is reported by the presenter (log and journal) and by `check`, and the presenter refuses to present. `glasses_present` is true when the glasses answer on their control port (decision 7 switches them to full SBS if needed); it is false, with a reason, when they are absent or silent. The reply is sent once the output is in SBS mode, or with false after a timeout. Until the driver gets a positive reply it reports no HMD, so a user with the driver registered and no glasses plugged in does not see a phantom XREAL headset (nor one with another headset). This is the one behaviour change in the driver and presenter.

### 7. Display mode: build on the existing SBS setter, add the way back

The presenter already has a control-port client (`presenter/src/glasses.rs`, `openspec/specs/glasses-setup`): on connect it sends the read-only getters 10015 (config) and 10273 (input mode), sets full SBS with the setter 10274 (value 1) only when the getter reports the regular mode, at most once per connection and three times per run, never retries a rejected or unanswered setter, and honours `--no-set-sbs`. `tools/vr_session.sh start` runs it as a one-shot (`--set-sbs-only`) and waits for the single 3840x1080 mode. This change keeps all of that and adds three things:

1. **The trigger moves to the driver.** The service has no reason to touch the glasses until SteamVR's driver connects (decision 5), so the SBS set is sequenced after the driver connects and before the window opens, and no longer depends on `vr_session.sh`.
2. **The way back.** `10274` with value 0 (2D) restores the user's previous mode. The previous mode is recorded in the state directory before the first set ("was 2D" or "was SBS"); only "was 2D" is restored, and "was SBS" sends nothing.
3. **The restore survives a crash.** It lives in the unit's `ExecStopPost`, reading the recorded mode, as well as in the presenter's clean exit and in `uninstall`.

```
 driver connects
   getters 10015, 10273 (as today)
     2D  -> record "was 2D", set SBS (10274 = 1), output re-plugs (about 1.8 s),
            wait for the 3840x1080 mode, open the window
     SBS -> record "was SBS", send nothing
 session: the existing reconnect logic re-sets SBS if the glasses drop to 2D (sleep, replug),
          inside the existing limit of three setter sends per run
 driver disconnects (after the grace), or the service stops for any reason
   "was 2D"  -> set 2D (10274 = 0); "was SBS" -> leave it
```

Rules, carried over from the agreed handling of anything sent to the glasses (`docs/handoff.md` section 4):
- Allowlist in code: the read-only getters already used (10015, 10273) and 10274 with value 0 or 1. Anything else is refused. No camera or sensor requests, and the other menu settings (Follow mode, Stabilizer, auto-sleep) are never touched.
- One request at a time, reply checked before the next, the existing per-connection and per-run limits, and the restore counts against its own small bound, never an unbounded loop. If the control port is unreachable or silent (glasses asleep), nothing is retried blindly and the handshake reports why.
- The glasses' drops to 2D are not understood yet (`tools/analyze_control_events.py` exists to find out). Because the service runs once per SteamVR session, the per-run limit resets per session; a glasses unit that keeps reverting hits the limit and the presenter says so.
- These are the only writes to the glasses, and they only happen while SteamVR's driver is connected. With SteamVR not running the glasses are left in whatever mode the user chose.

Effects to expect: each switch re-plugs the glasses' display, so a Plasma desktop sees an output vanish and return and may rearrange windows. That is stated to the user in `setup` and the README, not hidden.

Why: it removes the manual menu step after every replug or sleep, and it makes "use the glasses as a virtual screen, then play a VR game, then go back" work without the user touching the glasses.

### 8. The presenter follows the glasses' output, and never uses another

Today the presenter picks a monitor by name (default `DP-1`) and, every 500 ms, moves its window back if the compositor put it elsewhere (`about_to_wait`). This becomes:
- Identify the glasses by EDID (manufacturer `MRG`, product `0x4102`, read from `/sys/class/drm/*/edid`), map that connector to the compositor's output name, and match winit's monitor list on it. `--monitor` stays as an override.
- The window exists only while the glasses' output exists in SBS mode. If the output disappears (a mode switch re-plugs it for about 1.8 s, sleep, unplug) the window is destroyed or hidden, never left fullscreen on another output; when it reappears the window is recreated there. winit 0.30 has no monitor hotplug event, so the existing poll stays as the detection mechanism.
- While the glasses are unreachable, `glasses_present` is false in the handshake.

Why: a stereo image fullscreened on the user's main monitor is the worst failure here, and the Deck's `DP-1` is a coincidence, not a rule.

### 9. Safe edits to the SteamVR settings

`steamvr.vrsettings` is rewritten by SteamVR on exit, so `fix` refuses while SteamVR runs. It writes a timestamped backup before the first change, records which keys it changed and their previous values in a small manifest under the state directory, and `uninstall` uses the manifest (not the whole backup) to put back only those keys.

### 10. State and logs

Presenter logs go to the journal (`journalctl --user -u xreal-linux`); the settings backup and change manifest live under `$XDG_STATE_HOME/xreal-linux/`. The `/tmp/presenter.log` path in the scripts moves with it.

### 11. CI builds the release

A workflow builds the AppImage from a tag in the pinned container, runs the symbol-version check and uploads the file to the release. No secrets are involved.

## Risks / Trade-offs

- [The old-glibc driver still does not load in a future Steam runtime] → The symbol check and the "activated" log line are tested on the Deck on every release; `check` reports a driver that did not activate.
- [The service cannot put a window on the glasses' output under some compositor or in game mode] → The first release targets a Plasma desktop session; game mode is recorded by an experiment, not promised.
- [The AppImage has no FUSE] → `setup` detects it and documents `--appimage-extract-and-run`.
- [Socket activation hides a startup failure from the driver] → The handshake reports the reason (version, no glasses); the service logs to the journal and `check` reads it.
- [A crash or kill leaves the glasses in SBS] → The restore is in the unit's `ExecStopPost`, driven by the recorded previous mode; `check` reports a recorded mode that was never restored.
- [The glasses drop to 2D mid-session and the re-set loops] → Bounded retries with backoff, then stop and report.
- [Each switch re-plugs the display and the desktop rearranges windows] → Documented in `setup` and the README; only happens when SteamVR starts and stops using the glasses.
- [Restoring value 0 may not bring back the user's exact 2D aspect and refresh mode] → Record the DRM mode before and compare after in the integration check; report any difference.
- [Editing the SteamVR settings corrupts them] → Refuse while SteamVR runs, back up first, write atomically, keep a manifest for undo.
- [`vrpathreg` is missing or changes] → `check` reports it; the fallback edits are not attempted silently.
- [Hard-coded Deck assumptions (`DP-1`, `card1`)] → Decision 7; the Deck is the first test, another distro the second.
- [AMD/Wayland/Bazzite specific] → Install paths and the setup logic are distro-neutral; the verified GPU is AMD only, which the README says.

## Migration Plan

Existing checkout users keep `tools/*.sh`; nothing is removed. The first release is a tag with the AppImage. Rollback is `uninstall` followed by the checkout workflow. Settings the user must keep are Follow mode, Stabilizer off, auto-sleep off, and the SteamVR settings `fix` manages; full SBS is now set by the presenter.

## Open Questions

- Whether user units are running in the Deck's game mode, and whether SteamVR is usable there at all (not answered; desktop mode is the supported target meanwhile).
- Whether setting value 0 returns the glasses to the exact 2D mode they were in (aspect ratio and refresh rate). Switching to full SBS is verified; the way back is checked in the integration task.
- Naming and version scheme for releases: does not affect the specs or the tasks beyond a string.

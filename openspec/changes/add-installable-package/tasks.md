# Tasks

## 1. Reproducible builds on an old-glibc baseline

- [ ] 1.1 Pick the build base: build the driver and the presenter on the Steam Runtime SDK and on an older distro image; run each on the Deck and one other distro. Verify: SteamVR activates the driver (log line) and the presenter shows its test pattern for the chosen base; the choice and the failures of the others are recorded in `docs/findings.md`.
- [ ] 1.2 Pin the build image in the repo and add one script that builds both pieces into `dist/`. Verify: a clean checkout builds on the Deck and on a PC with a single command, producing the same file list.
- [ ] 1.3 Symbol-version check: list the driver's glibc symbol versions and fail when one exceeds the baseline. Verify: it passes on the real build and fails on a build with a deliberately newer symbol (a test fixture).
- [ ] 1.4 Remove the hard-coded repo path and `/tmp/presenter.log` from the scripts, driver and presenter. Verify: `grep` finds neither in the shipped files; presenter output appears in the journal when run as a unit.

## 2. Handshake: protocol version and `glasses_present`

- [ ] 2.1 The driver's first message carries a protocol version; the presenter's reply carries it and `glasses_present` (glasses answer on the control port; false with a reason otherwise). On a mismatch the presenter logs both versions and refuses to present. Verify: unit tests for match and mismatch; on the Deck, an old driver with a new presenter produces the logged two-version message.
- [ ] 2.2 The driver reports no HMD until it has a positive reply, and reports one again when it gets it. Verify: on the Deck with the glasses unplugged SteamVR starts with no XREAL headset (driver log line); plugging them in and restarting SteamVR shows the headset.

## 3. Presenter lifecycle

- [ ] 3.1 Take the listening socket from systemd (`LISTEN_PID`/`LISTEN_FDS`) when present, otherwise bind as today; keep the `SO_PEERCRED` check; add a `--socket-name` option. Verify: unit test of the fd handoff; `systemd-socket-activate` with a driver connection starts the presenter and the driver logs "connected"; a second presenter on the same name fails with a clear message.
- [ ] 3.2 Create the window only when the driver has connected and the glasses' output is available, and exit a few seconds after the driver disconnects (a SteamVR restart inside the grace keeps the process). Verify: on the Deck no presenter process and no window exist until SteamVR starts, and none a few seconds after it exits (`pgrep`, journal).
- [ ] 3.3 Find the glasses' output by EDID (manufacturer `MRG`, product `0x4102`) mapped to the compositor's output name; `--monitor` overrides. Verify: unit tests on sample EDIDs; on the Deck the presenter picks the glasses' output without `--monitor`, and with a second monitor attached it still does.
- [ ] 3.4 Follow the output as it comes and goes: destroy or hide the window when the glasses' output is missing or not SBS, recreate it on the glasses' output when it returns, never fullscreen on another output. Verify: on the Deck unplug and replug the glasses during a session; the window never appears on the internal screen and presentation resumes on return (log lines and a frame dump).
- [ ] 3.5 Measure cold-start time from the driver's first connect to the first presented frame, through the socket unit and the AppImage, including a 2D to SBS switch. Verify: the numbers are recorded in `docs/findings.md` and SteamVR starts without the driver giving up.

## 4. Display mode control (builds on `presenter/src/glasses.rs`)

- [ ] 4.1 Extend the request allowlist in `glasses.rs` to the setter value 0 as well as 1 (id 10274 only), keeping the getters 10015 and 10273 and refusing every other id or value, one request at a time. Verify: unit tests with recorded replies, including refusal of other ids and values; on the Deck the getter matches the DRM mode list.
- [ ] 4.2 Sequence the SBS set after the driver connects and before the window opens (replacing the `vr_session.sh` one-shot for the service), wait for the single 3840x1080 mode, and record the previous mode ("was 2D" or "was SBS") in the state directory before the first set. Verify: on the Deck from 2D the glasses reach SBS and the window appears on them (journal shows each request, reply and the time taken); from SBS no setter is sent; the existing `--no-set-sbs` still disables it.
- [ ] 4.3 Restore on driver disconnect and whenever the service stops, including a crash: the unit's `ExecStopPost` reads the recorded mode and sets 2D only for "was 2D", with its own small bound. Verify: after quitting SteamVR the glasses return to 2D and the DRM mode list matches the one recorded before; with `kill -9` on the presenter the unit still restores; from "was SBS" nothing is sent.
- [ ] 4.4 Check the existing mid-session behaviour inside the service: the glasses drop to 2D (sleep, replug), the reconnect logic re-sets SBS within the three-per-run limit, and the window follows (task 3.4). Verify: on the Deck sleep and wake the glasses during a session; SBS returns and presenting resumes; with the control port blocked the retries stop at the limit and the journal says why.
- [ ] 4.5 Unreachable or silent glasses: `glasses_present` is false with the reason, and `check` reports a recorded mode that was never restored. Verify: with the control port blocked the handshake reports it; after a simulated crash without restore `check` flags the recorded mode.

## 5. AppImage and `setup`

- [ ] 5.1 Assemble the AppImage (presenter, driver files, setup logic, version file), host GPU libraries not bundled, with the `--appimage-extract-and-run` fallback documented. Verify: it runs on the Deck and one other distro; its file list contains no captures or personal data (a script checks for `captures/` and `*.rgba`).
- [ ] 5.2 Dialog helper: `kdialog`, then `zenity`, then the terminal. Verify: unit tests on the selection logic; launched from a file manager with no terminal on the Deck, `setup` shows a dialog.
- [ ] 5.3 `setup` self-install: copy the AppImage and the driver to `$XDG_DATA_HOME/xreal-linux/`, register the driver through `vrpathreg`, never overwrite files that are not its own, offer an update when the AppImage is newer, support a dry run. It tells the user plainly that the glasses will switch to full SBS and back when SteamVR starts and stops and that the desktop may rearrange windows. Verify: runs in a clean container as a normal user and again (idempotent); refuses as root; the dry run changes nothing (a diff of the home directory before and after); the notice is in the output.
- [ ] 5.4 `setup` installs and enables the socket and service units (with `ExecStopPost`) after consent. Verify: `systemctl --user status xreal-linux.socket` is active after `setup`; the service is not running until SteamVR starts.
- [ ] 5.5 `check`: port the checks from `doctor.sh` (glasses on USB, IMU, display mode reported but 2D is not a failure, SteamVR, driver registration, settings, units, `WAYLAND_DISPLAY` in the user manager, versions) with pass/warn/fail and fixes. Verify: unit tests on synthetic states; on the Deck it matches `doctor.sh` output line for line apart from the display-mode line, and exits non-zero on a failure.
- [ ] 5.6 `check` ends with the glasses' own settings to confirm by hand: Follow mode, Stabilizer off and auto-sleep off, with where to find each (full SBS is no longer one). Verify: the output contains those three and not the display mode.
- [ ] 5.7 `fix`: set the SteamVR settings with a backup, consent prompts, refuse while SteamVR runs, change manifest for undo. Verify: tests on a temporary settings file (declined change writes nothing; manifest restores only the changed keys); on a clean user account on the Deck a `fix` then `check` passes.
- [ ] 5.8 `status`, with reprojection on by default and the existing environment options passed to the service. Verify: on the Deck `status` reports the units, driver, glasses, recorded mode and last session; the service runs with reprojection on.
- [ ] 5.9 Make `tools/doctor.sh` and `tools/vr_session.sh` thin wrappers around the same logic where practical, stopping the units first when they are enabled. Verify: both still pass their existing checks from a checkout, with and without the units enabled.
- [ ] 5.10 `uninstall`: disable and remove the units, restore a recorded mode if one is pending, deregister the driver, restore the changed settings keys, remove the installed files. Verify: on a clean Deck account, setup, fix, uninstall returns the home directory, the settings file and the driver registry to their earlier contents, apart from the state directory.
- [ ] 5.11 Write the user quick start (`README.md` top section, replacing the "exploration" status) with the Deck and PC paths. Verify: following only the quick start on a clean Deck user account reaches a passing `check` (user check).

## 6. CI release

- [ ] 6.1 A workflow that builds the AppImage from a tag in the pinned image, runs the symbol check and attaches the file to the release. Verify: a test tag on a fork or branch produces the AppImage and a failing symbol check fails the workflow.

## 7. Integration

- [ ] 7.1 End-to-end on a clean Deck user account (desktop mode) and one PC distro: download the AppImage, `setup`, `fix`, start SteamVR with the glasses in 2D, use the dashboard for a minute, quit SteamVR, `uninstall`. Verify: a user check against the scenarios in the specs, with SteamVR delivering 60 new frames/s, the glasses switching to SBS on start and back to their earlier 2D mode on quit (the DRM mode before and after compared), usable as a plain monitor before and after, and the uninstall leaving nothing behind.
- [ ] 7.2 Experiment (record only): are the user units active in the Deck's game mode, and can the service get a window on the glasses' output there. Verify: the result, positive or negative, is written to `docs/findings.md` and `docs/open-questions.md`; no promise is made in the README either way.
- [ ] 7.3 Experiment (record only): Flatpak Steam loading the driver from the data directory. Verify: SteamVR activates the driver (log line), or the limitation is recorded and `check` reports it.

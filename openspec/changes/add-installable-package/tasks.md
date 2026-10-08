# Tasks

## 1. Reproducible builds on an old-glibc baseline

- [ ] 1.1 Pick the build base: build the driver and the presenter on the Steam Runtime SDK and on an older distro image; run each on the Deck and one other distro. Verify: SteamVR activates the driver (log line) and the presenter shows its test pattern for the chosen base; the choice and the failures of the others are recorded in `docs/findings.md`.
- [ ] 1.2 Pin the build image in the repo and add one script that builds both pieces into `dist/`. Verify: a clean checkout builds on the Deck and on a PC with a single command, producing the same file list.
- [ ] 1.3 Symbol-version check: list the driver's glibc symbol versions and fail when one exceeds the baseline. Verify: it passes on the real build and fails on a build with a deliberately newer symbol (a test fixture).
- [ ] 1.4 Remove the hard-coded paths from the driver and presenter (the repo path, `/tmp/presenter.log`, `card1`/`DP-1` assumptions) in favour of state-directory logs and DRM-based output detection. Verify: the presenter finds the glasses' output on the Deck without `--monitor`; logs appear under `$XDG_STATE_HOME/xreal-linux/`.

## 2. Protocol version

- [ ] 2.1 Add a protocol version to the first message each side sends; the presenter reports a mismatch, refuses to present and shows the test pattern. Verify: unit tests for match and mismatch; on the Deck, an old driver with a new presenter produces the logged two-version message and a test pattern.

## 3. Release archive and install script

- [ ] 3.1 Assemble the archive (presenter, driver, `xreal-setup`, version file, `install.sh`). Verify: its file list matches the spec and it contains no captures or personal data (a script checks for `captures/` and `*.rgba`).
- [ ] 3.2 `install.sh`: no root, dry run, idempotent, never overwrites files that are not its own, driver to `$XDG_DATA_HOME/xreal-linux/driver/xreal`. Verify: runs in a clean container as a normal user and again (idempotent); refuses as root; dry run changes nothing (a diff of the home directory before and after).
- [ ] 3.3 Uninstall: remove only installed files. Verify: install then uninstall leaves the home directory as it was, apart from the state directory's logs.
- [ ] 3.4 Document install and uninstall in a user quick start (`README.md` top section) with the Deck and PC paths. Verify: following only the quick start on a clean Deck user account reaches a passing `check` (user check).

## 4. `xreal-setup`

- [ ] 4.1 `check`: port the checks from `doctor.sh` (glasses on USB, IMU, display mode, SteamVR, driver registration, settings, presenter, versions) with pass/warn/fail and fixes. Verify: unit tests on synthetic states; on the Deck it matches `doctor.sh` output line for line, and exits non-zero on a failure.
- [ ] 4.2 `check` ends with the glasses' own settings to confirm by hand. Verify: the output contains Follow mode, Stabilizer off and auto-sleep off with where to find each.
- [ ] 4.3 `fix`: register the driver through `vrpathreg`, set the SteamVR settings with a backup, consent prompts, refuse while SteamVR runs, change manifest for undo. Verify: tests on a temporary settings file (declined change writes nothing; manifest restores only the changed keys); on a clean user account on the Deck a `fix` then `check` passes.
- [ ] 4.4 `start`, `stop`, `status`, with reprojection on by default and the existing environment options. Verify: on the Deck a start brings up the presenter and SteamVR and a stop ends both, with the same behaviour as `tools/vr_session.sh`.
- [ ] 4.5 Make `tools/doctor.sh` and `tools/vr_session.sh` thin wrappers around the same logic where practical, keeping checkout behaviour. Verify: both still pass their existing checks from a checkout.
- [ ] 4.6 Uninstall deregisters the driver and restores the changed settings keys. Verify: on a clean Deck account, install, fix, uninstall returns the settings file and the driver registry to their earlier contents.

## 5. CI release

- [ ] 5.1 A workflow that builds the archive from a tag in the pinned image, runs the symbol check and uploads the archive. Verify: a test tag on a fork or branch produces the archive and a failing symbol check fails the workflow.

## 6. Flatpak (stage two, gated on experiments)

- [ ] 6.1 Experiment: run the presenter in a Flatpak on the Deck against a running SteamVR (GPU, a fullscreen window on the glasses' output, the abstract socket, link-local TCP). Verify: the test pattern shows on the glasses and the driver connects; failures and the needed permissions recorded in `docs/findings.md`.
- [ ] 6.2 Experiment: Flatpak Steam loading the installed driver. Verify: SteamVR activates the driver from the data directory (log line), or the limitation is recorded and `check` reports it.
- [ ] 6.3 If 6.1 passes, write the Flatpak manifest for the app side, with `xreal-setup` installing the driver to the data directory. Verify: install the Flatpak on a clean Deck account and reach a passing `check`; if 6.1 fails, record why and stop with the archive as the release.

## 7. Integration

- [ ] 7.1 End-to-end on a clean Deck user account and one PC distro: install from the archive, `fix`, `start`, use the dashboard for a minute, `stop`, uninstall. Verify: a user check against the scenarios in the specs, with SteamVR delivering 60 new frames/s and the uninstall leaving nothing behind.

# installable-package Specification

## Purpose

Ship the project as one downloadable AppImage that installs for the current user without root, can be updated and undone, and keeps the driver and presenter versions in agreement.

## Requirements

### Requirement: One downloadable file

A release SHALL be a single AppImage for x86_64 Linux, attached to the project's release page, containing the presenter, the SteamVR driver, the setup logic and a version file naming the commit. The driver and presenter in it SHALL be built from the same commit on a fixed, documented build baseline, and the file SHALL NOT contain any capture or personal data. It SHALL use the host's Vulkan, Wayland and xkbcommon libraries instead of bundling them.

#### Scenario: Downloading and running a release

- **WHEN** a user downloads the AppImage, makes it executable and runs it
- **THEN** it starts the interactive setup, with no source checkout, compiler or root needed

#### Scenario: A host without FUSE

- **WHEN** the AppImage cannot mount because FUSE is missing
- **THEN** the documentation and the setup output name the `--appimage-extract-and-run` fallback

### Requirement: Self-install without root

Setup SHALL install for the current user only, SHALL refuse to run as root, SHALL be safe to run again, SHALL support a dry run that shows what it would change, and SHALL NOT modify system directories or require `sudo`. It SHALL copy the AppImage and the driver to stable paths under `$XDG_DATA_HOME/xreal-linux/` (the paths Steam and the service units refer to), and SHALL offer to update both in place when it is run from a newer AppImage.

#### Scenario: Fresh install

- **WHEN** setup runs for a user with nothing installed and the user agrees
- **THEN** the AppImage and the driver are copied under the user's data directory, the driver is registered with SteamVR, setup prints what it installed and where, and no system path is touched

#### Scenario: Dry run

- **WHEN** setup runs with the dry-run option
- **THEN** it lists what it would place and register and changes nothing

#### Scenario: Run as root

- **WHEN** setup is run as root
- **THEN** it refuses and says to run it as the user who runs SteamVR

#### Scenario: Newer AppImage

- **WHEN** the user runs an AppImage newer than the installed copy
- **THEN** setup offers to replace the installed AppImage and driver in place, at the same paths, and does so only if the user agrees

### Requirement: Uninstall and undo

The package SHALL provide an uninstall that stops and removes the systemd units it installed, restores the glasses' previous display mode if a restore is still pending, deregisters the driver from SteamVR, restores the SteamVR settings it changed from its change record, and removes only the files it installed, leaving anything it did not create untouched.

#### Scenario: Uninstall after setup

- **WHEN** a user who ran setup then uninstalls
- **THEN** the units are gone, the driver is no longer registered, the settings it changed are back to their earlier values, and the installed files are gone

#### Scenario: A file the package did not create

- **WHEN** a file at one of its install paths is not the package's own
- **THEN** install and uninstall leave it alone and say so

### Requirement: Driver and presenter versions agree

The driver and the presenter SHALL exchange a protocol version when they connect. When the versions differ, the presenter SHALL report both and SHALL refuse to present rather than misinterpret messages, and the setup check SHALL report the mismatch.

#### Scenario: Matching versions

- **WHEN** a driver and presenter from the same release connect
- **THEN** they proceed normally

#### Scenario: Stale driver after an update

- **WHEN** the presenter is updated but SteamVR still has the old driver loaded
- **THEN** the presenter logs the two versions, shows its test pattern, and tells the user to restart SteamVR

### Requirement: Runs on a supported base without the repository

An installed package SHALL run without the source repository present and without a fixed home-directory path, on the Steam Deck (Bazzite) and on a mainstream Linux gaming PC distribution, with logs in the user's journal and its records (settings backup, change record, recorded display mode) under the user's state directory instead of `/tmp`.

#### Scenario: No repo on the machine

- **WHEN** the package is installed on a machine with no checkout
- **THEN** a SteamVR session works and the presenter's log is readable with `journalctl --user -u xreal-linux`

## ADDED Requirements

### Requirement: Release archive

A release SHALL consist of one archive per supported architecture (x86_64 Linux) containing the presenter, the SteamVR driver, the setup command, a version file and an install script. The driver and presenter in it SHALL be built from the same commit on a fixed, documented build baseline, and the archive SHALL NOT contain any capture or personal data.

#### Scenario: Unpacking a release

- **WHEN** the archive is extracted
- **THEN** it holds the presenter, the driver, `xreal-setup`, `install.sh` and a version file naming the commit

### Requirement: Install without root

`install.sh` SHALL install for the current user only, SHALL refuse to run as root, SHALL be safe to run again, SHALL support a dry run that shows what it would change, and SHALL NOT modify system directories or require `sudo`. It SHALL install the driver to a stable path under `$XDG_DATA_HOME` so updating the app does not move it.

#### Scenario: Fresh install

- **WHEN** `install.sh` runs for a user with nothing installed
- **THEN** the files are placed under the user's home, the command prints what it installed and where, and no system path is touched

#### Scenario: Dry run

- **WHEN** `install.sh --dry-run` runs
- **THEN** it lists the files it would place and changes nothing

#### Scenario: Run as root

- **WHEN** `install.sh` is run as root
- **THEN** it refuses and says to run it as the user who runs SteamVR

### Requirement: Uninstall and undo

The package SHALL provide an uninstall that removes only the files it installed, deregisters the driver from SteamVR and restores the SteamVR settings it changed from the backup it made, leaving anything it did not create untouched.

#### Scenario: Uninstall after setup

- **WHEN** a user who ran setup then uninstalls
- **THEN** the driver is no longer registered, the settings it changed are back to their earlier values, and the installed files are gone

#### Scenario: A file the package did not create

- **WHEN** a file at one of its install paths is not the package's own
- **THEN** install and uninstall leave it alone and say so

### Requirement: Driver and presenter versions agree

The driver and the presenter SHALL exchange a protocol version when they connect. When the versions differ, the presenter SHALL report both and SHALL refuse to present rather than misinterpret messages, and `xreal-setup` SHALL report the mismatch.

#### Scenario: Matching versions

- **WHEN** a driver and presenter from the same release connect
- **THEN** they proceed normally

#### Scenario: Stale driver after an update

- **WHEN** the presenter is updated but SteamVR still has the old driver loaded
- **THEN** the presenter logs the two versions, shows its test pattern, and tells the user to restart SteamVR

### Requirement: Runs on a supported base without the repository

An installed package SHALL run without the source repository present and without a fixed home-directory path, on the Steam Deck (Bazzite) and on a mainstream Linux gaming PC distribution, with logs written under the user's state directory instead of `/tmp`.

#### Scenario: No repo on the machine

- **WHEN** the package is installed on a machine with no checkout
- **THEN** starting a session works and logs appear under the user's state directory

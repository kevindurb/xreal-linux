# Spec Delta

## Purpose

Let the game-mode panel detect, install and update the xreal-linux runtime from a released artifact, verifying it before it is used, so a Deck owner never has to leave game mode or open a terminal to get the runtime in place.

## ADDED Requirements

### Requirement: Detect the installed runtime and its version

The panel SHALL report whether the runtime is installed, and when it is, which version is installed.

#### Scenario: Installed

- **WHEN** the runtime is installed
- **THEN** the panel reports the installed version

#### Scenario: Not installed

- **WHEN** no runtime is installed
- **THEN** the panel reports that it is not installed

### Requirement: Install and update from a release

The panel SHALL install the runtime by downloading the released AppImage, placing it and running the runtime's own `setup` for the session user, and SHALL report the outcome. When a newer version is available it SHALL update in place, at the same paths, rather than installing a second copy.

#### Scenario: Fresh install

- **WHEN** an install action succeeds on a machine with no runtime
- **THEN** the runtime files and user units exist and the runtime's own check reports them in place

#### Scenario: Fresh install leaves every mode off

- **WHEN** a fresh install completes
- **THEN** the runtime files and all the user units are placed but none is enabled, and the panel shows Off until a mode is selected

#### Scenario: Update

- **WHEN** an update action is run with an older runtime installed
- **THEN** the installed files are replaced at the same paths and the reported version becomes the new one

### Requirement: Verify before use and refuse on mismatch

The panel SHALL verify a downloaded artifact against a published checksum and SHALL refuse to place, run or set up anything whose checksum does not match.

#### Scenario: Corrupt or tampered download

- **WHEN** the downloaded file's checksum does not match the published one
- **THEN** the panel refuses, reports it, and changes nothing on the machine

### Requirement: The plugin installs the version it expects

The panel SHALL request the runtime version that matches its own release, and when only a different version is available it SHALL report the mismatch rather than installing an incompatible one.

#### Scenario: Matching release

- **WHEN** the plugin's release version has a matching AppImage on the release page
- **THEN** that AppImage is the one installed

#### Scenario: No matching artifact

- **WHEN** no AppImage matches the plugin's expected version
- **THEN** the panel reports the mismatch and installs nothing

### Requirement: Installation never needs root

The panel SHALL run the runtime's `setup` as the session user and SHALL NOT require or use root for installation.

#### Scenario: Install action

- **WHEN** an install or update runs
- **THEN** `setup` runs as the session user, installs under that user's directories only, and no elevation is requested

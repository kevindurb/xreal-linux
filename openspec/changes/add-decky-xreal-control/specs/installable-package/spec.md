# Spec Delta

## MODIFIED Requirements

### Requirement: One downloadable file

A release SHALL attach a single AppImage for x86_64 Linux to the project's release page, containing the presenter, the SteamVR driver, the setup logic and a version file naming the commit. The driver and presenter in it SHALL be built from the same commit on a fixed, documented build baseline, and the file SHALL NOT contain any capture or personal data. It SHALL use the host's Vulkan, Wayland and xkbcommon libraries instead of bundling them. From the same commit the release SHALL also carry the Decky plugin bundle and a checksum file for the downloadable artifacts, so the plugin and the runtime it installs come from one build.

#### Scenario: Downloading and running a release

- **WHEN** a user downloads the AppImage, makes it executable and runs it
- **THEN** it starts the interactive setup, with no source checkout, compiler or root needed

#### Scenario: A host without FUSE

- **WHEN** the AppImage cannot mount because FUSE is missing
- **THEN** the documentation and the setup output name the `--appimage-extract-and-run` fallback

#### Scenario: Release artifacts for the plugin

- **WHEN** a release is published
- **THEN** it carries the AppImage, a checksum file for it, and the Decky plugin bundle, all built from the same commit

## ADDED Requirements

### Requirement: The head-look service is offered opt-in

Setup SHALL offer to install the head-look user service, written and marked as the package's own like its other units, and SHALL leave it disabled unless the user asks for it (when setup asks, or later from the game-mode panel's switch). Uninstall SHALL stop and remove it.

#### Scenario: Setup with the head-look service declined

- **WHEN** setup runs and the user does not opt in to the head-look service
- **THEN** no head-look service is written and nothing of it runs

#### Scenario: Setup with the head-look service accepted

- **WHEN** setup runs and the user opts in
- **THEN** the head-look user service is written and enabled for that user

#### Scenario: Installed from the game-mode panel

- **WHEN** the panel installs the runtime (setup places every unit but enables none)
- **THEN** the head-look service is written but disabled and nothing of it runs, so the panel's switch is what enables head mouse

#### Scenario: Uninstall

- **WHEN** the package is uninstalled after the head-look service was installed
- **THEN** the head-look service is stopped and removed along with the package's other units

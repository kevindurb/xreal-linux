## MODIFIED Requirements

### Requirement: Headset in driver direct mode

The driver SHALL register an HMD named "XREAL 1S" that advertises a display component and a driver direct-mode component and sends its own vsync events, so that SteamVR's compositor starts without a DRM-lease display, but only once the presenter has confirmed that the glasses are present; until then it SHALL report no HMD.

#### Scenario: SteamVR starts with the driver registered

- **WHEN** SteamVR starts with the driver registered and forced as the active driver, and the presenter reports the glasses present
- **THEN** the compositor reports "Headset is using driver direct mode" and renders

#### Scenario: No glasses

- **WHEN** SteamVR starts with the driver registered and the presenter reports no glasses, or does not answer
- **THEN** the driver logs the reason and reports no HMD

### Requirement: Loadable inside SteamVR

The driver SHALL be built with a statically linked C++ runtime and no dependencies beyond libc, libm and libpthread so it loads inside SteamVR's older runtime, SHALL be built on a baseline whose glibc symbol versions do not exceed those of Steam's container runtime, and SHALL NOT assume the repository's location: it works when registered from any path.

#### Scenario: Library dependencies

- **WHEN** the built library's dynamic dependencies are listed
- **THEN** they contain only libm and libc

#### Scenario: Symbol versions

- **WHEN** the built library's glibc symbol versions are listed
- **THEN** none exceeds the documented baseline

#### Scenario: Registered from the data directory

- **WHEN** the driver is registered from the user's data directory and SteamVR starts
- **THEN** SteamVR loads it and the driver log shows it activated

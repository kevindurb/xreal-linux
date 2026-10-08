## MODIFIED Requirements

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

## MODIFIED Requirements

### Requirement: Session control

The shipped setup command (`xreal-setup start`, `stop` and `status`; `tools/vr_session.sh` in a checkout) SHALL start and stop the presenter and SteamVR together, restart only the presenter on request, report status, and pass options through (`XREAL_REPROJECT`, `XREAL_EXTRA_ARGS`). The presenter SHALL run with `--reproject` unless `XREAL_REPROJECT=0`, because reprojection is what hides SteamVR's missed frames.

#### Scenario: Start with defaults

- **WHEN** it is started with no environment overrides
- **THEN** the presenter runs with `--reproject`

#### Scenario: Start with reprojection

- **WHEN** it is started with `XREAL_REPROJECT=1`
- **THEN** the presenter runs with `--reproject`

#### Scenario: Reprojection turned off

- **WHEN** it is started with `XREAL_REPROJECT=0`
- **THEN** the presenter runs without `--reproject`

### Requirement: Read-only diagnostics

The shipped check (`xreal-setup check`; `tools/doctor.sh` in a checkout) SHALL check, without changing anything, that the glasses are on USB, the IMU stream flows, the display mode is full SBS, SteamVR is installed with its launcher capability set, the driver is built and registered, the SteamVR settings are right and the presenter is built, and SHALL exit non-zero when any check fails. It SHALL also warn, not fail, when `driver_xreal.hold_after_present` is false or `driver_xreal.running_start_ms` is set below 8, because SteamVR's dashboard then shows single bad frames.

#### Scenario: Everything in place

- **WHEN** all preconditions hold
- **THEN** it prints a pass for each check and exits zero

#### Scenario: Pacing settings that let bad frames through

- **WHEN** `driver_xreal.hold_after_present` is false, or `driver_xreal.running_start_ms` is below 8
- **THEN** it warns that dashboard frames may glitch and names the setting

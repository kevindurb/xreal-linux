## MODIFIED Requirements

### Requirement: Glasses in full SBS

The glasses SHALL be in full side-by-side mode, which presents a single 3840x1080 mode on their DisplayPort output, while SteamVR is using them. Their other modes (16:9, 16:10, ultrawide, half SBS) do not give each eye its own image. The presenter SHALL switch the glasses to full SBS after the driver connects and before it opens its window, SHALL record the mode they were in, and SHALL restore the previous 2D mode when the session ends; with SteamVR not running the glasses SHALL be left in the mode the user chose. From a checkout, `tools/vr_session.sh start` keeps its one-shot `--set-sbs-only` and confirmation.

#### Scenario: Regular mode at start

- **WHEN** `tools/vr_session.sh start` is run and the glasses are in the regular mode (their output offers more than the single 3840x1080 mode)
- **THEN** the one-shot `--set-sbs-only` sets full SBS once, and the presenter and SteamVR are started only after the output offers the single 3840x1080 mode

#### Scenario: Regular mode at SteamVR start

- **WHEN** the driver connects and the glasses are in a 2D mode (their output offers more than the single 3840x1080 mode)
- **THEN** the previous mode is recorded, full SBS is set once, and the window opens only after the output offers the single 3840x1080 mode

#### Scenario: Session ends

- **WHEN** the driver disconnects and the recorded mode was 2D
- **THEN** the presenter sets 2D again and the output's mode list matches the one seen before the session

#### Scenario: Already in full SBS

- **WHEN** the glasses are already in full SBS when the driver connects
- **THEN** nothing is sent at the start or at the end

#### Scenario: SBS is not reached

- **WHEN** the output does not become the single 3840x1080 mode within about 15 s (for example with `--no-set-sbs`, or the setter was rejected)
- **THEN** the presenter says so, replies to the driver that the glasses are not usable, and does not present

### Requirement: The presenter sets full side-by-side itself

The presenter SHALL, unless started with `--no-set-sbs`, send `NRDpSetInputMode` = 1 (`28 22 00 00 00 08 80 00 00 03 1a 02 08 01`) only when the input mode getter reports 0, at most once per control connection and at most three times per run, log each send and the result, and never retry a rejected or unanswered setter. It SHALL send `NRDpSetInputMode` = 0 only to restore a mode it recorded as 2D before it set full SBS, with its own small bound, and SHALL send no other state-changing request to the glasses (the id 10274 with values 0 and 1 is the only allowlisted setter). `--print-calibration` SHALL NOT send either.

#### Scenario: Glasses in regular mode at start

- **WHEN** the getter returns 0 on a new connection
- **THEN** the presenter sends the setter once, logs `accepted` on reply status 0, and the output becomes the single 3840x1080 mode

#### Scenario: Glasses already in side by side

- **WHEN** the getter returns 1, or `--no-set-sbs` is given
- **THEN** no setter is sent

#### Scenario: Setter rejected

- **WHEN** the setter reply carries a non-zero status
- **THEN** the presenter logs the status and does not retry on that connection

#### Scenario: Glasses that keep reverting

- **WHEN** the glasses fall back to the regular mode on each of several connections
- **THEN** the setter is sent for the first three connections only, and later connections log that the limit is reached

#### Scenario: Any other request

- **WHEN** code asks the control-port client to send any id or value outside the allowlist
- **THEN** the client refuses and nothing is sent

### Requirement: Session control

In a checkout, `tools/vr_session.sh` SHALL start and stop the presenter and SteamVR together, restart only the presenter on request, report status, and pass options through (`XREAL_REPROJECT`, `XREAL_EXTRA_ARGS`, for example `--no-set-sbs`), stopping the installed units first when they are enabled. For an installed package the service units run the presenter and SteamVR is started by the user. The presenter SHALL run with `--reproject` unless `XREAL_REPROJECT=0`, because reprojection is what hides SteamVR's missed frames.

#### Scenario: Start with defaults

- **WHEN** it is started with no environment overrides
- **THEN** the presenter runs with `--reproject`

#### Scenario: Start with reprojection

- **WHEN** it is started with `XREAL_REPROJECT=1`
- **THEN** the presenter runs with `--reproject`

#### Scenario: Reprojection turned off

- **WHEN** it is started with `XREAL_REPROJECT=0`
- **THEN** the presenter runs without `--reproject`

#### Scenario: Units enabled

- **WHEN** `tools/vr_session.sh start` is run while `xreal-linux.socket` is active
- **THEN** it stops the units first so the manual presenter can bind the socket

### Requirement: Read-only diagnostics

The check (`tools/doctor.sh` in a checkout, the setup check when installed) SHALL check, without changing anything, that the glasses are on USB, the IMU stream flows, SteamVR is installed with its launcher capability set, the driver is built or installed and registered, the SteamVR settings are right and the presenter is built or installed, and SHALL exit non-zero when any check fails. It SHALL report the display mode without failing on a 2D mode (the presenter switches it), and SHALL warn, not fail, when `driver_xreal.hold_after_present` is false or `driver_xreal.running_start_ms` is set below 8, because SteamVR's dashboard then shows single bad frames.

#### Scenario: Everything in place

- **WHEN** all preconditions hold
- **THEN** it prints a pass for each check and exits zero

#### Scenario: Pacing settings that let bad frames through

- **WHEN** `driver_xreal.hold_after_present` is false, or `driver_xreal.running_start_ms` is below 8
- **THEN** it warns that dashboard frames may glitch and names the setting

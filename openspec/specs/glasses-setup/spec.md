# glasses-setup Specification

## Purpose
The conditions the glasses, SteamVR and the desktop must satisfy for the headset to work, and the tools that check or establish them. Implemented by `tools/doctor.sh` and `tools/vr_session.sh`.

## Requirements

### Requirement: Glasses in full SBS

The glasses SHALL be in full side-by-side mode, which presents a single 3840x1080 mode on their DisplayPort output. Their other modes (16:9, 16:10, ultrawide, half SBS) do not give each eye its own image.

#### Scenario: Regular mode at start

- **WHEN** `tools/vr_session.sh start` is run and the glasses are in the regular mode (their output offers more than the single 3840x1080 mode)
- **THEN** the presenter sets full SBS once, SteamVR is started only after the output offers the single 3840x1080 mode, and the presenter's window moves to the glasses without a restart

#### Scenario: SBS is not reached

- **WHEN** the output does not become the single 3840x1080 mode within about 15 s (for example with `--no-set-sbs` in `XREAL_EXTRA_ARGS`, or the setter was rejected)
- **THEN** the script says so, prints the relevant presenter log lines, does not start SteamVR, stops the presenter and exits non-zero

### Requirement: Glasses menu settings that cannot be detected

The user SHALL set Follow mode, Stabilizer off, and auto sleep off in the glasses' own menu. The system cannot read the first two, so setup guidance SHALL state them. With the Stabilizer on, or in Anchor mode, the glasses move the image against head motion that the headset already compensates for.

#### Scenario: Setup guidance

- **WHEN** `tools/doctor.sh` finishes
- **THEN** it reminds the user of Follow mode, Stabilizer off and auto sleep off

### Requirement: SteamVR settings

SteamVR's `steamvr.vrsettings` SHALL force the `xreal` driver and set `power.turnOffScreensTimeout` very large and `power.pauseCompositorOnStandby` to false (the headset has no proximity sensor, so SteamVR otherwise enters standby after about five seconds of stillness and pauses the compositor). Motion smoothing SHOULD be off and the per-eye render size modest on a Steam Deck.

#### Scenario: Default timeout left in place

- **WHEN** `power.turnOffScreensTimeout` is still the five-second default
- **THEN** `tools/doctor.sh` warns that this causes standby and stutter

### Requirement: Desktop windows must not land on the glasses

At least one display other than the glasses SHALL be enabled so Steam and SteamVR windows have somewhere to open; otherwise, in full SBS, they appear in one eye.

#### Scenario: Glasses are the only display

- **WHEN** only the glasses' output is enabled
- **THEN** `tools/doctor.sh` warns that desktop windows will appear in one eye

### Requirement: Session control

`tools/vr_session.sh` SHALL start and stop the presenter and SteamVR together, restart only the presenter on request, report status, and pass options through (`XREAL_REPROJECT`, `XREAL_EXTRA_ARGS`, for example `--no-set-sbs`). The presenter SHALL run with `--reproject` unless `XREAL_REPROJECT=0`, because reprojection is what hides SteamVR's missed frames.

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

`tools/doctor.sh` SHALL check, without changing anything, that the glasses are on USB, the IMU stream flows, the display mode is full SBS, SteamVR is installed with its launcher capability set, the driver is built and registered, the SteamVR settings are right and the presenter is built, and SHALL exit non-zero when any check fails. It SHALL also warn, not fail, when `driver_xreal.hold_after_present` is false or `driver_xreal.running_start_ms` is set below 8, because SteamVR's dashboard then shows single bad frames.

#### Scenario: Everything in place

- **WHEN** all preconditions hold
- **THEN** it prints a pass for each check and exits zero

#### Scenario: Pacing settings that let bad frames through

- **WHEN** `driver_xreal.hold_after_present` is false, or `driver_xreal.running_start_ms` is below 8
- **THEN** it warns that dashboard frames may glitch and names the setting

### Requirement: Lightweight environment option

The setup guidance SHALL recommend running SteamVR without Home (`steamvr.enableHomeApp` false) with a compositor background such as `<SteamVR>/resources/backgrounds/aurorasky.png` in `steamvr.background`, as the default: it leaves the GPU to the content, and Home on is an option when there is headroom. `tools/doctor.sh` SHALL report which is set, warning, not failing, when Home is on.

#### Scenario: Home off with the aurora background

- **WHEN** `steamvr.enableHomeApp` is false and `steamvr.background` names `aurorasky.png`
- **THEN** SteamVR shows the aurora sky and grid floor around the dashboard instead of the Home room, and `tools/doctor.sh` reports Home as off

### Requirement: Presenter reads the glasses' calibration and events over the control port

The presenter SHALL, on connecting to the control port (TCP 52999), send the read-only `NRGlassesGetConfig` and `NRDpGetInputMode` requests, derive the per-eye field of view, IPD, IMU matrices and display orientations from the config reply, send the field of view and IPD to the driver, cache the reply per unit outside the repository under a file name that does not contain the serial number, use the newest cache when the control port is unreachable, and log the glasses' events with timestamps (temperature events at most once a minute). It SHALL treat a quiet connection as normal and reconnect only when the connection is closed or fails (a dead peer is detected by TCP keepalive).

#### Scenario: Quiet control port

- **WHEN** no event arrives for longer than 30 s
- **THEN** the presenter keeps the same connection and logs nothing

#### Scenario: Connection closed

- **WHEN** the glasses close the connection or it fails
- **THEN** the presenter logs it and reconnects, reading the config again

### Requirement: The presenter sets full side-by-side itself

The presenter SHALL, unless started with `--no-set-sbs`, send `NRDpSetInputMode` = 1 (`28 22 00 00 00 08 80 00 00 03 1a 02 08 01`) only when the input mode getter reports 0, at most once per control connection and at most three times per run, log each send and the result, and never retry a rejected or unanswered setter. `--print-calibration` SHALL NOT send it.

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

### Requirement: Analyse why the glasses drop to 2D

The repository SHALL provide `tools/analyze_control_events.py`, which reads a presenter log and, for each drop from full side-by-side to 2D (or a vanished connector), lists the control events in the preceding window (default 30 s) with their meaning, then tabulates which events occur before drops against their overall rate.

#### Scenario: One drop in a log

- **WHEN** the log has a `[display ...] modes:` line that leaves `3840x1080` at +31.5 s and control events before it
- **THEN** the output lists those events with their offsets before the drop and ends with the comparison table

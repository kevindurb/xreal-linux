# glasses-setup Specification

## Purpose
The conditions the glasses, SteamVR and the desktop must satisfy for the headset to work, and the tools that check or establish them. Implemented by `tools/doctor.sh` and `tools/vr_session.sh`.

## Requirements
### Requirement: Glasses in full SBS

The glasses SHALL be in full side-by-side mode, which presents a single 3840x1080 mode on their DisplayPort output. Their other modes (16:9, 16:10, ultrawide, half SBS) do not give each eye its own image.

#### Scenario: Wrong mode

- **WHEN** `tools/vr_session.sh start` is run and the glasses' output offers anything other than the single 3840x1080 mode
- **THEN** it refuses to start and says the glasses must be switched to full SBS

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

`tools/vr_session.sh` SHALL start and stop the presenter and SteamVR together, restart only the presenter on request, report status, and pass options through (`XREAL_REPROJECT`, `XREAL_EXTRA_ARGS`).

#### Scenario: Start with reprojection

- **WHEN** it is started with `XREAL_REPROJECT=1`
- **THEN** the presenter runs with `--reproject`

### Requirement: Read-only diagnostics

`tools/doctor.sh` SHALL check, without changing anything, that the glasses are on USB, the IMU stream flows, the display mode is full SBS, SteamVR is installed with its launcher capability set, the driver is built and registered, the SteamVR settings are right and the presenter is built, and SHALL exit non-zero when any check fails.

#### Scenario: Everything in place

- **WHEN** all preconditions hold
- **THEN** it reports no failures and exits zero

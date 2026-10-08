## ADDED Requirements

### Requirement: Check the machine

`xreal-setup check` SHALL report, without changing anything, each precondition (glasses on USB, the IMU stream, display mode, SteamVR installed, driver present and registered, SteamVR settings, presenter present, driver and presenter versions) as pass, warn or fail with the fix for each, and SHALL exit non-zero when any check fails.

#### Scenario: Everything in place

- **WHEN** all preconditions hold
- **THEN** every line passes and the exit status is zero

#### Scenario: Glasses not in full SBS

- **WHEN** the display shows a normal 2D mode
- **THEN** the check fails with the instruction to switch the glasses to full SBS in their menu

### Requirement: Apply safe fixes with consent

`xreal-setup fix` SHALL be able to register the driver with SteamVR and set the SteamVR settings the setup needs, SHALL show each change and ask before applying it (unless told to proceed), SHALL refuse to edit the settings while SteamVR is running, and SHALL back up the settings file before the first change. It SHALL NOT need root.

#### Scenario: Settings that need changing

- **WHEN** `forcedDriver` is not `xreal` and SteamVR is stopped
- **THEN** it shows the change, asks, backs up the file, applies it and a second `check` passes

#### Scenario: SteamVR is running

- **WHEN** SteamVR is running
- **THEN** it refuses to edit the settings and says to stop SteamVR first

#### Scenario: User declines

- **WHEN** the user declines a change
- **THEN** nothing is written and the check still reports it

### Requirement: Say what only the user can set

For the glasses' own settings that the host can neither read nor write (Follow mode, Stabilizer off, auto-sleep off), `xreal-setup` SHALL list them with where to find each in the glasses' menu, SHALL NOT claim to have verified them, and SHALL NOT write to the glasses.

#### Scenario: Always listed

- **WHEN** `xreal-setup check` runs
- **THEN** the output ends with those settings as things to confirm by hand

### Requirement: Run a session

`xreal-setup start` and `stop` SHALL start and stop the presenter and SteamVR together, and `status` SHALL report both, using the same options as the repository's session script (reprojection on by default).

#### Scenario: Start and stop

- **WHEN** the user runs `xreal-setup start`, then `xreal-setup stop`
- **THEN** the presenter and SteamVR start together and then both stop

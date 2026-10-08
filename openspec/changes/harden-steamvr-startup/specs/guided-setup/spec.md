# Spec Delta

## ADDED Requirements

### Requirement: Detect and clear a safe-mode block of the driver

The check SHALL fail when the SteamVR settings mark the XREAL driver as blocked by SteamVR's safe mode (`driver_xreal.blocked_by_safe_mode` true), saying that SteamVR disabled the driver after a crash and that `fix` clears it. `fix` SHALL clear the flag under the same rules as its other settings changes: show the change and ask first, refuse while SteamVR is running, back up the file before the first change, and record the previous value so uninstall restores only that key.

#### Scenario: Driver blocked after a crash

- **WHEN** `driver_xreal.blocked_by_safe_mode` is true
- **THEN** the check fails and names `fix`, and the exit status is non-zero

#### Scenario: Clearing the block

- **WHEN** the user runs `fix` with SteamVR stopped and accepts the change
- **THEN** the settings file is backed up, the flag is removed, and a second check passes

#### Scenario: SteamVR is running

- **WHEN** `fix` is run while SteamVR is running and the driver is blocked
- **THEN** it refuses to edit the settings and says to stop SteamVR first

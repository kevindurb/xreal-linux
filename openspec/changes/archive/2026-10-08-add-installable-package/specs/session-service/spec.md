## ADDED Requirements

### Requirement: Nothing runs until SteamVR's driver connects

The installed package SHALL provide a systemd user socket unit that owns the driver link's abstract socket (`@xreal-presenter-<uid>`) and a service unit that runs the presenter only when the driver connects. With SteamVR not running there SHALL be no presenter process, no window on the glasses' output and no request sent to the glasses.

#### Scenario: Idle

- **WHEN** the units are enabled and SteamVR is not running, with the glasses plugged in and in any 2D mode
- **THEN** no presenter process exists, the glasses stay in their mode, and they work as an ordinary monitor

#### Scenario: SteamVR starts

- **WHEN** SteamVR loads the driver
- **THEN** systemd starts the presenter, which accepts the driver's queued connection

### Requirement: The presenter stops when SteamVR stops

The service SHALL exit shortly after the driver disconnects (a SteamVR restart within a short grace keeps the process), SHALL restore the glasses' previous display mode on every stop including a crash, and SHALL NOT be restarted in a tight loop when it fails.

#### Scenario: SteamVR quits

- **WHEN** SteamVR exits normally
- **THEN** the presenter exits after the grace period, the window is gone, and the glasses are back in the mode recorded before the session

#### Scenario: Presenter killed

- **WHEN** the presenter is killed during a session with the glasses recorded as "was 2D"
- **THEN** the unit's stop step still sets the glasses back to 2D

### Requirement: The driver reports a headset only when the glasses are usable

The presenter's reply to the driver's first message SHALL carry the protocol version and whether the glasses are present (they answer on the control port); until the driver has a positive reply it SHALL report no HMD to SteamVR, so a machine with the driver registered but no glasses, or another headset in use, shows no XREAL headset.

#### Scenario: Glasses unplugged

- **WHEN** SteamVR starts with the driver registered and the glasses unplugged
- **THEN** the driver logs that the presenter reported no glasses and SteamVR shows no XREAL headset

#### Scenario: Glasses plugged in

- **WHEN** SteamVR starts with the glasses plugged in, in a 2D mode
- **THEN** the presenter switches them to full SBS, replies positively, and the headset appears

### Requirement: Manual runs do not collide with the units

When the socket unit is enabled, starting a presenter by hand from a checkout SHALL either stop the units first (through `tools/vr_session.sh`) or fail with a message naming the socket and the unit, never run a second competing presenter.

#### Scenario: Manual presenter with units enabled

- **WHEN** a presenter is started by hand on the same socket name while the socket unit is active
- **THEN** it exits with a message that the socket is in use by `xreal-linux.socket`

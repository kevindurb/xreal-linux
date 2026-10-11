# Spec Delta

## Purpose

Give XREAL glasses one game-mode control panel in the Steam Deck's Quick Access Menu: a single mutually-exclusive mode selector (off, head mouse, SteamVR), a status readout, and the actions that drive the installed runtime through the session user's services.

## ADDED Requirements

### Requirement: Mutually-exclusive mode selector

The panel SHALL offer exactly one active mode among **Off**, **Head mouse** and **SteamVR**. Selecting a mode SHALL enable and start the unit that mode needs and disable and stop the other tier's unit, so head mouse and the SteamVR service are never both active. The panel SHALL show which mode is active, and on opening SHALL reflect the units' actual state rather than assume the last selection.

#### Scenario: Head mouse selected

- **WHEN** the wearer selects Head mouse
- **THEN** the head-look service is enabled and started, the SteamVR socket is disabled and stopped, and the panel shows Head mouse as active

#### Scenario: SteamVR selected

- **WHEN** the wearer selects SteamVR
- **THEN** the SteamVR socket is enabled and started, the head-look service is disabled and stopped, and the panel shows SteamVR as active

#### Scenario: Off selected

- **WHEN** the wearer selects Off
- **THEN** both the head-look service and the SteamVR socket are disabled and stopped and the glasses are left as an ordinary monitor

#### Scenario: State changed outside the panel

- **WHEN** the panel opens and the units were changed elsewhere (for example a terminal or `setup`)
- **THEN** the panel shows the actual active state, not a remembered one

### Requirement: Actions run as the session user

The panel SHALL perform its unit actions and run the runtime as the user who owns the active desktop session, and SHALL NOT require root or a typed command. This SHALL hold even when the panel's own backend runs with more privilege than that user.

#### Scenario: Toggled from game mode

- **WHEN** a mode is selected in game mode
- **THEN** the actions take effect for the session user (their `systemctl --user` units and their `/dev/uinput` device), and no password or terminal is needed

### Requirement: Status readout

The panel SHALL show whether the glasses are reachable, whether each tier's unit is active and enabled, and the installed runtime version, refreshed when the panel opens and after each action.

#### Scenario: Glasses unplugged

- **WHEN** the glasses are not reachable
- **THEN** the panel reports them unreachable and does not claim head tracking is working

#### Scenario: After an action

- **WHEN** a mode is selected
- **THEN** the status reflects the result of the action, including a failure and its reason

### Requirement: The panel requires the runtime

The panel SHALL detect whether the installed runtime is present, and when it is not SHALL say so and point to the installation action instead of failing silently or leaving a toggle that does nothing.

#### Scenario: Runtime not installed

- **WHEN** the panel opens with no runtime installed
- **THEN** it reports that the runtime is missing and offers installation

### Requirement: The panel does not talk to the glasses itself

The panel SHALL send no request to the glasses directly; every action SHALL go through the runtime, so the project's rule that only allowlisted requests may ever reach the glasses is preserved. Producing the panel's status SHALL NOT change the glasses' display mode.

#### Scenario: Any panel action

- **WHEN** any mode is selected or status is read
- **THEN** only the runtime's documented, allowlisted requests are sent, and the glasses' display mode is unchanged by status reads

### Requirement: Leaving SteamVR is guarded during a live session

The panel SHALL refuse, or require an explicit confirmation, to switch away from SteamVR mode while a SteamVR session is running, so the presenter is not stopped and the display is not yanked mid-session.

#### Scenario: SteamVR running and Off selected

- **WHEN** SteamVR is running and the wearer selects Off or Head mouse
- **THEN** the panel does not stop the running session without confirmation and says why

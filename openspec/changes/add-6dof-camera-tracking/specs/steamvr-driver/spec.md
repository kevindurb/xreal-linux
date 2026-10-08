## MODIFIED Requirements

### Requirement: Report the tracked pose

The driver SHALL report to SteamVR each new orientation and angular velocity it receives from the presenter, stamped with its age (`poseTimeOffset`) from the host time the sample arrived, and SHALL report the position and linear velocity too when the presenter sends a trusted position. Otherwise it SHALL place the head at a configurable height (`driver_xreal.head_height`, default 1.5 m). It SHALL fall back to an untracked identity pose when no presenter pose is available, and SHALL turn SteamVR's head model off while a trusted position is reported. Angular velocity and the head model are controllable with `send_angular_velocity` and `head_model` for diagnosis.

#### Scenario: Presenter tracking

- **WHEN** the presenter sends valid poses without a position
- **THEN** SteamVR's head pose follows the glasses' orientation at the configured height, updated only when a new sample arrives (and at least every 100 ms)

#### Scenario: Presenter tracking with position

- **WHEN** the presenter sends valid poses with a trusted position
- **THEN** SteamVR's head pose follows the glasses' orientation and position, and the head model is off

#### Scenario: Position lost

- **WHEN** the presenter stops sending a position
- **THEN** the driver returns to the fixed head height with the head model on, without a jump larger than the last trusted position's change

#### Scenario: No presenter

- **WHEN** no presenter is connected
- **THEN** the head pose is the untracked identity orientation

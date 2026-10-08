## MODIFIED Requirements

### Requirement: Publish the pose to the driver

The system SHALL publish the latest orientation and the world-frame angular velocity to the SteamVR driver about every 2 ms over the driver link, together with position, linear velocity and the tracking tier when the position tier is 6DoF, and SHALL mark the pose invalid while the IMU is disconnected. Without a trusted position the message SHALL say so and orientation behaviour SHALL be unchanged.

#### Scenario: IMU connection lost

- **WHEN** the IMU stream drops
- **THEN** the pose sent to the driver is marked invalid until the stream is back

#### Scenario: Position available

- **WHEN** the position tier is 6DoF
- **THEN** each published pose carries the position and linear velocity with the orientation

#### Scenario: Position not trusted

- **WHEN** the tier is 3DoF or 6DoF-lost
- **THEN** the pose is published without a position and the orientation is identical to the IMU-only result

## ADDED Requirements

### Requirement: Head position from camera and IMU

When the Eye is present and calibrated, the system SHALL estimate the head's position in metric units, fused with the IMU orientation, and publish position, linear velocity and orientation continuously. The position SHALL be relative to where tracking started, with the height set by the user's floor height setting.

#### Scenario: Leaning forward

- **WHEN** a wearer in a textured room leans forward by 30 cm and back
- **THEN** the published position moves forward by 30 cm (within 5 cm) and back, and the view in SteamVR moves with it

#### Scenario: Sitting still

- **WHEN** the head is still for 60 seconds
- **THEN** the published position drifts by less than 2 cm

### Requirement: Tracking tiers and fallback

The system SHALL report one of three tiers: 3DoF (no camera or no calibration), 6DoF (position tracked) or 6DoF-lost (camera present but tracking lost). It SHALL fall back to 3DoF behaviour within one second of losing visual tracking, SHALL NOT publish a position it does not trust, and SHALL return to 6DoF without a restart when tracking recovers.

#### Scenario: Camera covered

- **WHEN** the camera is covered while tracking
- **THEN** the tier changes to 6DoF-lost within one second, orientation keeps working from the IMU, and position holds its last trusted value instead of jumping

#### Scenario: Tracking recovers

- **WHEN** the camera is uncovered in a textured room
- **THEN** the tier returns to 6DoF without restarting anything and position resumes without a visible jump larger than 2 cm

#### Scenario: Featureless view

- **WHEN** the camera faces a blank wall
- **THEN** the system does not publish a drifting position

### Requirement: Latency and rate

The system SHALL publish the fused pose at the IMU rate with the position updated at least at the camera's rate, and SHALL keep the added latency from camera exposure to the published position under 50 ms on the Steam Deck while SteamVR runs.

#### Scenario: Running with SteamVR

- **WHEN** SteamVR is running Home on the Deck and the tracker is on
- **THEN** SteamVR still delivers 60 new frames per second and the position latency stays under 50 ms

### Requirement: Status and diagnostics

The system SHALL expose its tier, tracking quality and calibration status to `tools/doctor.sh` and to the presenter's report, and SHALL log each tier change with its cause.

#### Scenario: Doctor with no calibration

- **WHEN** `tools/doctor.sh` runs with the Eye present but no calibration file
- **THEN** it warns, not fails, and says how to calibrate

### Requirement: Offline scoring

The system SHALL score a recorded capture against a tape-measured or externally tracked reference path, reporting position error over the run, so tracking can be tested without a wearer.

#### Scenario: Scoring a replay

- **WHEN** a recorded walk of known length is replayed and scored
- **THEN** the tool reports end-point error and maximum deviation

## ADDED Requirements

### Requirement: Camera calibration data

The system SHALL hold, per device, the camera intrinsics (including lens distortion), the camera-to-IMU rotation and translation and the camera-to-IMU time offset, SHALL load them at start from a calibration file, and SHALL refuse to run position tracking without a valid calibration, falling back to the 3DoF tier and saying why.

#### Scenario: Calibration present

- **WHEN** a valid calibration file for the connected glasses exists
- **THEN** position tracking uses it and reports the calibration's date and reprojection error

#### Scenario: Calibration missing or for other glasses

- **WHEN** no calibration file exists, or its device serial does not match the connected glasses
- **THEN** position tracking is not started, the system runs in the 3DoF tier, and the status says calibration is needed

### Requirement: Guided calibration

The system SHALL provide a calibration procedure that a user can run with a printed target and the glasses, which records the necessary data, solves the calibration, reports its quality, and writes the calibration file only when the quality meets a stated threshold.

#### Scenario: Good calibration run

- **WHEN** the user follows the procedure and the solved reprojection error is under the threshold
- **THEN** the calibration file is written and the status reports the error

#### Scenario: Poor calibration run

- **WHEN** the solved error exceeds the threshold, or the target was not seen from enough angles
- **THEN** no file is written and the tool says what to repeat

### Requirement: Calibration quality is checkable offline

The system SHALL let a recorded capture be scored against a calibration, reporting reprojection error and the time offset estimate, without the glasses attached.

#### Scenario: Scoring a capture

- **WHEN** a recorded calibration capture is scored
- **THEN** the tool prints the reprojection error and time offset

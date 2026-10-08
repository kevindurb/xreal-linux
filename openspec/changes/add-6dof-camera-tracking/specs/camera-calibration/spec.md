## ADDED Requirements

### Requirement: Camera calibration data

The system SHALL hold, per device, the camera intrinsics (including lens distortion), the camera-to-IMU rotation and translation and the camera-to-IMU time offset. It SHALL obtain the intrinsics and the camera-to-IMU transform from the glasses' own factory calibration by a read-only request, SHALL cache the response per device serial outside the repository, SHALL take the time offset from a measurement stored for the same device, and SHALL refuse to run position tracking without a valid calibration, falling back to the 3DoF tier and saying why.

#### Scenario: Glasses answer

- **WHEN** the glasses are connected and answer the configuration request
- **THEN** position tracking uses the camera calibration in the answer together with the stored time offset, and reports the calibration's date (`last_modified_time`) and the reprojection error of the last check

#### Scenario: Glasses cannot be asked

- **WHEN** the configuration request fails or times out but a cached copy for the connected serial exists
- **THEN** the cached copy is used and the status says so

#### Scenario: Calibration missing or for other glasses

- **WHEN** no calibration is available, or its device serial does not match the connected glasses
- **THEN** position tracking is not started, the system runs in the 3DoF tier, and the status says calibration is needed

#### Scenario: Time offset not yet measured

- **WHEN** no measured time offset is stored for the connected glasses
- **THEN** position tracking is not started and the status says the time offset must be measured

### Requirement: Guided calibration

The system SHALL provide a procedure that a user can run with the glasses to measure the camera-to-IMU time offset and to check the factory calibration with a printed target, which records the necessary data, reports its quality, and stores the result only when the quality meets a stated threshold. If the factory calibration fails its check, the procedure SHALL say so and offer a full fit that records the data, solves for the intrinsics and extrinsics and writes a calibration that overrides the factory values.

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

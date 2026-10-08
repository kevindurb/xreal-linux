# imu-tracking Specification

## Purpose
Turn the XREAL glasses' IMU stream into a head orientation (3DoF) that the SteamVR driver can report. Implemented in `presenter/src/tracking.rs`.

## Requirements
### Requirement: Read the IMU stream from the glasses

The system SHALL read the IMU stream over TCP port 52998 of the glasses' link-local address (`169.254.1.1`, falling back to `169.254.2.1`) and SHALL reconnect automatically when the connection is lost.

#### Scenario: Glasses connected at start

- **WHEN** the presenter starts with the glasses connected
- **THEN** it connects to the IMU port and begins producing orientation without user action

#### Scenario: Glasses unplugged and plugged back in

- **WHEN** the IMU connection drops and the glasses become reachable again
- **THEN** the system reconnects on its own and orientation resumes

### Requirement: Parse IMU records robustly

The system SHALL parse 134-byte records identified by the six-byte header `28 36 00 00 00 80` and SHALL NOT match on header bytes 6 and 7, which differ between glasses sessions. It SHALL use only records of type `0x0B`, read the timestamp as a little-endian u64 at offset 14, read gyro (rad/s) and accelerometer (m/s^2) as six little-endian f32 values from offset 34, and discard records containing non-finite values.

#### Scenario: Header bytes 6-7 differ

- **WHEN** records arrive whose bytes 6-7 are `38 41` in one session and `28 be` in another
- **THEN** both are parsed

#### Scenario: Non-IMU record

- **WHEN** a record has a type other than `0x0B` (for example the all-NaN type `0x04`)
- **THEN** it is ignored

#### Scenario: Partial record at the end of a read

- **WHEN** a read ends in the middle of a record
- **THEN** the partial bytes are kept and completed by the next read

### Requirement: Sensor frame to body frame conversion

The system SHALL convert sensor-frame vectors to the OpenVR body frame (x right, y up, z back) as `(x, -y, -z)`, which matches measurements of the glasses (gravity reads about -9.8 on sensor Y; yaw left is negative gyro Y, pitch up is positive gyro X, and tilting to the left shoulder is negative gyro Z).

#### Scenario: Turning the head left

- **WHEN** the wearer turns left
- **THEN** the orientation rotates by a positive angle about the body's up axis

#### Scenario: Looking up

- **WHEN** the wearer looks up
- **THEN** the orientation rotates by a positive angle about the body's x axis

#### Scenario: Tilting to the left shoulder

- **WHEN** the wearer tilts toward the left shoulder
- **THEN** the orientation rotates by a positive angle about the body's z axis

### Requirement: Orientation fusion

The system SHALL integrate the gyro into an orientation, SHALL initialise pitch and roll from gravity with yaw zero at start, and SHALL correct pitch and roll toward the accelerometer's gravity direction only while the accelerometer magnitude is within 20% of 1 g. It SHALL NOT correct yaw (the glasses have no magnetometer), so yaw drifts with residual gyro bias.

#### Scenario: Head still

- **WHEN** the glasses are held still for ten seconds
- **THEN** the reported orientation changes by less than half a degree

#### Scenario: Accelerometer disturbed

- **WHEN** the accelerometer magnitude is more than 20% away from 1 g (for example during a fast shake)
- **THEN** gravity correction is skipped for those samples and the gyro alone drives the orientation

### Requirement: Gyro bias tracking

The system SHALL estimate the gyro bias while the head is still (gyro rate below 0.03 rad/s with the accelerometer within 0.3 m/s^2 of 1 g for at least 0.5 s), with a time constant of about one second, and SHALL subtract it from the gyro.

#### Scenario: Bias drifts as the glasses warm up

- **WHEN** the gyro bias moves slowly while the head is still
- **THEN** the estimate follows it and yaw drift stays small

### Requirement: Publish the pose to the driver

The system SHALL publish the latest orientation and the world-frame angular velocity to the SteamVR driver about every 2 ms over the driver link, and SHALL mark the pose invalid while the IMU is disconnected.

#### Scenario: IMU connection lost

- **WHEN** the IMU stream drops
- **THEN** the pose sent to the driver is marked invalid until the stream is back

### Requirement: Simulated head motion for testing

The system SHALL offer a debug mode (`--sim-pose`) that replaces the IMU with a synthetic head sweep, with configurable yaw amplitude and pitch offset and amplitude, so UI interaction in SteamVR can be exercised without anyone wearing the glasses.

#### Scenario: Simulated sweep

- **WHEN** the presenter is started with `--sim-pose --sim-yaw 70`
- **THEN** the pose sent to the driver sweeps the yaw to the left and right instead of following the IMU

### Requirement: Apply the glasses' factory IMU matrices

The system SHALL multiply each raw gyro and accelerometer vector by the 3x3 `gyro_calib_mat` and `accl_calib_mat` from the glasses' own calibration (read over the control port) before the frame conversion, unless started with `--no-imu-calibration`. It SHALL NOT seed the gyro bias from the calibration or use its temperature table, because the measured stream bias does not match them (`docs/findings.md`). Whether the matrices apply as `M * v` is unverified.

#### Scenario: Calibration available

- **WHEN** the presenter has the calibration and receives a gyro vector
- **THEN** the filter integrates the vector multiplied by `gyro_calib_mat`

#### Scenario: Calibration disabled

- **WHEN** the presenter is started with `--no-imu-calibration`
- **THEN** the filter integrates the raw vectors

### Requirement: Optional magnetometer yaw bound

The system SHALL, only when started with `--mag-yaw`, fit a hard-iron offset to the magnetometer records (type 4), wait until the fit has been stable, and then nudge yaw toward the field's horizontal direction recorded at that moment, rejecting readings whose strength departs from the fitted one. Without the flag the magnetometer SHALL NOT affect the pose.

#### Scenario: Gyro yaw drifted after calibration

- **WHEN** `--mag-yaw` is set, the magnetometer has been calibrated by rotating the glasses through many directions, and the filter's yaw is wrong by 20 degrees with the glasses still
- **THEN** the yaw error shrinks to a few degrees (checked with synthetic data only; not yet on hardware)

### Requirement: Guided magnetometer calibration and report

The system SHALL provide `--mag-calibrate`, which without SteamVR reads the magnetometer records while the wearer turns the glasses through many directions, prints the sample count, the number of the 26 direction bins visited, the fitted offset, per-axis scale and radius and the residual, and saves the fit for this unit (in a file named by a hash, never by the serial number) only when at least 21 directions were visited, at least 1000 thinned samples were collected, the residual is at most 10 % of the radius and the scales are within 0.5 to 2. A cloud of samples spread over less than 10 microtesla SHALL count as no directions. With `--mag-yaw`, a saved calibration SHALL be applied (before the frame conversion) instead of learning the offset, and `--mag-report` SHALL hold still for 60 s and print the yaw drift of the filter with and without the correction on the same samples. None of this is validated on real magnetometer data.

#### Scenario: Wearer covers the sphere

- **WHEN** the samples have a fixed offset and per-axis gains and cover the sphere (synthetic data with noise)
- **THEN** the fit recovers the offset within about 0.6 microtesla and equalises the axes within 3 %, and the calibration is saved

#### Scenario: Glasses resting on a table

- **WHEN** the samples vary by under a few microtesla
- **THEN** no directions are counted and nothing is saved

## ADDED Requirements

### Requirement: Read the Eye camera stream

The system SHALL read the Eye's frames over TCP port 52997 of the glasses' link-local address (`169.254.1.1`, falling back to `169.254.2.1`), SHALL recognise a frame by its fixed size and four-byte start marker, SHALL resynchronise on the marker after a short or corrupt frame, and SHALL reconnect automatically when the connection is lost. It SHALL work with glasses that have no Eye attached by reporting that no camera is present, not by failing.

#### Scenario: Eye attached at start

- **WHEN** the tracker starts with the glasses and Eye connected
- **THEN** it begins producing decoded frames at the stream's rate without user action

#### Scenario: Stream interrupted mid-frame

- **WHEN** a read ends in the middle of a frame, or the connection drops and returns
- **THEN** the partial frame is discarded, decoding resumes on the next frame marker and no stale frame is delivered

#### Scenario: No Eye

- **WHEN** the glasses are connected without an Eye
- **THEN** the camera is reported absent and nothing else in the system stops working

### Requirement: Decode frames into grayscale images

The system SHALL decode each frame into the clean grayscale view and SHALL expose the two interleaved renderings as separate images, with the pixel packing and image geometry recorded in `docs/findings.md` once verified. Decoding SHALL NOT drop frames at the stream's rate on the Steam Deck.

#### Scenario: Normal frame

- **WHEN** a valid frame arrives
- **THEN** the clean grayscale image is available with the correct width, height and aspect (4:3, matching the camera size the glasses report) and a decoded frame can be saved as a PNG for inspection

### Requirement: Timestamp frames on the IMU clock

The system SHALL give every decoded frame a timestamp on the same clock as the IMU samples, using the frame's own timestamp field and the glasses' timing records, and SHALL report the residual uncertainty. It SHALL NOT use host arrival time as the frame time except as a labelled fallback.

#### Scenario: Shake test

- **WHEN** the glasses are shaken while camera and IMU are recorded
- **THEN** the lag between optical motion and gyro motion measured on the frame timestamps is within 5 ms

### Requirement: Record and replay

The system SHALL record camera frames and IMU samples with their timestamps to a capture directory, SHALL replay a capture through the same decoding and tracking path as live data, and SHALL keep captures out of the repository.

#### Scenario: Replay equals live

- **WHEN** a recorded session is replayed
- **THEN** the tracker produces the same poses it produced live, within numerical tolerance

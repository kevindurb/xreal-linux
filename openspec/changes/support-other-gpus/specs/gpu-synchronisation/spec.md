## ADDED Requirements

### Requirement: Wait for SteamVR's writes on every GPU

The presenter SHALL NOT read a SteamVR swap image without a defined wait for the work writing it. It SHALL use the GPU's exported write fence where the kernel provides one, SHALL use a documented bounded fallback wait where it does not, and SHALL never skip the wait silently.

#### Scenario: Sync file available

- **WHEN** the export of a sync file from the swap image succeeds
- **THEN** the presenter waits on it before reading the image

#### Scenario: Sync file not available

- **WHEN** the export fails for the swap images (for example because the fd is not a dma-buf)
- **THEN** the presenter uses the fallback wait for each frame and logs once that it is doing so

### Requirement: Report the active strategy

The presenter SHALL log, once per session and in its periodic report, which synchronisation method and which vblank-time source (present wait or acquire estimate) are active, and the capture metadata SHALL record whether the fence was supported per frame.

#### Scenario: Strategy visible

- **WHEN** a session starts
- **THEN** the log names the wait method and the vblank source

### Requirement: Torn frames are detectable

The system SHALL provide a way to detect half-drawn frames in a capture (the existing sweep capture and analysis) on any GPU, so a synchronisation fallback can be judged by data.

#### Scenario: Fallback compared with the fence path

- **WHEN** the same sweep capture is run with the fallback wait and, where available, with the fence wait
- **THEN** the analysis reports the bad-frame counts for each so they can be compared

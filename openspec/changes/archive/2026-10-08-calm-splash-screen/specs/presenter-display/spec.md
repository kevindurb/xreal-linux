## RENAMED Requirements

- FROM: `### Requirement: Test pattern when nothing is presenting`
- TO: `### Requirement: Calm splash when nothing is presenting`

## MODIFIED Requirements

### Requirement: Calm splash when nothing is presenting

The presenter SHALL show a calm splash whenever no driver is connected or the driver is not presenting a usable frame, and SHALL count such frames while a driver is connected so they can be monitored. The splash SHALL be identical in the left and right halves (the same pixels at the same positions, so it has no stereo disparity and no per-eye colour difference), static, dark, and low in contrast, with a small centred mark and the text "Waiting for SteamVR". It SHALL NOT draw bright borders, a centre line, or anything that moves. Each time the splash starts to show, it SHALL fade in from black over about one second.

#### Scenario: No driver

- **WHEN** the presenter runs with the glasses in full SBS and without SteamVR
- **THEN** both eyes show the same dark splash with the text "Waiting for SteamVR", and nothing on it moves

#### Scenario: Fade in

- **WHEN** the splash first appears (the window is first shown, or SteamVR stops presenting after having presented)
- **THEN** the picture rises from black to its full level over about one second, and the first frame is black

#### Scenario: Driver connected but not yet presenting

- **WHEN** the driver is connected and has not yet presented a usable frame
- **THEN** the splash is shown and each such frame is counted in the fallback-frame count

#### Scenario: SteamVR starts presenting

- **WHEN** SteamVR presents its first usable frame while the splash is showing
- **THEN** the splash is replaced by SteamVR's frame, and no part of the splash remains visible

## ADDED Requirements

### Requirement: Stereo test pattern

The presenter SHALL, only when started with `--test-pattern`, draw the side-by-side stereo test pattern instead of the eye images and instead of the splash, also when SteamVR is not connected: the left half tinted red and the right half tinted blue, white borders and a centre line marking the two halves, a cross-hair at the centre of each eye, and a square that slides across each half with a different horizontal offset in each eye. Without the flag the presenter SHALL never draw it.

#### Scenario: Checking per-eye stereo

- **WHEN** the presenter runs with `--test-pattern` on glasses in full SBS, with or without SteamVR
- **THEN** the left eye sees only the red half, the right eye sees only the blue half, and the sliding square appears at a depth offset from the frame

#### Scenario: Default run

- **WHEN** the presenter runs without `--test-pattern`
- **THEN** no red or blue tinted frame and no sliding square is ever shown, whether or not SteamVR is presenting

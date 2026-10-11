# flat-stereo-mode Specification

## Purpose

Set, report and restore the glasses' full side-by-side display mode outside SteamVR, so a game that renders side-by-side 3D itself gives each eye its own image with no SteamVR involved.

## Requirements

### Requirement: Set full side-by-side on demand

The system SHALL set the glasses to full side-by-side (the single 3840x1080 mode) on request, without a SteamVR session and without the presenter's window, using only the allowlisted display-mode setter, and SHALL confirm that the mode changed.

#### Scenario: Glasses in a 2D mode

- **WHEN** the setter is asked to switch on and the glasses offer more than the single 3840x1080 mode
- **THEN** the setter is sent once and the output becomes the single 3840x1080 mode

#### Scenario: Already in full side-by-side

- **WHEN** the glasses are already in full side-by-side
- **THEN** nothing is sent

#### Scenario: Setter rejected or unanswered

- **WHEN** the setter is rejected or receives no reply
- **THEN** the system reports the failure and does not retry

### Requirement: Record and restore the previous mode

Before switching to full side-by-side the system SHALL record the mode the glasses were in, and on request SHALL restore the previous 2D mode. It SHALL only restore a mode it recorded as 2D.

#### Scenario: Ending a flat-stereo session

- **WHEN** the user ends flat stereo and the recorded mode was a 2D mode
- **THEN** the glasses return to that recorded 2D mode

#### Scenario: Recorded mode was full side-by-side

- **WHEN** the glasses were already in full side-by-side before flat stereo was used
- **THEN** no change is made when flat stereo ends

#### Scenario: Nothing recorded

- **WHEN** a restore is requested with no recorded mode
- **THEN** the system reports that there is nothing to restore

### Requirement: Report the current mode

The system SHALL report whether the glasses are in full side-by-side and the current input mode; when the control port cannot be reached it SHALL say so and exit non-zero.

#### Scenario: Status requested

- **WHEN** the mode is reported while the glasses are reachable
- **THEN** the output states whether full side-by-side is set

#### Scenario: Glasses unreachable

- **WHEN** the mode is reported while the control port cannot be reached
- **THEN** the output says the glasses are unreachable and the command exits non-zero

### Requirement: Re-set full side-by-side after a drop, boundedly

While flat stereo is active, if the glasses drop back to a 2D mode the system SHALL re-set full side-by-side up to a bounded number of times and SHALL report when the bound is reached.

#### Scenario: Drop to 2D during flat stereo

- **WHEN** an active flat-stereo session sees the glasses fall back to a 2D mode
- **THEN** full side-by-side is set again and the drop is reported

#### Scenario: The glasses keep reverting

- **WHEN** the glasses return to 2D on more occasions than the bound allows
- **THEN** the system reports the limit and stops re-setting

### Requirement: Does not fight a running SteamVR session

When a SteamVR session owns the glasses, the flat-stereo command SHALL refuse to change the mode so that it cannot conflict with the presenter, and SHALL say why.

#### Scenario: SteamVR is running

- **WHEN** a flat-stereo command is run while a SteamVR session owns the glasses
- **THEN** it reports that the presenter owns the display mode and sends no setter

### Requirement: Only the allowlisted request

Flat-stereo mode SHALL send no request to the glasses other than the allowlisted `NRDpSetInputMode` request with value 0 or 1.

#### Scenario: Any other request

- **WHEN** code attempts to send any other request through the flat-stereo path
- **THEN** the control-port client refuses and nothing is sent

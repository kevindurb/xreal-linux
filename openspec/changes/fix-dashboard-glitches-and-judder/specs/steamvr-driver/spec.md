## MODIFIED Requirements

### Requirement: Vsync follows the presenter

The driver SHALL declare each SteamVR vsync 2 ms (a running start) before the glasses' next real vblank, as timed from the presenter's vblank reports, and SHALL fall back to a timer at the display frequency only when the presenter has not reported one for 100 ms. `PostPresent` SHALL NOT hold SteamVR by default. With `driver_xreal.hold_after_present` set to true, `PostPresent` SHALL hold SteamVR until the next running start, or for at most `driver_xreal.hold_max_ms` milliseconds when that is set. The advertised vsync-to-photons time SHALL be the running start plus one refresh unless `driver_xreal.seconds_from_vsync_to_photons` is set.

#### Scenario: Presenter reports vblanks

- **WHEN** the presenter reports vblank times
- **THEN** each vsync event is declared about 2 ms before the following vblank

#### Scenario: Presenter stops

- **WHEN** the presenter stops sending vsync notifications
- **THEN** within 100 ms the driver resumes timer-driven vsync so SteamVR keeps running

#### Scenario: Default pacing

- **WHEN** `driver_xreal.hold_after_present` is not set
- **THEN** `PostPresent` returns without waiting and the driver log says "hold after present off"

#### Scenario: Capped hold for diagnosis

- **WHEN** `driver_xreal.hold_after_present` is true and `driver_xreal.hold_max_ms` is 4
- **THEN** `PostPresent` never blocks SteamVR for more than 4 ms, and the driver log reports the cap

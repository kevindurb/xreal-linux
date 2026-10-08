## MODIFIED Requirements

### Requirement: Display geometry

The driver SHALL advertise a per-eye render size (default 1920x1080, the panel's size, overridable with `driver_xreal.render_width` and `render_height`), a 3840x1080 window split into two eye viewports, a display frequency (default 60 Hz), and a symmetric field of view that matches the presenter's reprojection constants.

#### Scenario: Lower render size configured

- **WHEN** `driver_xreal.render_width` and `render_height` are set to 1280 and 720
- **THEN** SteamVR's recommended render target size follows

### Requirement: Vsync follows the presenter

The driver SHALL declare each SteamVR vsync a running start before the glasses' next real vblank, as timed from the presenter's vblank reports, and SHALL fall back to a timer at the display frequency only when the presenter has not reported one for 100 ms. The running start SHALL default to 8 ms and be settable with `driver_xreal.running_start_ms`, because with 2 ms SteamVR's dashboard showed single bad frames. With `driver_xreal.hold_after_present` (default true), `PostPresent` SHALL hold SteamVR until the next running start, or for at most `driver_xreal.hold_max_ms` milliseconds when that is set. The advertised vsync-to-photons time SHALL be the running start plus one refresh unless `driver_xreal.seconds_from_vsync_to_photons` is set.

#### Scenario: Presenter reports vblanks

- **WHEN** the presenter reports vblank times
- **THEN** each vsync event is declared about 8 ms before the following vblank

#### Scenario: Presenter stops

- **WHEN** the presenter stops sending vsync notifications
- **THEN** within 100 ms the driver resumes timer-driven vsync so SteamVR keeps running

#### Scenario: Default pacing

- **WHEN** neither `driver_xreal.hold_after_present` nor `driver_xreal.running_start_ms` is set
- **THEN** the driver log says "hold after present on" and "running start 8.0 ms"

#### Scenario: Capped hold for diagnosis

- **WHEN** `driver_xreal.hold_after_present` is true and `driver_xreal.hold_max_ms` is 4
- **THEN** `PostPresent` never blocks SteamVR for more than 4 ms, and the driver log reports the cap

#### Scenario: Dashboard sweep with Home on

- **WHEN** the dashboard is open, the simulated head sweeps across its toolbar and the default pacing is used
- **THEN** a 480-frame `--dump` capture contains no bad frames, measured with `tools/find_bad_frames.py`

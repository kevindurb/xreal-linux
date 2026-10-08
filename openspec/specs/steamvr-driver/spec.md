# steamvr-driver Specification

## Purpose
Present the glasses to SteamVR as a headset using driver direct mode, so SteamVR needs no DRM lease of the display. Implemented in `driver/src/xreal_driver.cpp`.

## Requirements

### Requirement: Headset in driver direct mode

The driver SHALL register an HMD named "XREAL 1S" that advertises a display component and a driver direct-mode component and sends its own vsync events, so that SteamVR's compositor starts without a DRM-lease display.

#### Scenario: SteamVR starts with the driver registered

- **WHEN** SteamVR starts with the driver registered and forced as the active driver
- **THEN** the compositor reports "Headset is using driver direct mode" and renders

### Requirement: Display geometry

The driver SHALL advertise a per-eye render size (default 1920x1080, the panel's size, overridable with `driver_xreal.render_width` and `render_height`), a 3840x1080 window split into two eye viewports, a display frequency (default 60 Hz), and a symmetric field of view (about 42 degrees horizontal by 25 degrees vertical per eye, from the glasses' factory display calibration) that matches the presenter's reprojection constants.

#### Scenario: Lower render size configured

- **WHEN** `driver_xreal.render_width` and `render_height` are set to 1280 and 720
- **THEN** SteamVR's recommended render target size follows

### Requirement: Swap textures from SteamVR

The driver SHALL allocate swap-texture sets (three images each) through SteamVR's IPC resource manager, obtain a file descriptor for each image, forward the sets to the presenter, resend all existing sets when a presenter (re)connects, and tell the presenter when a set is destroyed.

#### Scenario: Presenter starts after SteamVR

- **WHEN** the presenter connects while sets already exist
- **THEN** every existing set is sent to it

### Requirement: Never hand SteamVR an image the presenter may read

Once the presenter has reported a frame it is using, the driver SHALL NOT return from `GetNextSwapTextureSetIndex` an image of a frame it sent the presenter until the presenter reports using a newer frame. If every image is held for about a refresh and a half, the driver SHALL reclaim the oldest so SteamVR keeps running.

#### Scenario: Presenter one frame behind

- **WHEN** the presenter is still using frame N and frame N+1 has been sent
- **THEN** SteamVR's next image is the set's third image, not N's or N+1's

#### Scenario: Presenter stalls

- **WHEN** the presenter reports no newer frame while all three images are held
- **THEN** after about a refresh and a half the driver hands out the oldest frame's image and logs it

### Requirement: Forward every presented frame

On each `Present` the driver SHALL send the presenter the left and right set and slot of layer 0, the head orientation SteamVR rendered the frame for (from the layer's pose), and each eye's valid texture bounds.

#### Scenario: Frame presented

- **WHEN** SteamVR calls `Present` with one layer
- **THEN** the presenter receives one message identifying both eyes' textures, the render pose and the bounds

### Requirement: Report the tracked pose

The driver SHALL report to SteamVR each new orientation and angular velocity it receives from the presenter, stamped with its age (`poseTimeOffset`) from the host time the sample arrived, placed at a configurable head height (`driver_xreal.head_height`, default 1.5 m), and SHALL fall back to an untracked identity pose when no presenter pose is available. Angular velocity and the head model are controllable with `send_angular_velocity` and `head_model` for diagnosis.

#### Scenario: Presenter tracking

- **WHEN** the presenter sends valid poses
- **THEN** SteamVR's head pose follows the glasses' orientation at the configured height, updated only when a new sample arrives (and at least every 100 ms)

#### Scenario: No presenter

- **WHEN** no presenter is connected
- **THEN** the head pose is the untracked identity orientation

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

### Requirement: Loadable inside SteamVR

The driver SHALL be built with a statically linked C++ runtime and no dependencies beyond libc, libm and libpthread so it loads inside SteamVR's older runtime.

#### Scenario: Library dependencies

- **WHEN** the built library's dynamic dependencies are listed
- **THEN** they contain only libm and libc

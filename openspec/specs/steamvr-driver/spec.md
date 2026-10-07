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

The driver SHALL advertise a per-eye render size (default 1920x1080, overridable with `driver_xreal.render_width` and `render_height`), a 3840x1080 window split into two eye viewports, a display frequency (default 60 Hz), and a symmetric field of view that matches the presenter's reprojection constants.

#### Scenario: Lower render size configured

- **WHEN** `driver_xreal.render_width` and `render_height` are set to 1280 and 720
- **THEN** SteamVR's recommended render target size follows

### Requirement: Swap textures from SteamVR

The driver SHALL allocate swap-texture sets (three images each) through SteamVR's IPC resource manager, obtain a file descriptor for each image, forward the sets to the presenter, resend all existing sets when a presenter (re)connects, and tell the presenter when a set is destroyed.

#### Scenario: Presenter starts after SteamVR

- **WHEN** the presenter connects while sets already exist
- **THEN** every existing set is sent to it

### Requirement: Forward every presented frame

On each `Present` the driver SHALL send the presenter the left and right set and slot of layer 0, the head orientation SteamVR rendered the frame for (from the layer's pose), and each eye's valid texture bounds.

#### Scenario: Frame presented

- **WHEN** SteamVR calls `Present` with one layer
- **THEN** the presenter receives one message identifying both eyes' textures, the render pose and the bounds

### Requirement: Report the tracked pose

The driver SHALL report to SteamVR the orientation and angular velocity it receives from the presenter, placed at a configurable head height (`driver_xreal.head_height`, default 1.5 m), and SHALL fall back to an untracked identity pose when no presenter pose is available. Angular velocity and the head model are controllable with `send_angular_velocity` and `head_model` for diagnosis.

#### Scenario: Presenter tracking

- **WHEN** the presenter sends valid poses
- **THEN** SteamVR's head pose follows the glasses' orientation at the configured height

#### Scenario: No presenter

- **WHEN** no presenter is connected
- **THEN** the head pose is the untracked identity orientation

### Requirement: Vsync follows the presenter

The driver SHALL emit SteamVR vsync events when the presenter reports a display refresh and SHALL fall back to a timer at the display frequency only when the presenter has not reported one for 100 ms.

#### Scenario: Presenter stops

- **WHEN** the presenter stops sending vsync notifications
- **THEN** within 100 ms the driver resumes timer-driven vsync so SteamVR keeps running

### Requirement: Loadable inside SteamVR

The driver SHALL be built with a statically linked C++ runtime and no dependencies beyond libc, libm and libpthread so it loads inside SteamVR's older runtime.

#### Scenario: Library dependencies

- **WHEN** the built library's dynamic dependencies are listed
- **THEN** they contain only libm and libc

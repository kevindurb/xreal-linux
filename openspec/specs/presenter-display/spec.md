# presenter-display Specification

## Purpose
Show SteamVR's frames on the glasses. The presenter owns the glasses' display, imports SteamVR's per-eye textures and presents them side by side. Implemented in `presenter/src/main.rs` and `presenter/shaders`.

## Requirements

### Requirement: Fullscreen on the glasses' output

The presenter SHALL run fullscreen on the glasses' output (default `DP-1`, selectable with `--monitor`) and SHALL put its window back on that output within about half a second if the compositor places it elsewhere, for example after the glasses re-plug.

#### Scenario: Glasses re-plug after a mode change

- **WHEN** the glasses' output disappears and returns
- **THEN** the presenter moves its window back to the glasses' output once it exists again

### Requirement: Side-by-side stereo layout

The presenter SHALL draw the left eye's image into the left half and the right eye's image into the right half of a swapchain that fills the glasses' output, and SHALL be used with the glasses in full SBS (a single 3840x1080 mode).

#### Scenario: Full SBS

- **WHEN** the glasses are in full SBS and SteamVR is presenting
- **THEN** each eye sees only its own half of the picture

### Requirement: Test pattern when nothing is presenting

The presenter SHALL draw a side-by-side stereo test pattern whenever no driver is connected or the driver is not presenting a usable frame, and SHALL count such frames while a driver is connected so they can be monitored.

#### Scenario: No driver

- **WHEN** the presenter runs without SteamVR
- **THEN** the left half shows a red-tinted pattern and the right half a blue-tinted one, with a moving square offset between the eyes

### Requirement: Import SteamVR's textures

The presenter SHALL receive each texture set's file descriptors from the driver and import them into its own Vulkan device as opaque-fd external memory, using SteamVR's exact image parameters (format, size, usage, mutable-format flag, optimal tiling) on the same GPU, importing lazily on first use and releasing sets when the driver destroys them.

#### Scenario: Driver connects after the presenter

- **WHEN** SteamVR starts while the presenter is already running
- **THEN** the presenter imports the sets the first presented frame refers to and shows that frame

#### Scenario: Presenter restarted while SteamVR runs

- **WHEN** the presenter is restarted
- **THEN** the driver reconnects, resends every existing set, and presentation resumes

### Requirement: Honour the valid region of each layer

The presenter SHALL sample only the valid bounds SteamVR reports for each eye's texture, in both the plain copy and the reprojection pass.

#### Scenario: Partial bounds

- **WHEN** SteamVR reports bounds smaller than the full texture
- **THEN** only that region is shown, scaled to fill the eye's half

### Requirement: Do not show a frame SteamVR may still be drawing

The presenter SHALL show the most recently presented frame, chosen after the swapchain image is acquired, and SHALL NOT read it until SteamVR's GPU work writing it has finished (waited on the GPU through the dma-buf's exported sync file, or on the CPU for up to 25 ms where the GPU cannot import one).

#### Scenario: Frame still being written

- **WHEN** SteamVR has presented a frame whose GPU work has not finished
- **THEN** the presenter's read of that frame starts only after the work finishes

#### Scenario: Older frames are never shown

- **WHEN** a newer frame has been presented
- **THEN** the presenter does not read an older swap image, which SteamVR may already be redrawing

### Requirement: Optional rotational reprojection

With `--reproject`, the presenter SHALL warp each eye by the head rotation between the pose SteamVR rendered the frame for and the latest tracked pose, using the field of view the driver advertises, and SHALL fall back to the plain copy when either pose is missing. It SHALL correct rotation only. It assumes SteamVR's universe origin has not been rotated by a recentre.

#### Scenario: Head turns after the frame was rendered

- **WHEN** the head has turned since SteamVR rendered a frame
- **THEN** the frame is shifted by the corresponding angle before it is shown

#### Scenario: No tracked pose

- **WHEN** the pose is marked invalid
- **THEN** the frame is shown without reprojection

### Requirement: Drive SteamVR's vsync from the real display

The presenter SHALL tell the driver about each display refresh: with present wait, the time its previous frame reached the display (keeping one frame queued); otherwise when the swapchain returns an image. After repeated present-wait timeouts it SHALL fall back to the acquire estimate and SHALL try present wait again at least every 5 s, so a startup stall does not disable it for the rest of the run. It SHALL also report the SteamVR frame it reads from, so the driver knows which older frames it has released.

#### Scenario: Steady state

- **WHEN** SteamVR is running with the presenter connected
- **THEN** SteamVR's `Present` count advances at the display's refresh rate

#### Scenario: Present wait stalls while SteamVR starts

- **WHEN** the presenter and SteamVR start together and present wait times out three times in a row
- **THEN** the presenter logs the fallback, uses the acquire estimate, and within 5 s logs that present wait is in use again once it stops timing out

### Requirement: Local-user-only driver link

The presenter SHALL listen on the abstract unix socket `@xreal-presenter-<uid>` (reachable from inside SteamVR's container) and SHALL accept connections only from processes running as the same user.

#### Scenario: Another user connects

- **WHEN** a process of a different user connects to the socket
- **THEN** the presenter rejects it

### Requirement: Debug capture of what the glasses show

The presenter SHALL provide `--dump DIR` to write the centre crop of each eye's half of the displayed image, with per-frame metadata, on request (by creating `DIR/trigger`). The copy SHALL be part of the frame's own GPU submission and the files SHALL be written off the render thread, so capturing does not change the frame timing being captured.

#### Scenario: Capturing a glitch

- **WHEN** `DIR/trigger` is created
- **THEN** the next N frames are written for both eyes with a metadata row each

### Requirement: Report motion smoothness

The presenter's periodic report SHALL include, for the new SteamVR frames shown since the last report, the median step of SteamVR's render pose between consecutive frames, and how many steps were near repeats (under a quarter of the median) and double steps (over 1.75 times the median), so judder can be measured without a person wearing the glasses.

#### Scenario: Smooth simulated pan

- **WHEN** the presenter runs with `--sim-pose --sim-yaw 40` and SteamVR renders every refresh for evenly spaced poses
- **THEN** each report shows a non-zero median step and near-repeat and double-step counts close to zero, apart from the pan's turnarounds

### Requirement: Reprojection hides missed SteamVR frames

With `--reproject` the presenter SHALL show, on every refresh, the newest SteamVR frame warped to the current head pose, so that when SteamVR misses a frame the displayed motion stays even. Measured with `tools/judder_report.py` on a steady simulated turn, the share of refreshes that do not move SHALL be lower, and the share that move twice as far SHALL be lower, than without `--reproject`.

#### Scenario: Steady simulated turn with the pacing hold on

- **WHEN** the presenter runs with `--reproject --sim-pose --sim-yaw 40` and `--dump` captures 480 refreshes
- **THEN** `tools/judder_report.py` reports fewer double shifts than the same capture without `--reproject`

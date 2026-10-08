# presenter-display Specification

## Purpose
Show SteamVR's frames on the glasses. The presenter owns the glasses' display, imports SteamVR's per-eye textures and presents them side by side. Implemented in `presenter/src/main.rs` and `presenter/shaders`.

## Requirements

### Requirement: Fullscreen on the glasses' output

The presenter SHALL identify the glasses' output by its EDID (manufacturer `MRG`, product `0x4102`) and run fullscreen only on it, with `--monitor` as an override. The window SHALL exist only while the driver is connected and the glasses' output exists in full SBS; when that output disappears (a mode switch, sleep or unplug) the window SHALL be destroyed or hidden and SHALL NOT be left fullscreen on any other output, and when the output returns the window SHALL be recreated on it within about a second. Where a window is placed elsewhere by the compositor, the presenter SHALL put it back by leaving fullscreen and re-entering it on alternate attempts (asking for the same fullscreen output again did not move it).

#### Scenario: Glasses re-plug after a mode change

- **WHEN** the glasses' output disappears and returns during a session
- **THEN** no presenter window is shown on any other output meanwhile, and the presenter shows its window on the glasses' output once it exists again

#### Scenario: A second monitor is attached

- **WHEN** another monitor is attached and the glasses are in full SBS
- **THEN** the presenter still chooses the glasses' output without `--monitor`

#### Scenario: Glasses absent

- **WHEN** the driver connects and the glasses' output does not exist
- **THEN** no window is created on another output and the presenter reports that the glasses are absent

### Requirement: Side-by-side stereo layout

The presenter SHALL draw the left eye's image into the left half and the right eye's image into the right half of a swapchain that fills the glasses' output, and SHALL be used with the glasses in full SBS (a single 3840x1080 mode).

#### Scenario: Full SBS

- **WHEN** the glasses are in full SBS and SteamVR is presenting
- **THEN** each eye sees only its own half of the picture

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

The presenter SHALL listen on the abstract unix socket `@xreal-presenter-<uid>` (reachable from inside SteamVR's container), either one it binds itself or one handed over by systemd, and SHALL accept connections only from processes running as the same user.

#### Scenario: Another user connects

- **WHEN** a process of a different user connects to the socket
- **THEN** the presenter rejects it

#### Scenario: Socket handed over by systemd

- **WHEN** the presenter is started by the socket unit with the listening socket passed in
- **THEN** it serves the driver's queued connection on that socket and applies the same same-user check

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

### Requirement: Test grid

The presenter SHALL, only when started with `--test-grid`, draw a straight-line grid (lines 120 picture pixels apart, a frame, a centre cross, and in the four corners a ruler with a tick every 20 rows measured from the top and bottom edges) instead of the eye images, also when SteamVR is not connected, so the glasses' visible area can be judged without SteamVR.

#### Scenario: Counting visible rows

- **WHEN** the presenter runs with `--test-grid` on glasses in full SBS
- **THEN** each eye shows the grid, and the wearer can count how many corner ticks at the top and bottom edges are visible

### Requirement: Optional per-eye display rotation

The presenter SHALL, only when started with `--eye-rotation` (or `--eye-rotation-reversed`), rotate each eye's sampling rays by that display's factory orientation relative to the other display, split evenly between the eyes, in the same pass as reprojection (identity head rotation when `--reproject` is off). Without the flag the eye images are drawn as SteamVR rendered them. The sign convention of the factory quaternions is not verified on hardware; `--eye-rotation-reversed` reads them the other way for comparison.

#### Scenario: Factory orientations of the tested unit

- **WHEN** `--eye-rotation` is set and the left display is turned 0.84 degrees about the vertical axis and the right display 0.03 degrees the other way
- **THEN** the two eyes' images are shifted by about 0.44 degrees (about 19 px at the panel's focal length) in opposite directions

#### Scenario: Flag absent

- **WHEN** neither flag is given
- **THEN** the rotation matrices are the identity and nothing moves

### Requirement: Log display-mode changes with the control events

The presenter SHALL log every change of the glasses' connector mode list as `[display +T s] modes: ...`, on the same clock as the `[control +T s]` event lines, so that `tools/analyze_control_events.py` can list the events that preceded each drop from full side-by-side (a single `3840x1080` mode) to 2D.

#### Scenario: The glasses drop to 2D

- **WHEN** the mode list stops being exactly `3840x1080`
- **THEN** a `[display ...]` line is logged and the analyser reports the control events in the 30 s before it

### Requirement: Stereo test pattern

The presenter SHALL, only when started with `--test-pattern`, draw the side-by-side stereo test pattern instead of the eye images and instead of the splash, also when SteamVR is not connected: the left half tinted red and the right half tinted blue, white borders and a centre line marking the two halves, a cross-hair at the centre of each eye, and a square that slides across each half with a different horizontal offset in each eye. Without the flag the presenter SHALL never draw it.

#### Scenario: Checking per-eye stereo

- **WHEN** the presenter runs with `--test-pattern` on glasses in full SBS, with or without SteamVR
- **THEN** the left eye sees only the red half, the right eye sees only the blue half, and the sliding square appears at a depth offset from the frame

#### Scenario: Default run

- **WHEN** the presenter runs without `--test-pattern`
- **THEN** no red or blue tinted frame and no sliding square is ever shown, whether or not SteamVR is presenting

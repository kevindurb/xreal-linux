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

The presenter SHALL tell the driver about each display refresh: with present wait, the time its previous frame reached the display (keeping one frame queued); otherwise when the swapchain returns an image. It SHALL also report the SteamVR frame it reads from, so the driver knows which older frames it has released.

#### Scenario: Steady state

- **WHEN** SteamVR is running with the presenter connected
- **THEN** SteamVR's `Present` count advances at the display's refresh rate

### Requirement: Local-user-only driver link

The presenter SHALL listen on the abstract unix socket `@xreal-presenter-<uid>` (reachable from inside SteamVR's container) and SHALL accept connections only from processes running as the same user.

#### Scenario: Another user connects

- **WHEN** a process of a different user connects to the socket
- **THEN** the presenter rejects it

### Requirement: Debug capture and fence inspection

The presenter SHALL provide `--dump DIR` to write the centre crop of both eyes' source images with per-frame metadata on request (by creating `DIR/trigger`), waiting for the writer's dma-buf fence before reading each dumped frame.

#### Scenario: Capturing a glitch

- **WHEN** `DIR/trigger` is created
- **THEN** the next N frames are written for both eyes with a metadata row each

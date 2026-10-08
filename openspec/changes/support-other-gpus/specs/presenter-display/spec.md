## MODIFIED Requirements

### Requirement: Import SteamVR's textures

The presenter SHALL receive each texture set's file descriptors from the driver and import them into its own Vulkan device as opaque-fd external memory, using SteamVR's exact image parameters (format, size, usage, mutable-format flag, optimal tiling) on the GPU SteamVR renders on, determined by device identity, importing lazily on first use and releasing sets when the driver destroys them.

#### Scenario: Driver connects after the presenter

- **WHEN** SteamVR starts while the presenter is already running
- **THEN** the presenter imports the sets the first presented frame refers to and shows that frame

#### Scenario: Presenter restarted while SteamVR runs

- **WHEN** the presenter is restarted
- **THEN** the driver reconnects, resends every existing set, and presentation resumes

### Requirement: Do not show a frame SteamVR may still be drawing

The presenter SHALL show the most recently presented frame, chosen after the swapchain image is acquired, and SHALL NOT read it until SteamVR's GPU work writing it has finished (waited on the GPU through the dma-buf's exported sync file, or on the CPU for up to 25 ms where the GPU cannot import one, or by the documented fallback wait where no sync file can be exported). The wait SHALL never be skipped silently.

#### Scenario: Frame still being written

- **WHEN** SteamVR has presented a frame whose GPU work has not finished
- **THEN** the presenter's read of that frame starts only after the work finishes

#### Scenario: Older frames are never shown

- **WHEN** a newer frame has been presented
- **THEN** the presenter does not read an older swap image, which SteamVR may already be redrawing

#### Scenario: No sync file can be exported

- **WHEN** the sync-file export fails for a swap image
- **THEN** the fallback wait is used and the log says so once

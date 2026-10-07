## MODIFIED Requirements

### Requirement: Drive SteamVR's vsync from the real display

The presenter SHALL tell the driver about each display refresh: with present wait, the time its previous frame reached the display (keeping one frame queued); otherwise when the swapchain returns an image. After repeated present-wait timeouts it SHALL fall back to the acquire estimate and SHALL try present wait again at least every 5 s, so a startup stall does not disable it for the rest of the run. It SHALL also report the SteamVR frame it reads from, so the driver knows which older frames it has released.

#### Scenario: Steady state

- **WHEN** SteamVR is running with the presenter connected
- **THEN** SteamVR's `Present` count advances at the display's refresh rate

#### Scenario: Present wait stalls while SteamVR starts

- **WHEN** the presenter and SteamVR start together and present wait times out three times in a row
- **THEN** the presenter logs the fallback, uses the acquire estimate, and within 5 s logs that present wait is in use again once it stops timing out

## ADDED Requirements

### Requirement: Report motion smoothness

The presenter's periodic report SHALL include, for the new SteamVR frames shown since the last report, the median step of SteamVR's render pose between consecutive frames, and how many steps were near repeats (under a quarter of the median) and double steps (over 1.75 times the median), so judder can be measured without a person wearing the glasses.

#### Scenario: Smooth simulated pan

- **WHEN** the presenter runs with `--sim-pose --sim-yaw 40` and SteamVR renders every refresh for evenly spaced poses
- **THEN** each report shows a non-zero median step and near-repeat and double-step counts close to zero, apart from the pan's turnarounds

### Requirement: Hide SteamVR's bad frames when no upstream fix works

If neither the compositor sync layer nor `steamvr.enableLinuxVulkanAsync` removes SteamVR's dashboard bad frames, the presenter SHALL provide `--filter-bad-frames`. With it, a new SteamVR frame that differs sharply from the previously shown frame SHALL be held back for one refresh, with the previous frame shown again in its place. A frame that is still different on the next refresh SHALL be shown, so a real change is delayed by at most one refresh. While a frame is held back, the presenter SHALL NOT release the previously shown frame's image to SteamVR.

#### Scenario: Isolated bad frame during a pointer sweep

- **WHEN** `--filter-bad-frames` is on, the glasses are in full SBS, and SteamVR composites one bad frame (scene missing in one eye) between two normal ones
- **THEN** the glasses show the previous normal frame instead, and a `--dump` capture of the sweep contains no bad frames

#### Scenario: Real scene change

- **WHEN** `--filter-bad-frames` is on and the dashboard opens, changing the image for good
- **THEN** the new image appears one refresh later than without the filter

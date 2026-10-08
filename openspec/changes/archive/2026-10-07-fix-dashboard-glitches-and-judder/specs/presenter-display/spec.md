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

### Requirement: Reprojection hides missed SteamVR frames

With `--reproject` the presenter SHALL show, on every refresh, the newest SteamVR frame warped to the current head pose, so that when SteamVR misses a frame the displayed motion stays even. Measured with `tools/judder_report.py` on a steady simulated turn, the share of refreshes that do not move SHALL be lower, and the share that move twice as far SHALL be lower, than without `--reproject`.

#### Scenario: Steady simulated turn with the pacing hold on

- **WHEN** the presenter runs with `--reproject --sim-pose --sim-yaw 40` and `--dump` captures 480 refreshes
- **THEN** `tools/judder_report.py` reports fewer double shifts than the same capture without `--reproject`

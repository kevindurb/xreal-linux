## ADDED Requirements

### Requirement: Workaround for SteamVR's compositor reusing in-flight GPU work

While SteamVR-for-Linux #952 is unfixed, the setup SHALL include the measured workaround that removes the dashboard bad frames without the driver's PostPresent hold. That is the `VK_LAYER_STEAMVR_compositor_sync` Vulkan layer built from reviewed source, or `steamvr.enableLinuxVulkanAsync` if that alone is enough. `tools/doctor.sh` SHALL report whether the chosen workaround is active, warning, not failing, when it is missing.

#### Scenario: Layer active

- **WHEN** SteamVR has started with the layer installed
- **THEN** `vrcompositor-linux.txt` contains the layer's "active in vrcompositor" line and `tools/doctor.sh` reports the workaround as active

#### Scenario: Workaround missing

- **WHEN** neither the layer nor the async setting is in place
- **THEN** `tools/doctor.sh` warns that dashboard frames may glitch unless `driver_xreal.hold_after_present` is true, which causes judder

### Requirement: Lightweight environment option

The setup guidance SHALL document running SteamVR without Home (`steamvr.enableHomeApp` false) with a compositor background such as `<SteamVR>/resources/backgrounds/aurorasky.png` in `steamvr.background`. On the Steam Deck this is the configuration that measured the best frame rate and responsiveness.

#### Scenario: Home off with the aurora background

- **WHEN** `steamvr.enableHomeApp` is false and `steamvr.background` names `aurorasky.png`
- **THEN** SteamVR shows the aurora sky and grid floor around the dashboard instead of the Home room

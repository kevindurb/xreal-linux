## ADDED Requirements

### Requirement: Use the GPU SteamVR renders on

The presenter SHALL use the same GPU that SteamVR renders the swap textures on, identified by device identity (UUID or LUID), and SHALL NOT pick a device merely because it can present to its window. It SHALL log the device it chose and why.

#### Scenario: One GPU

- **WHEN** the machine has a single GPU
- **THEN** the presenter uses it and logs its name and identity

#### Scenario: Two GPUs, SteamVR on the discrete one

- **WHEN** the machine has two GPUs, the glasses' output is on one and SteamVR renders on the other
- **THEN** the presenter uses SteamVR's GPU for import and says how it reaches the glasses' output

### Requirement: Say when the GPUs do not match

When the GPU SteamVR renders on cannot be determined or cannot present to the glasses' output, the presenter SHALL say so in its log and in `xreal-setup`/doctor output, SHALL show its test pattern instead of garbage, and SHALL name the override that selects a device by hand.

#### Scenario: Cannot reach the output from SteamVR's GPU

- **WHEN** SteamVR's GPU cannot present to the glasses' output
- **THEN** the presenter logs which GPU each side uses and that cross-GPU presentation is not supported, and shows the test pattern

### Requirement: Manual override

The presenter SHALL accept an explicit device choice (by name or UUID) that takes precedence over automatic selection, and SHALL log that the choice was manual.

#### Scenario: Override given

- **WHEN** the user passes a device identity
- **THEN** that device is used and the log says it was selected manually

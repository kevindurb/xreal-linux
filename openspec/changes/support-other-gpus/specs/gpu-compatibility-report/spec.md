## ADDED Requirements

### Requirement: Compatibility report

A command SHALL produce a text report listing the GPU(s) with vendor, driver and version, the Vulkan extensions the presenter needs and whether each is present, whether the swap-image import worked, which synchronisation method and vblank source were active, the result of a short sweep capture, and the glasses' display mode. It SHALL contain no personal data, no captured images and no paths beyond the tool's own.

#### Scenario: Running the report

- **WHEN** the user runs it with the glasses and SteamVR running
- **THEN** it prints a report that can be pasted into an issue

#### Scenario: Missing prerequisites

- **WHEN** SteamVR is not running or the glasses are absent
- **THEN** it reports which parts it could not test and why instead of failing

### Requirement: Supported hardware table

The documentation SHALL keep a table of GPUs tested with their report outcome, and a GPU SHALL be listed as supported only when its report shows the import, a defined synchronisation method and a sweep capture within a stated bad-frame threshold.

#### Scenario: A tested GPU

- **WHEN** a report meets the criteria
- **THEN** the GPU appears in the table as supported with the date and release

#### Scenario: A GPU that fails

- **WHEN** a report shows the import or synchronisation failing
- **THEN** the table lists it as not working with the reason

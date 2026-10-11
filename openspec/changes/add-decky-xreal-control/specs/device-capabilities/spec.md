# Spec Delta

## Purpose

Report which XREAL device is connected and which controls it supports in a machine-readable form, so a generic front-end can render itself from the device's capabilities and further XREAL models can be added without changing the front-end.

## ADDED Requirements

### Requirement: Report the connected device

The runtime SHALL report whether an XREAL device is connected, the model it identified, and how it was identified, and SHALL report clearly when no device is connected.

#### Scenario: A supported device is connected

- **WHEN** an XREAL 1S is connected
- **THEN** the report names the connected device as the 1S

#### Scenario: No device

- **WHEN** no XREAL device is reachable
- **THEN** the report says no device is connected and exits non-zero

### Requirement: Report the controls a device supports

The report SHALL name the controls supported for the connected model, so a front-end renders only controls the device supports and does not offer behavior the device cannot do.

#### Scenario: A supported device

- **WHEN** the connected device is the 1S
- **THEN** the report includes the controls the 1S supports, including the head-mouse and SteamVR modes

#### Scenario: An unsupported model

- **WHEN** a device is recognized as an XREAL model that this build does not support
- **THEN** the report says the model is unsupported rather than reporting invented controls

### Requirement: The report is machine-readable

The runtime SHALL provide the device and capability report as structured data that a program can parse without screen-scraping free text, and SHALL preserve the existing human-readable status for people.

#### Scenario: Parsed by a front-end

- **WHEN** a program requests the structured report
- **THEN** it receives fields naming the connected device, its model, the supported controls and the runtime version

### Requirement: Producing the report reads only

Producing the report SHALL send no state-changing request to the glasses and SHALL work in whatever display mode the glasses are in.

#### Scenario: Report on glasses in a 2D mode

- **WHEN** the report is produced while the glasses are in a 2D mode
- **THEN** the glasses' display mode is unchanged

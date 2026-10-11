# Spec Delta

## Purpose

Expose the XREAL glasses' head orientation to ordinary desktop games as a virtual input device, so head motion can drive a game's look controls without SteamVR.

## ADDED Requirements

### Requirement: Head-look runs without SteamVR or the driver

The head-look service SHALL run on its own — with no SteamVR session, no SteamVR driver and no presenter window — reading the glasses' IMU and emitting input events. It SHALL NOT require the SteamVR driver link and SHALL NOT need root.

#### Scenario: Started with the glasses and no SteamVR

- **WHEN** the head-look service is started while the glasses are connected and SteamVR is not running
- **THEN** it connects to the glasses and a virtual input device for it appears on the system

#### Scenario: Started without the glasses

- **WHEN** the head-look service is started while the glasses are not reachable
- **THEN** it reports that the glasses are not available and emits no head-motion events until they are

### Requirement: Read the glasses' head orientation

The service SHALL obtain the glasses' orientation and angular velocity from the IMU stream the presenter uses (TCP port 52998 of the glasses' link-local address), reconnect on its own when the connection drops, and mark its output as unavailable while the IMU is disconnected.

#### Scenario: Glasses unplugged and plugged back in

- **WHEN** the IMU connection drops and the glasses become reachable again
- **THEN** the service reconnects on its own and head-motion events resume

#### Scenario: IMU disconnected

- **WHEN** the IMU stream is not available
- **THEN** the service emits no head-motion events and reports the loss

### Requirement: Provide a virtual input device

The service SHALL create a userspace virtual input device that other applications see as an ordinary gamepad and/or mouse without root and without a manual udev rule, SHALL give it a recognisable name, and SHALL remove it when the service ends.

#### Scenario: Device appears on start

- **WHEN** the service starts successfully
- **THEN** a virtual input device with the service's documented name is listed among the system's input devices

#### Scenario: Device removed on exit

- **WHEN** the service exits or is terminated
- **THEN** its virtual input device is no longer listed

#### Scenario: No root required

- **WHEN** the service is started by the user who owns the active desktop session
- **THEN** it creates the device without elevation

### Requirement: Selectable mapping model

The service SHALL offer at least a **rate** model and a **stick** model, selectable when it is started, SHALL state which model is active, and SHALL refuse an unknown model by naming the ones it supports.

#### Scenario: Rate model

- **WHEN** the rate model is active and the wearer turns their head at a steady angular rate
- **THEN** the emitted axis moves at a rate proportional to that angular rate and returns to zero when the head stops turning

#### Scenario: Stick model

- **WHEN** the stick model is active and the wearer holds their head at an angle from neutral
- **THEN** the emitted axis is proportional to that angle up to a clamp, and returns to zero when the head returns to neutral

#### Scenario: Unknown model requested

- **WHEN** a model name that the service does not support is requested
- **THEN** the service refuses to start and names the supported models

### Requirement: Deadzone and sensitivity

The service SHALL apply a configurable deadzone so that motions below it (hand tremor and residual gyro bias) produce no output, and a configurable sensitivity, so that a still head does not move the view.

#### Scenario: Head within the deadzone

- **WHEN** the head is still, or moves less than the deadzone
- **THEN** the service emits no head-motion event

#### Scenario: Sensitivity changed

- **WHEN** the sensitivity is changed
- **THEN** the emitted output for the same head motion changes in proportion

### Requirement: Recenter

The service SHALL provide a recenter action that makes the current head pose the neutral direction, after which the emitted output is zero for that pose.

#### Scenario: Recenter while turned

- **WHEN** the wearer is turned to the left and triggers recenter
- **THEN** the emitted output becomes zero at that pose and further output is measured from it

### Requirement: Does not change the glasses

The head-look service SHALL only read from the glasses; it SHALL send no state-changing request to the glasses and SHALL work in whatever display mode the glasses are in.

#### Scenario: Running does not change the display mode

- **WHEN** the head-look service is running
- **THEN** the glasses' display input mode is unchanged throughout

### Requirement: Report status

The service SHALL report, at start and at a low periodic rate, the virtual device name, the active mapping model, whether the glasses' IMU is connected, and the last recenter, so a user can tell that it is running.

#### Scenario: Readable while running

- **WHEN** the service is running
- **THEN** its output names the device and model and states whether the glasses are connected

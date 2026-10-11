# Spec Delta

## ADDED Requirements

### Requirement: Runnable as an opt-in user service

The package SHALL provide a user service that runs the head-look service, disabled by default, so head mouse can be turned on and off as a service instead of a foreground command. When the service is enabled it SHALL run without a terminal, reconnect to the glasses on its own when the IMU connection drops, and remove its virtual input device when it stops. When it is disabled no head-look process and no virtual input device SHALL exist.

#### Scenario: Enabled and the user is logged in

- **WHEN** the head-look service is enabled and the user's session is running
- **THEN** the head-look service runs with no terminal open and its virtual input device is present

#### Scenario: Disabled

- **WHEN** the head-look service is disabled
- **THEN** no head-look process runs and its virtual input device is not present

#### Scenario: IMU drops while the service is enabled

- **WHEN** the glasses become unreachable while the service is running and later return
- **THEN** the service reconnects on its own and head-motion events resume, emitting nothing while the IMU is unavailable

#### Scenario: The service stops

- **WHEN** the head-look service is stopped or disabled
- **THEN** its virtual input device is removed from the system

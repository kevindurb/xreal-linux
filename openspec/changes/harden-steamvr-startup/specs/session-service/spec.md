# Spec Delta

## ADDED Requirements

### Requirement: A late display environment does not kill the presenter

When the presenter is started by the service and neither `WAYLAND_DISPLAY` nor `DISPLAY` is in its environment, it SHALL look for them in the systemd user manager's environment and wait up to a bounded time (10 s) for them to appear, then continue with them. If none appears in that time it SHALL exit with a message that names the missing display and the remedy, rather than failing with a window-system error. It SHALL NOT wait when a display is already present.

#### Scenario: SteamVR launched right after login

- **WHEN** SteamVR connects the driver before the desktop session has put `WAYLAND_DISPLAY` into the user manager's environment, and it appears within the wait
- **THEN** the presenter starts, opens its window and accepts the driver's connection

#### Scenario: Display already present

- **WHEN** the presenter starts with `WAYLAND_DISPLAY` set
- **THEN** it starts at once with no wait

#### Scenario: No display ever appears

- **WHEN** no display variable appears within the wait
- **THEN** the presenter exits with a message saying no display was found and that the desktop session must be running, and it is not restarted in a loop

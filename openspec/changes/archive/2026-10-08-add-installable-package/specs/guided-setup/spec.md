## ADDED Requirements

### Requirement: Ask through whatever dialog is available

The interactive commands (setup, check, fix, status, uninstall) SHALL ask the user for decisions through `kdialog`, then `zenity`, then the terminal, using the first that is available and usable, so a launch from a file manager with no terminal still works. The background service SHALL NOT prompt.

#### Scenario: Launched from a file manager

- **WHEN** the AppImage is started without a terminal on a KDE desktop
- **THEN** setup asks its questions in `kdialog` windows

#### Scenario: No dialog tool

- **WHEN** neither `kdialog` nor `zenity` is installed and there is a terminal
- **THEN** setup asks in the terminal

### Requirement: Check the machine

The check SHALL report, without changing anything, each precondition (glasses on USB, the IMU stream, SteamVR installed, driver present and registered, SteamVR settings, the systemd units, `WAYLAND_DISPLAY` in the user manager's environment, driver and presenter versions, and any recorded display mode that was never restored) as pass, warn or fail with the fix for each, and SHALL exit non-zero when any check fails. It SHALL report the glasses' current display mode but SHALL NOT fail on a 2D mode, because the presenter switches to full SBS when SteamVR starts.

#### Scenario: Everything in place

- **WHEN** all preconditions hold
- **THEN** every line passes and the exit status is zero

#### Scenario: Glasses in a 2D mode

- **WHEN** the glasses are in a normal 2D mode and nothing else is wrong
- **THEN** the check reports the mode, passes, and says the presenter will switch them to full SBS when SteamVR starts

#### Scenario: A pending restore

- **WHEN** a previous display mode is recorded in the state directory and the presenter is not running
- **THEN** the check warns that the glasses were not restored and says how to restore them

### Requirement: Apply safe fixes with consent

`fix` SHALL be able to register the driver with SteamVR and set the SteamVR settings the setup needs, SHALL show each change and ask before applying it (unless told to proceed), SHALL refuse to edit the settings while SteamVR is running, SHALL back up the settings file before the first change, and SHALL record each changed key with its previous value so uninstall restores only those keys. It SHALL NOT need root.

#### Scenario: Settings that need changing

- **WHEN** `forcedDriver` is not `xreal` and SteamVR is stopped
- **THEN** it shows the change, asks, backs up the file, applies it and a second check passes

#### Scenario: SteamVR is running

- **WHEN** SteamVR is running
- **THEN** it refuses to edit the settings and says to stop SteamVR first

#### Scenario: User declines

- **WHEN** the user declines a change
- **THEN** nothing is written and the check still reports it

### Requirement: Tell the user what will happen to the glasses

Before the user agrees to install the units, setup SHALL state that the glasses will switch to full SBS when SteamVR starts using them and back to their previous mode afterwards, that each switch re-plugs the glasses' display for about two seconds so the desktop may rearrange windows, and that nothing is done while SteamVR is not running.

#### Scenario: Consent to the service

- **WHEN** setup offers to enable the background service
- **THEN** the offer includes that statement and the user can decline

### Requirement: Say what only the user can set

For the glasses' own settings that the host can neither read nor write (Follow mode, Stabilizer off, auto-sleep off), setup and the check SHALL list them with where to find each in the glasses' menu and SHALL NOT claim to have verified them. The display mode is not one of them.

#### Scenario: Always listed

- **WHEN** the check runs
- **THEN** the output ends with those three settings as things to confirm by hand

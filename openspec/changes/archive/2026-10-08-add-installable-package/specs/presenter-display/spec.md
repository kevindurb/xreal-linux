## MODIFIED Requirements

### Requirement: Fullscreen on the glasses' output

The presenter SHALL identify the glasses' output by its EDID (manufacturer `MRG`, product `0x4102`) and run fullscreen only on it, with `--monitor` as an override. The window SHALL exist only while the driver is connected and the glasses' output exists in full SBS; when that output disappears (a mode switch, sleep or unplug) the window SHALL be destroyed or hidden and SHALL NOT be left fullscreen on any other output, and when the output returns the window SHALL be recreated on it within about a second. Where a window is placed elsewhere by the compositor, the presenter SHALL put it back by leaving fullscreen and re-entering it on alternate attempts (asking for the same fullscreen output again did not move it).

#### Scenario: Glasses re-plug after a mode change

- **WHEN** the glasses' output disappears and returns during a session
- **THEN** no presenter window is shown on any other output meanwhile, and the presenter shows its window on the glasses' output once it exists again

#### Scenario: A second monitor is attached

- **WHEN** another monitor is attached and the glasses are in full SBS
- **THEN** the presenter still chooses the glasses' output without `--monitor`

#### Scenario: Glasses absent

- **WHEN** the driver connects and the glasses' output does not exist
- **THEN** no window is created on another output and the presenter reports that the glasses are absent

### Requirement: Local-user-only driver link

The presenter SHALL listen on the abstract unix socket `@xreal-presenter-<uid>` (reachable from inside SteamVR's container), either one it binds itself or one handed over by systemd, and SHALL accept connections only from processes running as the same user.

#### Scenario: Another user connects

- **WHEN** a process of a different user connects to the socket
- **THEN** the presenter rejects it

#### Scenario: Socket handed over by systemd

- **WHEN** the presenter is started by the socket unit with the listening socket passed in
- **THEN** it serves the driver's queued connection on that socket and applies the same same-user check

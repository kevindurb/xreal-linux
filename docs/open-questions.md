# Open questions and next steps

## Unverified

- Whether XRLinuxDriver actually works on this 1S. Its source lists the ID (`3318:043e`), but it has not been run here.
- Whether the glasses' connector has the DRM `non-desktop` property, and what a genuine stereo side-by-side mode offers (not yet observed).
- How to get SteamVR to display on the glasses: the connector is not `non-desktop`, so SteamVR's DRM-lease requirement fails
  (see findings.md). Candidate fixes: flag it non-desktop (EDID override), or present frames from our own driver.
- Yaw sign has no independent (accelerometer) check. Axes, units and the pitch/roll signs are measured, see
  [findings.md](findings.md); a repeat run, ideally by another wearer and with the on-glasses stabilizer state noted,
  would firm it up.
- What ports 52990-52995 do (silent in 25 s of recording), and whether any accept commands. Nothing was ever sent to them.
- What exactly each 52996 record marks. Its timestamps are on the same clock as the IMU and camera and it runs at exactly two
  records per camera frame, but it carries no pose. The 52999 status values (about 44-61) are unidentified.
- Whether the on-glasses 6DoF pose exists at all: no stream seen so far carries one, so 6DoF probably has to be computed on the
  host from the camera and IMU.
- Whether the HID interfaces take control commands (brightness, display mode, etc.). Nothing was written to them.
- The Eye stream: true image geometry, why the left half is interleaved, and the pixel packing.
- Whether 6DoF can be done from a single camera view plus the IMU, and what calibration that needs.
- How the Eye is started. It streams only in the glasses' own anchor mode. The vendor SDK has camera start/stop requests
  (`docs/xreal-link-messages.md`), but the port that takes requests, any handshake, and whether Follow mode with the Stabilizer off
  allows the camera are unknown. Plan: `docs/anchor-capture-plan.md` (observation only).
- Whether the magnetometer in the IMU stream (record type 4, 400 Hz, `docs/findings.md`) can bound yaw drift: it needs calibration
  (the factory values may be readable via request 10018) and a check of how its offset changes with display state and load.
- The meaning of the SDK enum values (pixel format, resolution, exposure type, space/scene mode) and the request header
  fields beyond `msg_id` and length.

## Game mode and the head-look service (change `add-decky-xreal-control`)

- Whether the **Decky panel renders in game mode** and whether its run-as-session-user helper reaches the user manager there.
  The helper was exercised by hand over SSH against the Deck's real user manager; the panel itself has not been opened.
- Whether the **opt-in head-look service** survives a reboot enabled and reconnects the IMU on its own, and whether the
  virtual input device appears/disappears cleanly on enable/disable/uninstall. Code and unit text are in; the on-Deck run is pending.
- Whether **install/update from the panel** works end to end. It depends on a release tag that carries
  `xreal-linux-x86_64.AppImage` and a `SHA256SUMS`; that is produced by the new release workflow, which has not run yet.
  The install now runs `setup --yes --no-enable` (all units placed, none enabled), verified locally with a dry run.
- Whether the **selected mode persists** across a reboot (the panel reads unit enabled/active; persistence is the units' own behaviour).
- The **guard** against switching away from SteamVR during a live session is implemented but not exercised on the Deck.

## Suggested order

1. Run an existing 3DoF stack against the 1S on the Deck (patch the USB ID if needed).
2. Choose how to feed it into VR: Monado/OpenXR on the device, or streaming from a PC with WiVRn or ALVR.
3. Return to the camera for positional tracking.

## Environment notes

- The test Deck runs Bazzite (immutable root), so system packages go through `rpm-ostree`, Flatpak, Distrobox or
  Homebrew.
- Grabbing data needs no root on that setup: the `hidraw` nodes were world readable and the TCP ports are open on the
  link-local network.

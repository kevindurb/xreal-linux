# Open questions and next steps

## Unverified

- Whether XRLinuxDriver or `xreal_one_driver` work with the 1S's USB ID (`3318:043e`).
- Yaw sign has no independent (accelerometer) check. Axes, units and the pitch/roll signs are measured, see
  [findings.md](findings.md); a repeat run, ideally by another wearer and with the on-glasses stabilizer state noted,
  would firm it up.
- What ports 52990-52995 and 52999 do, and whether any accept commands. Nothing was ever sent to them.
- What port 52996 carries, and whether its timestamps line up with camera frames.
- Whether the HID interfaces take control commands (brightness, display mode, etc.). Nothing was written to them.
- The Eye stream: true image geometry, why the left half is interleaved, and the pixel packing.
- Whether 6DoF can be done from a single camera view plus the IMU, and what calibration that needs.

## Suggested order

1. Run an existing 3DoF stack against the 1S on the Deck (patch the USB ID if needed).
2. Choose how to feed it into VR: Monado/OpenXR on the device, or streaming from a PC with WiVRn or ALVR.
3. Return to the camera for positional tracking.

## Environment notes

- The test Deck runs Bazzite (immutable root), so system packages go through `rpm-ostree`, Flatpak, Distrobox or
  Homebrew.
- Grabbing data needs no root on that setup: the `hidraw` nodes were world readable and the TCP ports are open on the
  link-local network.

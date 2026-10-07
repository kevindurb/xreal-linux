# Open questions and next steps

## Unverified

- Whether XRLinuxDriver actually works on this 1S. Its source lists the ID (`3318:043e`), but it has not been run here.
- Whether the glasses' connector has the DRM `non-desktop` property, and what a genuine stereo side-by-side mode offers (not yet observed).
- Whether SteamVR can drive the glasses as its headset display on this Deck (SteamVR is not installed yet).
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

## Suggested order

1. Run an existing 3DoF stack against the 1S on the Deck (patch the USB ID if needed).
2. Choose how to feed it into VR: Monado/OpenXR on the device, or streaming from a PC with WiVRn or ALVR.
3. Return to the camera for positional tracking.

## Environment notes

- The test Deck runs Bazzite (immutable root), so system packages go through `rpm-ostree`, Flatpak, Distrobox or
  Homebrew.
- Grabbing data needs no root on that setup: the `hidraw` nodes were world readable and the TCP ports are open on the
  link-local network.

# xreal-linux

Notes and tooling from an effort to make **XREAL 1S** glasses (with the **XREAL Eye** camera) behave like a
SteamVR-style gaming headset on Linux. Test host: Steam Deck running Bazzite 44 (Fedora Atomic / Kinoite).

Status: **exploration / reverse engineering**. Nothing here is a finished driver.

| Piece | Status |
|---|---|
| Video (DisplayPort over USB-C) | Works as a plain monitor, single mode 3840x1080 (side-by-side stereo) |
| Audio | Works (standard USB audio class) |
| Buttons | Work (HID mouse / consumer control) |
| Head tracking, 3DoF (IMU) | Protocol decoded, already implemented upstream (see below). Not yet run end to end on the 1S |
| Eye camera | Stream found and partly decoded, format not fully understood |
| 6DoF / SLAM | Not started |
| SteamVR / OpenXR integration | Not started |

See [docs/findings.md](docs/findings.md) for the details and [docs/open-questions.md](docs/open-questions.md)
for what is unverified.

## Tools

- `tools/decode_imu.py` - read the IMU stream from the glasses and print gyro/accel.
- `tools/decode_camera_frame.py` - split a captured camera frame into its parts and save PNGs.

Both only read from the glasses. Nothing in this repo writes to the glasses.

## Privacy note

Camera captures show the inside of a home, so raw captures and rendered images are deliberately not in this repo
(`captures/` is git-ignored).

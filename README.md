# xreal-linux

Notes and tooling from an effort to make **XREAL 1S** glasses (with the **XREAL Eye** camera) behave like a
SteamVR-style gaming headset on Linux. Test host: Steam Deck running Bazzite 44 (Fedora Atomic / Kinoite).

Status: **exploration / reverse engineering**. Nothing here is a finished driver.

| Piece | Status |
|---|---|
| Video (DisplayPort over USB-C) | Works as a plain monitor in several aspect modes: 16:10 (1920x1200) and 16:9 (1920x1080) at 60, 90 and 120 Hz, and ultrawide (e.g. 3840x1080, 32:9) |
| Audio | Works (standard USB audio class) |
| Buttons | Work (HID mouse / consumer control) |
| Head tracking, 3DoF (IMU) | Protocol decoded, already implemented upstream (see below). Not yet run end to end on the 1S |
| Eye camera | Stream found and partly decoded, format not fully understood |
| 6DoF / SLAM | Not started |
| SteamVR / OpenXR integration | Not started |

## Goal

Make the glasses usable as a SteamVR-style headset on Linux, starting on a Steam Deck and working on any Linux gaming PC.
It should work in two tiers, so people without the XREAL Eye still benefit:

- **3DoF (rotation only), IMU only.** Works with the glasses alone.
- **6DoF (rotation and position), IMU plus the Eye camera.** An optional upgrade when the camera is present.

See [docs/findings.md](docs/findings.md) for the details and [docs/open-questions.md](docs/open-questions.md)
for what is unverified.

## Tools

- `tools/decode_imu.py` - read the IMU stream from the glasses and print gyro/accel.
- `tools/decode_camera_frame.py` - split a captured camera frame into its parts and save PNGs.

- `tools/imu_web/` - a browser page that streams live IMU graphs and walks you through direction tests
  (yaw / pitch / roll) to pin down which gyro axis and sign each head motion is. See below.

All of these only read from the glasses. Nothing in this repo writes to the glasses.

### IMU axis check page

Standard-library Python server plus a static page, no installs needed.

```sh
# on the Deck (after tools/imu_web/deploy.sh copied it over)
python3 ~/xreal-linux/tools/imu_web/server.py            # reads 169.254.1.1:52998
# from your own machine
ssh -L 8765:localhost:8765 steamdeck                     # then open http://localhost:8765/
```

1. Press **Calibrate** and hold still for 3 s (gyro bias is measured and removed).
2. Press **Run all** (or **Run** on one row). For each test you move to the pose and hold still; the page measures the
   angle by integrating the gyro, cross-checks it against the accelerometer's gravity direction, then asks you to
   return to centre and checks that you got back to where you started. Direction and target angle are editable per row.
3. The result table says, for example, "yaw left -> gyro -Y". **Save** writes `captures/axis-map-*.json`.

Other sources for developing without the glasses: `--source replay:captures/imu_yaw.bin` replays a capture, and
`--source sim` generates clearly labelled synthetic data (its axis conventions are invented).

Tests: `node tools/imu_web/test_core.mjs` and `python3 -m unittest tools/imu_web/test_server.py`.
The server binds to 127.0.0.1 by default; `--bind 0.0.0.0` exposes an unauthenticated API that can write into
`captures/`, so avoid it on untrusted networks.

## Privacy note

Camera captures show the inside of a home, so raw captures and rendered images are deliberately not in this repo
(`captures/` is git-ignored).

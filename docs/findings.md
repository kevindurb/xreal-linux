# Findings

Everything below was observed on one XREAL 1S (firmware `bcdDevice` 4.09) plus an XREAL Eye, connected to a
Steam Deck. "Observed" means measured here; "from upstream" means taken from other projects' source or docs and
not independently checked.

## USB device

`3318:043e XREAL XREAL 1S`, USB 2.0 high speed, one configuration, 9 interfaces:

| Interface | Class | Linux driver | Role |
|---|---|---|---|
| 0 | HID (1024-byte interrupt IN/OUT, 34-byte vendor report descriptor) | `usbhid` (`hidraw`) | Likely a control channel. Silent when idle. Nothing was ever written to it |
| 1-2 | CDC NCM | `cdc_ncm` | Virtual Ethernet link |
| 3-4 | CDC ECM | `cdc_ether` | Second virtual Ethernet link |
| 5-7 | USB Audio | `snd-usb-audio` | 48 kHz stereo playback (32-bit) and a 2-channel mic terminal |
| 8 | HID | `usbhid` | Mouse, consumer control, system control (the physical buttons), plus vendor reports 4 and 5 |

No UVC camera and no `/dev/video*` node appears for the Eye. IMU data is not exposed as an input or IIO device.

## Display

The glasses show up as a DRM connector whose mode list depends on the aspect-ratio mode the glasses are set to. The
glasses can act as a 16:9 monitor, a 16:10 monitor, or one of a few ultrawide monitors, and the EDID changes with the
setting.

- **Ultrawide mode** (first session): a single mode, `3840x1080`. That is a **32:9 ultrawide monitor**, not a
  stereo side-by-side image (an earlier version of these notes got this wrong). Its refresh rate was not recorded.
- **16:10 / 16:9 mode** (later session, EDID decoded; manufacturer `MRG`, product `0x4102`, name `XREAL 1S`): six
  detailed timings, `1920x1200` and `1920x1080`, each at 60, 90 and 120 Hz.
- A genuine stereo side-by-side mode is a separate feature (an upstream driver README says holding the brightness-up
  button enables it). It has not been observed here, so its modes and refresh rates are unknown.
- Whether the connector carries the DRM `non-desktop` property was not determined. The EDID decoded here has no
  obvious VR/headset block.

## Network interfaces

Each CDC interface becomes a network interface on the host and the glasses run a DHCP server on both:

| Host interface | Host address | Glasses address |
|---|---|---|
| CDC NCM (`...i1`) | `169.254.2.10/24` | `169.254.2.1` |
| CDC ECM (`...i3`) | `169.254.1.10/24` | `169.254.1.1` |

Ping RTT was about 0.4 ms to `169.254.1.1` and 2-3 ms to `169.254.2.1`. NetworkManager configures both
automatically.

TCP ports **52990-52999** are open on both addresses. What each did when connected to read-only (no data sent):

| Port | Behaviour |
|---|---|
| 52998 | Streams IMU records immediately (below) |
| 52997 | Streams large camera frames immediately (below) |
| 52996 | Streams 38-byte timestamp/counter records at 120 Hz (below). No pose data |
| 52999 | Sends a few short status-like messages (below), at least on connect. Not observed to carry pose |
| 52990-52995 | Accept the connection and sent nothing in 25 s of recording. Possibly control channels, unconfirmed and untested |

### Port 52996: timestamps

Records are 38 bytes: magic `27 31 00 00 00 20`, a u64 timestamp at offset 14, a u16 counter at offset 22 that goes up
by exactly 1 every record, and zeros elsewhere. In a 25 s recording there were 2997 records, about **120 Hz and exactly two
per camera frame**. The timestamps are on the **same nanosecond clock as the IMU and the camera frame headers**, which
makes this stream (and the camera header timestamp at offset 23) useful for aligning camera frames with IMU samples.
During a recording with the wearer yawing, nodding and leaning, no pose or motion payload appeared here, so it is not
an on-glasses 6DoF pose. What exactly each record marks (exposure start, a per-eye trigger, etc.) is unconfirmed.

### Port 52999: status messages

Five messages in 25 s, each starting `27 8a 00 00 00` followed by a type byte (`07` or `09`), a few small fields and a
trailing float32 with values 43.9, 54.5, 54.8, 60.0 and 61.0. These look like temperature-type readings, but that is a
guess. Earlier 3-4 s reads saw nothing, so these may be sent only occasionally.

## Port 52998: IMU

Fixed 134-byte records, about 1400 per second in total. Layout (little endian), matching the public
`xreal_one_driver` source:

| Offset | Size | Meaning |
|---|---|---|
| 0 | 6 | Magic `28 36 00 00 00 80` (bytes 6-7 were `38 41` in early captures and `28 be` in a later session, so they are a varying field and must not be matched on) |
| 14 | 8 (u64) | Timestamp. Median step about 0.99 ms |
| 30 | 4 (u32) | Record type |
| 34 | 24 (6 x f32) | Type `0x0b`: gyro x, y, z then accel x, y, z |

- Type `0x0b`: about 1000 Hz. At rest the first three floats are near zero (std about 0.005) and the last three have
  a magnitude of about 9.77, which is gravity in m/s^2. So gyro is in rad/s and accel in m/s^2.
- Type `0x04`: about 400 Hz and every float is NaN. Probably a sensor that is absent, e.g. a magnetometer. Upstream
  drivers ignore these records.

### IMU axes and signs (measured)

From one run of the guided direction tests in `tools/imu_web` (raw result in [axis-map.json](axis-map.json)): the wearer
moved to a pose, held still, then returned to centre, and the page integrated the gyro over each move.

| Motion | Gyro axis | Sign |
|---|---|---|
| Yaw | Y | turn left = negative, turn right = positive |
| Pitch | X | look up = positive, look down = negative |
| Roll | Z | tilt to the left shoulder = negative, to the right shoulder = positive |

- Gyro units are rad/s. For pitch and roll the integrated gyro angle matched the angle of the accelerometer's gravity
  vector to within about 5% (54.3 vs 53.5 deg, 52.9 vs 53.5, 41.3 vs 39.5, 40.8 vs 38.8).
- For pitch and roll the gravity vector rotated in the opposite sense to the gyro on the same axis, as expected for a
  right-handed gyro, which independently supports those two signs.
- Yaw cannot be cross-checked this way (turning in place barely changes gravity), so its sign rests on the gyro alone.
- Returning to centre left a net error of 0.1-1.4 deg on every test. Gyro bias was about (-0.008, -0.001, 0.000) rad/s.
- Peak rates of up to about 3.9 rad/s (roughly 220 deg/s) were seen with no obvious clipping.
- Limits: a single run with one wearer. Secondary axes picked up around 10 deg during some moves, which is normal head
  motion. This is the glasses' own sensor frame, not a fused head pose, and the on-glasses stabilizer state during the
  run is unknown.

## Port 52997: Eye camera

- About 60 frames per second, every frame exactly 193,862 bytes, each starting with `27 48 00 02`.
- Header bytes 23-26 change per frame (probably a timestamp).
- The image payload starts at about byte 318 and is **189 rows of 1024 bytes**.
- Pixel values sit on about 16 evenly spaced levels, i.e. roughly 4 bits of information per byte.
- Each 1024-byte row has two halves:
  - Right half: a clean 512 x 189 grayscale image.
  - Left half: columns are interleaved. Even columns are a darker rendering and odd columns a brighter one of the
    same scene, each 256 x 189.
- Horizontal shift between all of these views peaks at zero, so there is no stereo baseline: it is a single
  viewpoint. The exposure or gain explanation for the left half is a guess.
- The picture looked vertically squashed at 189 rows, so the true sensor geometry may differ from what is assumed here.
- No intrinsics, extrinsics or IMU-to-camera timing are known.

## Upstream work (from other projects, not verified here)

- [rohitsangwan01/xreal_one_driver](https://github.com/rohitsangwan01/xreal_one_driver): Rust IMU driver for the
  One series over `169.254.2.1:52998`. Same layout as above. No control commands and no camera code.
- [wheaney/xrealOneDeviceKit](https://github.com/wheaney/xrealOneDeviceKit): wraps that driver for xrDeviceKit.
- [wheaney/XRLinuxDriver](https://github.com/wheaney/XRLinuxDriver) and Breezy Desktop: 3DoF only. The docs list
  One, One Pro and 1S as supported, with the stabilizer/anchor features disabled on the glasses and the latest
  firmware. Its `src/devices/xreal.c` lists USB vendor `0x3318` with product IDs `0x043e` and `0x043d` as the 1S (and
  `0x0437`/`0x0438` as the One, `0x0435`/`0x0436` as the One Pro), and opens One-series devices through
  `device_imu_open_xreal_one()`. So this glasses' ID (`0x043e`) is recognised. An earlier note here claiming otherwise
  came from a faulty page summary and was wrong. That it actually tracks correctly on this unit has not been tested.
- XREAL's SDK 3.1 documents 6DoF with the Eye on Android hosts only. There is no Linux support.
- No public work on ports 52996, 52997 or 52990-52995 or on the Eye's stream format was found. The search was not
  exhaustive and some pages (Monado merge requests) could not be read.

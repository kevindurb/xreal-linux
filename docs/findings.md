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

The glasses show up as a DRM connector with a single mode, `3840x1080` (two 1920x1080 eyes side by side).

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
| 52996 | Streams small records, magic `27 31 00 00 00 20`, containing a counter and timestamp. Possibly camera metadata, unconfirmed |
| 52999, 52990-52995 | Accept the connection, send nothing within 3-4 s. Possibly control channels, unconfirmed and untested |

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
- Axis conventions and gyro units were not verified with motion. Only a stationary capture was analysed.

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
  firmware. A summary of its source listed USB PIDs `0x0437` and `0x0438` for the One, which does not include the
  1S's `0x043e`. This was not checked in the source.
- XREAL's SDK 3.1 documents 6DoF with the Eye on Android hosts only. There is no Linux support.
- No public work on ports 52996, 52997 or 52990-52995 or on the Eye's stream format was found. The search was not
  exhaustive and some pages (Monado merge requests) could not be read.

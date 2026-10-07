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

## SteamVR on the Deck (tried 2026-10-07)

Setup: SteamVR 2.17.10 (Steam app 250820, 5.2 GB) installed through the running Steam client on a Plasma 6.7 Wayland
session; the Deck's own panel was disabled so the glasses (`DP-1`, 1920x1200 at 120 Hz) were the only display.

- First launch asks for superuser access and runs `pkexec setcap CAP_SYS_NICE=eip` on `vrcompositor-launcher`. That worked.
- SteamVR's bundled `null` driver (a simulated headset) can be enabled from `config/steamvr.vrsettings` and **loaded and
  activated fine** with a window placed on the glasses' output.
- The compositor then failed to start: `Tried to find direct display through Wayland: (nil)`,
  `CHmdWindowSDL: VR requires direct mode`, `VRInitError_Compositor_CannotDRMLeaseDisplay`. On this Linux build SteamVR
  will not draw into an ordinary desktop window; it needs a DRM lease of the display.
- KWin does advertise `wp_drm_lease_device_v1`, but the glasses' connector has DRM property **`non-desktop = 0`** (read
  directly with libdrm), and KDE lists it as a normal enabled output. Compositors normally only lease non-desktop
  connectors, so there is nothing for SteamVR to lease. The EDID (manufacturer `MRG`, product `0x4102`) has no obvious
  VR marker.
- Not yet tried: flagging the connector non-desktop (e.g. an EDID override), or having our own driver present frames
  itself through the driver direct-mode interface (which the `null` driver advertises) so SteamVR's compositor does not
  need the lease.
- SteamVR also ships a `gamepad` driver (off by default), not yet tried.
- Rootless podman over SSH failed (`open /run/libpod/alive.lck: permission denied`, even with `XDG_RUNTIME_DIR` set).
  Not yet diagnosed; it may work from a local terminal session.

### Driver direct mode works (prototype, 2026-10-07)

A minimal driver (`driver/`) that sets `Prop_HasDriverDirectModeComponent_Bool`, implements
`IVRDriverDirectModeComponent` and gets its swap textures from `VRIPCResourceManager()` (`NewSharedVulkanImage`,
`RefResource`, `ReceiveSharedFd`) made SteamVR 2.17.10 start its compositor **without a DRM lease**. The compositor logged
`Direct mode: enabled` and `Headset is using driver direct mode`, SteamVR Home allocated 3-deep swap sets
(2714x1527, VK format 43, about 17 MB per texture, each with a dma-buf fd), and `Present` was called at the configured
60 Hz. The compositor also logs `No Vulkan command buffer open in CGpuTiming::MarkEvent!` errors, which have not been
investigated. Nothing is shown on the glasses yet: the driver only receives frames. Next steps are getting the dma-bufs
onto the glasses' output (for example a fullscreen Wayland surface with `zwp_linux_dmabuf_v1`, which KWin advertises) and
feeding the IMU into the pose. Whether the glasses have a true stereo side-by-side mode (needed to give each eye its own
image) is still unverified.

## Control and event traffic (passive capture while changing glasses settings)

`tools/watch_control.py` listens (read-only, nothing is ever sent) on all ten TCP ports, the XREAL HID nodes, the button
input devices, the DRM connector/EDID and udev. Run 1: the wearer opened the glasses' menu and switched into full SBS.

- **Port 52999 is an event/status channel from the glasses to the host.** Messages are `27 <id> 00 00 00 <payload>`.
  Seen: `8a` heartbeat every ~10 s with float32 values around 44-61 (probably temperatures, unconfirmed); `2e` bursts of
  70-byte messages (two indices, a changing counter) at 18.8-29 s while the wearer was in the menu; a single `12`
  message; and short 8-byte `66` and `3d` messages at and after the mode switch. Their meanings are unconfirmed.
- **Changing display mode re-plugs the display, not the USB link.** The DRM connector went `disconnected` for about 1.8 s
  and came back with a different EDID and a single mode `3840x1080` (full SBS). No TCP session dropped and no USB
  re-enumeration was seen, so the IMU and control streams keep running across a mode change.
- **Menu use produced no HID or input events** on the XREAL nodes (two pointer-type input nodes could not be read for
  permission reasons), so any report of the menu or buttons is probably carried by the 52999 messages.
- Port 52996's rate dropped from 4.6 kB/s to about 1.2-1.9 kB/s right after the mode switch (unexplained).
- XRLinuxDriver names these display modes for the Air line: `1920x1080` at 60/72/90/120 Hz, SBS `3840x1080` at 60/72/90 Hz
  and a half-SBS `1920x1080` at 60 Hz. It marks the 1S as SBS-capable but does not open its HID "MCU" controller for
  One-series glasses, and neither it, xrealOneDeviceKit nor xreal_one_driver mention port 52999 or a display-mode command
  for the One series. How to command the glasses (for example to enter SBS automatically) is therefore unknown.

## Mode changes mapped to traffic (watcher run 3)

Wearer sequence (wall clock 09:47:01 + t): baseline, full SBS, half SBS, SBS off, anchor, follow, then an accidental volume
change and a brightness change. Every display-mode change re-plugged the display for about 1.7 s while all TCP sessions
and the USB link stayed up.

| Mode | Connector after re-plug | EDID hash | Modes |
|---|---|---|---|
| Off (normal) | connected | `0a6d3728` | 8 modes: `1920x1200` and `1920x1080`, each at 60/90/120 Hz, plus others |
| Full SBS | connected | `d14a10ed` | one mode, `3840x1080` |
| Half SBS | connected | `f6fdc196` | three modes, all `1920x1080` |

So the current mode can be **detected from the DRM mode list or EDID** with no protocol knowledge.

- Port 52999 `27 66 00 00 00 02 18 xx` is a display-link message: `xx=02` arrives at the instant the connector disconnects
  and `xx=01` messages follow once it reconnects.
- Port 52999 `27 3d ...` 8-byte messages (`18 01` / `18 02` alternating) appear around menu interaction; meaning unconfirmed.
- Menu interaction shows as `27 2e` pairs (indices 01 and 02, with a kind byte of 01, 02 or 03) and a `27 12` 70-byte
  message when the menu opens; meaning of the kinds is unconfirmed.
- **Volume** keys arrive over standard HID: a consumer-control report `02 ea 00` (usage `0x00EA`, Volume Decrement) on the
  buttons interface plus `KEY_VOLUMEDOWN` (code 114) on the input device, and at the same time a `27 12` message with kind `09`.
- **Brightness** changes produced **no HID or input events**; they appeared only as `27 12` messages on 52999 (kind `07` then `06`
  with a counter running 8 to 1 and then 2 to 9, which looks like a level).
- **Port 52996's rate follows the display state**, dropping to about 0.4 kB/s while the display link was down and recovering
  afterwards (4.6 kB/s normally, which is 120 records/s in the 120 Hz mode). This contradicts the earlier guess that it is
  camera frame metadata: it looks like **per-refresh display timing on the same clock as the IMU**. That would be useful for
  latency prediction. Not verified record by record.
- **The camera stream (52997) is not always on.** It was idle at the start of this run (glasses in follow mode), began
  streaming after the switch to anchor mode, and was idle again afterwards (a 4 s read after returning to follow got no
  data). Earlier runs had it streaming at about 11.5 MB/s from the start. Likely the glasses only run the camera when a
  feature such as anchor needs it. Not yet confirmed by controlled toggling.
- Still unknown: any host-to-glasses command, including how to switch SBS from the host.

## Full SBS stereo and the follow-mode Stabilizer (verified by the wearer)

- **Full SBS gives real per-eye stereo.** With the glasses in full SBS (single `3840x1080` mode), the presenter's test pattern
  showed the left half to the left eye and the right half to the right eye, with the expected depth offset. In the normal 2D
  mode both halves are visible to both eyes.
- **Anchor and follow are both still selectable in full SBS.** They are independent of the SBS setting.
- **Follow mode is only rigidly head-locked with the Stabilizer off.** With it on, the glasses slowly move the image to follow
  the head, which would add to any compensation we render. The setting is in the glasses' menu: double-click the X button,
  then Display, then Stabilizer (XREAL's One-series guide). The wearer confirmed that Stabilizer off made the image rigidly
  attached. XRLinuxDriver's advice to disable the stabilizer/anchor features on the glasses is for the same reason.
  A VR setup therefore needs: full SBS, follow mode, Stabilizer off.
- **The Stabilizer state cannot be read from the control channel.** Three toggles (off, on, off) produced identical-looking
  port 52999 sequences (`2e` bursts, one `12` message, a 12-message `2e` kind-03 burst); the only varying bytes are an
  increasing counter. A setup flow has to ask the user to set it.

## Idle soak test and SteamVR behaviour on the Deck (2026-10-07)

With the glasses sitting still for 10 minutes while SteamVR Home ran at 1280x720 per eye:

- **Gyro bias is stable:** mean per 30 s window on X moved from -0.00666 to -0.00605 rad/s (about 0.03 deg/s over 10 minutes), Y stayed within
  0.0001 of -0.00055 and Z within 0.0001 of -0.00025. Noise was about 0.002 rad/s (0.1 deg/s) per axis at 1 kHz. The X bias drift went with
  slowly rising sensor temperatures (the 52999 heartbeat floats, about +0.6 C on one sensor over the first 3 minutes), so expect some bias
  movement while the glasses warm up; tracking the bias whenever the head is still handles it.
- **No standby events and a perfectly steady 60 Hz** (exactly 300 `Present` calls per 5.0 s for the whole run) once SteamVR's
  `power.turnOffScreensTimeout` (default 5 s) and `power.pauseCompositorOnStandby` were overridden.
- **The GPU is the bottleneck, not our code.** With SteamVR Home the Deck's GPU reads about 95% busy even with the head still
  (`steamtours` at over 100% CPU), at 1280x720 per eye; at the 1920x1080 SteamVR asked for it was pinned at 100%.
- **Rotational reprojection** (presenter `--reproject`, shaders in `presenter/shaders`) runs on the Deck: the head pose SteamVR rendered
  each frame for (taken from the layer's `mHmdPose`) and our own fused pose agree to 0.00-0.04 degrees while the glasses are at rest, which
  confirms the pose conversion end to end. The visual result has not been checked yet.

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
- Type `0x04`: exactly 400 Hz (median step 2.500 ms). The six floats at offset 34 are NaN, which is why this was first read as an
  absent sensor, **but the record carries a magnetometer reading further in** (see "Magnetometer in the IMU stream" below). Upstream
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

## Review of korejan/steamvr-compositor-sync (read before installing; commit 994577b, v0.1.0)

- **What it is:** a Vulkan layer for SteamVR's `vrcompositor`. It adds its own timeline-semaphore signal to every queue submit and
  makes the compositor wait before it begins, resets or frees a command buffer (or resets/destroys a descriptor pool) that the GPU may
  still be executing (SteamVR-for-Linux #952). It is written for the freeze on NVIDIA; that it also removes our single bad frames is
  the hypothesis being tested here, not a claim of the project.
- **What it touches:** `getenv` for its own `STEAMVR_COMPOSITOR_SYNC_*` variables only. No network, no exec, no file reads or writes in
  `src/`. It is inert unless the process is `vrcompositor` (or `STEAMVR_COMPOSITOR_SYNC_FORCE=1`).
- **What the install writes:** three files under the prefix (default `~/.local`): `lib/libVkLayer_steamvr_compositor_sync.so`, an explicit
  layer manifest, and an implicit `VK_LAYER_LUNARG_override` manifest whose `app_keys` name the registered `vrcompositor` path(s). The
  script refuses to replace a file that is not its own and refuses to install when another override or a loader settings file would
  conflict. `--uninstall` removes only those files.
- **Surprises:** the override uses the loader's single per-application override slot, so it conflicts with vkconfig overrides; the
  layer is built with static libstdc++ for the Steam runtime. It needs CMake 3.25+ and a C++20 compiler to build from source.

## Dashboard bad frames and judder: what was measured (2026-10-07, Steam Deck, SteamVR 2.18.2)

Method: `tools/measure_dashboard.sh` (dashboard open, `--sim-pose --sim-yaw 40 --sim-pitch -30 --sim-pitch-amp 0`, 480-frame `--dump`,
`tools/find_bad_frames.py` in a container), driven by `tools/measure_matrix.sh` / `tools/measure_sweep.sh`. Two captures per cell.

**Bad frames per 480-frame sweep, no layer, async off:**

| | Home off | Home on |
|---|---|---|
| hold on (running start 2 ms) | 0, 0 | 1, 2 |
| hold off | 11, 13 | 11, 10 |

- **The hold hides the bad frames; nothing here fixes them at the source.** They are in SteamVR's own output, whatever we do.
- **korejan/steamvr-compositor-sync does not remove them** (hold off: Home off 9, 5; Home on 6, 6). It loads in vrcompositor
  from `~/.local` (the pressure-vessel container does not hide it; log line "active in vrcompositor"), but its summary shows only
  descriptor-pool swaps and zero command-buffer waits, so it is not addressing this.
- **`steamvr.enableLinuxVulkanAsync` does not remove them** (hold off, layer off: Home off 8, 6; Home on 12, 6; with the layer too:
  7, 10 and 10, 6). SteamVR logs nothing about async, so whether it engages under driver direct mode is unknown.
- **Our fence wait is not the cause.** `fence_pending` was set on 106 of 480 frames, so the exported write fences are real and we
  wait on them (ALVR's direct-mode driver does the same). The bad frames do not line up with pending fences (3 of 13) or with frame age.
- **A later running start removes them with the hold on** (Home on, 2 captures each, new driver setting `driver_xreal.running_start_ms`,
  which moves both the declared vsync and the end of the `PostPresent` hold): 2 ms: 0, 1; 4 ms: 0, 1; 6 ms: 0, 2; **8, 10 and 12 ms: 0, 0**.
  SteamVR's own rate with Home on is 47-55 new frames/s whatever the running start: Home is GPU-bound on the Deck.

**Judder (what is displayed), `tools/judder_report.py`: horizontal image shift per refresh during a steady 40 deg/s turn, no dashboard,
no reprojection.** Stalls are refreshes that moved under a third of the median, doubles moved over 1.65 times:

| | Home off stalls / doubles | Home on stalls / doubles |
|---|---|---|
| hold on, running start 2 ms | 56% / 1% | 32% / 16% |
| hold on, running start 8 ms | 45% / 20% | 22% / 20% |
| hold off, running start 2 ms | 22% / 6% | 14% / 24% |
| hold off, running start 8 ms | 18% / 3% | 13% / 15% |

- **The hold is what makes motion judder** (about half the refreshes repeat an image with Home off), and a later running start does not
  change that. Turning it off halves the stalls, but with Home on 15-24% of refreshes still double, from SteamVR's GPU-bound frame rate.
**With `--reproject` (rotational reprojection in the presenter), same method, one capture per cell:**

| | Home off stalls / doubles | Home on stalls / doubles |
|---|---|---|
| hold on, running start 2 ms | 16% / 3% | 13% / 1% |
| hold on, running start 8 ms | 18% / 1% | 11% / 4% |
| hold off, running start 2 ms | 17% / 4% | 14% / 4% |

- **Reprojection removes the judder, with the hold on or off.** Doubles fall from 16-20% to 1-4% and the stalls from 22-56% to 11-18%;
  the remaining stalls are the same with the hold off, so they are not the hold. The cell for hold off at running start 8 ms was not run (the
  matrix was stopped), and these are single captures, so treat differences of a few percent as noise. Reprojection does not change SteamVR's output,
  so it should not change the bad-frame counts above, but that combination (reprojection on, hold on, running start 8 ms, Home on) has not been run end to end.
- **Adopted default, confirmed by the wearer ("perfect": no flicker, smooth motion, no complaints about latency or warping), Home on:** hold on, `running_start_ms` 8, `--reproject` on. That gave 0 bad frames in two sweeps (without reprojection)
  and the smoothest displayed motion.
- **The hold is still needed.** With the hold off, a later running start does not remove the bad frames (1920x1080, 240-frame sweeps, two captures each):
  running start 8 ms: Home on 1 and 6, Home off 3 and 8; running start 12 ms: Home on 2 and 1, Home off 11 and 5. So the fix is the combination:
  hold on, running start 8 ms or more, and reprojection for the smoothness the hold costs.

**Present wait** times out when the presenter starts before SteamVR and then never recovers on the newest present id, because while the
fallback is active the queue runs up to the swapchain depth ahead of the display. The presenter now probes an id from 4 presents back every
5 s, and present wait returns within seconds; frame age drops from about 16 ms max to about 1.4 ms once it does.

**Build:** the driver must be linked with `-fno-math-errno` (now in `driver/build.sh`), otherwise a Fedora 44 build needs `sqrtf@GLIBC_2.43`.

**Render size (2026-10-07):** with the defaults above (hold on, running start 8 ms, `--reproject`) and Home on, the Deck's GPU averaged about 56% at
1280x720 and 58% at 1920x1080 with the head still, SteamVR delivered 60 new frames/s, and the wearer called 1920x1080 per eye "buttery". 1920x1080
is now the driver default. Under the dashboard sweep capture at 1920x1080 (the capture's own copies add some load) the GPU averaged
about 70% with Home on (90th percentile and peak 100%) and 20-29% with Home off. Heavier content than Home has not been tried.

## Capture recorder check (6DoF change, task 1.1)

`tools/capture_eye.py` (format in `docs/capture-format.md`) recorded 10 s with the glasses still, in follow mode, display at 60 Hz:
52998 delivered 1873856 bytes (about 1398 x 134-byte slots/s, which includes non-IMU record types, not yet split by type),
52996 delivered 600 records (60/s, consistent with "follows the display refresh"), and **52997 delivered nothing**: the camera
was idle, as noted above. A camera capture therefore needs the glasses in a mode that starts the Eye (anchor mode so far).


## Magnetometer in the IMU stream (measured offline on existing captures; corrects the earlier "no magnetometer" note)

IMU message `10294` interleaves two record types (u32 at packet offset 30): type `0x0b` at exactly 1000 Hz (gyro and accel, as above)
and type `0x04` at exactly 400 Hz. Every timestamp series is strictly increasing once split by type; the merged series is not,
because the two types are clocked 0.27 ms apart. The type `0x04` record is:

| Offset | Content |
|---|---|
| 34-57 | six f32 = NaN (unused fields) |
| 58 | 3 x f32: magnetic field vector, probably microtesla, in the sensor frame |
| 70 | f32 = 25.0 in every record seen (likely a temperature, a fixed default) |
| 74-133 | zeros, one repeated pattern of small constants, and a few bytes that look like padding |

Evidence that this is a real magnetometer (all four IMU captures in `captures/`, 20-38 s each):
- **At rest** (`xreal_52998_still.bin`) the vector length is **49.6 uT**, standard deviation 0.5 uT, which is a normal Earth-field
  magnitude; the components are about (-24.5, 10.5, -41.7).
- **Moving**, the length wanders (30.8 to 57.3 uT in `imu_move.bin`), as expected for an uncalibrated sensor near ferrous parts.
- **It tracks yaw.** In `imu_yaw.bin`, the tilt-compensated magnetic heading (horizontal components about the gravity direction taken from
  the accelerometer) against the gyro yaw integrated about the same axis has a **correlation of -0.98** (the sign is only an
  axis-handedness convention). The regression slope is -0.36, i.e. the raw heading moves about 0.36 degrees per degree of real yaw:
  the sensor is **not calibrated** (hard-iron offset and soft-iron scaling from the glasses' own electronics).
- The SDK has the matching pieces: `MSG_W_MAG_CALIBR_DATA`/`R_MAG_CALIBR_DATA` (0x1B/0x1C), `NRGlassesGetMagCalibrationData` (10018) and
  `...SetMagCalibrationData` (10019), `nativeSet/GetMagneticState`, and strings `Factory mag bias`, `online_calib_mag_bias`,
  `EkfMagneticOutlierCountThreshold` in its IMU tracker, i.e. XREAL's own tracker fuses this magnetometer for yaw.

Consequences for the project (not acted on yet): yaw drift, the main weakness of the IMU-only tracking in the presenter, could be bounded
with this sensor after a calibration (an ellipsoid fit over a slow rotation of the glasses in all directions, or the factory values via
request 10018 once requests can be sent). Caveats: the sensor sits near the display and USB electronics, so the offsets may change with
brightness or load; indoors the field is disturbed; the heading must be fused gently (the SDK uses an outlier gate). Not yet measured: whether the
offset changes with display state, and the factory calibration values.


## Anchor-mode observation run (2026-10-08, capture `anchor-03`, wearer toggling anchor mode from the glasses' menu)

Run per `docs/anchor-capture-plan.md`: all ten TCP ports plus both HID nodes (`hidraw4`, `hidraw5`), read-only, 150.7 s, glasses on the latest
firmware (wearer's statement), Follow mode with Stabilizer off at the start, pointed at the Deck screen and a desk. Phases: get-ready 0-10 s,
follow 10-25 s, toggle-on 25-65 s, anchor 65-95 s, toggle-off 95-135 s, follow 135-150 s. Only the small event files are kept in
`docs/samples/anchor-03/` (the 197 MB camera stream is not in the repo).

**Measured**
- **The camera streams only while the glasses are in anchor mode.** 1,019 frames (message 10056, 193,856-byte payloads, 0 bytes skipped), arriving
  from about 30 s to 100 s, i.e. from a few seconds into the toggle-on window to a few seconds into the toggle-off window; none in Follow mode
  before or after. The rate in this run was **15 frames/s** (75 per 5 s, median step 66.8 ms), not the 60 fps of the earlier sample
  (`captures/cam_52997_sample.bin`); the frame header structure is identical in both. Why the rate differs is not known (a different anchor
  or camera state is possible).
- **The frames are usable pictures**: a wide-angle view with the desk, a monitor and a mug in clear detail (right half, 512 x 189, grey levels
  stretched; faint vertical striping from the column interleaving). Plenty of texture for visual tracking.
- **Nothing else changed on the host-visible interfaces.** Ports 52990-52995 sent no bytes at all; both HID nodes produced **zero bytes** for the
  whole run; the IMU stayed at 1000 Hz (gyro/accel) and 399 Hz (magnetometer) and the timestamp stream at 59.9 Hz through both switches. So
  switching anchor mode on the glasses is **not announced on HID or on the silent ports**; the only host-visible signals are the camera
  stream itself and three event messages on 52999 (below).
- **The camera, IMU and timestamp streams share one device clock**: the IMU timestamps span 1455-1606 s, the camera frames 1487.02-1555.04 s.
- **Event messages on port 52999** at each switch (decoded in `docs/xreal-link-messages.md`, section 10): `10030` (twice) and `10002` (once) at
  30.7-31.1 s and again at 99.2-99.7 s. `10002` carries a nanosecond device timestamp (1486.512 s and 1555.072 s) that is **0.51 s before the
  first camera frame and 0.03 s after the last**, so it is the camera/anchor-session start and stop event. Message `10045` (2-byte
  protobuf payloads `18 01` / `18 02`, 81 in the run, irregular) continued throughout and is unrelated to the toggles.

**Inferred**
- The glasses start and stop the camera on their own initiative when their menu enters and leaves anchor mode; no host request preceded
  it on any interface that was recorded. That does **not** show whether a host request can start it in Follow mode: no host-to-glasses message
  was observed because none was sent.
- The camera runs at a lower rate than before here; anchor mode may use the camera at 15 fps on this firmware.

**Open**
- Whether a host request (SDK `NRGrayscaleCameraStart`, inferred id 10053) can start the camera with the glasses in Follow mode and the Stabilizer off.
- Which port accepts requests (none of 52990-52995 carried anything), and the request header and any handshake.
- Whether anchor mode itself (Stabilizer behaviour) conflicts with using the glasses as a headset, which was the original objection.


## First host-to-glasses request (2026-10-08)

One read-only request (`NRGlassesGetSWVersion`, 8 bytes) sent to each of ports 52990-52995: every port accepted the connection and closed it
10 ms after receiving the packet, without a reply; the streams were unaffected. Details and the likely reason (a missing packet header and/or
handshake) are in `docs/xreal-link-messages.md` section 11. **Superseded:** the control port is 52999 and requests need a transaction id (section 13); a read-only request there succeeded. The anchor-mode run above is still the only observed way the camera starts.

## Camera frame geometry against the glasses' own camera size (2026-10-08)

The glasses' factory calibration (`docs/xreal-link-messages.md`, section 13) gives the SLAM camera as **504 x 378** (4:3). The Eye stream's image
is 189 rows of 1024 bytes (`tools/decode_camera_frame.py`).

- **Observed, on one recorded frame** (`captures/cam_52997_sample.bin`, frame 1): the right half (512 x 189) stretched to twice its height is a
  natural-looking 4:3 view of a room, so the clean picture uses every second row of the 378 (see the later note on the header, which declares 504 x 378 with stride 512).
- **Checked and rejected:** that two image rows share each 1024-byte payload row (left half as the even rows, right half as the odd rows).
  Adjacent-row correlation is 0.88 for the right half alone and 0.53 when interleaved, and the interleaved picture shows a row pattern.
- **Inferred, not checked:** the factory vertical focal length (about 239 px) and principal point row (about 190) halve for the stream's pixels
  (about 119 px and 95), and only the first 504 of the 512 stored columns are image. A printed grid decides this (task 1.2 of
  `openspec/changes/add-6dof-camera-tracking`), as does whether the missing rows are skipped or binned.


## The Eye camera starts from a host request in Follow mode, but only for four frames (2026-10-08)

Setup: glasses in full SBS, Follow mode, Stabilizer off, SteamVR and the presenter stopped, `tools/xreal_session.py` holding one connection to the
control port (52999) and reading 52997 and 52996. Every state-changing request was shown to the wearer byte for byte and approved first.

| Step | Request (body `18 00`, transaction ids 1-3) | Result |
|---|---|---|
| A (read-only) | `NRDpGetInputMode` 10273, `NRDpGetWorkingState` 10085, `NRGlassesGetSupportedDevices` 10016 | all answered: input mode 1 (side by side, matches the display), working state 1, supported devices 1571 (meaning unknown) |
| B | `NRGrayscaleCameraCreate` 10047 | answered `22 00` (empty response body: success) |
| C | `NRGrayscaleCameraStart` 10053 (inferred id) | answered `22 00`; **the first camera frame arrived about 0.5 s later** |
| D | `NRGrayscaleCameraStop` 10054 (inferred id), sent about 50 s later | no reply within 5 s |

- **The camera does start from a host request, with the glasses in Follow mode and the Stabilizer off.** The earlier reading that it needs the
  glasses' own anchor mode is not the whole story, and the inferred ids 10053 and 10054 are the right ones: Start produced frames.
- **It streamed only 4 frames.** They came 66.8 ms apart (15 fps, the anchor-mode rate), 193,862 bytes each, with the same header bytes 6-21 as the
  anchor-mode frames; then the stream went quiet (0 bytes in a 3 s read of 52997 about 10 s later). No start event (10002) appeared on 52999, which
  the anchor-mode session does send.
- **The frame timestamp field:** a **little-endian u64 of nanoseconds at packet offset 23** (1201.164643 s, 1201.231466 s, ...), advancing exactly
  66.8 ms per frame. Whether it is on the IMU's clock is still to be checked (task 1.3).
- **Afterwards** the IMU stream still ran at 1,400 records/s with clean framing, the display mode and the USB device were unchanged, and the
  session tool's cleanup Stop was not needed (it had sent none itself).
- **Not yet known:** what keeps the stream going. Candidates, none tried: the `InitSet*` requests (10048-10052) that the SDK sends between Create and
  Start (their values are not documented); a heartbeat or an IMU/vsync session (`NRImuStart` 10036, `NRVsyncStart` 10031) started on the same
  connection; or the firmware stopping a camera it did not start itself outside anchor mode. A repeated Start was also not tried.

### What happened when the camera sequence was repeated (2026-10-08)

Same glasses, same tool, about 25 minutes after the first session, no replug in between:

| Session | What was sent | Result |
|---|---|---|
| 2 | Create, then Start 2.5 s later | Create answered (`22 00`); **Start got no reply and no frames came** |
| 3 | Create | **no reply** |
| 4 | the read-only getter `NRDpGetInputMode` | **no reply** (it was answered in step A before the camera was started) |

- The IMU stream (1,400 records/s), the timestamp stream (60/s), the USB device and the display mode stayed normal throughout, so only the
  **control server** stopped answering. The connection is still accepted. It stopped after the second Start.
- Reading: the glasses seem to handle control requests one at a time, and the camera start never completed, so everything queued behind it. The
  first session's four-frame burst and its unanswered Stop may already have been the camera pipeline stalling; the repeat made it worse. This is
  an inference.
- **Do not send a Start again to glasses that have already run the camera since power-up** (precaution). A replug is needed to recover (the glasses then fall back to their
  normal 2D mode; set full SBS again in their menu).
- **Possible alternative explanation:** the wearer thinks the glasses had fallen asleep, which they do on their own. After the replug the control server answered
  again, and a query showed auto sleep **off** but the **proximity (wearing) sensor on** (`NRProximityIsEnable` = 1, wearing state 0, probably not worn). So the glasses may
  go idle by wearing state, not by the power-save timer, and an idle control server could look the same as a stalled camera. This is untested; the next camera run keeps
  the glasses worn.
- Still unknown: whether a camera that is configured first (the `InitSet*` requests) streams continuously. Their values are not known
  (`docs/xreal-link-messages.md` section 13.5).

### The four-frame burst reproduces; the frame header declares the image size (2026-10-08, after the replug)

- **Same sequence, fresh replug, glasses worn** (the wearing-state getter read 1; later readings showed that its values do not map to worn and not worn the way this
  line first assumed, see the end of this section), full SBS set by the host:
  Create answered, Start sent 10 s later answered, the first frame 0.5 s after Start, **exactly 4 frames again** (66.8 ms apart), then silence; Stop got no reply again.
  So the burst is repeatable and is not the glasses falling asleep.
- **The control server stayed alive this time:** after the unanswered Stop, the read-only getters were still answered (input mode 1, wearing state 1). The earlier
  silence therefore needed more than a Start and an unanswered Stop; the second Start without a replug, or sleep, remain the candidates.
- **The frames are normal pictures.** A host-started frame shows the desk and a monitor, at the same brightness as the anchor-mode frames. The frame headers have the same
  structure as the anchor-mode ones: the only bytes that vary between frames are the timestamp and two small counters (packet offsets 263, 264 and 275), and nothing in
  them looks like a mode or a flag.
- **The frame header declares the size:** little-endian u32 **504** at packet offset 11, u32 **378** at offset 15 and u16 **512** (the row stride) at offset 19. After a
  320-byte header the payload is 512 x 378 = 193,536 bytes, which is exactly the rest of the packet. This agrees with the config's 504 x 378 and means the
  "189 rows of 1024 bytes" reading is the same bytes re-chunked. Even and odd rows still differ strongly (the clean picture is the odd rows; the even rows look striped), so how
  the two row sets relate is still open (task 1.2). It also revises the earlier note that the stream carries "every second sensor row": the frame holds all 378 rows, and
  the clean picture uses half of them.
- **Not found in the vendor service:** an acknowledgement or per-frame reply for camera frames. The frame id (10056) appears only in message-class registration code.
  The `InitSet*` values the service passes still have to be recovered by tracing `ImpGrayCamera`'s virtual calls (its virtual table is at `0x2377d50` in `libnr_service.so`).

### Starting the IMU and vsync before the camera changes nothing; the wearing state is not what it seemed (2026-10-08)

Fresh replug, then on one connection: `NRDpSetInputMode` = 1 (full SBS), `NRImuStart` (10036) and `NRVsyncStart` (10031), both with the body `18 00`, then `NRGrayscaleCameraCreate`,
nine seconds, `NRGrayscaleCameraStart`.

- **`NRImuStart` and `NRVsyncStart` were answered `22 00` (success) and changed nothing visible:** the IMU stream stayed at 1,400 records/s and the timestamp stream at 60/s.
- **The camera still gave exactly 4 frames** (first frame 0.5 s after Start, 66.8 ms apart), then silence, and Stop got no reply. So an IMU or vsync start on the connection is not what keeps the camera streaming.
- **Sending Start a second time without a replug is known to fail:** the earlier session 2 got no reply to it, no frames, and the control server then stayed silent until a replug.
- **The wearing state does not behave like a worn flag.** `NRProximityGetWearingState` read 1 (during the camera run before this), then 0 on 16 samples over 20 s with the glasses on the wearer's face,
  then 2 (twice, in 2D and in SBS). The notification id 10045 (`18 01` / `18 02`) toggles between 1 and 2 around display changes, and 10086 does too. The proximity threshold getters
  (`NRProximityGetFarThreshold`, `GetNearThreshold`) answer with an **error code, 5004**, not a value, and `NRProximityIsEnable` reads 1. The meaning of the values 0, 1 and 2 is unknown, so the earlier
  "1 = worn" is withdrawn. Nothing yet explains why the glasses drop back to 2D mode on their own.
- **Next:** the `InitSet*` requests (values not known; trace them offline) and the SDK's own empty-body form `1a 00` are the remaining untried differences from the vendor sequence. Each hardware attempt costs a replug.


### Empty-body requests, a repeated Start and Stop then Start do not sustain the camera (2026-10-08)

Fresh replug (2D mode, no setter sent), one connection, camera requests with the SDK's empty body `1a 00`, each one approved before it was sent.

| Step | Packet (txid) | Reply | Frames |
|---|---|---|---|
| Create 10047 | `273f00000006 80000003 1a00` | `22 00` | 0 |
| Start 10053 (9 s later) | `274500000006 80000004 1a00` | `22 00` | **4**, first 0.5 s after Start, then silence |
| second Start | `...80000005 1a00` | `22 00` in 4 ms | none |
| Stop 10054 | `274600000006 80000007 1a00` | `22 00` in 3 ms | none |
| Start after Stop | `...80000008 1a00` | `22 00` in 3 ms | none |

- **`1a 00` bodies behave like `18 00` for Create and Start**: same replies, same 4-frame burst. Stop **is answered with `1a 00`** (`22 00`); the earlier Stops used `18 00` and got no reply.
- **A second Start without a replug did not silence the control server** (a getter answered afterwards). The silence in session 2 had another cause; the withdrawn rule "never Start twice" was overcautious.
- **The burst is once per replug in every variant tried**: Start, a repeated Start, and Stop then Start all answer success and send nothing more. The streams on 52996 stayed at 120/s throughout and the USB device did not change.
- **The `InitSet*` values are not recoverable from ControlGlasses 3.1.0.** The service only registers and handles the requests (the registration code at `0x1b67a50` to `0x1b717f4` is static constructors; the wrappers' only callers are their own request handlers); `libnr_api.so` and `libnr_loader.so` only export the `NRGrayscaleCameraInitSet*Base` functions, and no other library or the dex files call them. The values are chosen by the SDK's callers (an app or tracking plugin), so they would have to come from a different package (for example the Nebula APK) or from the public SDK headers.
- **Still untried:** Create again after a Stop, and the `InitSet*` requests between Create and Start.

### The factory IMU bias does not match the stream (2026-10-08, read-only, glasses at rest on a desk)

A 12 s capture of the IMU stream (about 12,000 samples, nothing sent) against the config's IMU block:

| Quantity | Stream mean | Factory value |
|---|---|---|
| gyro (rad/s) | -0.00653, -0.00036, -0.00020 (sd about 0.002, 0.001, 0.001) | `gyro_bias` -0.01126, -0.00156, 0.00007; its temperature table gives -0.0097 to -0.0117 in X over 22 to 43 C |
| accel (m/s^2) | -0.062, -9.04, -3.72, length 9.78 | `accel_bias` 0.0117, 0.0418, 0.0012 |

So the stream is not already bias-corrected (the mean is not zero), but the factory bias is not the stream's offset either (X differs by 0.0048 rad/s, 0.27 deg/s), and which temperature sensor belongs to the IMU is unknown. The presenter therefore keeps estimating the bias online and applies only the factory 3x3 matrices (gyro scale about 1.0058, 1.0000, 1.0060). The matrix convention (`M * v`) is not verified.

The presenter now reads the config on every control-port connection, sends the display-derived field of view and IPD to the driver (message type 7), and logs the glasses' events (`[control +T s] event ...`), which is the data for the open question of why the glasses drop to 2D.


### Display distortion grid, the magnetometer at rest, and how to find the 2D drop-out cause (2026-10-08)

**Display distortion grid** (`display_distortion.left_display` / `right_display` in the config, read from the unit): 61 columns x 39 rows, four numbers per point: the panel pixel (x 0-1920 and y 0-1216, 32 apart) and a second position in panel pixels. The second position differs from the first by under 1 px at the centre, 10-12 px sideways at the mid-edges and 22-26 px both ways at the corners (about 1.3 % of the width); the corners move **outward** (for example (0,0) to about (-22,-14)), the rows run past the 1200-row panel. **Direction (inferred, not verified):** read as "the panel pixel is seen at this position of the ideal picture", the optics magnify the edges (pincushion), so to show the ideal picture each panel pixel must sample the ideal picture at the listed position; that is what the shader does (`--factory-distortion`), and it is the natural lookup for a fragment shader (the grid is indexed by output pixel). If the vendor meant the inverse, the edges would be bent the wrong way and `--factory-distortion-reversed` (first-order inverse) would look right instead. The 1080-row picture is assumed centred in the 1200 rows (panel y = picture y + 60); that is the same unmeasured assumption as the vertical field of view. Runtime check on the Deck (own screen, no glasses): the pass runs with no errors and the test grid's frame bends toward the edges as the grid says; whether it matches the optics is for the wearer to judge.

**Magnetometer at rest, new reading:** on the glasses lying on a desk after the replug the field read (-61, 5, 52) uT, **81 uT** in magnitude, at 400 records/s, against 49.6 uT with components (-24.5, 10.5, -41.7) in the first captures. The orientation differs, but 81 against 50 uT means the hard-iron offset is not constant between sessions (display state, firmware, or magnetic parts near the desk), so a saved calibration may go stale. The collector counts a still sensor as no directions (its noise cloud has every direction around its own centre), which an earlier version got wrong.

**The 2D drop-outs:** the presenter now logs every change of the connector mode list next to the glasses' events on one clock; `tools/analyze_control_events.py /tmp/presenter.log` lists the events before each drop. No drop has been captured with the new logging yet.

### Wearer check of the test grid and the factory distortion (2026-10-08)

The glasses were put in full SBS by the presenter's `--set-sbs` (one `NRDpSetInputMode` = 1, answered `22 00`), then `--test-grid` ran on DP-1 (3840x1080).

- **Plain grid (no correction):** the lines looked straight to the wearer, with no black bars above or below the picture. The top border was visible; the bottom border was hidden by the lens's own curved edge (4 lines above and 4 below the centre cross were visible; in the right eye a sliver of the bottom border showed at the outer corner). So the bottom is cut by the optics, not by a vertical offset. Why there are **no black bars** is still unexplained: the vertical field of view assumption (a 1080-row picture centred in 1200 rows) is neither confirmed nor contradicted.
- **`--factory-distortion`:** the corners curved away from the wearer; no better, worse than plain.
- **`--factory-distortion-reversed`:** all four corners were cut off and only a sliver of border showed at the top middle; also worse than plain.
- **Conclusion:** applying the factory `display_distortion` grid in either direction made the picture worse. The glasses seem to correct their optics themselves (or the grid means something else), so the flags stay off by default and the grid is not worth pursuing further without the vendor's definition of it.

### Magnetometer calibration on the Deck: the fit does not converge (2026-10-08)

`xreal-presenter --mag-calibrate`, glasses in full SBS showing the desktop, wearer turning them through all directions twice (about 45 s, 18,000 samples at 400 Hz).

- **No stable hard-iron offset.** The fitted centre's X component walked from about -12 uT to -46 uT to -145 uT during the run (a fixed offset would not move), the radius from 23 to 93 uT, the per-axis scale from (0.6, 2.7, 0.6) to (0.5, 1.7, 1.2), the residual stayed at 11-18 % (never under the 3-6 % seen on partial coverage), and the counted directions flipped between 18 and 22 of 26. Nothing was saved.
- **The glasses hold no stored calibration:** `NRGlassesGetMagCalibrationData` (10018, read-only, `18 00`) was answered `22 00` (empty), consistent with the config's default mag bias and scale.
- **Reading:** the field the sensor reports depends on something that changes while the glasses move or run (the earlier note that the sensor sits beside the display and USB electronics; the rest magnitude was 81 uT in one session and 49.6 uT in another). Until that is understood, a hard-iron fit is not trustworthy, and `--mag-yaw` stays experimental and off.
- **What would settle it:** a raw capture of the field at rest for several minutes in each display state (regular, SBS, display off) to see whether it drifts with the display, and a rotation with the glasses far from the Deck. Not done.

### Eye orientations: the displays are turned 0.885 degrees against each other (2026-10-08, arithmetic from the config, not yet worn)

The config's `display.target_q_left_display` and `target_q_right_display` (Hamilton x, y, z, w, in the IMU frame D: x right, y down, z forward; the eye positions in the same section agree with that frame: left x -56.7 mm, right +7.2 mm, both about 22 mm below and 26 mm behind the IMU) give rotation vectors of (-0.317, 0.842, 0.011) degrees for the left display and (-0.315, -0.030, 0.163) degrees for the right. The relative rotation is 0.885 degrees: **0.872 degrees about the vertical axis** (about 38 px at the panel's focal length of 2490 px, 19 px per eye if split), 0.15 degrees of roll (2.5 px at the picture's edge) and no relative pitch (so no vertical disparity). If the quaternions are the display's orientation in the IMU frame, the left display looks 0.84 degrees towards the nose and the right straight ahead, so zero-disparity content converges at IPD / 0.0152 rad = about 4.2 m instead of at infinity. SteamVR's driver API has no per-eye rotation (only the IPD and a projection), so the presenter can correct it in its warp pass: `--eye-rotation` (opt-in, sign convention unverified, `--eye-rotation-reversed` for the other). The eye-centre offset from the IMU (about 2.5 cm to one side) changes nothing for a rotation-only pose and is not used.

### Removed: the factory distortion grid and the magnetometer calibration code (2026-10-08)

`--factory-distortion`, `--factory-distortion-reversed`, `--mag-calibrate`, `--mag-report` and `--mag-yaw` (with `presenter/src/magcal.rs`, the grid upload in the warp pass and the magnetometer parsing) were removed after the two negative results recorded above. They can be recovered from git history at commit `33d4419`.

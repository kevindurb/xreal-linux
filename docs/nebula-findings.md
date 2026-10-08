# What the XREAL Nebula app says about the glasses' messages

Static inspection of `ai.nreal.nebula.universal_3.8.1.xapk` (Android, arm64), done to find out how the official app starts the
Eye camera, since the camera only streams in anchor mode here and the 6DoF change needs it in Follow mode with the Stabilizer
off (see `docs/findings.md`, "Capture recorder check").

**Nothing here was run or sent to the glasses.** Everything below was read from the files (strings, class lists, constants,
disassembly). Statements are marked **read** (seen directly), **decoded** (worked out from disassembly) or **inferred**.
The APK and its extracted files are not in the repo; only facts are recorded here. The full name list is in
`docs/nebula-api-names.txt`.

## Headline

- **The Nebula 3.8.1 XAPK has no XREAL One / 1S wire protocol.** None of the ports 52990-52999, the magic bytes
  (`27 48 00 02`, `28 36 00 00 00 80`, `27 31 00 00 00 20`) or any glasses TCP code was found in its dex, Unity metadata or
  bundled native libraries (**read**, by search). The SDK runtime (`libnr_api.so`, `libnr_service.so`) is not in it: Nebula
  fetches it at run time as a Google Play **dynamic feature module named `nrservice`** (`FeaturesManager.loadSdkFeature`,
  "SDK downloading"), which mirror XAPKs leave out.
- **The runtime is available from XREAL directly.** XREAL's developer download page links a public APK, `ControlGlasses`
  (section 8), that bundles `libnr_api.so`, `libnr_service.so`, `libnr_glasses_api.so` (the newer, One-aware version),
  `libnr_external_sensor.so`, `libnr_dual_agent_tracking.so`, `libnr_rgb_camera.so` and more. Most of the message
  information below section 8 comes from there.
- **The camera start is an RPC, and it is in the runtime (section 10).** `libnr_api.so` / `libnr_service.so` contain a
  protobuf-lite request/response layer to the glasses with 183 named requests, including `NRGrayscaleCameraCreate`,
  `NRGrayscaleCameraInitSet{AutoExposureType,ExposureTime,Gain,ImageResolution,PixelFormat}`, `NRGrayscaleCameraStart`/`Stop`,
  `NRImuStart`/`Stop`/`StartExt`/`SetFrequencyExt`, `NRVsyncStart`/`Stop`, `NRUsbSetNetworkEnable`, and the mode requests
  `NRGlassesSetSpaceMode` / `SetSceneMode`. The runtime talks to the glasses over **USB** (libusb), not over the TCP ports
  52990-52999. The glasses' HID interface 0 (two 1024-byte interrupt endpoints) is the likely carrier. The request
  framing and the protobuf field layouts are **not recovered yet**, so no request can be sent yet.
- The app has an explicit tracking-mode switch (**6DoF / 3DoF / 0DoF / 0DoF stable**) and a "SLAM mode" setting, and the SDK
  has grayscale-camera and RGB-camera APIs. That fits the camera being started by a tracking-mode request rather than by
  anchor mode as such (**inferred**).
- Devices named in the Nebula SDK layer: `XrealAir`, `XrealAIR2`, `XrealAIR2_PRO`, `XrealAIR2_ULTRA`, `Xreal_HONOR_AIR`. The
  newer `libnr_glasses_api.so` also knows the One series under the name **Gina** (section 8.2).

## Package layout (read)

| Part | What it is |
|---|---|
| `ai.nreal.nebula.universal.apk` (24 MB) | Android shell: 5 dex files, Flutter UI (`libapp.so`, `libflutter.so`), Firebase, Agora, ExoPlayer |
| `config.arm64_v8a.apk` (272 MB) | Native libraries (below) |
| `unityassets.apk` (1.05 GB) | Unity data: `global-metadata.dat` (IL2CPP names), glasses firmware images, scenes |

Native libraries of interest: `libNrealXRPlugin.so` (Unity XR plugin, version string `1.0.2.2024080819`), `libnr_glasses_api.so`
(firmware/HID/USB toolkit), `libnr_libusb.so` (libusb), `libil2cpp.so` / `libunity.so`, `libopencv_java4.so`, SNPE (Qualcomm
neural runtime) libraries, Agora (calls). `libnrb.so` had nothing readable.

## 1. Tracking modes (read)

Unity metadata, enum `TrackingType`: `Tracking6Dof`, `Tracking3Dof`, `Tracking0Dof`, `Tracking0DofStable`.
Related names: `ChangeTo6Dof`, `Enable6DOF`, `Is6dof`, `IsInSlam0Dof`, `ChangeSlamMode`, `EnsureSlamTrackingMode`,
`EnsureTrackingType`, `AdaptTrackingType`, `AutoAdaptTrackingType`, `ChangeStartTrackingType`, `HomeSlamMode`,
`OnChangeTrackingMode`, `get_IsTrackModeChanging`, `REQ_CODE_CHANGE_SLAM_MODE`, `CheckPopUp0DofStableTip`,
`m_HasKnown0DofStableSetting`, `LogEventSlamModeSwith`.

Plugin entry point (`libNrealXRPlugin.so`): `NrealSDK::SwitchTrackingType(const char*)`. It takes a **string**, so the mode
is passed by name. The accepted strings are not visible in the plugin's string table (the mode is probably built in managed
code); the likely values are the four enum names above (**inferred**). Other tracking calls: `InitializeTracking`,
`StartTracking`, `PauseTracking`, `ResumeTracking`, `StopTracking`, `ShutdownTracking`, `GetHeadPose`, `GetIsHeadHeadPoseReady`,
`GetLostTrackingReason`, `GetHeadTrackingHandle`.

Lost-tracking reasons (enum `LostTrackingReason`, **read**): `PRE_INITIALIZING`, `INITIALIZING`, `EXCESSIVE_MOTION`,
`INSUFFICIENT_FEATURES`, `RELOCALIZING`, `ENTER_VRMODE`. Supported features (`NRSupportedFeature`):
`NR_FEATURE_RGB_CAMERA`, `NR_FEATURE_WEARING_STATUS_OF_GLASSES`, `NR_FEATURE_CONTROLLER`,
`NR_FEATURE_PERCEPTION_HEAD_TRACKING_ROTATION`, `NR_FEATURE_PERCEPTION_HEAD_TRACKING_POSITION`. Perception features
(`NR_PERCEPTION_FEATURE_*`): `NONE`, `HAND_TRACKING`, `MESHING`, `TRACKABLE_PLANE`, `TRACKABLE_IMAGE`, `TRACKABLE_ANCHOR`.

"0DoF stable" is a named mode of its own with a user-facing tip, which is probably what the glasses' Stabilizer / anchor
mode is called inside the app (**inferred**). The head-pose extension returns `accBias` and `gyroBias`
(`GetHeadPoseExtended`), so the SDK's tracker exposes IMU biases.

## 2. Camera APIs in the SDK (read, names only)

Grayscale (tracking) camera: `NRGrayscaleCameraCreate/Destroy/Start/Pause/Resume/Stop/SetCaptureCallback`,
`NRGrayscaleCameraImageGetData`, `...GetTime` (nanoseconds, on the HMD clock), `...GetGain`, `...GetExposureTime`,
`...GetCameraIdMask`, `...ImageDestroy`, `NRGrayscaleCameraProjectPoint/UnProjectPoint`. Camera ids:
`NR_GRAYSCALE_CAMERA_ID_0` to `_3` (up to four cameras, selected by a mask). The Unity layer reads per-camera intrinsics,
distortion and resolution (`GetGrayCameraIntrinsicMatrix`, `GetGrayCameraDistortion`, `GetGrayCameraResolution`) and the
pose of each camera from the head (`GrayEyePoseFromHead`, `MagneticPoseFromHead`). Camera models:
`NR_CAMERA_MODEL_RADIAL`, `NR_CAMERA_MODEL_FISHEYE`, `NR_CAMERA_MODEL_FISHEYE624`.

RGB camera: `NRRGBCameraCreate/Destroy/Start/Stop/SetCaptureCallback/SetImageFormat`, `NRRGBCameraImageGetRawData`,
`...GetResolution`, `...GetHMDTimeNanos`, `NRRGBCameraProjectPoint/UnProjectPoint`; Unity: `IsRGBCameraEnable` (on the
glasses-control object). Errors: `NRRGBCameraDeviceNotFindError`, `NRDPDeviceNotFindError`.

This is the app's API surface for camera frames, with frame timestamps on the HMD clock, which matches the findings that
the camera header carries a timestamp on the IMU clock. The layout of the frames over the wire is not here.

## 3. Glasses control API in the SDK (read, names only)

`NRGlassesControl*` (172 names with the other families in `docs/nebula-api-names.txt`). Calls:
`Create/Destroy/Start/Stop/Pause/Resume`; `Get/SetBrightness`, `GetBrightnessLevelNumber`, `Get/SetBrightnessSet`,
`Get/SetDuty`, `Get/Set2D3DMode`, `GetDisplayStereoMode`, `Get/SetDisplayMapParams`, `SetDisplayState`, `SetDPLevel`,
`SetDPESDParam`, `Get/SetElectrochromicLevel`, `GetElectrochromicTotalLevel`, `Get/SetLightIntensityState`,
`Get/SetPowerMode`, `Get/SetPsensorSwitchState`, `GetPsensorIsWearing`, `Get/SetSleepTime`, `GetTemperatureData`,
`GetTemperatureLevel`, `GetVersion`, `GetDPFwVersion`, `GetDspVersion`, `Get7211ICStatus`, `GetGlassesID`, `GetGlassesSN`,
`GetGlassesRunStatus`, `GetActivatedState`, `GetActivationTime`, `GetIfGlassesDisplayFine`, `SetIMUFrequencyDivider`,
`SetLogTrigger`, `StartErrorsAndEventsReport`, `ToggleKey`; parameter block `NRGlassesControlParams` with
`NR_GLASSES_CONTROL_PARAMS_DP_MAP` and `NR_GLASSES_CONTROL_PARAMS_LED_RGB_MODE`.

Callbacks: key events (`KeyEventGetType/Function/Param`), brightness key / value, light intensity, over-temperature, wearing,
plug-off (`SetGlassesDisconnectedCallback`), hardware error and hardware event, notify-quit-app.

Mentions worth following up:
- `SetIMUFrequencyDivider` exists: the IMU rate is settable through the SDK.
- `GetDisplayStereoMode` / `Get2D3DMode`: display mode is readable and settable through the SDK.
- `NRIMUCalibration` with phases `PitchUp` / `PitchDown`: the app can run an IMU calibration.
- Brightness key events: `NR_BRIGHTNESS_KEY_DOWN`, `NR_BRIGHTNESS_KEY_UP`.
- Temperature levels: `TEMPERATURE_LEVEL_NORMAL`, `_WARM`, `_HOT`.

Rendering / display / head-tracking natives the plugin imports from `libnr_api.so` (the complete list of C entry points the
plugin resolves by name; **read**): `NRGetVersion`, `NRAPICreate`; `NRDisplayCreate/Start/Stop/Destroy/Pause/Resume`;
`NRHMDCreate/Start/Stop/Destroy/Pause/Resume`, `NRHMDGetComponentPoseFromHead`, `...Fov`, `...Resolution`, `...Intrinsic`,
`...Distortion`, `...RefreshRate`; `NRRenderingCreate/InitSetFlags/Start/Stop/Destroy/Pause/Resume/DoRenderEx/
GetFramePresentTime/SetRefreshScreen/AcquireFrame`; `NRPerceptionGroup*` and `NRPerceptionCreate/Start/Stop/Destroy/Pause/
Resume`; `NRHeadTrackingCreate/Destroy/AcquireHeadPose`, `NRHeadPoseGetPose/GetTrackingReason/Destroy`;
`NRMetricsCreate/Start/GetCurrFramePresentCount/GetDroppedFrameCount/GetFrameCompositeTime/GetAppFrameLatency/Stop/Destroy/
Pause/Resume`; `NRBufferSpec*`; `NRSwapchain*`; `NRFrameAcquireBuffers/Submit/Compose`; `NRBufferViewport*`;
`NRFrameGetViewportCount/GetBufferViewport/SetBufferViewport`. Two things stand out: the SDK asks for the head pose with
a handle and returns a **tracking reason**, and there is a frame-compose path with per-frame present time, as in the
presenter.

## 4. Android control layer: glasses commands (read)

Package `ai.nreal.glasses.control.Sdk`, class `glasses_api`. It binds to a service and calls
`command(int cmd, String arg) -> String` (AIDL `IGlassesCommand.command_exec`). Events come back through
`IGlassesEvent` / `IEvent_cb` as `onEvent(byte[])` plus `onConnect(int)`, `onDisconnect`, `onProgress(int)`, `onFinish(boolean)`.
Other service calls: `start_sdk`, `glasses_do_ota(path)`, `need_ota(path)`, `registerReceiveListener`,
`unregisterReceiveListener`.

Command ids (constants in `glasses_api`, **read**):

| Id | Name | Id | Name |
|---|---|---|---|
| 0 | GET_BRIGHTNESS | 12 | SET_POWER_MODE |
| 1 | SET_BRIGHTNESS | 13 | GET_SLAM_CONFIG |
| 2 | GET_2D3D | 14 | SET_SLAM_CONFIG |
| 3 | SET_2D3D | 15 | SET_REBOOT_FLAG |
| 4 | GET_PSENSOR_CLOSED | 16 | SET_UPGRADE |
| 5 | SET_PSENSOR_CLOSED | 17 | GET_VERSION |
| 6 | GET_PSENSOR_NON_CLOSED | 18 | GET_7211_VERSION |
| 7 | SET_PSENSOR_NON_CLOSED | 19 | GET_GLASSES_ID |
| 8 | GET_HOST_ID | 20 | GET_TEMP |
| 9 | SET_HOST_ID | 21 | GET_LIGHT_SWITCH |
| 10 | SET_DUTY | 22 | SET_LIGHT_SWITCH |
| 11 | GET_DUTY | 23 | SET_SONY_REBOOT |

(each name is `GLASSES_CMD_<name>`). **`GET_SLAM_CONFIG` (13) and `SET_SLAM_CONFIG` (14) are the commands to look at for the
camera question**: they read and write a SLAM configuration on the glasses. Their argument format is not in this package
(`arg` is a string; probably JSON, **inferred**, since the firmware toolkit contains cJSON).

Model-selection code: `GlassesFactory.create(int)` returns `AirGlasses` or `EllaGlasses`. `EllaGlasses` knows the USB
interface roles `TYPE_CONNECTION_AUDIO=0`, `RGB=1`, `OV=2` (the OV580 camera chip), `HID=3`, `TTY=4`, `IMU=5`,
`GLASSES_CONFIG=6`. `glasses_api` keeps a `UsbDevice` each for audio, HID, `OV580`, RGB and TTY. Connection states
(`GlassesManager.GlassesState`): `uninitialized`, `start_polling`, `usb_connect`, `usb_disconnect`, `usb_ota`.

Display switching (app-level, `GlassesSwitchMode`): `GLASSES_2D_1080=1`, `GLASSES_3D_540=2`, `GLASSES_3D_1080=3`,
`GLASSES_3D_1080_72=4`, `GLASSES_2D_1080_SMALL=156`. Broadcast actions (`GlassesConstant`): `display_switch_2d`,
`display_switch_2d_small`, `display_switch_3d`, `display_switch_result`, `display_switch_close_app`, `device_disconnected`;
extras `display_mode`, `key_from_event`, `code`, `data`. Error codes: `switchError=3`, `switchFastError=13`,
`lowBattery=64536`.

## 5. Wire framing for the older glasses (decoded from `libnr_glasses_api.so`)

The library `libnr_glasses_api.so` is a firmware-update and HID/USB message toolkit for earlier models. It is **not** the One
protocol, but it shows how XREAL frames messages on USB HID/bulk, and the same style may apply to a host-to-glasses
command if one exists on the One. Builders: `cmd_build_flora`, `cmd_build_sdk`, `cmd_build_p55`, `cmd_build` (host-to-glasses
"command" frames) and `cmd_build_flora_imu`, `cmd_build_imu` (IMU-channel frames). The CRC table at the start of the CRC code
is the standard CRC-32 (table[1] = `0x77073096`), checked in the file.

Command frame (`cmd_build_flora`, `cmd_build_sdk`, same layout; **decoded**, offsets in bytes):

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | `0xFD` |
| 1 | 4 | CRC-32 (zlib) over bytes `5 .. 5+len-1`, little endian |
| 5 | 2 | `len`: length from offset 5 to the end of the payload = `17 + payload length`, little endian |
| 7 | 4 | request id / timestamp argument, little endian |
| 11 | 4 | zero |
| 15 | 2 | message id, little endian |
| 17 | 5 | zero |
| 22 | n | payload |

Total frame length is `len + 5`. The message id is read back from offset 15 (`get_msgid_flora`). The generic accessors switch on a glasses-type field: for
type 1 (older Air) the message id is the single byte at offset 7 and the request id is 0; for other types the message id is
the u16 at offset 15 and the request id is the u32 at offset 7 (`get_xreal_msgid`, `get_xreal_request_id`).

IMU-channel frame (`cmd_build_flora_imu`; **decoded**): byte 0 = `0xAA`, byte 7 = a one-byte command id, CRC-32 at offset 1 and
`len` at offset 5 computed the same way, with `len = 3 + payload length`, payload at offset 22 (Flora) or 8 (`cmd_build_imu`).

Not recovered: the callers of these builders and therefore the actual message ids and payloads. They are not called from
inside this library (no direct calls or function-pointer relocations were found), so they are used by the runtime or
service that is missing from the package.

## 6. Firmware-update messages (read, names only)

`libnr_glasses_api.so` exports the update path for four hardware generations: Air (`send_air_*`, `air_mcu_ota`,
`air_dp_ota`, `air_dsp_update`), Air P55 (`*_p55_*`), Flora (`send_flora_*`, `flora_soc_ota`, `flora_dp_ota`,
`flora_dsp_ota`, `flora_rommode_upgrade` with a boot-ROM USB upload) and Ella (default firmware names `default_ella_firmware`,
`default_ov580_bootloader`, `default_ov580_firmware`, audio images `default_audio_dolly/LG/universal/box/mainland`), plus a
Realsil/Rs audio flash set (`RsAudio_*`). Version and diagnostics calls: `GetAirFwVersion`, `GetFloraFwVersion`,
`GetAirP55F_FwVersion`, `GetAirP55E_FwVersion`, `Get_Air_OLED_Vsync_Number`, `Get_Glasses_FW_Version`. Strings show the
update stages: check DP/MCU/SOC/DSP version, "dp version 0" recovery, flash check, per-packet write and progress.
Entry points for the Android side: `NROTA_Init`, `NROTAStart`, `NROTASendMsg`, `NROTASendMsgWithHandle`,
`NROTAReadMsgOnlyWithHandle`, `NROTAGetHandle`, `NROTAGetNumber`. The camera has its own bootloader and firmware
("camera bootloader fw name", `default_ov580_bootloader`) and the log lines say the UVC camera and IMU channels are closed
while the DSP/DP are updated.

The app bundles update images in `assets/nr_ota_default/flora/` with `glasses.cfg` naming the MCU
(`12.1.00.489_20240612.bin`), DP (`1140.bin`) and DSP (`09.A.00.001_20230615.bin`) images. Not examined further. **Do not
send anything from these paths to the glasses**: it is a firmware writer.

## 7. What the control channel on port 52999 might correspond to (inferred)

The 52999 messages seen so far (`27 8a 00 00 00` + type `07` / `09`, with a trailing float32 looking like a temperature) are
in the same "events" family as the SDK's callbacks: temperature, over-temperature, hardware event, key event, wearing, light
intensity. This is a hypothesis; nothing in the package confirms the byte layout.

## Open questions this leaves

1. Where `libnr_api.so` and the control service come from on a phone (a separate install, or downloaded). Getting them
   (`adb shell pm path` on the phone, or the APK of the runtime app) is the next step, because the One-series messages and
   the camera start are most likely there.
2. What string `SwitchTrackingType` takes, and whether "6DoF" starts the camera on the One series.
3. The argument format and effect of `GET/SET_SLAM_CONFIG`.
4. Whether the One series is controlled over the TCP ports (as seen) or HID, and which side initiates.
5. A live capture of the phone talking to the glasses while the 6DoF mode is toggled would answer 2-4 directly.

## 8. The SDK runtime: XREAL ControlGlasses 1.1.0 (XREAL SDK 3.0.0)

**Source.** `developer.xreal.com/download` links public APKs on `public-resource.xreal.com/download/`:
`NRSDKForUnity_2.4.0_Release_20241206/ControlGlasses-1.0.0.apk`, `NRSDKForUnity_2.4.1_Release_20250102/ControlGlasses-1.0.1.apk`
and `XREALSDK_Release_3.0.0.20250314/ControlGlasses-1.1.0.20250307172552-release.apk` (324 MB, the one examined). The SDK
packages themselves were not downloaded (the page gates them). The APK and its extracted libraries are kept outside the repo.

Native libraries in it (arm64): `libnr_api.so` (38 MB, the C API the Unity plugin loads), `libnr_service.so` (39 MB, the same
code plus the glasses service), `libnr_glasses_api.so` (USB/HID/socket and firmware layer), `libnr_external_sensor.so` (plugin
host for external sensors), `libnr_dual_agent_tracking.so`, `libnr_hand_tracking.so`, `libnr_image_tracking.so`,
`libnr_meshing.so`, `libnr_spatial_anchor.so`, `libnr_rgb_camera.so`, `libnr_loader.so`, `libnr_libusb.so`, SNPE and
OpenCV. Firmware bundles for `air`, `p55`, `flora` and **`gina`** (MCU `15.1.02.221_20250210`, DSP `15.A.00.069_20241211`,
pilot `1.3.1.20250211191030`) are included; they are not examined and **must not be sent to the glasses**.

The 6DoF code in `libnr_api.so` / `libnr_service.so` is a visual-inertial SLAM stack (source paths under
`perception_6dof/vi-slam/...`: VIO estimator, bundle adjustment, loop closure, multi-map merging). Its handle setup logs
`Handle {} device type is NR_DEVICE_TYPE_INVALID, set TrackingType IMU_TRACKING_AIR` and `set default definition: LUGIN_3DOF`,
so the default tracking type is an IMU-only 3DoF one chosen by device type. Camera models it accepts:
`CAMERA_MODEL_FISHEYE`, `FISHEYE624`, `RADIAL`, `ATAN`, `POLY3`.

### 8.1 Embedded protocol descriptors (read, verbatim in `docs/nebula-protocol-descriptors.json`)

`libnr_api.so`, `libnr_service.so` and `libnr_rgb_camera.so` each carry the same six JSON descriptors ("protocol_version"
with `support_identifers`, a list of device identifiers the layout applies to). These describe the **Air / Light / Ella-era
hardware** (OV580 camera chip over UVC/HID), not the One's TCP streams, but they define the vocabulary:

| Version | Content |
|---|---|
| `0x20191212`, `0x20200323` | `gray_cam` frame header: `timestamp` u64 @0, `image_type` u16 @8, `sync_ts` u64 @10, `frame_number` u32 @18, `exposure_0` u16 @22, `gain_0` u16 @24, `signature` u32 @26; images 640x480 (resolution_height 481 includes a header row), 2 channels, 30 fps, product `Camera-OV580`; `cam_timestamp_offset` default 37600; plus a 128-byte IMU report (gyro/acc/mag as s32 with numerator/denominator scaling, temperature, per-sensor u64 timestamps; `who` = 18) |
| `0x20200316` | Text-style event/command protocol: fields `command_type`, `command_id` and ASCII payload for `vsync`, `temperature`, `ambient_light`, `temple_temperature`, `7211_update_status`, `magnetometer`, `psensor`, `key_brightness`, `sleep_event`, `brightness`, `2d3d`, `trylightscreen`, `activate`, `log_event`, `dev_status`, `support_devices`, `support_displays`, `heartbeat`, and `const_variable` listing the command types and codes (below) |
| `0x20210601` (first) | Binary message-id protocol for the MCU (`mcu_information`, `simple_messsage`, event messages `MSG_E_*`) (below) |
| `0x20210601` (second), `0x20220101` | 64-byte IMU reports (`report_id` u8, `version`, `temperature`, u64 `timestamp`, gyro/acc/mag as s16 or s24 with numerator/denominator, `sensor_timestamp`, `mag_update_flag`); the `0x20220101` form packs each axis in 3 bytes |

Command types in the `0x20200316` text protocol: `CMD_INVALID 0x00`, `CMD_SET 0x31`, `CMD_RESP_SET 0x32`, `CMD_GET 0x33`,
`CMD_RESP_GET 0x34`, `CMD_EVENT 0x35`, `CMD_SUPER 0x40`. Command codes (`CMD_*`, hex): `RW_BRIGHTNESS 31`, `RW_LEVEL_LIGHT_MAX 32`,
`RW_2D_3D_STATE 33`, `RW_SPEAKER_GAIN 36`, `RW_BRIGHTNESS_EXT 38`, `RW_POWER 39`, `RW_P_SENSOR_MIN 44`, `RW_P_SENSOR_MAX 45`,
`RW_DUTY 4D`, `RW_HOST_ID 42`, `R_GLASSES_GID 43`, `R_GLASSES_TEMP 49`, `R_HW_VERSION 46`, `R_7211_VERSION 48`, `W_7211_UPDATE 58`,
`RW_LIGHT_SWITCH 4C`, `RW_FLASH_LIGHT 57`, `RW_SLEEP_TIME 51`, `RW_VSYNC_SWITCH 4E`, `RW_MAG_SWITCH 55`, `W_OV580_RESET 54`,
`RW_TEMPERATURE 60`, `S_SET_UPGRADE_FLAG 38`, `S_BEGIN_UPGRADE 39`, `S_REBOOT_GLASSES 52`, `S_SYSTEM_SEND_STATUS 33`,
`S_HEARTBEAT 4B`, `S_SDK_VERSION 4E`, `R_GLASSES_VERSION_EXT 61`, `R_TEMPERATURE_EXT 53`, `R_MACHINE_ID 55`,
`R_GLASSES_RUN_STATUS 5A`, `R_DISPLAY_INFO 64`, `RW_OLED_BRIGHTNESS 62`, `RW_ACTIVATION 65`, `RW_ACTIVATION_TIME 66`,
`RW_PRIVILEGATION 67`, `RW_RGB_SWITCH 68`, `R_DEV_STATUS 6A`, `R_LOG_TRIGGER 6B`, `R_SUPPORT_DEVICES 6C`,
`R_SUPPORT_DISPLAYS 6D`, `RW_BRIGHTNESS_SWITCH 6E`, `RW_MAG_CALIBRATION 6F`, `R_RESERVED_SN0 70`, `R_RESERVED_SN1 71`,
`W_DISPLAY_DEFAULT_START_MODE 70`, `R_DISPLAY_DEFAULT_START_MODE 76`, `S_SWITCH_OLED 58`, `S_DUMP_DP_REGISTERS 59`,
`R_KEY_SWITCH_2D3D 79`, `W_KEY_SWITCH_2D3D 73`, `R_DECOUPLE_PSENSOR_FLAG 78`, `W_DECOUPLE_PSENSOR_FLAG 72`,
`S_GET_DISPLAY_STATUS 56`, `R_BRIGHTNESS_LEVEL_NUMBER 7A`, `R_DSP_VERSION 7B`, `R_GLASSES_TEMP_LEVEL 7C`, `R_PSENSOR_STATUS 7D`.
Several codes are reused between read and write sides with different meanings (for example `0x58`, `0x38`, `0x39`, `0x55`,
`0x70`); the descriptor does not say which are valid together, so treat the table as a vocabulary, not a spec.

Message ids of the binary `0x20210601` MCU protocol (`R` = read, `W` = write, `N` = notify, hex):
`01 R_VSYNC_FUCTION`, `02 W_VSYNC_FUCTION`, `03 R_BRIGHTNESS_LEVEL`, `04 W_BRIGHTNESS_LEVEL`, `05 R_BRIGHTNESS_FINE_GRAINED`,
`06 W_BRIGHTNESS_FINE_GRAINED`, `07 R_DISPLAY_2D_3D`, `08 W_DISPLAY_2D_3D`, `09 R_PSENSOR_VALUE`, `0B R_PSENSOR_FAR_AWAY`,
`0C W_PSENSOR_FAR_AWAY`, `0D R_HOST_ID`, `0E W_HOST_ID`, `0F R_DISPLAY_DUTY`, `10 W_DISPLAY_DUTY`, `11 R_TEMPERATURE_FUNCTION`,
`12 W_TEMPERATURE_FUNCTION`, `13 R_TEMPLE_TEMPERATURE`, `14 R_BOARD_TEMPERATURE`, `15 R_GLASSID`, `16 R_7211_FW_VERSION`,
`17 R_DEVICE_PARAMETERS`, `18 R_DSP_VERSION`, `19 W_CANCEL_ACTIVATION`, `1A W_HEARTBEAT`, `1B R_MAG_CALIBR_DATA`,
`1C W_MAG_CALIBR_DATA`, `1D R_SLEEP_TIME`, `1E W_SLEEP_TIME`, `1F W_FORCE_CTRL_OLED`, `24 R_PSENSOR_CLOSED`,
`25 W_PSENSOR_CLOSED`, `26 R_MCU_APP_FW_VERSION`, `27 R_HW_VERSION`, `29 R_ACTIVATION_TIME`, `2A W_ACTIVATION_TIME`,
`2B R_PRIVILEGATION`, `2C W_PRIVILEGATION`, `2D W_LOG_TRIGGER`, `2E R_DISPLAY_STATUS`, `30 W_TRY_CTRL_DISPLAY_STATUS`,
`31 W_SDK_VERSION`, `32 R_MACHINE_ID`, `33 R_GLASSES_RUN_STATUS`, `34 R_DEV_LIST`, `35 R_DISPLAY_LISTS`, `36 R_ACTIVATION`,
`37 W_ACTIVATION`, `38 W_OLED_ONLY_BRIGHT_LEVEL`, `39 R_OLED_ONLY_BRIGHT_LEVEL`, `3E W_UPDATE_MCU_APP_FW_PREPARE`,
`3F W_MCU_APP_FW_UPDATE_START`, `40 W_MCU_APP_FW_UPDATE_TRANSMIT`, `41 W_MCU_APP_FW_UPDATE_FINISH`, `42 W_BOOT_JUMP_TO_APP`,
`43 R_BOOT_FW_VERSION`, `44 W_MCU_APP_JUMP_TO_BOOT`, `45 W_UPDATE_DSP_APP_FW_PREPARE`, `46 W_UPDATE_DSP_APP_FW_START`,
`47 W_UPDATE_DSP_FW_TRANSMIT`, `48 W_UPDATE_DSP_FW_FINISH`, `4A W_DSP_RESET`, `4D R_OLED_COORDINATE`, `4E W_OLED_COORDINATE`,
`54 W_ENTER_SLEEP`, `55 W_SLEEP_MODE`, `56 R_SLEEP_MODE`, `57 R_RESERVED_SN0`, `58 R_RESERVED_SN1`, `60 W_HOST_TYPE`,
`61 W_IMU_FREQ_DIVIDE`, `6A R_PSENSOR_SWITCH`, `6B W_PSENSOR_SWITCH`, `72 W_CTRL_OLED`, `73 N_START_ERRORS_AND_EVENTS_REPORT`,
`7A R_GLASSES_IS_CLOSE_HEAD`, `7B R_GLASSES_BRIGHTNESS_LEVEL_NUM`, `80 W_DP_ESD_PARAM`, `81 W_DP_HDCP_ENABLE`, `83 W_DP_LEVEL`,
`84 W_KEY_FUNCTION`, `9D W_OLED_DISPLAY_MAP_PARAM`, `9E R_OLED_DISPLAY_MAP_PARAM`, `9F R_REAL_DISPLAY_MODE`,
`A1 R_SCREEN_STATUS`, `A2 R_IMU_INTERRUPT_COUNT`, `A3 R_GLASSES_DISPLAY_STATUS`, `A4 W_REBOOT_DEVICE`, `B0 R_EC_GEAR_NUM`,
`B1 R_EC_GEAR_NOW`, `B2 W_SET_EC_GEAR`, `B6 R_SWITCH_CAL_OLED`, `B7 W_SWITCH_CAL_OLED`, `C0 W_DEFAULT_DISPLAY_2D_3D`,
`C1 R_DEFAULT_DISPLAY_2D_3D`, `C2 R_TEMPERATURE_LEVEL`, `C3 W_LED_WORK_MODE`, `C4 R_LED_WORK_MODE`, `C5 W_SERIAL_LOG_STATUS`,
`C6 R_SERIAL_LOG_STATUS`, `C7 W_LED_RGB_MODE`, `C8 R_LED_RGB_MODE`.

Event messages the glasses send (`MSG_E_*`, 16-bit ids; payload offsets from the descriptor):

| Id | Name | Payload |
|---|---|---|
| `0x6C02` | TEMPLE_TEMPERATURE | `temperature` s32 @0 |
| `0x6C03` | SCREEN_ERR_LOG | (none described) |
| `0x6C04` | PSENSOR | `status` s32 @0 |
| `0x6C05` | KEY | `key_type` u32 @0, `key_func` u32 @4, `key_param` u32 @8 |
| `0x6C07` | SLEEP | `status` s32 @0 |
| `0x6C08` | 7211_UPDATE_STATUS | `status` s32 @0 |
| `0x6C09` | ERR_LOG | (none) |
| `0x6C0A` | CRC_ERROR | (none) |
| `0x6C0B` | VSYNC | `sequence` u64 @0, `timestamp` u64 @8 |
| `0x6C12` | FOREHEAD_TEMPERATURE | `temperature` s32 @0 |
| `0x6C15` | ERRORS_AND_EVENTS | (none) |
| `0x6C18` | SCREEN_STATUS_CHANGED | `status` s32 @0 |
| `0x6C19` | OVER_TEMPERATURE_ISDANGER | `status` s32 @0 |

`MSG_W_HEARTBEAT` (`0x1A`): `host_timestamp` u64 @0, `viewer_recv_timestamp` u64 @8, `viewer_send_timestamp` u64 @16.

Relation to what is already known about the One's streams (**inferred**, to be checked against captures):
- `MSG_E_VSYNC` (sequence u64 + timestamp u64) looks like the same thing as the 52996 records (timestamp u64 and a counter that
  goes up by one). The 52996 record has a 14-byte header before the u64 timestamp at offset 14, consistent with a frame header
  followed by a small payload; that header would carry the message id (`0x6C0B`).
- `MSG_E_KEY`, `MSG_E_PSENSOR`, `MSG_E_SLEEP`, `MSG_E_*TEMPERATURE` (3 x s32/u32) match the "temperature-type float32"
  status messages seen on 52999 only loosely (those carry a float32), so 52999 may use a newer layout.
- `MSG_W_IMU_FREQ_DIVIDE` (0x61) is the message behind `NRGlassesControlSetIMUFrequencyDivider` (section 3).
- `MSG_N_START_ERRORS_AND_EVENTS_REPORT` (0x73) is the message behind `NRGlassesControlStartErrorsAndEventsReport`. Event
  streams on 52999 may need it, or may be pushed unasked.

### 8.2 Channel names for the One series (read)

`libnr_glasses_api.so` (`GLASS_*` identifiers): `GLASS_GINA`, `GLASS_GINA_KERNEL`, `GLASS_GINA_KERNEL_IMU`,
`GLASS_GINA_KERNEL_VSYNC`, `GLASS_GINA_UBOOT`, `GLASS_GINA_UBOOT_ROM`, with ROM identifiers `GINA_ROM_VID`/`GINA_ROM_PID`; the
dex has `SDK_GLASS_GINA_L` and `SDK_GLASS_GINA_M`. "Gina" is XREAL's code name for the One-series glasses. It lists
**separate IMU and VSYNC channels** next to the kernel (control) channel, which matches the One's separate IMU (52998) and
timestamp (52996) ports. Other generations in the same list: `GLASS_AIR*`, `GLASS_AIR_P55_IMU`, `GLASS_AIR_HONOR_IMU`,
`GLASS_FLORA_*`, `GLASS_GF_KERNEL_IMU`. The library connects to a Gina glass **by TCP socket** over the two link-local
addresses (**read**): it tries `169.254.1.1` and, if that fails, `169.254.2.1` ("gina ip 0 connect failed, try ip 1"),
connection type from `connect type : %d`. The control socket used by the firmware-update path is TCP port **50180** (from the
`sockaddr` in `get_socket_connection`, **decoded**); the update commands are `read_gina_bin_version`, `read_gina_flash_type`,
`read_gina_mtd_numbers`, `read_gina_mtd_list`, `read_gina_mmc_list`, `read_gina_start_partition`, `write_gina_bin_start/
transmit/finish`, `write_gina_img_info_start/transmit/finish`, `write_gina_segment_start/transmit/finish`. These are
firmware writers; do not use them. The ordinary control port(s) were not located; the IMU and camera interfaces are
reported by the library as "imu interface: in max length %d, out max length %d" (the IMU is a bidirectional endpoint).

### 8.3 Frame format of the control channel (decoded in `libnr_glasses_api.so`)

Same framing as section 5, now in the newer library: `cmd_build_sdk` (command frame, byte 0 `0xFD`, CRC-32 at 1, u16 `len` at
5, request id u32 at 7, message id u16 at 15, payload at 22) and `cmd_build_imu` (byte 0 `0xAA`, one-byte command id at 7,
payload at 8). `send_xreal_usb_msg(_timeout)`, `xreal_send_usb_only`, `xreal_read_usb_only`, `get_xreal_msgid`,
`get_xreal_request_id`, `get_xreal_event_back`. A direct caller of these builders was again not found inside the library
(they are exported for `libnr_service.so`). **The One's streams use a different leading byte (`0x27 ...`, `0x28 ...`)** than
`0xFD`/`0xAA`, so the One's TCP messages are probably framed by another builder or the same content with a different header;
that mapping is not established.

### 8.4 Camera-related calls in the control layer (read / partly decoded)

- `NRBSPSetUsbConfig`, `NRBSPSetUsbConfigAll`, `NRBSPGetUsbConfig`, `NRBSPGetUsbConfigAll` (plus `...WithHandle` forms):
  get/set the glasses' USB gadget configuration. **Corrected after tracing (section 9.2):** this is not a camera switch. It is
  a bit field of USB functions (`ncm`, `ecm`, `uac`, `hid_ctrl`, `mtp`, `mass_storage`, `enable`), message ids 0xD2 / 0xD3.
- `NRBSPGetCameraStatus` (message 0xD5): queries camera state; the reply format is not decoded (section 9.2).
- `NRGlassesControlResetOv580` (`nativeResetOv580`): resets the OV580 camera chip (`CMD_W_OV580_RESET 0x54` in the older
  text protocol).
- Camera-side classes in `libnr_service.so`: `CameraOv580`, `Ov580TimeAlign`, `ImuCtlProtocol_Ov580`,
  `ImuProtocolOv580Factory`, `CamProtocol_Generic_Ov580`, `ImuDataProtocol_Generic_Ov580`, `ConfigFlashIO_Ov580`: a camera and
  IMU time-alignment component (`Ov580TimeAlign`) exists, which is the camera-IMU offset problem in task 1.3.
- In `libnr_external_sensor.so`: a plugin host (`ExternalSensor_Register/Initialize/Start/Stop/SetProperty/GetProperty/Pause/
  Resume/Release/SetHeartbeatCallback`), `EXT_DEVICE_TYPE`, `EXT_PARAMETER_REQUEST`, `EXT_LIGHT_6DOF`, and
  `PluginJSON2Slamconfig` / `nrealJSON2Slamconfig` / `nrealGlobalConfigJSON2Slamconfig`: SLAM configuration is a JSON document
  converted to the tracker's config, so `GET/SET_SLAM_CONFIG` (section 4) very likely carries JSON.

## Open questions after this pass

1. Which message (or USB config value) makes the Eye stream. The candidates are in 8.4; a live capture of Nebula/ControlGlasses
   against the glasses, or tracing `NRBSPSetUsbConfig` / `SwitchTrackingType` into the sender, would settle it.
2. How the One's `0x27 ...` TCP frames map to the `0xFD`/`0xAA` builders above, and which port carries control requests.
3. The accepted values for `SwitchTrackingType` and the JSON schema of the SLAM config.
4. Whether the glasses accept host commands on the TCP ports at all, or only the Android service's own USB/HID path.

Nothing has been sent to the glasses by this work.

## 9. Messages the SDK's glasses layer can send (traced in `libnr_glasses_api.so`)

Method: every call to `send_xreal_usb_msg` / `send_xreal_usb_msg_timeout` / `cmd_build_*` in the library was found through
its PLT stub (the earlier "no callers" result was because exported functions are called through the PLT), and the message id
(the second argument, `w1`) and payload length (`w3`) read from the instructions before the call. Ids are read from immediate
values, so they are reliable where listed; payload meanings are only given where the code shows them.

### 9.1 Table (decimal id, hex id, sending function, payload length when constant)

| Id (hex) | Sent by | Payload |
|---|---|---|
| 38 (0x26) | `wait_for_service_ready` | none seen (`R_MCU_APP_FW_VERSION` in the MCU table) |
| 203 (0xCB) | `NROTAGetSDKGlassType` | none (reply fills `XrealGlassTypeInfo`: type and hardware id; types `SDK_GLASS_GF`, `SDK_GLASS_GINA_L`, `SDK_GLASS_GINA_M`, `SDK_GLASS_UNKNOWN`, `SDK_GLASS_ERROR`) |
| 204 (0xCC) | `NROTAGetSKUString` | none (reply is the SKU string) |
| 207 (0xCF) | `NROTAGetFiles` | file-list query (with 208 / 209: list, read, finish) |
| 208 (0xD0) | `NROTAGetFiles` | |
| 209 (0xD1) | `NROTAGetFiles` | |
| 210 (0xD2) | `NRBSPGetUsbConfig*` | none: reads the USB gadget config |
| 211 (0xD3) | `NRBSPSetUsbConfig*` | 4 bytes, see 9.2 |
| 212 (0xD4) | `get_internal_code_for_handle` | none |
| 213 (0xD5) | `NRBSPGetCameraStatus*` | none: camera status query |
| 62 / 68 (0x3E / 0x44) | `NROTASetGlassToBoot`, `NROTARebootGlass` | jump to boot / reboot (MCU table: `W_UPDATE_MCU_APP_FW_PREPARE`, `W_MCU_APP_JUMP_TO_BOOT`) |
| 28672-28676 (0x7000-0x7004) | `NROTAGetOfflineLog*` | offline-log reads (0x7001 and 0x7002 carry 4 bytes) |
| 28683 (0x700B) | `NROTAExecCMD*` | **runs a command on the glasses**; do not use |
| 4608-4631 (0x1200-0x1217) | Gina boot/update path | `read_gina_mtd_numbers 0x1200`, `read_gina_mtd_list`/`mmc_list 0x1201`, `read_gina_start_partition 0x1202`, `write_gina_img_info_start 0x1203` (8 bytes), `..._transmit 0x1204`, `..._finish 0x1205`, `write_gina_segment_start 0x1206` (20 bytes), `..._transmit 0x1207`, `..._finish 0x1208` (1 byte), `read_gina_bin_version 0x1209`, `write_gina_bin_start 0x120A`, `..._transmit 0x120B`, `..._finish 0x120C`, `read_gina_flash_type 0x120D`, `set_clear_usrdata_part 0x1212` (1 byte), `read_app_version 0x1213`, `upgrade_app_start 0x1214`, `upgrade_app_transmit 0x1215`, `upgrade_app_finish 0x1216`, `get_boot_crc 0x1217` |
| 21, 22, 24, 38, 60-72, 116, 184, 186 | DP / DSP / MCU update helpers | firmware transfer steps (`xreal_glasses_dp_upgrade`, `xreal_send_dsp_audio_fw`, `xreal_send_mcu_soc_fw`) |

Everything with a firmware, boot or flash purpose in this table writes to the glasses' flash. **None of it should ever be sent
from our tools.** The read-only ones (0xCB, 0xCC, 0xD2, 0xD5, 0x1200-0x1202, 0x1209, 0x120D, 0x1213) are the only candidates
for a careful, user-approved probe later.

### 9.2 Details worth keeping

- **USB config (0xD2 / 0xD3).** The Java class `UsbConfigList` has seven int fields in this order: `ncm`, `ecm`, `uac`,
  `hid_ctrl`, `mtp`, `mass_storage`, `enable`. `NRBSPSetUsbConfig(iface, value)` builds a u32 with `(value & 3)` placed at bit
  `2 * iface` for iface 0..6 (decoded from the jump table: iface 0 -> shift 0, 1 -> 2, 2 -> 4, 3 -> 6, 4 -> 8, 5 -> 10,
  6 -> 12), so each of the seven functions has a 2-bit setting; `NRBSPSetUsbConfigAll` writes all seven at once. Matching the
  two network functions we already see on the Deck (CDC NCM at 169.254.2.1 and CDC ECM at 169.254.1.1) is **inferred** from
  the field names. The meaning of the 2-bit values is not shown (0 = off is the likely value of the default constructor).
  This is a real host-to-glasses configuration message, so it could change which USB functions the glasses present: do not
  send it.
- **Camera status (0xD5).** Replies are consumed by `OtaManager.getCameraStatus() -> int`; the status values are not visible.
  The OTA code uses it to decide whether the camera needs its bootloader or firmware updated ("camera bootloader fw name",
  "camera fw name"), so it probably reports the camera module's firmware state, not whether it is streaming.
- **Glass type (0xCB).** The glasses report their own type (`GF`, `GINA_L`, `GINA_M`); `GF` is probably the Air 2 Ultra family
  and the two `GINA` values the One / One Pro.

### 9.3 The TCP ports are not used by this SDK

No port number in 52990-52999 appears as an immediate or in a port table in any `libnr_*` library of ControlGlasses 1.1.0, nor
in Nebula's own libraries (the decoder was validated by finding the OTA port, 50180, in `get_socket_connection`). The only TCP
client found is the firmware path (port 50180 on the two link-local addresses). The SDK's Gina IMU and VSYNC channels are USB
endpoints (`GLASS_GINA_KERNEL_IMU`, `GLASS_GINA_KERNEL_VSYNC`; "imu interface: in max length %d, out max length %d"). So the
camera/IMU/timestamp streams on 52996-52998 that we read are likely a different service in the glasses (for example for
desktop clients), and the Android SDK's way to enable the Eye is by a USB message or mode that is not in this table.

## Where this leaves the camera question

1. The ControlGlasses SDK has no TCP-stream code and no message that reads like "start camera". The camera on the One is likely
   started by firmware logic keyed to a mode or SLAM configuration the SDK sets over USB (`GET/SET_SLAM_CONFIG`, or
   `SwitchTrackingType`).
2. The remaining places to look are `libnr_service.so`'s handling of its Gina device (how it receives camera frames: its
   OV580 classes are UVC/HID-based), and a live capture of the USB traffic between a phone and the glasses.
3. A read-only probe from the Deck (for example 0xCB, 0xCC, 0xD5) would need its own approval and a working transport
   (USB HID/bulk, not the TCP ports).

## 10. The glasses RPC layer (found in `libnr_api.so` / `libnr_service.so`)

### 10.1 What it is

Both libraries contain the same RPC client: a generated protobuf-lite message pair per request (`proto.NR<Name>Base`,
`proto.NR<Name>Req`, `proto.NR<Name>Rsp`; 586 type names in all) and handlers named `NR<Name>::HandleRpcResponse` /
`::HandleRpcTimeout` (183 requests, **read**). Log strings show the shape of a call: `[{}] Call NRGrayscaleCameraInitSetExposureTime
start` / `end, {}`, `NRGlassesGetSWVersion::HandleRpcTimeout`, `N get rsp time {}`, `N get rsp timeout`. Requests have a
response and a timeout, so the layer is a request/response protocol with an id-to-handler dispatch. Because the build uses
protobuf-lite, **field names and numbers are not in the binary**; they can only be recovered from the generated serialisation
code or from live traffic. All 183 request names and the 586 type names are in `docs/nebula-rpc-names.txt`.

Families (counts): Audio 27, Display 30, Dp 24, Ec 5 (electrochromic dimming), Glasses 28 (+2 get/set CPU mode), GrayscaleCamera 8,
Imu 5, Led 2, Misc 4, PowerSave 8, Proximity 8, RgbCamera 16, Storage 7, Temperature 3, Usb 2, Vsync 2, plus `NRRebootGlasses`
and `NRShutdownGlasses`.

### 10.2 The requests that matter for tracking

| Request | Role (from the name; semantics unverified) |
|---|---|
| `NRGrayscaleCameraCreate` | create the tracking camera object on the glasses |
| `NRGrayscaleCameraInitSetAutoExposureType`, `...SetExposureTime`, `...SetGain`, `...SetImageResolution`, `...SetPixelFormat` | pre-start camera configuration |
| `NRGrayscaleCameraStart`, `NRGrayscaleCameraStop` | start / stop the camera stream |
| `NRImuStart`, `NRImuStop`, `NRImuStartExt`, `NRImuStopExt`, `NRImuSetFrequencyExt` | IMU stream control (the IMU also has a frequency divider, `MSG_W_IMU_FREQ_DIVIDE 0x61`) |
| `NRVsyncStart`, `NRVsyncStop`, `NRGlassesGetVsyncOffsetTime` | display-refresh timestamp stream and its offset to the sensor clock |
| `NRUsbGetNetworkEnable`, `NRUsbSetNetworkEnable` | turn the USB network function on or off |
| `NRGlassesStartEventsReport`, `NRGlassesStopEventsReport`, `NRGlassesSetNetLogEnable` | event stream and network log |
| `NRGlassesGetStartupState` | boot/readiness state |
| `NRGlassesSetSpaceMode`, `NRGlassesSetSceneMode`, `NRGlassesRecenter` | space / scene mode and recentre (likely where 3DoF/6DoF and stabilizer-like behaviour is set) |
| `NRGlassesSetUltraWideEnable`, `NRGlassesGetUltraWideEnable` | enable the ultra-wide camera (the camera hardware on the Air 2 Ultra / Eye class) |
| `NRRgbCameraCreate/InitSet*/Start/Stop/Release/GetConfig/SetConfig/GetPluginState` | RGB camera (same shape as the grayscale one, plus plugin state) |
| `NRGlassesGetConfig`, `NRMiscGetDeviceType`, `NRMiscGetHostType`, `NRMiscSetScheduler`, `NRGlassesGetSupportedDevices` | device and host identification, scheduling |

The grayscale-camera, IMU and vsync start/stop requests have the same names as the three streams we read from the One
(camera 52997, IMU 52998, timestamps 52996), and the camera stream being idle until something starts it is exactly what a
`NRGrayscaleCameraStart` request would explain (**inferred**). The glasses themselves start the camera for their own anchor
mode, which is consistent with the stream appearing there.

### 10.3 The transport is USB, not the TCP ports

`libnr_api.so` and `libnr_service.so` import libusb (`libusb_claim_interface`, `libusb_interrupt_transfer`,
`libusb_control_transfer`, `libusb_detach_kernel_driver`, ...) and are linked against `libnr_libusb.so`; no TCP port in
52990-52999 is used (the immediate-value scan, validated on the OTA port 50180, finds only ordinary constants; the libraries do import
`connect`, but its use was not traced, so a TCP path cannot be fully ruled out).

USB layout of the connected XREAL 1S (read with `lsusb -v` on the Deck, 3318:043e), which maps onto the SDK's `UsbConfigList`
function names (`ncm`, `ecm`, `uac`, `hid_ctrl`, ...):

| Interface | Class | Endpoints | Role |
|---|---|---|---|
| 0 | HID (3) | EP1 IN and EP1 OUT, interrupt, 1024 bytes | control channel (`hid_ctrl`): the likely carrier of the RPC |
| 1 | Communications, subclass 13 (NCM control) | EP3 IN interrupt | CDC NCM (169.254.2.x) |
| 2 | CDC Data, alt 1 | EP2 IN/OUT bulk, 512 | NCM data |
| 3 | Communications, subclass 6 (Ethernet) | EP5 IN interrupt | CDC ECM (169.254.1.x) |
| 4 | CDC Data, alt 1 | EP4 IN / EP3 OUT bulk, 512 | ECM data |
| 5, 6, 7 | Audio (control, streaming out, streaming in) | EP4 OUT, EP6/EP7 IN | UAC audio |
| 8 | HID (3), boot-interface subclass | EP8 IN, EP5 OUT | second HID, probably the input/keys channel |

On the Deck the HID nodes appear as `/dev/hidraw0..5` (world-readable and writable: `crw-rw-rw-`); which of them belongs to
interface 0 or 8 has not been checked.

### 10.4 What is still unknown

1. The framing of an RPC on the wire: the request id or message id, the length, any checksum, and how Base wraps Req. The 0xFD
   frame of sections 5 and 8.3 (CRC-32, `len`, request id u32 at 7, message id u16 at 15, payload at 22) is the best guess for
   the container, with the protobuf as payload, but the SDK's RPC client was not traced to confirm this.
2. The protobuf fields of `NRGrayscaleCameraStartReq` and of its `Create` / `InitSet*` requests, and the response.
3. Whether the One's firmware accepts the same RPC over its TCP service (the streams on 52996-52998 exist, but this SDK
   never uses them).
4. Whether the camera will stream with the glasses in Follow mode and the Stabilizer off once started, or whether the glasses
   only allow it in a mode that forces the stabilizer on.

### 10.5 Safe next steps

- Passive: read the HID interrupt IN endpoints (`/dev/hidraw*`) for a short time while changing nothing, to see whether the
  glasses send any unsolicited reports and what their framing looks like. No bytes are sent. Needs the user's go-ahead because
  it opens a control device.
- Static: trace the RPC client in `libnr_api.so` from `NRGrayscaleCameraStart::` to the serialiser and the HID write, to
  recover the container format and field numbers. Offline only.
- A phone-to-glasses USB capture while Nebula starts and stops tracking would give both for free.
- Sending any request, even a read-only one, to the glasses is a new kind of action for this project (nothing has been sent so
  far) and needs explicit approval first.

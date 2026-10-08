# XrealLink message ids and the One's TCP stream framing

Derived from XREAL ControlGlasses 3.1.0 (`libnr_service.so`) and checked against a real capture. Companion to
`docs/nebula-findings.md`. Nothing here has been sent to the glasses.

## 1. Framing (verified on the camera stream, inferred for the others)

Every packet of the One's TCP streams starts with:

| Offset | Size | Field |
|---|---|---|
| 0 | 2 | `msg_id`, big endian |
| 2 | 4 | payload length, big endian (bytes that follow this field; total packet = length + 6) |
| 6 | ... | rest of the packet header and payload |

Verified on `captures/cam_52997_sample.bin` (a recorded camera stream): the first bytes are `27 48 00 02 f5 40`, i.e. `msg_id` = 0x2748 =
**10056** and length = 0x0002F540 = 193,856, so each frame is 193,862 bytes and consecutive frames start at exact multiples
(offsets 0x0, 0x2F546, 0x5EA8C, 0x8DFD2). This is the same `msg_id` space as the SDK's `XrealLink` packets
(`packet_msg group_id:{} msg_id:{} packet_id:{} freq_count:{} header.timestamp_ns:{}` in the service's log format). The other stream
magics from `docs/findings.md` fit the same rule, **not yet re-verified on live data** (the Deck was off):

| Stream | First bytes | `msg_id` | Length field | Total |
|---|---|---|---|---|
| 52997 camera | `27 48 00 02 f5 40` | 10056 (verified) | 193,856 (verified) | 193,862 |
| 52996 timestamps | `27 31 00 00 00 20` | 10033 | 32 | 38 (matches the observed 38-byte records) |
| 52998 IMU | `28 36 00 00 00 80` | 10294 | 128 | 134 (matches the observed 134-byte records) |
| 52999 events | `27 8a 00 00 00 ..` | 10122 | per message | |

Header fields after the length are named in the SDK log line (`group_id`, `packet_id`, `freq_count`, `timestamp_ns`); the u64
nanosecond timestamp at offset 14 and the counters seen in the records are consistent with that, but the exact layout of bytes 6-21 is
not decoded (the camera header bytes 6-21 are constant across frames: `00 00 00 00 01 f8 01 00 00 7a 01 00 00 00 02 00`).

## 2. Request message ids (179 of 183 resolved)

How the table was made (**read** from the code): the service registers every message id at start-up with a handler object
(`map<uint16 id, handler>`, 323 ids); each request's handler class has a `NR<Name>::HandleRpcRequest` method, so the id registered
immediately before that handler's vtable is the request id. The ids come out consecutive and grouped by function (the grayscale-camera
configuration calls are 10047-10052, IMU 10036/10037, vsync 10031/10032), which supports the mapping. Payloads are protobuf-lite
messages (`proto.NR<Name>Req`/`Rsp`; empty for `NRGrayscaleCameraStart`).

| Id | Request |
|---|---|
| 10003 | NRPowerSaveIsEnable |
| 10004 | NRPowerSaveSetEnable |
| 10005 | NRPowerSaveGetSleepTime |
| 10006 | NRPowerSaveSetSleepTime |
| 10008 | NRProximityIsEnable |
| 10009 | NRProximitySetEnable |
| 10010 | NRDisplayGetBrightnessLevelCount |
| 10011 | NRDisplayGetBrightnessLevel |
| 10012 | NRDisplaySetBrightnessLevel |
| 10013 | NRGlassesGetSWVersion |
| 10014 | NRGlassesSetSDKVersion |
| 10015 | NRGlassesGetConfig |
| 10016 | NRGlassesGetSupportedDevices |
| 10017 | NRGlassesGetVsyncOffsetTime |
| 10018 | NRGlassesGetMagCalibrationData |
| 10019 | NRGlassesSetMagCalibrationData |
| 10020 | NRGlassesStartEventsReport |
| 10021 | NREcGetLevelCount |
| 10022 | NREcGetLevel |
| 10023 | NREcSetLevel |
| 10025 | NRGlassesGetID |
| 10026 | NRGlassesGetSN |
| 10027 | NRGlassesGetSystemVersion |
| 10028 | NRGlassesGetHWVersion |
| 10029 | NRGlassesGetDspVersion |
| 10031 | NRVsyncStart |
| 10032 | NRVsyncStop |
| 10034 | NRRebootGlasses |
| 10035 | NRShutdownGlasses |
| 10036 | NRImuStart |
| 10037 | NRImuStop |
| 10039 | NRProximityGetFarThreshold |
| 10040 | NRProximitySetFarThreshold |
| 10041 | NRProximityGetNearThreshold |
| 10042 | NRProximitySetNearThreshold |
| 10043 | NRProximityGetValue |
| 10044 | NRProximityGetWearingState |
| 10047 | NRGrayscaleCameraCreate |
| 10048 | NRGrayscaleCameraInitSetPixelFormat |
| 10049 | NRGrayscaleCameraInitSetImageResolution |
| 10050 | NRGrayscaleCameraInitSetAutoExposureType |
| 10051 | NRGrayscaleCameraInitSetExposureTime |
| 10052 | NRGrayscaleCameraInitSetGain |
| 10057 | NRLedSetEnable |
| 10058 | NRLedGetEnable |
| 10059 | NRDisplayGetLuminanceMaxValue |
| 10060 | NRDisplayGetLuminanceMinValue |
| 10061 | NRDisplayGetLuminanceValue |
| 10062 | NRDisplaySetLuminanceValue |
| 10063 | NRDisplayGetDutyMaxValue |
| 10064 | NRDisplayGetDutyMinValue |
| 10065 | NRDisplayGetDutyValue |
| 10066 | NRDisplaySetDutyValue |
| 10067 | NRDisplaySetScreenEnable |
| 10068 | NRDisplayGetScreenEnable |
| 10069 | NRDisplayGetColorTemperature |
| 10070 | NRDisplaySetColorTemperature |
| 10071 | NRDisplayGetCurrentResolution |
| 10072 | NRDisplaySetCurrentResolution |
| 10073 | NRDisplayGetDefaultResolution |
| 10074 | NRDisplaySetDefaultResolution |
| 10075 | NRDisplayGetColorCalibrationType |
| 10076 | NRDisplaySetColorCalibrationType |
| 10078 | NRDpGetCurrentEdid |
| 10079 | NRDpSetCurrentEdid |
| 10080 | NRDpGetCurrentResolution |
| 10081 | NRDpGetDefaultEdid |
| 10082 | NRDpSetDefaultEdid |
| 10083 | NRDpSetHDCPEnable |
| 10084 | NRDpSetWorkingMode |
| 10085 | NRDpGetWorkingState |
| 10087 | NRPowerSaveEnter |
| 10089 | NRAudioInStart |
| 10090 | NRAudioInStop |
| 10091 | NRAudioGetCurrentMode |
| 10092 | NRAudioSetCurrentMode |
| 10093 | NRAudioGetDefaultMode |
| 10094 | NRAudioSetDefaultMode |
| 10095 | NRAudioIncreaseUacVolume |
| 10096 | NRAudioDecreaseUacVolume |
| 10097 | NRAudioGetVolumeMaxValue |
| 10098 | NRAudioGetVolumeMinValue |
| 10099 | NRAudioGetVolumeValue |
| 10100 | NRAudioSetVolumeValue |
| 10101 | NRAudioGetAlgorithm |
| 10102 | NRAudioSetAlgorithm |
| 10103 | NRAudioGetPAEnable |
| 10104 | NRAudioSetPAEnable |
| 10107 | NRRgbCameraInitSetAutoExposureType |
| 10108 | NRRgbCameraInitSetExposureTime |
| 10109 | NRRgbCameraInitSetGain |
| 10110 | NRRgbCameraCreate |
| 10111 | NRRgbCameraInitSetPixelFormat |
| 10112 | NRRgbCameraInitSetImageResolution |
| 10115 | NRRgbCameraRelease |
| 10116 | NRRgbCameraGetPluginState |
| 10119 | NREcGetValue |
| 10120 | NREcSetValue |
| 10121 | NRTemperatureGetValue |
| 10149 | NRAudioPlay |
| 10211 | NRAudioGetPAForceSilent |
| 10212 | NRAudioSetPAForceSilent |
| 10215 | NRAudioGetPAForceSound |
| 10216 | NRAudioSetPAForceSound |
| 10217 | NRMiscSetScheduler |
| 10218 | NRAudioGetVolumePercentage |
| 10219 | NRAudioSetVolumePercentage |
| 10220 | NRDisplayGetColorTemperatureBaseline |
| 10221 | NRDisplaySetGammaEnable |
| 10222 | NRGlassesGetBootCount |
| 10223 | NRRgbCameraInitSetCompression |
| 10224 | NRDisplaySetScreenEnableBsp |
| 10225 | NRDisplayGetScreenEnableBsp |
| 10226 | NRDpGetCurrentEdidBsp |
| 10227 | NRDpSetCurrentEdidBsp |
| 10228 | NRDpGetCurrentResolutionBsp |
| 10229 | NRGlassesGetSystemVersionCode |
| 10230 | NRGlassesGetProductName |
| 10231 | NRStorageGetAvailable |
| 10232 | NRStorageGetTotalSize |
| 10233 | NRStorageGetFreeSize |
| 10234 | NRStorageClearAll |
| 10235 | NRStorageSetFormat |
| 10236 | NRStorageSetMode |
| 10237 | NRStorageGetMode |
| 10238 | NRGlassesGetUsbVid |
| 10239 | NRGlassesGetUsbPid |
| 10240 | NRMiscGetDeviceType |
| 10241 | NRRgbCameraGetSN |
| 10243 | NRRgbCameraGetConfig |
| 10244 | NRRgbCameraSetConfig |
| 10245 | NRPowerSaveGetSleepTimeLevelCount |
| 10246 | NRPowerSaveGetSleepTimeLevel |
| 10247 | NRPowerSaveSetSleepTimeLevel |
| 10249 | NRDpGetDataInterruptEnable |
| 10250 | NRDpSetDataInterruptEnable |
| 10253 | NRDpGetDataTransmitMode |
| 10254 | NRDpSetDataTransmitMode |
| 10255 | NRDpGetCurrentEdidAndAudioBsp |
| 10256 | NRDpSetCurrentEdidAndAudioBsp |
| 10257 | NRAudioGetVolumeThousandth |
| 10258 | NRAudioSetVolumeThousandth |
| 10259 | NRMiscGetSystemUpgradeState |
| 10260 | NRAudioGetHostForceSilent |
| 10261 | NRAudioSetHostForceSilent |
| 10263 | NRGlassesGetSNCode |
| 10264 | NRGlassesGetSNValue |
| 10265 | NRGlassesGetStartupState |
| 10266 | NRMiscGetHostType |
| 10267 | NRSetGlassesCpuFrequencyMode |
| 10268 | NRGetGlassesCpuFrequencyMode |
| 10269 | NRGlassesStopEventsReport |
| 10270 | NRDisplayGetColorTemperatureLevelCount |
| 10271 | NRDisplayGetColorTemperatureLevel |
| 10272 | NRDisplaySetColorTemperatureLevel |
| 10273 | NRDpGetInputMode |
| 10274 | NRDpSetInputMode |
| 10275 | NRTemperatureGetStateProcessEnable |
| 10276 | NRTemperatureSetStateProcessEnable |
| 10277 | NRGlassesGetUltraWideEnable |
| 10278 | NRGlassesSetUltraWideEnable |
| 10279 | NRGlassesRecenter |
| 10280 | NRGlassesSetNetLogEnable |
| 10281 | NRGlassesSetSceneMode |
| 10282 | NRRgbCameraGetSNValue |
| 10283 | NRRgbCameraGetSNCode |
| 10284 | NRGlassesSetSpaceMode |
| 10285 | NRDpGetDataFilterModeBsp |
| 10286 | NRDpSetDataFilterModeBsp |
| 10287 | NRDpGetDataFilterMode |
| 10288 | NRDpSetDataFilterMode |
| 10289 | NRDpGetDataFilterModeCount |
| 10290 | NRDisplayGetCallbackEnable |
| 10291 | NRDisplaySetCallbackEnable |
| 10292 | NRUsbSetNetworkEnable |
| 10293 | NRUsbGetNetworkEnable |
| 10295 | NRImuStartExt |
| 10296 | NRImuStopExt |
| 10297 | NRImuSetFrequencyExt |

Four requests could not be resolved by this method (their handler strings are different) and are **inferred from the gaps** in the
numbering: `NRGrayscaleCameraStart` = **10053** and `NRGrayscaleCameraStop` = **10054** (between `InitSetGain` 10052 and the
camera frame stream 10056), `NRRgbCameraStart` = **10113** and `NRRgbCameraStop` = **10114** (between `InitSetImageResolution`
10112 and `Release` 10115). Treat these four as unconfirmed.

## 3. Other registered ids (not requests)

Registered but not tied to a request name by this analysis (143 ids): 1-2, 8, 200-201, 210-214, 220-222, 323, 400, 500, 600, 10000-10002, 10007, 10024, 10030, 10033, 10038, 10045, 10053-10054, 10056, 10077, 10086, 10113-10114, 10117-10118, 10122, 10242, 10262, 10294, 10298-10313, 10318-10332, 11000-11005, 12000-12046, 12048-12065, 65500-65501. Their roles are inferred from the gaps:
**10033** is the vsync data packet, **10056** the camera frame, **10294** the IMU data packet, **10122** the status/event
packet, which are exactly the four stream types seen on the One's ports. The ranges 11000-11005 and 12000-12065 and the small ids
(1, 2, 8, 200-222, 323, 400, 500, 600, 65500, 65501) are the service's other messages (client/server notifications such as
`NotifyClientInfo`, `NotifyHMDInfo`, `NotifyPose`, `Heartbeat`, `StartControl`, `StopControl`); the id of each was not resolved.

## 4. What this implies (and does not)

- **The One's glasses speak the SDK's own protocol on their TCP ports**, with the same `msg_id` numbers as the SDK's request names.
  The camera stream we already read *is* message 10056, and the IMU and vsync streams are 10294 and 10033. That strongly suggests the
  glasses implement the same request set (for example `NRGrayscaleCameraStart` = 10053) and that the idle camera is waiting for such a
  request (**inferred**; the earlier finding that the camera runs in anchor mode fits).
- **Not established:** which of the ports 52990-52999 accepts requests, the exact header bytes 6-21 a request must carry (group id,
  packet id, ...), any handshake before requests are accepted, and the protobuf field numbers of the `Create`/`InitSet*` requests.
- **Nothing was sent.** Sending any of these would be the first host-to-glasses message of this project and needs explicit approval and
  a plan (start with read-only requests such as `NRGlassesGetSWVersion` 10013, `NRGlassesGetStartupState` 10265).

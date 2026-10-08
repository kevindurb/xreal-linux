# XrealLink message ids and the One's TCP stream framing

Derived from XREAL ControlGlasses 3.1.0 (`libnr_service.so`) and checked against a real capture. Companion to
`docs/nebula-findings.md`. Sections 1-10 are offline analysis; sections 11-13 record what was sent to the glasses (read-only requests only).

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

## 5. Verification against the captures in `captures/` (added)

All checks below were run on files already on disk (no glasses involved):

| Capture | Packets parsed with `msg_id` (BE u16) + length (BE u32) | Result |
|---|---|---|
| `imu_move.bin` | 53,190 | all `msg_id` **10294**, length 128, zero skipped bytes |
| `imu_yaw.bin` | 27,995 | all 10294, length 128, zero skipped bytes |
| `mv_52998.bin` | 34,934 | all 10294, length 128, zero skipped bytes |
| `xreal_52998_still.bin` | 6,993 | all 10294, length 128, zero skipped bytes |
| `mv_52996.bin` | 2,997 | all **10033**, length 32, zero skipped bytes (38-byte records) |
| `mv_52999.bin` | 5 | all **10122**, lengths 7 and 9 |
| `cam_52997_sample.bin` | 6 complete frames | **10056**, length 193,856, frames at exact 193,862-byte steps |
| `mv_52990.bin` ... `mv_52995.bin` | 0 | empty: those ports sent nothing |

So the framing `msg_id:u16be, length:u32be, payload` is verified for the camera, IMU, timestamp and event streams.

## 6. The payloads: protobuf-lite (event stream decoded)

The event port's messages decode as ordinary protobuf wire format. Message **10122** (a temperature notification):

| Payload (hex) | Decoded |
|---|---|
| `1a 05 15 00 00 70 42` | field 3 = { field 2 = float 60.0 } |
| `1a 07 08 02 15 9a 99 2f 42` | field 3 = { field 1 = 2, field 2 = float 43.9 } |
| `1a 07 08 01 15 00 00 5a 42` | field 3 = { field 1 = 1, field 2 = float 54.5 } |
| `1a 05 15 00 00 74 42` | field 3 = { field 2 = float 61.0 } |
| `1a 07 08 01 15 33 33 5b 42` | field 3 = { field 1 = 1, field 2 = float 54.8 } |

Field 1 is a sensor index (absent = 0), field 2 the temperature in degrees C. These are the five values seen in the earlier
status-message analysis in `docs/findings.md`, which is now explained: **52999 carries temperature notifications as protobuf**.

The wrapper has the same shape as the SDK's per-request `...Base` classes: `Base { field 3 = request/notification body,
field 4 = response body }` (parser constants `0x1a` = field 3 and `0x22` = field 4, section 11.10 of `nebula-findings.md`). The other
streams' payloads are not protobuf at the outer level (the IMU and timestamp records are fixed binary layouts, and the camera payload
starts with a binary block that carries a u64 nanosecond timestamp at packet offset 23 plus image meta data followed by the image),
but they share the same `msg_id` + length envelope.

IMU packet (10294, 128-byte payload): bytes 6-7 vary per packet (`38 41` or `28 be`), byte 8 is `fe` or `ff`, then zeros, the u64 nanosecond
timestamp is at packet offset 14, as documented in `docs/findings.md`. Timestamp packet (10033, 32-byte payload): payload starts with 8 zero
bytes, timestamp at offset 14, counter at offset 22.

## 7. Predicted wire form of a request (not sent, not verified)

Putting sections 1, 2 and 6 together, the most likely request is the same envelope with the request wrapped in the `Base` field 3:

    msg_id (BE16) | payload length (BE32) | protobuf: field 3 { request fields }

For a request with an empty body, such as `NRGrayscaleCameraStart` (inferred id 10053 = 0x2745) the payload would be `1a 00`
and the whole packet `27 45 00 00 00 02 1a 00`; a response would come back as `Base` field 4. A request with fields (for example
`NRGrayscaleCameraInitSetGain` 10052) needs the protobuf field numbers of its `Req` class, which are readable from its serialiser
(the same technique used on the `Base` class). Unknowns that could make the prediction wrong: whether requests are accepted on the
same ports as the streams or on one of the silent ones (52990-52995 accepted connections and sent nothing in all recorded
sessions), whether a handshake or version message must come first (`NRGlassesSetSDKVersion` 10014 exists), whether `Base` carries
more fields on the wire than the two the parser handles, and whether the firmware requires a session established with
`NRGlassesGetSWVersion`/`NRGlassesGetStartupState` first.

**No bytes were sent to the glasses.** Any experiment should start with a read-only request (`NRGlassesGetSWVersion` 10013,
`NRGlassesGetStartupState` 10265), on one connection, with a short timeout, and with the wearer's go-ahead.

## 8. Request and response layouts (protobuf field numbers)

Recovered from each message class's serialiser (the code writes the field tag bytes explicitly, so field numbers and wire types are
reliable; names and meanings are inferred from the request names). Notation: `{ 1: varint, 2: len }` = field 1 is a varint (int, enum or
bool), field 2 is length-delimited (string, bytes or a sub-message), `fixed32` is a float or 32-bit integer. All responses carry
**field 1 = result code** (0 = success, by convention; not verified). A packet is `msg_id` + length + `Base{ field 3 = request }`
(response: `Base{ field 4 = response }`, section 6). Layouts come from the ControlGlasses 3.0.0 client library (the 3.1.0 one stores its type
names differently and was not re-run); they are unlikely to have changed for these basic requests, but this is unchecked. For the
`Start`/`Stop` requests (inferred ids) the layout is taken from their own classes.

| Id | Request | Request body | Response body |
|---|---|---|---|
| 10003 | NRPowerSaveIsEnable | {} | { 1: varint, 2: varint } |
| 10004 | NRPowerSaveSetEnable | { 1: varint } | { 1: varint } |
| 10005 | NRPowerSaveGetSleepTime | {} | { 1: varint, 2: varint } |
| 10006 | NRPowerSaveSetSleepTime | { 1: varint } | { 1: varint } |
| 10008 | NRProximityIsEnable | {} | { 1: varint, 2: varint } |
| 10009 | NRProximitySetEnable | { 1: varint } | { 1: varint } |
| 10010 | NRDisplayGetBrightnessLevelCount | {} | { 1: varint, 2: varint } |
| 10011 | NRDisplayGetBrightnessLevel | {} | { 1: varint, 2: varint } |
| 10012 | NRDisplaySetBrightnessLevel | { 1: varint } | { 1: varint } |
| 10013 | NRGlassesGetSWVersion | {} | { 1: varint, 2: len } |
| 10014 | NRGlassesSetSDKVersion | { 1: len } | { 1: varint } |
| 10015 | NRGlassesGetConfig | {} | { 1: varint, 2: len } |
| 10016 | NRGlassesGetSupportedDevices | {} | { 1: varint, 2: varint } |
| 10017 | NRGlassesGetVsyncOffsetTime | {} | { 1: varint, 2: varint, 3: varint } |
| 10018 | NRGlassesGetMagCalibrationData | {} | { 1: varint, 2: len } |
| 10019 | NRGlassesSetMagCalibrationData | { 1: len } | { 1: varint } |
| 10020 | NRGlassesStartEventsReport | { 1: varint } | { 1: varint } |
| 10021 | NREcGetLevelCount | {} | { 1: varint, 2: varint } |
| 10022 | NREcGetLevel | unknown | unknown |
| 10023 | NREcSetLevel | unknown | unknown |
| 10025 | NRGlassesGetID | {} | { 1: varint, 2: len } |
| 10026 | NRGlassesGetSN | { 1: varint } | { 1: varint, 2: len, 3: len } |
| 10027 | NRGlassesGetSystemVersion | {} | { 1: varint, 2: len } |
| 10028 | NRGlassesGetHWVersion | {} | { 1: varint, 2: len } |
| 10029 | NRGlassesGetDspVersion | {} | { 1: varint, 2: len } |
| 10031 | NRVsyncStart | unknown | unknown |
| 10032 | NRVsyncStop | unknown | unknown |
| 10034 | NRRebootGlasses | {} | { 1: varint } |
| 10035 | NRShutdownGlasses | {} | { 1: varint } |
| 10036 | NRImuStart | unknown | unknown |
| 10037 | NRImuStop | unknown | unknown |
| 10039 | NRProximityGetFarThreshold | {} | { 1: varint, 2: varint } |
| 10040 | NRProximitySetFarThreshold | { 1: varint } | { 1: varint } |
| 10041 | NRProximityGetNearThreshold | {} | { 1: varint, 2: varint } |
| 10042 | NRProximitySetNearThreshold | { 1: varint } | { 1: varint } |
| 10043 | NRProximityGetValue | {} | { 1: varint, 2: varint } |
| 10044 | NRProximityGetWearingState | {} | { 1: varint, 2: varint } |
| 10047 | NRGrayscaleCameraCreate | {} | { 1: varint } |
| 10048 | NRGrayscaleCameraInitSetPixelFormat | { 1: varint } | { 1: varint } |
| 10049 | NRGrayscaleCameraInitSetImageResolution | { 1: varint } | { 1: varint } |
| 10050 | NRGrayscaleCameraInitSetAutoExposureType | { 1: varint } | { 1: varint } |
| 10051 | NRGrayscaleCameraInitSetExposureTime | { 1: varint } | { 1: varint } |
| 10052 | NRGrayscaleCameraInitSetGain | { 1: fixed32 } | { 1: varint } |
| 10053 (inferred) | NRGrayscaleCameraStart | {} | { 1: varint } |
| 10054 (inferred) | NRGrayscaleCameraStop | {} | { 1: varint } |
| 10057 | NRLedSetEnable | { 1: varint, 2: varint } | { 1: varint } |
| 10058 | NRLedGetEnable | { 1: varint } | { 1: varint, 2: varint } |
| 10059 | NRDisplayGetLuminanceMaxValue | {} | { 1: varint, 2: varint } |
| 10060 | NRDisplayGetLuminanceMinValue | {} | { 1: varint, 2: varint } |
| 10061 | NRDisplayGetLuminanceValue | {} | { 1: varint, 2: varint } |
| 10062 | NRDisplaySetLuminanceValue | { 1: varint } | { 1: varint } |
| 10063 | NRDisplayGetDutyMaxValue | {} | { 1: varint, 2: varint } |
| 10064 | NRDisplayGetDutyMinValue | {} | { 1: varint, 2: varint } |
| 10065 | NRDisplayGetDutyValue | {} | { 1: varint, 2: varint } |
| 10066 | NRDisplaySetDutyValue | { 1: varint } | { 1: varint } |
| 10067 | NRDisplaySetScreenEnable | { 1: varint, 2: varint } | { 1: varint } |
| 10068 | NRDisplayGetScreenEnable | {} | { 1: varint, 2: varint } |
| 10069 | NRDisplayGetColorTemperature | { 1: varint } | { 1: varint, 2: varint, 3: varint } |
| 10070 | NRDisplaySetColorTemperature | { 1: varint, 2: varint, 3: varint } | { 1: varint } |
| 10071 | NRDisplayGetCurrentResolution | {} | { 1: varint, 2: varint } |
| 10072 | NRDisplaySetCurrentResolution | { 1: varint } | { 1: varint } |
| 10073 | NRDisplayGetDefaultResolution | {} | { 1: varint, 2: varint } |
| 10074 | NRDisplaySetDefaultResolution | { 1: varint } | { 1: varint } |
| 10075 | NRDisplayGetColorCalibrationType | {} | { 1: varint, 2: varint } |
| 10076 | NRDisplaySetColorCalibrationType | { 1: varint } | { 1: varint } |
| 10078 | NRDpGetCurrentEdid | {} | { 1: varint, 2: varint } |
| 10079 | NRDpSetCurrentEdid | { 1: varint } | { 1: varint } |
| 10080 | NRDpGetCurrentResolution | {} | { 1: varint } |
| 10081 | NRDpGetDefaultEdid | {} | { 1: varint, 2: varint } |
| 10082 | NRDpSetDefaultEdid | { 1: varint } | { 1: varint } |
| 10083 | NRDpSetHDCPEnable | { 1: varint } | { 1: varint } |
| 10084 | NRDpSetWorkingMode | { 1: varint } | { 1: varint } |
| 10085 | NRDpGetWorkingState | {} | { 1: varint, 2: varint } |
| 10087 | NRPowerSaveEnter | {} | { 1: varint } |
| 10089 | NRAudioInStart | {} | { 1: varint } |
| 10090 | NRAudioInStop | unknown | unknown |
| 10091 | NRAudioGetCurrentMode | {} | { 1: varint, 2: varint } |
| 10092 | NRAudioSetCurrentMode | { 1: varint } | { 1: varint } |
| 10093 | NRAudioGetDefaultMode | {} | { 1: varint, 2: varint } |
| 10094 | NRAudioSetDefaultMode | { 1: varint } | { 1: varint } |
| 10095 | NRAudioIncreaseUacVolume | {} | { 1: varint } |
| 10096 | NRAudioDecreaseUacVolume | {} | { 1: varint } |
| 10097 | NRAudioGetVolumeMaxValue | {} | { 1: varint, 2: varint } |
| 10098 | NRAudioGetVolumeMinValue | {} | { 1: varint, 2: varint } |
| 10099 | NRAudioGetVolumeValue | {} | { 1: varint, 2: varint } |
| 10100 | NRAudioSetVolumeValue | { 1: varint } | { 1: varint } |
| 10101 | NRAudioGetAlgorithm | {} | { 1: varint, 2: varint } |
| 10102 | NRAudioSetAlgorithm | { 1: varint } | { 1: varint } |
| 10103 | NRAudioGetPAEnable | { 1: varint } | { 1: varint, 2: varint } |
| 10104 | NRAudioSetPAEnable | { 1: varint, 2: varint } | { 1: varint } |
| 10107 | NRRgbCameraInitSetAutoExposureType | { 1: varint } | { 1: varint } |
| 10108 | NRRgbCameraInitSetExposureTime | { 1: varint } | { 1: varint } |
| 10109 | NRRgbCameraInitSetGain | { 1: fixed32 } | { 1: varint } |
| 10110 | NRRgbCameraCreate | {} | { 1: varint } |
| 10111 | NRRgbCameraInitSetPixelFormat | { 1: varint } | { 1: varint } |
| 10112 | NRRgbCameraInitSetImageResolution | { 1: varint } | { 1: varint } |
| 10113 (inferred) | NRRgbCameraStart | {} | { 1: varint } |
| 10114 (inferred) | NRRgbCameraStop | {} | { 1: varint } |
| 10115 | NRRgbCameraRelease | {} | { 1: varint } |
| 10116 | NRRgbCameraGetPluginState | {} | { 1: varint, 2: varint } |
| 10119 | NREcGetValue | unknown | unknown |
| 10120 | NREcSetValue | unknown | unknown |
| 10121 | NRTemperatureGetValue | { 1: varint } | { 1: varint, 2: fixed32 } |
| 10149 | NRAudioPlay | unknown | unknown |
| 10211 | NRAudioGetPAForceSilent | { 1: varint } | { 1: varint, 2: varint } |
| 10212 | NRAudioSetPAForceSilent | { 1: varint, 2: varint } | { 1: varint } |
| 10215 | NRAudioGetPAForceSound | { 1: varint } | { 1: varint, 2: varint } |
| 10216 | NRAudioSetPAForceSound | { 1: varint, 2: varint } | { 1: varint } |
| 10217 | NRMiscSetScheduler | { 1: varint, 2: varint, 3: varint } | { 1: varint } |
| 10218 | NRAudioGetVolumePercentage | {} | { 1: varint, 2: varint } |
| 10219 | NRAudioSetVolumePercentage | { 1: varint } | { 1: varint } |
| 10220 | NRDisplayGetColorTemperatureBaseline | { 1: varint, 2: varint, 3: varint, 4: varint } | { 1: varint, 2: varint, 3: varint } |
| 10221 | NRDisplaySetGammaEnable | { 1: varint } | { 1: varint } |
| 10222 | NRGlassesGetBootCount | {} | { 1: varint, 2: varint } |
| 10223 | NRRgbCameraInitSetCompression | { 1: varint } | { 1: varint } |
| 10224 | NRDisplaySetScreenEnableBsp | { 1: varint } | { 1: varint } |
| 10225 | NRDisplayGetScreenEnableBsp | {} | { 1: varint, 2: varint } |
| 10226 | NRDpGetCurrentEdidBsp | {} | { 1: varint, 2: varint } |
| 10227 | NRDpSetCurrentEdidBsp | { 1: varint } | { 1: varint } |
| 10228 | NRDpGetCurrentResolutionBsp | {} | { 1: varint } |
| 10229 | NRGlassesGetSystemVersionCode | {} | { 1: varint, 2: len } |
| 10230 | NRGlassesGetProductName | {} | { 1: varint, 2: len } |
| 10231 | NRStorageGetAvailable | {} | { 1: varint, 2: varint } |
| 10232 | NRStorageGetTotalSize | {} | { 1: varint, 2: varint } |
| 10233 | NRStorageGetFreeSize | {} | { 1: varint, 2: varint } |
| 10234 | NRStorageClearAll | {} | { 1: varint } |
| 10235 | NRStorageSetFormat | {} | { 1: varint } |
| 10236 | NRStorageSetMode | { 1: varint } | { 1: varint } |
| 10237 | NRStorageGetMode | {} | { 1: varint, 2: varint } |
| 10238 | NRGlassesGetUsbVid | {} | { 1: varint, 2: len } |
| 10239 | NRGlassesGetUsbPid | {} | { 1: varint, 2: len } |
| 10240 | NRMiscGetDeviceType | {} | { 1: varint, 2: varint } |
| 10241 | NRRgbCameraGetSN | {} | { 1: varint, 2: len, 3: len } |
| 10243 | NRRgbCameraGetConfig | {} | { 1: varint, 2: len } |
| 10244 | NRRgbCameraSetConfig | { 1: len } | { 1: varint } |
| 10245 | NRPowerSaveGetSleepTimeLevelCount | {} | { 1: varint, 2: varint } |
| 10246 | NRPowerSaveGetSleepTimeLevel | {} | { 1: varint, 2: varint } |
| 10247 | NRPowerSaveSetSleepTimeLevel | { 1: varint } | { 1: varint } |
| 10249 | NRDpGetDataInterruptEnable | {} | { 1: varint, 2: varint } |
| 10250 | NRDpSetDataInterruptEnable | { 1: varint } | { 1: varint } |
| 10253 | NRDpGetDataTransmitMode | {} | { 1: varint, 2: varint } |
| 10254 | NRDpSetDataTransmitMode | { 1: varint } | { 1: varint } |
| 10255 | NRDpGetCurrentEdidAndAudioBsp | {} | { 1: varint, 2: varint, 3: varint } |
| 10256 | NRDpSetCurrentEdidAndAudioBsp | { 1: varint, 2: varint } | { 1: varint } |
| 10257 | NRAudioGetVolumeThousandth | {} | { 1: varint, 2: varint } |
| 10258 | NRAudioSetVolumeThousandth | { 1: varint } | { 1: varint } |
| 10259 | NRMiscGetSystemUpgradeState | {} | { 1: varint, 2: varint } |
| 10260 | NRAudioGetHostForceSilent | { 1: varint } | { 1: varint, 2: varint } |
| 10261 | NRAudioSetHostForceSilent | { 1: varint, 2: varint } | { 1: varint } |
| 10263 | NRGlassesGetSNCode | { 1: varint } | { 1: varint, 2: len } |
| 10264 | NRGlassesGetSNValue | { 1: varint } | { 1: varint, 2: len } |
| 10265 | NRGlassesGetStartupState | {} | { 1: varint, 2: varint } |
| 10266 | NRMiscGetHostType | {} | { 1: varint, 2: varint } |
| 10267 | NRSetGlassesCpuFrequencyMode | { 1: varint } | { 1: varint } |
| 10268 | NRGetGlassesCpuFrequencyMode | {} | { 1: varint, 2: varint } |
| 10269 | NRGlassesStopEventsReport | { 1: varint } | { 1: varint } |
| 10270 | NRDisplayGetColorTemperatureLevelCount | {} | { 1: varint, 2: varint } |
| 10271 | NRDisplayGetColorTemperatureLevel | {} | { 1: varint, 2: varint } |
| 10272 | NRDisplaySetColorTemperatureLevel | { 1: varint } | { 1: varint } |
| 10273 | NRDpGetInputMode | {} | { 1: varint, 2: varint } |
| 10274 | NRDpSetInputMode | { 1: varint } | { 1: varint } |
| 10275 | NRTemperatureGetStateProcessEnable | { 1: varint } | { 1: varint, 2: varint } |
| 10276 | NRTemperatureSetStateProcessEnable | { 1: varint, 2: varint } | { 1: varint } |
| 10277 | NRGlassesGetUltraWideEnable | {} | { 1: varint, 2: varint } |
| 10278 | NRGlassesSetUltraWideEnable | { 1: varint } | { 1: varint } |
| 10279 | NRGlassesRecenter | {} | { 1: varint } |
| 10280 | NRGlassesSetNetLogEnable | { 1: varint } | { 1: varint } |
| 10281 | NRGlassesSetSceneMode | { 1: varint } | { 1: varint } |
| 10282 | NRRgbCameraGetSNValue | {} | { 1: varint, 2: len } |
| 10283 | NRRgbCameraGetSNCode | {} | { 1: varint, 2: len } |
| 10284 | NRGlassesSetSpaceMode | { 1: varint } | { 1: varint } |
| 10285 | NRDpGetDataFilterModeBsp | {} | { 1: varint, 2: varint } |
| 10286 | NRDpSetDataFilterModeBsp | { 1: varint } | { 1: varint } |
| 10287 | NRDpGetDataFilterMode | {} | { 1: varint, 2: varint } |
| 10288 | NRDpSetDataFilterMode | { 1: varint } | { 1: varint } |
| 10289 | NRDpGetDataFilterModeCount | {} | { 1: varint, 2: varint } |
| 10290 | NRDisplayGetCallbackEnable | {} | { 1: varint, 2: varint } |
| 10291 | NRDisplaySetCallbackEnable | { 1: varint } | { 1: varint } |
| 10292 | NRUsbSetNetworkEnable | { 1: varint } | { 1: varint } |
| 10293 | NRUsbGetNetworkEnable | {} | { 1: varint, 2: varint } |
| 10295 | NRImuStartExt | unknown | unknown |
| 10296 | NRImuStopExt | unknown | unknown |
| 10297 | NRImuSetFrequencyExt | { 1: varint } | { 1: varint } |

12 requests could not be read this way (the IMU and vsync start/stop families use different type names).

Notable ones for the 6DoF question (all layouts above, none sent):
- `NRGrayscaleCameraCreate` {} -> `{ 1: result }`; then `InitSetPixelFormat` `{ 1: varint }`, `InitSetImageResolution` `{ 1: varint }`,
  `InitSetAutoExposureType` `{ 1: varint }`, `InitSetExposureTime` `{ 1: varint }`, `InitSetGain` `{ 1: fixed32 }`; then
  `NRGrayscaleCameraStart` `{}`. The enum values (pixel format, resolution, exposure type) are not recovered.
- `NRGlassesGetSWVersion` (10013): request `{}`, response `{ 1: result, 2: string }`: the safest first request (read-only).
- `NRGlassesGetStartupState` (10265): request `{}`, response `{ 1: result, 2: varint state }`.
- `NRGlassesSetSpaceMode` (10284) and `SetSceneMode` (10281): `{ 1: varint }` (the mode value meanings are unknown).
- `NRUsbSetNetworkEnable` (10292): `{ 1: varint }`.

## 9. The on-glasses application was not analysed (decision)

The ControlGlasses 3.1.0 package also bundles the One's firmware images, including the glasses' application
`pilot_1.6.1.20250730115123.bin` (27.8 MB), which is the server side of this protocol. Its file has a short plain header (a magic, then the
version string) followed by a body with a flat byte distribution (not a simple XOR; no recognisable archive or ELF magic under a
single-byte key), i.e. it is protected. **Recovering the protocol from it would mean defeating the vendor's firmware protection, which
this project does not do**; the copy used for this check was deleted. The protocol facts in this document come from the host-side
SDK libraries and from the project's own captures only. Remaining unknowns (which port takes requests, any handshake, the enum values)
should be settled by observing a working host or by an approved, minimal, read-only probe of the glasses.


## 10. Event messages observed on port 52999 during the anchor-mode run (`docs/samples/anchor-03/`)

| Id | Count in 150 s | Payload | Notes |
|---|---|---|---|
| 10122 | 25 | protobuf `Base{3: {1: index, 2: float}}` | temperature notifications (45-56 C for sensor 0, 41-50 C for the others), steady through the run |
| 10045 | 81 | 2 bytes, `18 01` or `18 02` = protobuf field 3, varint 1 or 2 | irregular (0.1 s to 17 s apart), also during the get-ready phase; not tied to the mode switches; id sits next to `NRProximityGetWearingState` (10044), so a wearing-state change notification is plausible (**inferred**) |
| 10030 | 4 (2 per switch) | 64 raw bytes, not protobuf (it contains pointer-sized values from the glasses' own process): `u32 1` or `2` at offset 0 (index of the pair), `u32` at offset 8 = a seconds counter (1800836685 at the first switch, 1800836754 at the second: 69 s apart) | sent twice within 0.1 s at each anchor-mode switch (30.7 s, 99.2 s) |
| 10002 | 2 (1 per switch) | 64 raw bytes: `u32 1` at offset 0, **u64 nanosecond device timestamp at offset 12** (1486.512 s, then 1555.072 s) | 0.51 s before the first camera frame and 0.03 s after the last: camera/anchor session **start** and **stop** event |

Decoding note: `10030` and `10002` are binary structs, so a generic protobuf decoder reports them as undecoded (as `tools/xreal_link.py` does).
Rates measured in the same run: camera 15.0 Hz while on, timestamp stream 59.9 Hz, IMU 999.1 Hz + 397.9 Hz.


## 11. First host-to-glasses request: result (2026-10-08, `tools/xreal_probe.py`)

> **Superseded by section 13.** The packet below failed because the control port is 52999 (not 52990-52995) and a request must carry a transaction id.

The first message this project has ever sent to the glasses: one packet, `NRGlassesGetSWVersion` (id 10013, empty body),
`27 1d 00 00 00 02 1a 00` (8 bytes), to each of the six silent ports 52990-52995 in turn, one connection each, with the glasses on the
latest firmware in Follow mode, nothing else connected. The probe listens 1.5 s first, sends once, waits 3 s, closes; it cannot send
anything but the allowlisted read-only getters.

| Port | Before sending | After sending | Result |
|---|---|---|---|
| 52990-52995 (each) | accepted instantly, 0 bytes in 1.5 s | **0 bytes; the glasses closed the connection 10 ms after receiving the packet** | no reply on any of them |

- The close is caused by the packet: the same ports stayed open and silent for the whole 150 s anchor-mode capture when nothing was sent. So
  these ports **do read incoming data and reject this packet**.
- Afterwards the streams were unaffected (timestamps 60 Hz, IMU 1400 records/s, no parse errors), so the probe left no lasting effect.
- A rejection this fast fits a framing/handshake mismatch rather than "unknown request": the SDK's own sender logs
  `tcpIpSendMsg fail ... msg.size() <= packet_msg_header_length`, i.e. its packets carry a fixed-length header beyond the 6-byte
  `msg_id` + length frame, and the recorded stream packets have 16 more bytes before their payload (an 8-byte field block, then a u64
  nanosecond timestamp at packet offset 14). My packet had no such header. A request probably needs the full header and may need an
  initial handshake message (`NotifyClientInfo`/`NRGlassesSetSDKVersion`, id 10014); neither is known yet.
- Not yet tried: the push-stream ports (52996-52999) as request ports; a request with a full 22-byte header; a handshake first.


## 12. Why a "full header" retry was not attempted: the SDK's packet header and sockets are local IPC (2026-10-08)

> **Superseded by section 13.** No longer-header retry is needed; the framing is `msg_id`, length, transaction id, body.

Reading the SDK's own sender (ControlGlasses 3.1.0, `libnr_service.so`) to find the header a request needs:

- **XrealLink has two clients, both aimed at `127.0.0.1`**: the TCP client on port **8099** (section 11.2 of `docs/nebula-findings.md`) and a **UDP**
  client (`socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)`, network-order port constant `0xBB1B`, i.e. port **7099**) constructed with the same literal
  `127.0.0.1`. Both are the SDK-client-to-local-service link, not the glasses.
- **The library embeds KCP** (reliable UDP): the debug strings of `ikcp.c` (`input psh: sn=%lu ts=%lu`, `input ack: sn=%lu rtt=%ld rto=%ld`,
  `input probe`, `input wins: %lu`, `recv sn=%lu`, `[RI] %d bytes`, `[RO] %ld bytes`) are in `libnr_service.so`, `libnr_api.so` and `libnr_loader.so`.
  It is the reliable transport of that local link.
- **That link's packet header is 17 bytes**: `0xFD`, a u32, a u32 payload length, and a u64 nanosecond timestamp from `CLOCK_MONOTONIC`
  (payload at offset 0x11). This is **not** the header seen on the glasses' own TCP streams (`msg_id` u16 BE, length u32 BE, then the payload), so
  copying it would not make a request valid for the glasses.
- **No glasses-facing client code was found.** No code in the SDK libraries was found that connects to the glasses' link-local address and the
  ports 52990-52999, and the earlier immediate-value scan found none of those port numbers. The vendor's glasses control path that was found is
  USB HID with `0xFD`/`0xAA` frames (`cmd_build_sdk`, section 5 of `docs/nebula-findings.md`) and the MCU message table.

Consequences: (1) the rejection of my TCP request (section 11) has no known cause that a different header would fix; (2) there is no evidence
that the silent TCP ports are request ports at all (they may be push streams for other consumers that close on any input); (3) the camera
start for anchor mode is initiated by the glasses' own firmware (section 10, and `docs/findings.md`), and nothing here shows a host request
for it. A further host request would be a guess, so none was sent. The evidence that would settle the question is a capture of a working host
(a phone running the vendor app) while it starts and stops the camera.


## 13. The control port: framing, first working request and the factory calibration (2026-10-08)

**Source.** The framing below comes from the public Android library [Skarian/one-xr](https://github.com/Skarian/one-xr) (MIT licence;
`XrControlSession.kt` and `XrControlProtocol.kt`), written for the XREAL One and One Pro. It was found by a web search and read from its source.
It applies unchanged to the XREAL 1S used here (USB `3318:043e`, `bcdDevice` 4.09). This project's own code follows the documented wire format
and does not copy that library's code.

**The control channel is TCP port 52999** (the stream port is 52998, as in section 1). Frames on it:

| Offset | Size | Field |
|---|---|---|
| 0 | 2 | `msg_id`, big endian (one-xr calls it the magic) |
| 2 | 4 | length of everything after this field, big endian |
| 6 | 4 | **transaction id**, big endian; requests set the top bit (`id | 0x80000000`), responses echo it with the top bit clear |
| 10 | length - 4 | body |

- A response has the same `msg_id` as its request and the same transaction id. Frames the glasses send on their own (the temperature and
  other notifications of section 10, and the key events below) have **no transaction id**: their payload starts right after the 6-byte header.
- A read-only getter's request body is `18 00` (protobuf field 3, varint 0). A numeric setter's body is `1a <len> 08 <value>`.
- A response body is `22 <len> <nested>` (field 4). The nested message holds the value: field 2 (`12 <len> <bytes>`) for a string, field 2
  as a varint (`10 <varint>`) for a number, or just `08 <status>` for a setter, where a non-zero status (for example 10001) means the command was rejected.

Commands documented by one-xr (names are one-xr's):

| `msg_id` | Name | Kind |
|---|---|---|
| `0x271D` | get software version | read-only getter |
| `0x271F` | **get config** (calibration JSON, below) | read-only getter |
| `0x2729` | get id | read-only getter |
| `0x272D` | get DSP version | read-only getter |
| `0x271C` / `0x2727` | set brightness / set dimmer | setter, **not sent** |
| `0x2829` | set scene mode (0 = buttons enabled, 1 = disabled) | setter, **not sent** |
| `0x2822` | set display input mode (0 = regular, 1 = side by side) | setter, **not sent** |
| `0x272E` | key state change event, 64 raw bytes (little-endian key type, state, device time) | event from the glasses |

**Why sections 11 and 12 failed:** the packet went to ports 52990-52995 instead of 52999 and had no transaction id. The glasses closed those
connections within 10 ms, which is what a malformed frame gets.

### 13.1 The first working request

`tools/xreal_probe.py --txid --port 52999 --id 0x271f` sends exactly one packet and never retries. Its `--txid` mode only accepts port 52999 and
the four read-only getters above. The packet:

    27 1f 00 00 00 06 80 00 00 01 18 00

Result: the glasses replied within the 8-second wait (`msg_id` 0x271F, transaction id 1) with a **220,434-byte payload**: a protobuf string field
holding **220,422 characters of valid JSON**. Afterwards the glasses were unaffected: IMU stream at 1,400 records/s with clean framing, the same
USB device number, and the display mode unchanged.

### 13.2 What the config contains

The JSON is the glasses' factory calibration. Top-level keys: `FSN` (the unit's serial number), `IMU`, `RGB_camera`, `SLAM_camera`, `display`,
`display_distortion`, `glasses_version` (7 on this unit) and `last_modified_time`.

| Key | Contents (values from this unit) |
|---|---|
| `SLAM_camera.device_1` | radial camera model; resolution 504 x 378; focal length about 238.8 px; principal point about (253.3, 190.4); five distortion coefficients; rolling-shutter time 1.79 ms; **`imu_p_cam`** about (-25.0, 14.3, 3.8) mm and **`imu_q_cam`**, the camera pose relative to the IMU (quaternions are JPL order, x y z w) |
| `RGB_camera.device_1` | radial model; 2016 x 1512; focal length about 955 px; principal point about (1013, 762) |
| `display` | panel resolution 1920 x 1200; per-eye `k_left_display` / `k_right_display` (3x3 intrinsics, focal length about 2490 x 2470 px, principal point about (962, 605)); per-eye pose relative to the IMU (`target_p_*_display`, `target_q_*_display`, with `target_type` "IMU"); the x offsets differ by about 64.0 mm |
| `display_distortion` | per eye a 61 x 39 grid (`data` has 9,516 integers each, four per grid point), `type` 1 |
| `IMU.device_1` | accelerometer and gyro biases, 3x3 calibration matrices, noise figures, a 23-entry temperature-dependent gyro bias table, and `gyro_q_mag` = (-0.5, -0.5, 0.5, 0.5), the magnetometer's orientation relative to the gyro, with the magnetometer's bias and scale still at their defaults |

The values are per unit and the document includes the serial number, so it is **not stored in the repo**. A copy was kept outside it.

### 13.3 What this was used for, and what is still open

- **Field of view.** From the display intrinsics, assuming the 1080-row full SBS picture sits unscaled in the 1200-row panel (the wearer reported on 2026-10-08 that the
  world looked right and comfortable with these values; the vertical mapping was not measured), each eye sees about 42.2 degrees horizontally by 24.7 degrees vertically: half-tangents of 0.3857 and 0.2190 after averaging both
  eyes. The driver's old placeholder was 48.5 x 28.4 degrees. The driver's `GetProjectionRaw` and the presenter's reprojection constant now use
  the new values.
- **IPD.** The display offsets give 64.0 mm, and the driver now reports 64 mm (it was 63 mm); the wearer found it comfortable.
- **6DoF.** The camera's intrinsics and its pose relative to the IMU are given by the glasses. The camera-to-IMU *time offset* is not in the
  file. The config does not say how the camera starts.
- **Eye frame size (inference, not checked).** The camera stream's payload is 193,856 bytes; 512 x 378 = 193,536 plus 320 header bytes would fit,
  against the config's 504 x 378.
- **No camera command** is among the commands one-xr documents, so the question from section 11 (can a host request start the camera outside
  anchor mode) is still open.

### 13.4 Requests exercised on the control port (2026-10-08, `tools/xreal_session.py`)

| `msg_id` | Name | Body | Reply |
|---|---|---|---|
| 10015 | `NRGlassesGetConfig` | `18 00` | calibration JSON (section 13.1) |
| 10273 | `NRDpGetInputMode` | `18 00` | `22 02 10 01`: field 2 = 1 (side by side) |
| 10085 | `NRDpGetWorkingState` | `18 00` | `22 02 10 01`: field 2 = 1 |
| 10016 | `NRGlassesGetSupportedDevices` | `18 00` | `22 03 10 a3 0c`: field 2 = 1571 |
| 10003 | `NRPowerSaveIsEnable` | `18 00` | `22 00`: value 0, auto sleep is off (the unit after a replug) |
| 10005 | `NRPowerSaveGetSleepTime` | `18 00` | `22 00`: value 0 |
| 10008 | `NRProximityIsEnable` | `18 00` | `22 02 10 01`: field 2 = 1, the proximity (wearing) sensor is on |
| 10044 | `NRProximityGetWearingState` | `18 00` | `22 00`: value 0 (probably "not worn"; meaning inferred) |
| 10274 | `NRDpSetInputMode` (**setter**) | `1a 02 08 01` (value 1 = side by side; 0 = regular) | `22 00` (success); the display then switched to full SBS, see below |
| 10047 | `NRGrayscaleCameraCreate` | `18 00` | `22 00` (empty body = success) |
| 10053 | `NRGrayscaleCameraStart` (inferred id) | `18 00` | `22 00`; four camera frames follow on 52997 |
| 10054 | `NRGrayscaleCameraStop` (inferred id) | `18 00` | none within 5 s (the stream had already stopped) |

Notes: the SDK ids are the same numbers as the control port's magics (checked for every one of these). `18 00` (field 3, varint 0) was accepted as
the empty request body for all of them; the SDK's own form `1a 00` was not tried. A reply with an empty body means success; a request with a
parameter replies with the value in field 2, and a value of 0 is simply absent (protobuf omits defaults), so `22 00` from a getter means 0. See `docs/findings.md` for what the camera did.

### 13.5 Offline reading of the vendor camera client (`libnr_service.so`, ControlGlasses 3.1.0, 2026-10-08)

The service has one wrapper function per grayscale-camera request, laid out back to back (about 0xC8C bytes each; addresses are virtual addresses in this build):

| Request | Wrapper starts at | Logs |
|---|---|---|
| `NRGrayscaleCameraCreate` (10047) | `0x18fb7e8` | `Call NRGrayscaleCameraCreate start` |
| `...InitSetPixelFormat` (10048) | `0x18fc49c` | `... start, format={}` |
| `...InitSetImageResolution` (10049) | `0x18fd128` | `... start, resolution={}` |
| `...InitSetAutoExposureType` (10050) | `0x18fddb4` | `... start, type={}` |
| `...InitSetExposureTime` (10051) | `0x18fea40` | `... start, time={}` |
| `...InitSetGain` (10052) | `0x18ff6cc` | `... start, gain={}` |
| `NRGrayscaleCameraStart` (10053) | `0x1900354` (loads `0x2745`) | none found |
| `NRGrayscaleCameraStop` (10054) | `0x1900bf8` (loads `0x2746`) | none found |

- Each `InitSet*` takes **one integer** (`format`, `resolution`, `type`, `time`, `gain`); the request layout `{ 1: varint }` in section 8 fits.
- Start and Stop load exactly the ids we sent, which agrees with the camera answering Start.
- The wrappers sit in the class `DriverInterface<NRGrayscaleCameraInterface>` and are reached through its virtual table, so a direct search for callers finds none. **The integer values the service passes were not recovered**; tracing the virtual calls from `ImpGrayCamera` / `GrayscaleCameraProvider` is the next step if they are needed.
- The same library has an identical set for the RGB camera (`NRRgbCameraInitSet*`, plus `Release`, `GetPluginState` and a host time offset request, `NRRgbCameraSetHostTimeOffset`).

**Setting full SBS from the host (2026-10-08).** With the glasses in their normal 2D mode (`NRDpGetInputMode` = 0, the Deck saw `1920x1080` and `1920x1200`),
one `NRDpSetInputMode` request with value 1 (packet `28 22 00 00 00 08 80 00 00 02 1a 02 08 01`) was answered `22 00`. About 0.2 s later the glasses sent
notifications with id **10086** (not in the SDK table; body `18 02` once, then `18 01` three times, so probably a display state that goes through 2 and
settles at 1, next to `NRDpGetWorkingState` = 1), and within seconds `NRDpGetInputMode` read 1 and the Deck saw only `3840x1080`. The USB device did not re-enumerate.
`tools/xreal_session.py` allows this setter only with `--allow-display-mode` and only for the values 0 and 1. Ids 10087 (`NRPowerSaveEnter`) and probably 10088
sit just after 10086; if they are sent as events they may show when the glasses go to sleep, which has not been observed yet.


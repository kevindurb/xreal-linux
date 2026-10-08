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

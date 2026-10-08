# Capture format (version 2)

Written by `tools/capture_eye.py`; analysed with `tools/xreal_link.py`; the replay path (task 1.5) feeds the same files through the same reader as a live socket.
Captures live outside the repo (large; `captures/` is git-ignored).

A capture is a directory:

| File | Contents |
|---|---|
| `meta.json` | `format`, glasses `host`, `wall_start` (unix s), `start_ns`/`end_ns` (host monotonic), `phases` (`name`, `start_ns`, `seconds`), and a per-port summary (`bytes`, `error`, derived rate) |
| `port<N>.raw` | The verbatim TCP byte stream of port N (52997 camera, 52996 timestamps, 52998 IMU), exactly as received. Nothing is parsed or dropped, so a replay is byte-identical to the live stream |
| `port<N>.chunks` | One 12-byte entry per `recv()`: u64 LE host monotonic ns, u32 LE length. Gives arrival times for replay at real speed and for host-side latency; consecutive lengths sum to the size of the `.raw` file |

Records inside the streams are as described in `docs/findings.md` (camera frames 193,862 bytes starting `27 48 00 02`;
timestamp records 38 bytes starting `27 31 00 00 00 20`; IMU records 134 bytes starting `28 36 00 00 00 80`). Device
timestamps are in the records; the host monotonic times are only for arrival and phase cutting. Rate figures in
`meta.json` are byte counts divided by nominal record sizes, a quick check rather than a parse.

Phases (for example `still`, `shake`, `rotate`) are stored with host monotonic times so a capture is cut by
time into motions.


## Changes in version 2

- `capture_eye.py` now records **all ten TCP ports 52990-52999** by default (`--ports` to choose). The silent ports 52990-52995 are
  kept so that anything they start to send during a mode change is not missed.
- `--hid` additionally records every report read from the glasses' `/dev/hidrawN` nodes (found by USB vendor id 3318) into
  `hidrawN.raw` / `hidrawN.chunks`, same chunk format. The nodes are opened **read-only**; nothing is written to them.
- `meta.json` has `"format": 2`, a free-text `note` (record the glasses' mode, firmware and what you did), and for every TCP port
  a `msg_ids` table (count and rate per message id, using the framing below) and `skipped_bytes` (bytes that did not parse; 0 on a
  healthy stream). Ports that could not be reached are recorded with an `error`.
- A capture directory can be summarised with `tools/xreal_link.py DIR`.

## Message framing inside the streams

Every packet is `msg_id` (u16 big endian), payload length (u32 big endian), payload. See `docs/xreal-link-messages.md` for the ids.
The stream ids seen so far: 10056 camera frames (port 52997), 10033 timestamps (52996), 10294 IMU (52998), 10122 events (52999).
IMU packets interleave two record types (u32 at packet offset 30): `11` gyro and accelerometer at 1000 Hz, `4` magnetometer at 400 Hz
(see "Magnetometer in the IMU stream" in `docs/findings.md`).

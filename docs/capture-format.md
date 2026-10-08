# Capture format (version 1)

Written by `tools/capture_eye.py`; the replay path (task 1.5) feeds the same files through the same reader as a live socket.
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

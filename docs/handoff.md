# Handoff: where the project stands (2026-10-08)

Written so a fresh Claude session (or a person) can continue without the conversation. Read this, then `README.md`, `openspec/config.yaml` and the sections it links.

## 1. The project in five lines

XREAL 1S glasses used as a SteamVR headset on a Linux Steam Deck. A C++ SteamVR driver (`driver/`, direct mode) forwards SteamVR's frames to a Rust presenter
(`presenter/`) that owns the glasses' display and the IMU (3DoF head tracking), over an abstract unix socket. Tools and notes are in `tools/` and `docs/`; behaviour specs and
proposed changes are in `openspec/`. The long-term goal is 6DoF (position) from the glasses' Eye camera plus the IMU, with 3DoF as the fallback tier.

## 2. What works today (verified on the Deck)

- SteamVR (Home, dashboard) renders to the glasses at 60 fps, full resolution, without the earlier flicker or judder (archived change `2026-10-07-fix-dashboard-glitches-and-judder`).
- **The field of view and IPD now come from the glasses' own factory calibration**: half tangents 0.3857 horizontally and 0.2190 vertically (about 42 x 25 degrees), IPD 64 mm. The wearer
  reported it looks right and comfortable. (Assumes the 1080-row SBS picture sits unscaled in the 1200-row panel; not measured.)
- **The glasses answer host requests on TCP port 52999** (the control port): `docs/xreal-link-messages.md` section 13 has the frame format and every request tried. The host can read the whole
  factory calibration JSON (`GetConfig`), read and set the display input mode, and set **full SBS with one request**.
- **The Eye camera can be started from the host in Follow mode with the Stabilizer off, but it streams only 4 frames and stops.** Everything below is about that.

## 3. Hosts and how to work

| Host | What it is |
|---|---|
| This repo's checkout on the Mac, and the checkout on `ssh kevindurb@dev` | git clones; `main` is pushed after each step the user approves. A second Claude agent works on `dev`, so `git pull --ff-only` before editing |
| `ssh kevindurb@steamdeck` | the Steam Deck (Bazzite) with the glasses attached. `~/xreal-linux` there is **an rsync copy, not a git checkout**. No Rust or static libstdc++ on the host: build in podman |

Deploy and run (every ssh prints a harmless `mise: command not found`; filter with `grep -v mise`):

    rsync -a --exclude target --exclude target-new presenter/ kevindurb@steamdeck:xreal-linux/presenter/
    rsync -a --exclude xreal/bin --exclude .openvr driver/ kevindurb@steamdeck:xreal-linux/driver/
    rsync -a tools/ kevindurb@steamdeck:xreal-linux/tools/
    # on the Deck (export XDG_RUNTIME_DIR=/run/user/1000 first):
    cd ~/xreal-linux/driver && /usr/bin/podman run --rm -v "$PWD":/src:Z -w /src registry.fedoraproject.org/fedora:44 bash -c "dnf -y -q install gcc-c++ libstdc++-static binutils git >/dev/null 2>&1 && ./build.sh"
    cd ~/xreal-linux/presenter && /usr/bin/podman run --rm -v "$PWD":/src:Z -v xreal-cargo-cache:/root/.cargo:Z -w /src registry.fedoraproject.org/fedora:44 bash -c "dnf -y -q install rust cargo gcc >/dev/null 2>&1 && cargo build --release --target-dir /src/target-new" && cp target-new/release/xreal-presenter target/release/xreal-presenter
    ~/xreal-linux/tools/vr_session.sh start | stop | presenter | status | log     # start refuses unless the glasses are in full SBS (a single 3840x1080 mode)

On the Mac, tests: `python3 tools/test_xreal_link.py`; presenter `cargo test` and the driver build run in containers (`docker.io/library/rust:1`, `registry.fedoraproject.org/fedora:44`) through `podman`.
**Do not delete `presenter/Cargo.lock`** (it is tracked).

Gotchas learned the hard way:
- **Tailscale SSH kills child processes when the command ends.** Start anything that must keep running with `systemd-run --user --unit=NAME ...`, with `XDG_RUNTIME_DIR=/run/user/1000` and
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus` set.
- The Deck's Tailscale `accept-routes` is **on** again. It only matters if Waydroid runs (its `192.168.240.0/24` is swallowed by the `192.168.0.0/16` route a node called `router` advertises).
- The glasses **re-enumerate on a replug and come back in their normal 2D mode** (the host can set SBS again, section 5). The USB device number changes each time.
- Waydroid was tried (vendor Android apps cannot drive the 1S: their service library has no table entry for product id `0x043e`) and **fully reverted**; nothing of it remains on the Deck.
- The glasses' serial number is in the config JSON: **never commit it** (it was kept in `/tmp` on the Deck and in a scratch directory).

## 4. Rules for sending anything to the glasses (agreed with the user during this session; follow them)

1. **Read-only getters** may be sent through `tools/xreal_session.py` (they are on its allowlist). Add a getter to the allowlist only if its request layout is documented and it only reads.
2. **Every state-changing request** (setters, camera or sensor starts and stops) needs the user's explicit approval **for that exact request**, shown byte for byte first. Send one at a time and look at the reply
   before the next. The tool refuses these unless started with `--allow-camera`, `--allow-display-mode` or `--allow-sensor-start`, and only for the listed ids and bodies.
3. **Stop on anything unexpected** (no reply, an error code, a changed stream) and report; do not retry or try other ids without approval.
4. **Never send a camera Start twice between replugs.** A second Start got no reply and the glasses' control server then stayed silent until a replug.
5. After any test, check the glasses are unharmed: the IMU stream (port 52998) at about 1,400 records/s with clean 134-byte framing, the timestamp stream (52996) at 120/s in 2D and 60/s in SBS, the same USB device, and the display mode.
6. The user is often **away from the glasses**. Ask before any test that needs the wearer, and say clearly when you need them to replug.

## 5. Tool cheat sheet: `tools/xreal_session.py`

Holds one connection to the control port and sends requests one at a time from a command file. Start it under systemd, then drive it through the FIFO:

    systemd-run --user --unit=xreal-session -p StandardOutput=file:/tmp/xreal_session/stdout.log -p StandardError=file:/tmp/xreal_session/stderr.log \
        python3 ~/xreal-linux/tools/xreal_session.py --dir /tmp/xreal_session [--allow-camera] [--allow-display-mode] [--allow-sensor-start] --max-seconds 300
    echo "send 10273" > /tmp/xreal_session/cmd            # a getter; "send <id> <bodyhex>" for a setter; "quit" to finish
    cat /tmp/xreal_session/session.jsonl                    # kinds: connected, send, response, event, rates, no_response, refused, closed

- Request frame: `msg_id (2, BE) | length (4, BE) | transaction id (4, BE, top bit set) | body`; the body for a getter or an empty request is `18 00`; a numeric setter body is `1a 02 08 <value>`.
- **Reading a reply:** `22 00` = success or the value 0 (protobuf omits defaults); `22 02 10 VV` = value VV (field 2); `22 03 08 ..` with field 1 non-zero = an **error code** (5004 was seen for the unsupported proximity thresholds).
- The `msg_id`s are the SDK's request ids (`tools/xreal_link_ids.py`, `docs/xreal-link-messages.md`).
- On exit the tool sends a camera **Stop** if (and only if) it had sent Start and no Stop since. The camera never answered a Stop so far.
- It also reads 52997 (camera) and 52996 (timestamps) and keeps the first 12 camera frames in `camera_frames.bin` (193,862 bytes each).
- `tools/xreal_probe.py --txid ...` sends one read-only getter on its own connection (used for the first `GetConfig`).

Allowlisted today: getters 10003, 10005, 10008, 10013, 10015, 10016, 10025, 10029, 10039, 10041, 10044, 10085, 10273; setter 10274 (values 0 and 1 only, `--allow-display-mode`); camera 10047, 10053, 10054
(`--allow-camera`); sensor starts 10036, 10031 with body `18 00` (`--allow-sensor-start`).

## 6. The camera investigation so far (details in `docs/findings.md`, last four sections)

| # | Setup | Result |
|---|---|---|
| 1 | after a replug: Create, wait about 9 s, Start | both answered `22 00`; **4 frames** (first 0.5 s after Start, 66.8 ms apart = 15 fps), then silence; Stop unanswered; streams unaffected |
| 2 | about 25 min later, no replug: Create, Start 2.5 s later | Create answered, **Start unanswered, no frames**; then even Create and read-only getters went unanswered (streams still fine) until a replug. Possibly the glasses fell asleep (unproven) |
| 3 | replug, getters only | all answered; the host set full SBS (input mode 0 to 1, display `3840x1080`) |
| 4 | replug, SBS, Create, Start 10 s later | **4 frames again**; Stop unanswered; the control server stayed alive afterwards |
| 5 | replug, SBS, `NRImuStart`, `NRVsyncStart` (both `22 00`, no stream change), Create, Start 9 s later | **4 frames again**; Stop unanswered |

- The frames are normal pictures. Header (packet offsets): u32 LE width **504** at 11, u32 LE height **378** at 15, u16 LE stride **512** at 19, u64 LE nanosecond **timestamp at 23**; payload is 512 x 378 bytes after 320 header bytes (193,862 bytes in all).
  Even and odd rows differ (the clean picture is the odd rows); how the two row sets relate is open (task 1.2 of the 6DoF change).
- Not in the data: nothing in the frame headers explains the stop; the service has no per-frame acknowledgement that I found; no start event (10002) appears for a host-started camera (anchor mode sends one).
- **Still different from the vendor sequence (untried):** the `InitSet*` requests 10048-10052 (pixel format, resolution, auto-exposure type, exposure time, gain; one integer each, values unknown) and the SDK's own empty body `1a 00`.
- The wearing state (`NRProximityGetWearingState`, 10044) returned 0, 1 and 2 with the glasses on the wearer; the meaning is unknown, so do not gate on it. Auto sleep is off but the proximity sensor is on; the glasses still drop to 2D mode now and then.

## 7. Next steps, in order

1. **Offline (no hardware):** recover the `InitSet*` values from the service. `tools/re/README.md` has the setup and what is already known (wrapper addresses, `ImpGrayCamera` vtable `0x2377d50`). Find the callers of
   `ImpGrayCamera`'s methods and `GrayscaleCameraProvider`'s setup and read the constants.
2. **One clean hardware attempt** after a replug: SBS, Create, the `InitSet*` requests with the recovered values (if they were not recovered, ask the user before guessing any), wait, Start. Show every packet first. If that fails, try `1a 00` bodies.
3. **Decide the camera path** (task 0.5 of `openspec/changes/add-6dof-camera-tracking`): if a host-started camera cannot be sustained, re-scope 6DoF or stop it. Do not let this block the independent work below.
4. **Independent work that does not need the camera:**
   - The presenter reads `GetConfig` on connect (read-only), caches it per serial outside the repo, passes per-unit field of view and IPD to the driver, and applies the factory IMU calibration (biases, calibration matrices, temperature table).
   - Use the magnetometer (`gyro_q_mag` is in the config) to bound yaw drift in the 3DoF tier.
   - The presenter sets full SBS itself on connect (`NRDpSetInputMode` = 1; needs the user's approval to build, since it makes a setter part of the product).
   - Find out why the glasses drop to 2D: log the events on 52999 during a long VR session (ids 10045, 10086, and possibly 10087 `NRPowerSaveEnter`); consider `NRProximitySetEnable` (10009) only with approval.
   - Check the vertical field of view by eye (black bars above and below each eye's picture mean the 1080-row assumption holds).
5. The other proposed changes, `add-installable-package` and `support-other-gpus` (`openspec/changes/`), have not been started.

## 8. Status of the OpenSpec changes

| Change | Tasks | State |
|---|---|---|
| `add-6dof-camera-tracking` | 4 done, 25 open | proposal, design, specs and tasks updated today to start from the factory calibration; group 0 is the camera start question (0.5 decide, 0.6 try) |
| `add-installable-package` | 0 of 20 | not started |
| `support-other-gpus` | 0 of 17 | not started |
| `2026-10-07-fix-dashboard-glitches-and-judder` | archived | done |

## 9. State of the machines at the end of this session

- Glasses: full SBS (set by the host), Follow mode, Stabilizer off, camera run already used since the last replug (so **replug before the next camera Start**).
- Deck: no VR session and no `xreal-session` unit running; the rebuilt driver and presenter with the new field of view and IPD are installed; Waydroid stopped and reverted; `adb` removed.
- Repo: everything committed and pushed to `main`; `dev` is up to date.

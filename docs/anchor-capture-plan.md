# Capture plan: what the glasses do when the Eye starts

Goal: find out, **by observation only**, what changes on the glasses' interfaces when their own menu switches on anchor mode (the only
state in which the Eye camera has been seen streaming), so that a host-side start for the camera in Follow mode with the Stabilizer off
can be designed. Nothing in this plan sends a byte to the glasses.

## Why

The camera stream (port 52997, message 10056) is idle in Follow mode. The SDK has camera start/stop requests
(`NRGrayscaleCameraStart`, inferred id 10053, `docs/xreal-link-messages.md`), but we do not yet know which port takes requests, whether
a handshake is needed, or whether the glasses refuse the camera in Follow mode with the Stabilizer off. Watching the glasses start the
camera themselves answers some of that for free: which interfaces carry the change, which messages appear, and in what order.

## Before you start (a few minutes)

1. Deck on, glasses connected; `tools/doctor.sh` should show the glasses and the IMU stream. Stop SteamVR and the presenter
   (`tools/vr_session.sh stop`) so nothing else reads the ports.
2. Note the glasses' firmware version and current mode in the `--note` text (MyGlasses, or XREAL's OTA site).
3. Point the glasses at something textured and lit (the Deck's screen is enough), so a camera frame has content.
4. Copy the recorder: `scp tools/capture_eye.py steamdeck:/tmp/` (it needs only python3).

## The run (about 2 minutes)

    python3 /tmp/capture_eye.py ~/captures/anchor-01 --hid \
        --phases follow:20,toggle-on:15,anchor:30,toggle-off:15,follow:20 \
        --note "firmware X, follow mode, stabilizer off, glasses at the Deck screen"

The recorder announces each phase. Do this on the glasses: during **follow** do nothing; at **toggle-on** open the glasses' menu and
switch to **anchor mode** (take the time you need; the phase is only a marker); during **anchor** keep the glasses still and pointed at
the screen; at **toggle-off** switch back to follow mode; during the last **follow** do nothing.
If the camera does not start in anchor mode, repeat once with `--phases follow:20,toggle-on:30,anchor:30` and write down what the glasses
showed. If the display mode changes (the glasses leave full SBS), say so in the note: that affects the DRM mode list, not the capture.

## Running it so it survives (learned the hard way)

Over Tailscale SSH, anything started from the session is killed when the session ends, so a plain `&` or `nohup` does not keep the recorder
alive. Start it as a user service instead, and add `--notify --say` so the wearer gets prompts as notifications and spoken
words:

    ssh steamdeck 'export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
      systemd-run --user --unit=xreal-capture --collect -E XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR \
        -E DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS python3 /tmp/capture_eye.py \
        /var/home/kevindurb/captures/anchor-NN --hid --notify --say --phases get-ready:10,follow:15,toggle-on:40,anchor:30,toggle-off:40,follow:15'

Check progress with `systemctl --user is-active xreal-capture`; the capture directory name must not exist already. Do not mark notifications
"critical" (Plasma never dismisses them).

## Reading the result

    python3 tools/xreal_link.py ~/captures/anchor-01

What to look for, in order:
1. **Port 52997** (camera): does `id 10056` appear only during the anchor phase? Frame rate (~60 Hz) and `skipped_bytes` (0 expected).
2. **Ports 52990-52995**: any bytes at all, especially right at the toggle. First bytes of each new packet (`--dump ID N`) show the
   message id; compare with the table in `docs/xreal-link-messages.md` (a camera-related id in 10031-10057, 10107-10116, 10277-10284).
3. **Port 52999** (events): new message ids or new content at the toggle, besides the temperature notifications (10122).
4. **HID** (`hidraw*.raw`): reports at the toggle (the control path of the vendor SDK is USB HID); compare the leading bytes with the 0xFD / 0xAA frame
   description in `docs/nebula-findings.md` section 5.
5. **IMU/magnetometer** (port 52998): does anything change in the record rates (1000 Hz gyro/accel, 400 Hz magnetometer) at the toggle?
6. The `meta.json` per-port `msg_ids` table gives the same information in one place.

Record the findings in `docs/findings.md` (what was measured, what was inferred), and extend `docs/xreal-link-messages.md` with any id
that appears.

## What this does not do

It does not start the camera from the host. The next step, only if this shows a clear candidate, is a single minimal read-only request
(for example `NRGlassesGetSWVersion`, id 10013, empty body: `27 1D 00 00 00 02 1A 00`) on the port the observation suggests, with a short
timeout, and only with the wearer's explicit approval. Any camera start request is a separate, later decision.

## Result of the first run

Done on 2026-10-08 (`anchor-03`); results in `docs/findings.md` ("Anchor-mode observation run") and `docs/xreal-link-messages.md` section 10. In short: the camera
streamed only in anchor mode (15 fps in that run), the start and stop were announced on port 52999 by message 10002, and nothing at all
appeared on HID or on ports 52990-52995. A second run would be useful with the SDK-visible state recorded alongside (and, if a rate of 60 fps is
wanted, with whatever differed from the earlier 60 fps sample).

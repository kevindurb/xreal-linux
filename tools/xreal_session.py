#!/usr/bin/env python3
"""Hold ONE control connection to the XREAL glasses (TCP 52999) and send requests to it one at a time, on command.

Every state-changing request is sent only after the wearer approved that exact request; this tool never sends anything by itself
except the cleanup Stop described below. Meant for finding out whether a host request starts the Eye camera in Follow mode.

  * Requests are read, one per line, from a command FIFO:   send <id> [<body hex>]   |   quit
    <id> is decimal or 0x hex; the body defaults to `1800` (the read-only getter body); frame layout is docs/xreal-link-messages.md section 13.
  * Always allowed: the read-only getters in GETTERS. The camera requests in CAMERA are refused unless --allow-camera was given, and the
    display input mode setter in SETTERS unless --allow-display-mode was given (and then only with the body for value 0 or 1), and the
    IMU and vsync start requests in SENSOR_START unless --allow-sensor-start was given (and then only with the body `18 00`).
    Nothing else can be sent.
  * On exit (quit, timeout, signal, a lost connection) the tool sends NRGrayscaleCameraStop if, and only if, it sent Start and no Stop since.
  * While connected it also reads the camera stream (52997) and the timestamp stream (52996), counts frames, and keeps the first frames
    of a run in a file, so a started camera is visible at once. Everything is logged as JSON lines.

usage: xreal_session.py [--host 169.254.2.1] [--dir /tmp/xreal_session] [--allow-camera] [--max-seconds 600]
"""
import argparse
import json
import os
import signal
import socket
import struct
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from xreal_link import decode_pb  # noqa: E402

CONTROL, CAMERA_PORT, TIMESTAMP_PORT = 52999, 52997, 52996
CAMERA_FRAME = 193862  # 6-byte header + payload, docs/findings.md

# Read-only getters (request body `18 00`).
GETTERS = {
    10013: "NRGlassesGetSWVersion", 10015: "NRGlassesGetConfig", 10016: "NRGlassesGetSupportedDevices",
    10025: "NRGlassesGetID", 10029: "NRGlassesGetDspVersion", 10085: "NRDpGetWorkingState", 10273: "NRDpGetInputMode",
    10003: "NRPowerSaveIsEnable", 10005: "NRPowerSaveGetSleepTime", 10008: "NRProximityIsEnable", 10044: "NRProximityGetWearingState",
    10039: "NRProximityGetFarThreshold", 10041: "NRProximityGetNearThreshold",
}
CAMERA = {10047: "NRGrayscaleCameraCreate", 10053: "NRGrayscaleCameraStart", 10054: "NRGrayscaleCameraStop"}
# Setters with a numeric value: id -> name; the only accepted bodies are Base{3: {1: value}} for the values listed in SETTER_BODIES.
SETTERS = {10274: "NRDpSetInputMode"}  # 0 = regular, 1 = side by side
SETTER_BODIES = {10274: {bytes.fromhex("1a020800"), bytes.fromhex("1a020801")}}
# Start requests of the sensor streams (their request layouts are not known; `18 00` is the empty body). The matching Stop requests are NOT allowed.
SENSOR_START = {10036: "NRImuStart", 10031: "NRVsyncStart"}
START, STOP = 10053, 10054
DEFAULT_BODY = bytes.fromhex("1800")
TX_TOP_BIT = 0x80000000


def build_request(msg_id, body, txid):
    payload = struct.pack(">I", (txid | TX_TOP_BIT) & 0xFFFFFFFF) + body
    return struct.pack(">HI", msg_id, len(payload)) + payload


def check_allowed(msg_id, allow_camera, allow_display=False, body=None, allow_sensor=False):
    """Return the request's name, or raise ValueError if this tool must not send it."""
    if msg_id in GETTERS:
        return GETTERS[msg_id]
    if msg_id in SENSOR_START:
        if not allow_sensor:
            raise ValueError("%d (%s) needs --allow-sensor-start" % (msg_id, SENSOR_START[msg_id]))
        if body != DEFAULT_BODY:
            raise ValueError("%d (%s) accepts only the body %s" % (msg_id, SENSOR_START[msg_id], DEFAULT_BODY.hex()))
        return SENSOR_START[msg_id]
    if msg_id in SETTERS:
        if not allow_display:
            raise ValueError("%d (%s) needs --allow-display-mode" % (msg_id, SETTERS[msg_id]))
        if body not in SETTER_BODIES[msg_id]:
            raise ValueError("%d (%s) accepts only the bodies %s" % (msg_id, SETTERS[msg_id], sorted(b.hex() for b in SETTER_BODIES[msg_id])))
        return SETTERS[msg_id]
    if msg_id in CAMERA:
        if not allow_camera:
            raise ValueError("%d (%s) needs --allow-camera" % (msg_id, CAMERA[msg_id]))
        return CAMERA[msg_id]
    raise ValueError("id %d is not on the allowlist" % msg_id)


def decode_response(body):
    """Body after the transaction id: `22 <len> <nested protobuf>`. Returns decode_pb's list of (field, kind, value), or {"raw": hex}."""
    if len(body) >= 2 and body[0] == 0x22:
        n, i, shift = 0, 1, 0
        while True:
            b = body[i]
            i += 1
            n |= (b & 0x7F) << shift
            shift += 7
            if not b & 0x80:
                break
        nested = body[i:i + n]
        try:
            return decode_pb(nested) if nested else []
        except Exception:  # not protobuf: keep the bytes
            return {"raw": nested[:64].hex()}
    return {"raw": body[:64].hex()}


class Session:
    def __init__(self, a):
        self.a = a
        self.t0 = time.monotonic()
        self.log_f = open(os.path.join(a.dir, "session.jsonl"), "a", buffering=1)
        self.lock = threading.Lock()
        self.pending = {}  # (msg_id, txid) -> list for the response
        self.txid = 0
        self.started = False
        self.stop_requested = threading.Event()
        self.cam = {"frames": 0, "bytes": 0, "first_at": None}
        self.ts = {"records": 0}
        self.sock = None

    def log(self, kind, **kv):
        rec = {"t": round(time.monotonic() - self.t0, 3), "kind": kind, **kv}
        with self.lock:
            self.log_f.write(json.dumps(rec) + "\n")
        print(json.dumps(rec), flush=True)

    # ---- control connection
    def connect(self):
        self.sock = socket.create_connection((self.a.host, CONTROL), timeout=4)
        self.sock.settimeout(0.5)
        self.log("connected", host=self.a.host, port=CONTROL)
        threading.Thread(target=self.read_control, daemon=True).start()

    def read_control(self):
        buf = b""
        while not self.stop_requested.is_set():
            try:
                d = self.sock.recv(65536)
            except socket.timeout:
                continue
            except OSError as e:
                self.log("control_error", error=str(e))
                break
            if not d:
                self.log("control_closed_by_glasses")
                break
            buf += d
            while len(buf) >= 6:
                mid, ln = struct.unpack(">HI", buf[:6])
                if len(buf) < 6 + ln:
                    break
                body, buf = buf[6:6 + ln], buf[6 + ln:]
                self.handle_frame(mid, body)
        self.stop_requested.set()

    def handle_frame(self, mid, body):
        if len(body) >= 4:
            txid = struct.unpack(">I", body[:4])[0] & 0x7FFFFFFF
            slot = self.pending.get((mid, txid))
            if slot is not None:
                slot.append(decode_response(body[4:]))
                self.log("response", id=mid, txid=txid, body=body[4:36].hex(), decoded=slot[-1])
                return
        self.log("event", id=mid, length=len(body), head=body[:24].hex())

    def request(self, msg_id, body):
        name = check_allowed(msg_id, self.a.allow_camera, self.a.allow_display_mode, body, self.a.allow_sensor_start)
        self.txid += 1
        txid = self.txid
        pkt = build_request(msg_id, body, txid)
        slot = []
        self.pending[(msg_id, txid)] = slot
        self.log("send", id=msg_id, name=name, txid=txid, packet=pkt.hex())
        self.sock.sendall(pkt)
        if msg_id == START:
            self.started = True
        if msg_id == STOP:
            self.started = False
        end = time.monotonic() + 5
        while not slot and time.monotonic() < end and not self.stop_requested.is_set():
            time.sleep(0.05)
        self.pending.pop((msg_id, txid), None)
        if not slot:
            self.log("no_response", id=msg_id, txid=txid)

    # ---- the two streams (read only)
    def read_camera(self):
        while not self.stop_requested.is_set():
            try:
                s = socket.create_connection((self.a.host, CAMERA_PORT), timeout=3)
            except OSError:
                time.sleep(1)
                continue
            s.settimeout(0.5)
            buf, saved = b"", 0
            frames_f = open(os.path.join(self.a.dir, "camera_frames.bin"), "ab")
            while not self.stop_requested.is_set():
                try:
                    d = s.recv(262144)
                except socket.timeout:
                    continue
                except OSError:
                    break
                if not d:
                    break
                buf += d
                while len(buf) >= CAMERA_FRAME:
                    if buf[:4] != b"\x27\x48\x00\x02":
                        i = buf.find(b"\x27\x48\x00\x02", 1)
                        buf = buf[i:] if i > 0 else b""
                        break
                    frame, buf = buf[:CAMERA_FRAME], buf[CAMERA_FRAME:]
                    if self.cam["first_at"] is None:
                        self.cam["first_at"] = round(time.monotonic() - self.t0, 3)
                        self.log("camera_first_frame")
                    self.cam["frames"] += 1
                    self.cam["bytes"] += len(frame)
                    if saved < 12:
                        frames_f.write(frame)
                        saved += 1
            frames_f.close()
            s.close()
            time.sleep(1)

    def read_timestamps(self):
        while not self.stop_requested.is_set():
            try:
                s = socket.create_connection((self.a.host, TIMESTAMP_PORT), timeout=3)
            except OSError:
                time.sleep(1)
                continue
            s.settimeout(0.5)
            buf = b""
            while not self.stop_requested.is_set():
                try:
                    d = s.recv(65536)
                except socket.timeout:
                    continue
                except OSError:
                    break
                if not d:
                    break
                buf += d
                n = len(buf) // 38
                self.ts["records"] += n
                buf = buf[n * 38:]
            s.close()
            time.sleep(1)

    def report_loop(self):
        last_c, last_t, last = 0, 0, time.monotonic()
        while not self.stop_requested.wait(5):
            now = time.monotonic()
            dt = now - last
            self.log("rates", camera_fps=round((self.cam["frames"] - last_c) / dt, 2),
                     camera_frames_total=self.cam["frames"], timestamp_records_per_s=round((self.ts["records"] - last_t) / dt, 1))
            last_c, last_t, last = self.cam["frames"], self.ts["records"], now

    # ---- commands
    def command_loop(self):
        fifo = os.path.join(self.a.dir, "cmd")
        if not os.path.exists(fifo):
            os.mkfifo(fifo, 0o600)
        self.log("ready", fifo=fifo, allow_camera=self.a.allow_camera, allow_display_mode=self.a.allow_display_mode,
                 allow_sensor_start=self.a.allow_sensor_start)
        while not self.stop_requested.is_set():
            with open(fifo, "r") as f:  # blocks until a writer opens it
                for line in f:
                    parts = line.split()
                    if not parts:
                        continue
                    if parts[0] == "quit":
                        self.stop_requested.set()
                        return
                    if parts[0] != "send" or len(parts) < 2:
                        self.log("bad_command", line=line.strip())
                        continue
                    try:
                        self.request(int(parts[1], 0), bytes.fromhex(parts[2]) if len(parts) > 2 else DEFAULT_BODY)
                    except (ValueError, OSError) as e:
                        self.log("refused" if isinstance(e, ValueError) else "send_error", error=str(e), line=line.strip())

    def shutdown(self):
        self.stop_requested.set()
        if self.started and self.sock:
            try:
                self.log("cleanup_stop")
                self.sock.sendall(build_request(STOP, DEFAULT_BODY, self.txid + 1))
                time.sleep(1.0)
            except OSError:
                pass
        try:
            self.sock.close()
        except Exception:
            pass
        self.log("closed", camera_frames=self.cam["frames"], camera_first_at=self.cam["first_at"])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="169.254.2.1")
    ap.add_argument("--dir", default="/tmp/xreal_session")
    ap.add_argument("--allow-camera", action="store_true", help="permit the camera Create/Start/Stop requests")
    ap.add_argument("--allow-display-mode", action="store_true", help="permit NRDpSetInputMode with value 0 or 1")
    ap.add_argument("--allow-sensor-start", action="store_true", help="permit NRImuStart and NRVsyncStart with the body 18 00")
    ap.add_argument("--max-seconds", type=float, default=600)
    a = ap.parse_args()
    os.makedirs(a.dir, exist_ok=True)
    s = Session(a)
    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, lambda *_: s.stop_requested.set())
    s.connect()
    for fn in (s.read_camera, s.read_timestamps, s.report_loop, s.command_loop):
        threading.Thread(target=fn, daemon=True).start()
    s.stop_requested.wait(a.max_seconds)
    s.shutdown()


if __name__ == "__main__":
    main()

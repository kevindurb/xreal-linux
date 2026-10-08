#!/usr/bin/env python3
"""Record the XREAL One's TCP streams (and optionally its HID endpoints) while a person changes a mode. Read-only.

Nothing is ever written to the glasses: the TCP ports are only read, and the HID nodes are opened read-only.

Writes a capture directory (see docs/capture-format.md):
  meta.json        host, ports, start times, phases, per-port summary
  port<N>.raw      the verbatim byte stream of each TCP port, exactly as received
  port<N>.chunks   one 12-byte entry per recv(): u64 LE host monotonic ns, u32 LE length
  hidraw<N>.raw / .chunks   with --hid: every report read from /dev/hidrawN of the glasses (same chunk format)

Phases are announced on the terminal and stored with host monotonic times, so a capture can be cut into motions
or mode changes afterwards. Example for the anchor-mode experiment (docs/anchor-capture-plan.md):

  capture_eye.py ~/captures/anchor-01 --hid --phases follow:20,toggle:15,anchor:30,toggle:15,follow:20

Analyse afterwards with tools/xreal_link.py DIR.
"""
import argparse
import glob
import json
import os
import select
import socket
import struct
import sys
import threading
import time

DEFAULT_PORTS = tuple(range(52990, 53000))
VENDOR_ID = "00003318"  # XREAL's USB vendor id as it appears in a hidraw HID_ID


class TcpRecorder(threading.Thread):
    def __init__(self, host, port, outdir, stop):
        super().__init__(daemon=True)
        self.name_ = "port%d" % port
        self.host, self.port, self.stop = host, port, stop
        self.raw = open(os.path.join(outdir, self.name_ + ".raw"), "wb")
        self.chunks = open(os.path.join(outdir, self.name_ + ".chunks"), "wb")
        self.bytes = 0
        self.error = None

    def run(self):
        try:
            s = socket.create_connection((self.host, self.port), timeout=3)
            s.settimeout(0.5)
            while not self.stop.is_set():
                try:
                    data = s.recv(1 << 18)
                except socket.timeout:
                    continue
                if not data:
                    self.error = "closed by glasses"
                    break
                write(self, data)
        except OSError as e:
            self.error = str(e)
        finally:
            self.raw.close()
            self.chunks.close()


class HidRecorder(threading.Thread):
    """Passive: open /dev/hidrawN read-only and log whatever reports arrive. Sends nothing."""

    def __init__(self, node, outdir, stop):
        super().__init__(daemon=True)
        self.name_ = os.path.basename(node)
        self.node, self.stop = node, stop
        self.raw = open(os.path.join(outdir, self.name_ + ".raw"), "wb")
        self.chunks = open(os.path.join(outdir, self.name_ + ".chunks"), "wb")
        self.bytes = 0
        self.error = None

    def run(self):
        try:
            fd = os.open(self.node, os.O_RDONLY | os.O_NONBLOCK)
        except OSError as e:
            self.error = str(e)
            return
        try:
            while not self.stop.is_set():
                r, _, _ = select.select([fd], [], [], 0.5)
                if not r:
                    continue
                try:
                    data = os.read(fd, 4096)
                except BlockingIOError:
                    continue
                except OSError as e:
                    self.error = str(e)
                    break
                if data:
                    write(self, data)
        finally:
            os.close(fd)
            self.raw.close()
            self.chunks.close()


def write(rec, data):
    now = time.monotonic_ns()
    rec.raw.write(data)
    rec.chunks.write(struct.pack("<QI", now, len(data)))
    rec.bytes += len(data)


def glasses_hidraw_nodes():
    nodes = []
    for d in sorted(glob.glob("/sys/class/hidraw/hidraw*")):
        try:
            uevent = open(os.path.join(d, "device", "uevent")).read().upper()
        except OSError:
            continue
        if VENDOR_ID in uevent:
            nodes.append("/dev/" + os.path.basename(d))
    return nodes


def parse_phases(text):
    out = []
    for item in text.split(","):
        name, _, secs = item.partition(":")
        out.append((name, float(secs)))
    return out


def message_summary(path, seconds):
    """Count packets by msg_id using the framing msg_id(u16 BE) + length(u32 BE) + payload, resyncing on damage."""
    data = open(path, "rb").read()
    ids, i, skipped = {}, 0, 0
    while i + 6 <= len(data):
        mid, ln = struct.unpack_from(">HI", data, i)
        if 10000 <= mid <= 13000 and ln <= (4 << 20):
            if i + 6 + ln > len(data):
                break
            ids[mid] = ids.get(mid, 0) + 1
            i += 6 + ln
        else:
            i += 1
            skipped += 1
    return {"msg_ids": {str(k): {"count": v, "per_s": round(v / seconds, 1)} for k, v in sorted(ids.items())},
            "skipped_bytes": skipped}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("outdir")
    ap.add_argument("--host", default="169.254.2.1")
    ap.add_argument("--ports", default=",".join(str(p) for p in DEFAULT_PORTS),
                    help="comma separated TCP ports to record (default 52990-52999)")
    ap.add_argument("--hid", action="store_true", help="also record the glasses' hidraw nodes (read-only)")
    ap.add_argument("--phases", default="still:20,shake:20,rotate:20")
    ap.add_argument("--note", default="", help="free text stored in meta.json (firmware, glasses mode, what you did)")
    a = ap.parse_args()
    ports = [int(p) for p in a.ports.split(",") if p]
    phases = parse_phases(a.phases)
    os.makedirs(a.outdir, exist_ok=False)

    stop = threading.Event()
    recs = [TcpRecorder(a.host, p, a.outdir, stop) for p in ports]
    if a.hid:
        nodes = glasses_hidraw_nodes()
        print("hidraw nodes of the glasses:", nodes or "none found", flush=True)
        recs += [HidRecorder(n, a.outdir, stop) for n in nodes]
    start_ns, start_wall = time.monotonic_ns(), time.time()
    for r in recs:
        r.start()

    marks = []
    for name, secs in phases:
        marks.append({"name": name, "start_ns": time.monotonic_ns(), "seconds": secs})
        print(">>> %s for %.0f s" % (name.upper(), secs), flush=True)
        time.sleep(secs)
    end_ns = time.monotonic_ns()
    stop.set()
    for r in recs:
        r.join()

    dur = (end_ns - start_ns) / 1e9
    summary = {}
    for r in recs:
        entry = {"bytes": r.bytes, "error": r.error}
        if r.name_.startswith("port") and r.bytes:
            entry.update(message_summary(os.path.join(a.outdir, r.name_ + ".raw"), dur))
        summary[r.name_] = entry
    meta = {"format": 2, "host": a.host, "wall_start": start_wall, "start_ns": start_ns, "end_ns": end_ns,
            "duration_s": dur, "phases": marks, "note": a.note, "ports": summary}
    with open(os.path.join(a.outdir, "meta.json"), "w") as f:
        json.dump(meta, f, indent=2)
    print(json.dumps(summary, indent=2))
    cam = summary.get("port52997", {})
    if not cam.get("bytes"):
        print("NOTE: no camera data (port 52997). The Eye only streams while something starts it "
              "(docs/findings.md).", file=sys.stderr)


if __name__ == "__main__":
    main()

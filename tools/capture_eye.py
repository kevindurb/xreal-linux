#!/usr/bin/env python3
"""Record the Eye camera (52997), display-timestamp (52996) and IMU (52998) streams together. Read-only.

Writes a capture directory (see docs/capture-format.md):
  meta.json        host, ports, start times, phases, per-port summary
  port52997.raw    the verbatim byte stream of each port, exactly as received
  port52997.chunks one 12-byte entry per recv(): u64 LE host monotonic ns, u32 LE length
  ... same for 52996 and 52998

Phases ("still:20,shake:20,rotate:20") are announced on the terminal and stored with host
monotonic times, so a capture can be cut into motions afterwards.

  capture_eye.py OUTDIR [--host 169.254.2.1] [--phases still:20,shake:20,rotate:20]
"""
import argparse
import json
import os
import socket
import struct
import sys
import threading
import time

PORTS = (52997, 52996, 52998)
CAMERA_FRAME = 193862
CAMERA_MAGIC = bytes.fromhex("27480002")
TS_RECORD = 38
IMU_RECORD = 134


class Recorder(threading.Thread):
    def __init__(self, host, port, outdir, stop):
        super().__init__(daemon=True)
        self.host, self.port, self.stop = host, port, stop
        self.raw = open(os.path.join(outdir, f"port{port}.raw"), "wb")
        self.chunks = open(os.path.join(outdir, f"port{port}.chunks"), "wb")
        self.bytes = 0
        self.first_ns = None
        self.last_ns = None
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
                now = time.monotonic_ns()
                self.first_ns = self.first_ns or now
                self.last_ns = now
                self.raw.write(data)
                self.chunks.write(struct.pack("<QI", now, len(data)))
                self.bytes += len(data)
        except OSError as e:
            self.error = str(e)
        finally:
            self.raw.close()
            self.chunks.close()


def parse_phases(text):
    out = []
    for item in text.split(","):
        name, _, secs = item.partition(":")
        out.append((name, float(secs)))
    return out


def count_magic(path, magic, size):
    """Records of a fixed size starting with magic (resyncing is the reader's job; this is a summary)."""
    data = open(path, "rb").read()
    n, i = 0, 0
    while True:
        i = data.find(magic, i)
        if i < 0 or i + size > len(data):
            return n
        n += 1
        i += size


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir")
    ap.add_argument("--host", default="169.254.2.1")
    ap.add_argument("--phases", default="still:20,shake:20,rotate:20")
    a = ap.parse_args()
    phases = parse_phases(a.phases)
    os.makedirs(a.outdir, exist_ok=False)

    stop = threading.Event()
    recs = [Recorder(a.host, p, a.outdir, stop) for p in PORTS]
    start_ns, start_wall = time.monotonic_ns(), time.time()
    for r in recs:
        r.start()

    marks = []
    for name, secs in phases:
        marks.append({"name": name, "start_ns": time.monotonic_ns(), "seconds": secs})
        print(f">>> {name.upper()} for {secs:.0f} s", flush=True)
        time.sleep(secs)
    end_ns = time.monotonic_ns()
    stop.set()
    for r in recs:
        r.join()

    dur = (end_ns - start_ns) / 1e9
    summary = {}
    for r in recs:
        path = os.path.join(a.outdir, f"port{r.port}.raw")
        entry = {"bytes": r.bytes, "error": r.error}
        if r.port == 52997:
            entry["frames_by_size"] = r.bytes // CAMERA_FRAME
            entry["fps"] = round(r.bytes / CAMERA_FRAME / dur, 2)
        elif r.port == 52996:
            entry["records"] = r.bytes // TS_RECORD
            entry["per_s"] = round(r.bytes / TS_RECORD / dur, 1)
        else:
            entry["records_approx"] = r.bytes // IMU_RECORD
            entry["per_s"] = round(r.bytes / IMU_RECORD / dur, 1)
        summary[str(r.port)] = entry
    meta = {
        "format": 1,
        "host": a.host,
        "wall_start": start_wall,
        "start_ns": start_ns,
        "end_ns": end_ns,
        "duration_s": dur,
        "phases": marks,
        "ports": summary,
    }
    with open(os.path.join(a.outdir, "meta.json"), "w") as f:
        json.dump(meta, f, indent=2)
    print(json.dumps(summary, indent=2))
    if summary["52997"]["bytes"] == 0:
        print("NOTE: no camera data. The Eye only streams while a feature such as anchor mode needs it "
              "(docs/findings.md).", file=sys.stderr)


if __name__ == "__main__":
    main()

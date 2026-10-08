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

  capture_eye.py ~/captures/anchor-01 --hid --notify --say --phases follow:20,toggle-on:30,anchor:30,toggle-off:30,follow:20

With --notify (and --say to speak them aloud), each phase pops up a desktop notification (notify-send) on the machine running the recorder, so the person
wearing the glasses knows what to do next. Phase names with a built-in message: follow, toggle-on, anchor, toggle-off, still,
shake, rotate, done; any other name is shown as is.

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


MESSAGES = {
    "follow": "Follow mode: hold still and do nothing.",
    "toggle-on": "NOW: switch the glasses to ANCHOR mode from their menu.",
    "anchor": "Anchor mode on: hold still, glasses pointing at the Deck screen.",
    "toggle-off": "NOW: switch the glasses back to FOLLOW mode.",
    "still": "Hold the glasses completely still.",
    "shake": "Shake the glasses gently.",
    "rotate": "Rotate the glasses slowly in all directions.",
    "done": "Capture finished. You can stop.",
}


def notify(title, body, enabled, seconds=8):
    """Best-effort desktop notification; never raises. Works from an SSH session by pointing at the user's session bus."""
    if not enabled:
        return
    import shutil
    import subprocess
    exe = shutil.which("notify-send")
    if not exe:
        return
    env = dict(os.environ)
    run = env.setdefault("XDG_RUNTIME_DIR", "/run/user/%d" % os.getuid())
    env.setdefault("DBUS_SESSION_BUS_ADDRESS", "unix:path=%s/bus" % run)
    try:
        subprocess.run([exe, "-a", "XREAL capture", "-u", "normal", "-t", str(seconds * 1000), title, body],
                       env=env, timeout=5, capture_output=True)
    except Exception:
        pass


def say(text, enabled):
    """Best-effort spoken prompt (spd-say, else espeak-ng); never raises or blocks the capture."""
    if not enabled:
        return
    import shutil
    import subprocess
    for exe, args in (("spd-say", ["-w"]), ("espeak-ng", [])):
        path = shutil.which(exe)
        if path:
            try:
                subprocess.Popen([path] + args + [text], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            except Exception:
                pass
            return


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
    ap.add_argument("--notify", action="store_true",
                    help="show a desktop notification at each phase (and a heads-up 5 s before the next one)")
    ap.add_argument("--say", action="store_true", help="also speak each prompt aloud (spd-say or espeak-ng)")
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
    for k, (name, secs) in enumerate(phases):
        marks.append({"name": name, "start_ns": time.monotonic_ns(), "seconds": secs})
        print(">>> %s for %.0f s" % (name.upper(), secs), flush=True)
        notify("%s (%.0f s)" % (name.upper(), secs), MESSAGES.get(name, name), a.notify, min(int(secs), 20))
        say(MESSAGES.get(name, name), a.say)
        warn = secs - 5 if k + 1 < len(phases) and secs > 10 else None
        if warn:
            time.sleep(warn)
            nxt = phases[k + 1][0]
            notify("Next in 5 s: %s" % nxt.upper(), MESSAGES.get(nxt, nxt), a.notify, 5)
            say("Five seconds. Next: " + MESSAGES.get(nxt, nxt), a.say)
            time.sleep(5)
        else:
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
    notify("DONE", MESSAGES["done"], a.notify)
    say(MESSAGES["done"], a.say)
    cam = summary.get("port52997", {})
    if not cam.get("bytes"):
        print("NOTE: no camera data (port 52997). The Eye only streams while something starts it "
              "(docs/findings.md).", file=sys.stderr)


if __name__ == "__main__":
    main()

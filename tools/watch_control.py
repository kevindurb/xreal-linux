#!/usr/bin/env python3
"""Passively watch everything the XREAL glasses might use to signal control/state changes. Read-only.

Logs timestamped lines for: unexpected TCP traffic on ports 52990-52999 (with reconnects), raw HID
reads, button input events, DRM connector/EDID changes and udev events. High-rate known streams
(IMU 52998, camera 52997, timestamps 52996) are only summarised, but any record that does not match
the known layout is logged in full.

usage: watch_control.py [seconds] [logfile]
"""
import glob
import hashlib
import os
import re
import select
import socket
import struct
import subprocess
import sys
import threading
import time

DURATION = float(sys.argv[1]) if len(sys.argv) > 1 else 180
LOG = open(sys.argv[2] if len(sys.argv) > 2 else "/tmp/watch_control.log", "w", buffering=1)
T0 = time.time()
LOCK = threading.Lock()
STOP = threading.Event()
HOST = "169.254.1.1"
PORTS = list(range(52990, 53000))


def log(src, msg):
    with LOCK:
        LOG.write(f"{time.time() - T0:8.2f} {src:<10} {msg}\n")


def hexs(b, n=96):
    return b[:n].hex(" ") + (f" ...(+{len(b) - n})" if len(b) > n else "")


def tcp_watch(port):
    src = f"tcp{port}"
    known = {52998: (134, bytes.fromhex("283600000080"), (0x0B, 0x04), 30),
             52996: (38, bytes.fromhex("273100000020"), None, None)}
    was_up = None
    while not STOP.is_set():
        try:
            s = socket.create_connection((HOST, port), timeout=1.5)
            s.settimeout(1.0)
        except OSError as e:
            if was_up is not False:
                log(src, f"connect failed: {e}")
            was_up = False
            STOP.wait(0.5)
            continue
        if was_up is not True:
            log(src, "connected")
        was_up = True
        buf, count, last, bps = b"", 0, time.time(), 0
        try:
            while not STOP.is_set():
                try:
                    d = s.recv(65536)
                except socket.timeout:
                    continue
                if not d:
                    log(src, "closed by glasses")
                    break
                count += len(d)
                if port == 52997:                      # camera: summarise only, watch the header
                    buf = (buf + d)[:64] if len(buf) < 64 else buf
                    if len(buf) >= 23 and getattr(tcp_watch, "hdr", None) != buf[4:23]:
                        if getattr(tcp_watch, "hdr", None) is not None:
                            log(src, f"camera header changed: {hexs(buf[:32])}")
                        tcp_watch.hdr = buf[4:23]
                elif port in known:
                    size, magic, okTypes, typeOff = known[port]
                    buf += d
                    while len(buf) >= size:
                        i = buf.find(magic)
                        if i < 0:
                            buf = buf[-(len(magic) - 1):]
                            break
                        if len(buf) - i < size:
                            buf = buf[i:]
                            break
                        rec, buf = buf[i:i + size], buf[i + size:]
                        if okTypes is not None and struct.unpack_from("<I", rec, typeOff)[0] not in okTypes:
                            log(src, f"UNEXPECTED record type: {hexs(rec, size)}")
                else:
                    log(src, f"DATA {len(d)}B: {hexs(d)}")
                if time.time() - last >= 1.0:
                    rate = count / (time.time() - last)
                    if port in (52996, 52997, 52998) and abs(rate - bps) > 0.25 * max(bps, 1):
                        log(src, f"rate {rate / 1000:.1f} kB/s (was {bps / 1000:.1f})")
                        bps = rate
                    count, last = 0, time.time()
        except OSError as e:
            log(src, f"error: {e}")
        s.close()
        was_up = None
        STOP.wait(0.3)


def fd_watch(path, label, fmt):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    except OSError as e:
        log(label, f"cannot open {path}: {e}")
        return
    log(label, f"watching {path}")
    while not STOP.is_set():
        r, _, _ = select.select([fd], [], [], 0.5)
        if not r:
            continue
        try:
            d = os.read(fd, 4096)
        except OSError as e:
            log(label, f"read error: {e}")
            return
        if fmt == "input":
            for i in range(0, len(d) - 23, 24):
                sec, usec, typ, code, val = struct.unpack_from("<qqHHi", d, i)
                if typ != 0:                       # skip EV_SYN
                    log(label, f"event type={typ} code={code} value={val}")
        else:
            log(label, f"{len(d)}B: {hexs(d)}")


def drm_watch():
    conn = "/sys/class/drm/card1-DP-1"
    prev = None
    while not STOP.is_set():
        try:
            st = open(f"{conn}/status").read().strip()
            modes = open(f"{conn}/modes").read().split()
            edid = open(f"{conn}/edid", "rb").read()
            cur = (st, tuple(modes), hashlib.md5(edid).hexdigest()[:8])
        except OSError as e:
            cur = (f"unreadable: {e}", (), "")
        if cur != prev:
            uniq = sorted(set(cur[1]), key=cur[1].index)
            log("drm", f"status={cur[0]} edid={cur[2]} modes={len(cur[1])} unique={uniq[:12]}")
            prev = cur
        STOP.wait(0.25)


def udev_watch():
    p = subprocess.Popen(["udevadm", "monitor", "--udev", "--subsystem-match=drm", "--subsystem-match=usb",
                          "--subsystem-match=hidraw", "--subsystem-match=input", "--subsystem-match=net",
                          "--subsystem-match=sound"], stdout=subprocess.PIPE, text=True)
    while not STOP.is_set():
        line = p.stdout.readline()
        if not line:
            break
        if line.strip() and not line.startswith("monitor"):
            log("udev", line.strip())
    p.terminate()


def xreal_input_nodes():
    nodes = []
    txt = open("/proc/bus/input/devices").read().split("\n\n")
    for blk in txt:
        if "XREAL" in blk:
            name = re.search(r'N: Name="([^"]+)"', blk).group(1)
            for ev in re.findall(r"event\d+", blk):
                nodes.append((f"/dev/input/{ev}", name))
    return nodes


def xreal_hidraw():
    out = []
    for h in sorted(glob.glob("/sys/class/hidraw/hidraw*")):
        try:
            if "XREAL" in open(f"{h}/device/uevent").read():
                out.append("/dev/" + os.path.basename(h))
        except OSError:
            pass
    return out


def main():
    threads = [threading.Thread(target=tcp_watch, args=(p,), daemon=True) for p in PORTS]
    threads += [threading.Thread(target=drm_watch, daemon=True), threading.Thread(target=udev_watch, daemon=True)]
    for path in xreal_hidraw():
        threads.append(threading.Thread(target=fd_watch, args=(path, os.path.basename(path), "raw"), daemon=True))
    for path, name in xreal_input_nodes():
        threads.append(threading.Thread(target=fd_watch, args=(path, "in:" + os.path.basename(path), "input"), daemon=True))
    log("start", f"watching for {DURATION:.0f}s: {len(threads)} watchers")
    for t in threads:
        t.start()
    STOP.wait(DURATION)
    STOP.set()
    log("end", "done")


if __name__ == "__main__":
    main()

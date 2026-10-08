#!/usr/bin/env python3
"""Decode and summarise XREAL One TCP stream captures. Offline and read-only.

The One's streams (ports 52990-52999) carry packets framed as
    msg_id (u16 big endian) | payload length (u32 big endian) | payload
(see docs/xreal-link-messages.md). This tool parses capture files made by tools/capture_eye.py (a directory
of port*.raw files) or any raw dump of one stream, resynchronises after damage, names the message ids, and
reports rates, gaps and timestamp behaviour. The event stream (10122) is decoded as protobuf.

usage:
  xreal_link.py PATH [PATH ...]          summary of each capture directory or .raw/.bin file
  xreal_link.py PATH --dump ID [N]       print the first N packets (default 3) with message id ID
"""
import argparse
import collections
import glob
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
try:
    from xreal_link_ids import MESSAGE_NAMES
except ImportError:  # the tool still works without names
    MESSAGE_NAMES = {}

ID_MIN, ID_MAX = 10000, 13000
MAX_PAYLOAD = 4 << 20
# message id -> (name, offset of the u64 nanosecond timestamp inside the whole packet)
TIMESTAMPED = {10033: 14, 10294: 14, 10056: 23}


def parse_packets(data):
    """Yield (offset, msg_id, payload) and count bytes skipped while resynchronising."""
    i, skipped = 0, 0
    out = []
    n = len(data)
    while i + 6 <= n:
        mid, ln = struct.unpack_from(">HI", data, i)
        if ID_MIN <= mid <= ID_MAX and ln <= MAX_PAYLOAD:
            if i + 6 + ln > n:
                break  # truncated final packet
            out.append((i, mid, data[i + 6:i + 6 + ln]))
            i += 6 + ln
        else:
            i += 1
            skipped += 1
    return out, skipped


def varint(b, i):
    r = s = 0
    while True:
        c = b[i]
        i += 1
        r |= (c & 0x7F) << s
        s += 7
        if not c & 0x80:
            return r, i


def decode_pb(b, depth=0):
    """Generic protobuf wire decode -> list of (field, kind, value); sub-messages are decoded when they parse cleanly."""
    out, i = [], 0
    while i < len(b):
        k, i = varint(b, i)
        f, w = k >> 3, k & 7
        if f == 0:
            raise ValueError("field 0")
        if w == 0:
            v, i = varint(b, i)
            out.append((f, "varint", v))
        elif w == 1:
            out.append((f, "fixed64", b[i:i + 8].hex()))
            i += 8
        elif w == 5:
            raw = b[i:i + 4]
            i += 4
            out.append((f, "fixed32", {"float": round(struct.unpack("<f", raw)[0], 4),
                                       "int": struct.unpack("<i", raw)[0]}))
        elif w == 2:
            n, i = varint(b, i)
            v = b[i:i + n]
            if len(v) != n:
                raise ValueError("truncated")
            i += n
            sub = None
            if depth < 3 and n:
                try:
                    sub = decode_pb(v, depth + 1)
                except Exception:
                    sub = None
            out.append((f, "msg", sub) if sub else (f, "bytes", v.hex()))
        else:
            raise ValueError("wire type %d" % w)
    return out


def name_of(mid):
    return MESSAGE_NAMES.get(mid, "?")


def u64_at(buf, off):
    return struct.unpack_from("<Q", buf, off)[0]


def summarise(label, data):
    pk, skipped = parse_packets(data)
    print("== %s: %d bytes, %d packets, %d bytes skipped while resyncing" % (label, len(data), len(pk), skipped))
    if not pk:
        return
    by = collections.defaultdict(list)
    for off, mid, pl in pk:
        by[mid].append((off, pl))
    for mid in sorted(by):
        items = by[mid]
        lens = collections.Counter(len(p) for _, p in items)
        line = "   id %5d %-34s count %6d  payload len %s" % (
            mid, name_of(mid), len(items),
            ",".join("%d(x%d)" % (k, v) for k, v in lens.most_common(3)))
        print(line)
        if mid == 10294:
            report_imu_kinds(data, items)
        elif mid in TIMESTAMPED:
            ts = []
            for off, pl in items:
                pkt = data[off:off + 6 + len(pl)]
                if len(pkt) >= TIMESTAMPED[mid] + 8:
                    ts.append(u64_at(pkt, TIMESTAMPED[mid]))
            if len(ts) > 2:
                d = [b - a for a, b in zip(ts, ts[1:])]
                back = sum(1 for x in d if x <= 0)
                span = (ts[-1] - ts[0]) / 1e9
                print("      timestamps: %.3f s span, rate %.1f Hz, median step %.3f ms, max step %.3f ms, non-increasing %d"
                      % (span, (len(ts) - 1) / span if span > 0 else 0, sorted(d)[len(d) // 2] / 1e6, max(d) / 1e6, back))
        if mid == 10122:
            for _, pl in items[:5]:
                try:
                    print("      event:", json.dumps(decode_pb(pl)))
                except Exception as e:
                    print("      event (undecoded %s): %s" % (e, pl.hex()))


IMU_KINDS = {11: "gyro+accel", 4: "magnetometer"}


def report_imu_kinds(data, items):
    """IMU packets interleave record kinds (u32 at packet offset 30); timestamps are monotonic per kind, not merged."""
    by = collections.defaultdict(list)
    for off, pl in items:
        pkt = data[off:off + 6 + len(pl)]
        if len(pkt) >= 134:
            by[struct.unpack_from("<I", pkt, 30)[0]].append(u64_at(pkt, 14))
    for kind in sorted(by):
        ts = by[kind]
        label = IMU_KINDS.get(kind, "unknown")
        if len(ts) < 3:
            print("      kind %d (%s): %d records" % (kind, label, len(ts)))
            continue
        d = [b - a for a, b in zip(ts, ts[1:])]
        span = (ts[-1] - ts[0]) / 1e9
        print("      kind %-2d (%-12s): %6d records, %.1f Hz, median step %.3f ms, max step %.3f ms, non-increasing %d"
              % (kind, label, len(ts), (len(ts) - 1) / span if span > 0 else 0,
                 sorted(d)[len(d) // 2] / 1e6, max(d) / 1e6, sum(1 for x in d if x <= 0)))


def load_inputs(path):
    """Return [(label, bytes)] for a capture directory or a single file."""
    if os.path.isdir(path):
        res = []
        for f in sorted(glob.glob(os.path.join(path, "port*.raw"))):
            res.append((os.path.basename(f), open(f, "rb").read()))
        meta = os.path.join(path, "meta.json")
        if os.path.exists(meta):
            m = json.load(open(meta))
            print("# %s: host %s, %.1f s, phases %s" % (path, m.get("host"), m.get("duration_s", 0),
                                                       [p["name"] for p in m.get("phases", [])]))
        return res
    return [(os.path.basename(path), open(path, "rb").read())]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="+")
    ap.add_argument("--dump", nargs="+", metavar=("ID", "N"))
    a = ap.parse_args()
    for p in a.paths:
        for label, data in load_inputs(p):
            if a.dump:
                want = int(a.dump[0])
                n = int(a.dump[1]) if len(a.dump) > 1 else 3
                pk, _ = parse_packets(data)
                shown = 0
                for off, mid, pl in pk:
                    if mid == want and shown < n:
                        print("%s @%d id %d (%s) len %d: %s" % (label, off, mid, name_of(mid), len(pl), pl[:48].hex(" ")))
                        shown += 1
            else:
                summarise(label, data)


if __name__ == "__main__":
    main()

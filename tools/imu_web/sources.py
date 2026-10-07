"""IMU sample sources for the XREAL web viewer: live TCP, file replay, and a labelled simulator.

Everything here is read-only with respect to the glasses: the TCP source only receives.

A "sample" is [t, gx, gy, gz, ax, ay, az] with t in seconds since the source started
(gyro rad/s, accel m/s^2), matching the stream layout documented in docs/findings.md.
"""
import json
import math
import random
import socket
import struct
import sys
import time

MAGIC = bytes.fromhex("2836000000803841")
RECORD = 134
TYPE_IMU = 0x0B
TS_OFFSET, TYPE_OFFSET, FLOAT_OFFSET = 14, 30, 34
BATCH_SECONDS = 0.04


def parse_records(buf):
    """Pull complete IMU records out of buf. Returns ([(ts_ns, gx..az), ...], remainder)."""
    out, pos = [], 0
    while True:
        i = buf.find(MAGIC, pos)
        if i < 0:
            return out, buf[max(pos, len(buf) - (len(MAGIC) - 1)):]
        if len(buf) - i < RECORD:
            return out, buf[i:]
        pos = i + RECORD
        if struct.unpack_from("<I", buf, i + TYPE_OFFSET)[0] != TYPE_IMU:
            continue
        vals = struct.unpack_from("<6f", buf, i + FLOAT_OFFSET)
        if not all(math.isfinite(v) for v in vals):
            continue
        out.append((struct.unpack_from("<Q", buf, i + TS_OFFSET)[0],) + vals)


def to_sample(ts_ns, first_ns, vals):
    return [round((ts_ns - first_ns) / 1e9, 6)] + [round(v, 5) for v in vals]


class TcpSource:
    label_prefix = "live"

    def __init__(self, host, port):
        self.host, self.port = host, port
        self.label = f"live {host}:{port}"
        self.kind = "live"

    def run(self, hub, stop):
        while not stop.is_set():
            try:
                sock = socket.create_connection((self.host, self.port), timeout=3)
                sock.settimeout(5)
            except OSError as e:
                hub.set_connected(False, f"{self.host}:{self.port}: {e}")
                stop.wait(2)
                continue
            hub.set_connected(True)
            hub.publish_reset()
            buf, first, batch, last_flush = b"", None, [], time.monotonic()
            try:
                while not stop.is_set():
                    chunk = sock.recv(8192)
                    if not chunk:
                        raise OSError("connection closed by glasses")
                    recs, buf = parse_records(buf + chunk)
                    for r in recs:
                        if first is None:
                            first = r[0]
                        batch.append(to_sample(r[0], first, r[1:]))
                    if batch and time.monotonic() - last_flush >= BATCH_SECONDS:
                        hub.publish(batch)
                        batch, last_flush = [], time.monotonic()
            except OSError as e:
                hub.set_connected(False, str(e))
            finally:
                sock.close()
            stop.wait(2)


class ReplaySource:
    def __init__(self, path, speed=1.0):
        self.path, self.speed = path, speed
        self.label = f"replay {path}"
        self.kind = "replay"

    def run(self, hub, stop):
        with open(self.path, "rb") as f:
            recs, _ = parse_records(f.read())
        if not recs:
            hub.set_connected(False, f"no IMU records in {self.path}")
            return
        first = recs[0][0]
        samples = [to_sample(r[0], first, r[1:]) for r in recs]
        hub.set_connected(True)
        while not stop.is_set():
            hub.publish_reset()
            start, i = time.monotonic(), 0
            while i < len(samples) and not stop.is_set():
                now = (time.monotonic() - start) * self.speed
                j = i
                while j < len(samples) and samples[j][0] <= now:
                    j += 1
                if j > i:
                    hub.publish(samples[i:j])
                    i = j
                stop.wait(BATCH_SECONDS)


# --- simulator ---------------------------------------------------------------------------------
# Clearly synthetic. The axis/sign conventions below are INVENTED for testing the viewer and say
# nothing about the real glasses: yaw-left = -gyro Y, pitch-up = +gyro X, roll-left-shoulder = +gyro Z.

SIM_RATE = 1000
SIM_MOVE_S, SIM_HOLD_S, SIM_LEAD_S = 1.5, 2.5, 4.0
SIM_G = 9.77
SIM_BIAS = (0.012, -0.009, 0.007)
# (label, axis index, signed total angle in degrees)
SIM_SCRIPT = [
    ("yaw-left", 1, -45), ("yaw-right", 1, 45),
    ("pitch-up", 0, 30), ("pitch-down", 0, -30),
    ("roll-left", 2, 30), ("roll-right", 2, -30),
]
SIM_PERIOD = SIM_LEAD_S + len(SIM_SCRIPT) * 2 * (SIM_MOVE_S + SIM_HOLD_S)


def _ease(u):
    """Smooth 0..1 position and its derivative (per unit u)."""
    u = min(max(u, 0.0), 1.0)
    return 0.5 - 0.5 * math.cos(math.pi * u), 0.5 * math.pi * math.sin(math.pi * u)


def sim_truth(t):
    """Per-axis (angle_rad[3], rate_rad_s[3]) at time t."""
    t %= SIM_PERIOD
    ang, rate = [0.0] * 3, [0.0] * 3
    t -= SIM_LEAD_S
    if t < 0:
        return ang, rate
    slot = SIM_MOVE_S + SIM_HOLD_S
    k = int(t // (2 * slot))
    if k >= len(SIM_SCRIPT):
        return ang, rate
    _, axis, deg = SIM_SCRIPT[k]
    t -= k * 2 * slot
    total = math.radians(deg)
    if t < SIM_MOVE_S:                       # go
        p, dp = _ease(t / SIM_MOVE_S)
        ang[axis], rate[axis] = total * p, total * dp / SIM_MOVE_S
    elif t < slot:                           # hold at pose
        ang[axis] = total
    elif t < slot + SIM_MOVE_S:              # return
        p, dp = _ease((t - slot) / SIM_MOVE_S)
        ang[axis], rate[axis] = total * (1 - p), -total * dp / SIM_MOVE_S
    return ang, rate


def _rot_x(v, a):
    c, s = math.cos(a), math.sin(a)
    return (v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c)


def _rot_z(v, a):
    c, s = math.cos(a), math.sin(a)
    return (v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2])


def sim_sample(t, rng):
    ang, rate = sim_truth(t)
    a = _rot_z(_rot_x((0.0, -SIM_G, 0.0), -ang[0]), -ang[2])   # gravity as seen by the device
    g = [rate[i] + SIM_BIAS[i] + rng.gauss(0, 0.004) for i in range(3)]
    a = [a[i] + rng.gauss(0, 0.03) for i in range(3)]
    return [round(t, 6)] + [round(v, 5) for v in g + a]


class SimSource:
    def __init__(self):
        self.label = "SIMULATED (synthetic data, invented axis conventions)"
        self.kind = "sim"

    def run(self, hub, stop):
        rng = random.Random(1)
        hub.set_connected(True)
        hub.publish_reset()
        start, n = time.monotonic(), 0
        while not stop.is_set():
            target = int((time.monotonic() - start) * SIM_RATE)
            if target > n:
                hub.publish([sim_sample(i / SIM_RATE, rng) for i in range(n, target)])
                n = target
            stop.wait(BATCH_SECONDS)


def make_source(spec):
    if spec == "sim":
        return SimSource()
    if spec.startswith("replay:"):
        return ReplaySource(spec[len("replay:"):])
    if spec.startswith("tcp://"):
        host, _, port = spec[len("tcp://"):].partition(":")
        return TcpSource(host, int(port or 52998))
    raise ValueError(f"unknown source {spec!r}; use tcp://HOST[:PORT], replay:FILE or sim")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "dump-sim":
        rng = random.Random(1)
        n = int(float(sys.argv[2]) * SIM_RATE)
        json.dump([sim_sample(i / SIM_RATE, rng) for i in range(n)], sys.stdout, separators=(",", ":"))
    else:
        sys.exit("usage: sources.py dump-sim SECONDS")

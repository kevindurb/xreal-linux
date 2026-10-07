#!/usr/bin/env python3
"""Read the XREAL One-series IMU stream (TCP 52998) and print gyro/accel. Read-only."""
import socket
import struct
import sys

HOST = sys.argv[1] if len(sys.argv) > 1 else "169.254.2.1"
PORT = 52998
MAGIC = bytes.fromhex("28360000008038" "41")
RECORD = 134
TYPE_IMU = 0x0B


def main():
    s = socket.create_connection((HOST, PORT), timeout=3)
    buf = b""
    while True:
        chunk = s.recv(4096)
        if not chunk:
            return
        buf += chunk
        while len(buf) >= RECORD:
            i = buf.find(MAGIC)
            if i < 0:
                buf = buf[-(len(MAGIC) - 1):]
                break
            if len(buf) - i < RECORD:
                buf = buf[i:]
                break
            rec, buf = buf[i:i + RECORD], buf[i + RECORD:]
            ts, = struct.unpack_from("<Q", rec, 14)
            kind, = struct.unpack_from("<I", rec, 30)
            if kind != TYPE_IMU:
                continue
            gx, gy, gz, ax, ay, az = struct.unpack_from("<6f", rec, 34)
            print(f"{ts:>16} gyro(rad/s) {gx:+.4f} {gy:+.4f} {gz:+.4f}  accel(m/s^2) {ax:+.3f} {ay:+.3f} {az:+.3f}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass

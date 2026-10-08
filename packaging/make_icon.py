#!/usr/bin/env python3
"""Write a plain 256x256 PNG icon (two lens-like rounded rectangles on a dark square) without any imaging library."""
import struct
import sys
import zlib

W = H = 256


def pixel(x, y):
    def in_lens(cx):
        dx, dy = abs(x - cx) - 36, abs(y - 128) - 22
        return max(dx, 0) ** 2 + max(dy, 0) ** 2 <= 22 ** 2
    if in_lens(78) or in_lens(178):
        return (120, 200, 255, 255)
    return (24, 28, 40, 255)


raw = b"".join(b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(W)) for y in range(H))


def chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)

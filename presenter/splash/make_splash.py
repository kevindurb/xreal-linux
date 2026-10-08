#!/usr/bin/env python3
"""Generate the presenter's splash patch without any imaging library.

Writes, next to this script:
  splash.raw   width x height bytes of coverage (0 = background, 255 = full mark), row by row, top row first
  splash.size  "WIDTH HEIGHT\\n", read by src/splash.rs (a unit test checks it against the byte count of splash.raw)

With --png FILE it also writes a preview of the whole patch on the background, using the same levels as src/splash.rs.
The output is deterministic: running the script twice gives identical files.
"""
import os
import struct
import sys
import zlib

W, H = 640, 160
TEXT = "Waiting for SteamVR"
SCALE = 5        # picture pixels per font pixel
ADVANCE = 6      # font pixels per character, including one column of space
TEXT_TOP = 104
LENS_CENTRES = (W // 2 - 80, W // 2 + 80)
LENS_Y = 44
LENS_HALF = (40, 14)   # half-extent of the flat part; a rounded rectangle of this plus RADIUS
LENS_RADIUS = 24
# Levels used only by the preview; the presenter's are the constants BACKGROUND and MARK in src/splash.rs.
PREVIEW_BACKGROUND, PREVIEW_MARK = 8, 70

# 5x7 bitmap font for the letters of TEXT only.
FONT = {
    "W": ("X...X", "X...X", "X...X", "X.X.X", "X.X.X", "XX.XX", "X...X"),
    "a": (".....", ".....", ".XXX.", "....X", ".XXXX", "X...X", ".XXXX"),
    "i": ("..X..", ".....", ".XX..", "..X..", "..X..", "..X..", ".XXX."),
    "t": (".X...", ".X...", "XXXX.", ".X...", ".X...", ".X..X", "..XX."),
    "n": (".....", ".....", "X.XX.", "XX..X", "X...X", "X...X", "X...X"),
    "g": (".....", ".XXXX", "X...X", "X...X", ".XXXX", "....X", ".XXX."),
    "f": ("..XX.", ".X..X", ".X...", "XXX..", ".X...", ".X...", ".X..."),
    "o": (".....", ".....", ".XXX.", "X...X", "X...X", "X...X", ".XXX."),
    "r": (".....", ".....", "X.XX.", "XX..X", "X....", "X....", "X...."),
    "S": (".XXXX", "X....", "X....", ".XXX.", "....X", "....X", "XXXX."),
    "e": (".....", ".....", ".XXX.", "X...X", "XXXXX", "X....", ".XXX."),
    "m": (".....", ".....", "XX.X.", "X.X.X", "X.X.X", "X.X.X", "X.X.X"),
    "V": ("X...X", "X...X", "X...X", "X...X", "X...X", ".X.X.", "..X.."),
    "R": ("XXXX.", "X...X", "X...X", "XXXX.", "X.X..", "X..X.", "X...X"),
    " ": (".....",) * 7,
}


def lens_coverage(cx, x, y):
    """Fraction of the pixel inside a rounded rectangle centred at (cx, LENS_Y), by 4x4 supersampling."""
    inside = 0
    for sy in range(4):
        for sx in range(4):
            px, py = x + (sx + 0.5) / 4, y + (sy + 0.5) / 4
            dx = max(abs(px - cx) - LENS_HALF[0], 0)
            dy = max(abs(py - LENS_Y) - LENS_HALF[1], 0)
            if dx * dx + dy * dy <= LENS_RADIUS ** 2:
                inside += 1
    return inside / 16


def build():
    cov = [[0] * W for _ in range(H)]
    for cx in LENS_CENTRES:
        for y in range(LENS_Y - LENS_HALF[1] - LENS_RADIUS - 1, LENS_Y + LENS_HALF[1] + LENS_RADIUS + 2):
            for x in range(cx - LENS_HALF[0] - LENS_RADIUS - 1, cx + LENS_HALF[0] + LENS_RADIUS + 2):
                cov[y][x] = round(255 * lens_coverage(cx, x, y))
    left = (W - len(TEXT) * ADVANCE * SCALE + SCALE) // 2
    for i, ch in enumerate(TEXT):
        for row, bits in enumerate(FONT[ch]):
            for col, bit in enumerate(bits):
                if bit == "X":
                    for dy in range(SCALE):
                        for dx in range(SCALE):
                            cov[TEXT_TOP + row * SCALE + dy][left + (i * ADVANCE + col) * SCALE + dx] = 255
    return cov


def write_png(path, cov):
    rows = []
    for line in cov:
        px = bytearray([0])
        for c in line:
            v = PREVIEW_BACKGROUND + (PREVIEW_MARK - PREVIEW_BACKGROUND) * c // 255
            px += bytes((v, v, v))
        rows.append(bytes(px))

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0)) \
        + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    cov = build()
    with open(os.path.join(here, "splash.raw"), "wb") as f:
        f.write(bytes(v for line in cov for v in line))
    with open(os.path.join(here, "splash.size"), "w") as f:
        f.write(f"{W} {H}\n")
    if "--png" in sys.argv:
        write_png(sys.argv[sys.argv.index("--png") + 1], cov)


main()

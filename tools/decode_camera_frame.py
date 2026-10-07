#!/usr/bin/env python3
"""Split one captured Eye frame (from TCP 52997) into its parts and save PNGs.

usage: decode_camera_frame.py CAPTURE.bin [FRAME_INDEX]

Layout is partly reverse engineered: 193,862-byte frames, payload at ~byte 318 as 189 rows of 1024 bytes.
Right half is a 512x189 image; left half has even/odd columns as two 256x189 images. Needs numpy and Pillow.
"""
import sys

import numpy as np
from PIL import Image

FRAME = 193862
OFFSET = 318
ROWS, WIDTH = 189, 1024

path = sys.argv[1]
n = int(sys.argv[2]) if len(sys.argv) > 2 else 1
data = open(path, "rb").read()
f = np.frombuffer(data[n * FRAME:(n + 1) * FRAME], dtype=np.uint8)
assert f[:4].tobytes() == bytes.fromhex("27480002"), "frame does not start with the expected magic"
img = f[OFFSET:OFFSET + ROWS * WIDTH].reshape(ROWS, WIDTH)
left, right = img[:, :512], img[:, 512:]
for name, arr in (("right", right), ("left_even", left[:, 0::2]), ("left_odd", left[:, 1::2])):
    Image.fromarray(arr).resize((arr.shape[1], arr.shape[0] * 2)).save(f"frame{n}_{name}.png")
print("wrote frame%d_{right,left_even,left_odd}.png" % n)

#!/usr/bin/env python3
"""Look for glitch frames in a presenter --dump capture (raw RGBA crops written by the presenter).

usage: analyze_dump.py DIR      (needs numpy)

Reports frame-to-frame change, exact duplicates, and "isolated" frames that differ strongly from both neighbours while the
neighbours resemble each other (a single frame rendered for the wrong view, or a stale texture)."""
import glob
import re
import sys

import numpy as np

files = sorted(glob.glob(sys.argv[1].rstrip("/") + "/frame_*_L_*.rgba")) or sorted(glob.glob(sys.argv[1].rstrip("/") + "/frame_*.rgba"))
if not files:
    sys.exit("no frames")
w, h = map(int, re.search(r"_(\d+)x(\d+)\.rgba", files[0]).groups())
Y = np.stack([np.fromfile(f, dtype=np.uint8).reshape(h, w, 4)[:, :, :3].astype(np.float32).mean(axis=2) for f in files])
d = np.abs(np.diff(Y, axis=0)).mean(axis=(1, 2))
print(f"{len(files)} frames of {w}x{h}; frame-to-frame change: median {np.median(d):.2f}, p90 {np.percentile(d, 90):.2f}, max {d.max():.2f}")
print(f"exact duplicate frames: {int((d < 1e-6).sum())}")
iso = []
for i in range(1, len(files) - 1):
    d1, d2, dn = d[i - 1], d[i], np.abs(Y[i - 1] - Y[i + 1]).mean()
    if min(d1, d2) > 2.0 * max(dn, 1.0) and min(d1, d2) > 6:
        iso.append((i, round(float(d1), 1), round(float(d2), 1), round(float(dn), 1)))
print(f"isolated glitch frames (index, diff to previous, diff to next, previous vs next): {iso if iso else 'none'}")

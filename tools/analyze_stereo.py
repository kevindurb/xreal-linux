#!/usr/bin/env python3
"""Check that the left and right eye images of each dumped frame belong together (needs numpy).

usage: analyze_stereo.py DIR

For every frame it finds the horizontal shift that best aligns the left crop with the right crop. For a fixed scene that
disparity changes only slowly; if one eye is a frame behind the other, it jumps in proportion to head speed."""
import glob
import re
import sys

import numpy as np

d = sys.argv[1].rstrip("/")
lf = sorted(glob.glob(d + "/frame_*_L_*.rgba"))
w, h = map(int, re.search(r"_(\d+)x(\d+)\.rgba", lf[0]).groups())
def load(f): return np.fromfile(f, dtype=np.uint8).reshape(h, w, 4)[:, :, :3].astype(np.float32).mean(axis=2)
shifts, resid = [], []
for f in lf:
    L = load(f); R = load(f.replace("_L_", "_R_"))
    best = None
    for s in range(-80, 81, 2):
        a = L[:, max(0, s):w + min(0, s)]; b = R[:, max(0, -s):w + min(0, -s)]
        e = np.abs(a - b).mean()
        if best is None or e < best[0]: best = (e, s)
    resid.append(best[0]); shifts.append(best[1])
shifts = np.array(shifts); resid = np.array(resid)
step = np.abs(np.diff(shifts))
print(f"{len(lf)} frames; best L/R horizontal shift: median {np.median(shifts):.0f}px, min {shifts.min()}, max {shifts.max()}")
print(f"frame-to-frame change in that shift: median {np.median(step):.1f}px, p95 {np.percentile(step, 95):.1f}px, max {step.max()}px")
print(f"residual mismatch after aligning: median {np.median(resid):.2f}, p95 {np.percentile(resid, 95):.2f}, max {resid.max():.2f}")
odd = [(i + 1, int(step[i])) for i in range(len(step)) if step[i] > 12]
print("frames where the disparity jumps by more than 12px:", odd[:30] if odd else "none")

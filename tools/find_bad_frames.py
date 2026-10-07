#!/usr/bin/env python3
"""List bad frames (isolated per eye, vs neighbours) in a two-eye presenter --dump capture and the pointer timeline. Needs numpy.
usage: find_bad_frames.py DIR"""
import csv, glob, sys
import numpy as np
d = sys.argv[1].rstrip("/")
rows = list(csv.DictReader(open(d + "/meta.csv"))); n = len(rows)
def Y(i, e):
    f = glob.glob(f"{d}/frame_{i:04d}_{e}_*.rgba")[0]; w, h = map(int, f.rsplit("_", 1)[1][:-5].split("x"))
    return np.fromfile(f, dtype=np.uint8).reshape(h, w, 4)[:, :, :3].astype(np.float32).mean(axis=2)
bad = []; ptr = []
for e in "LR":
    P = [Y(i, e) for i in range(n)]
    for i in range(1, n - 1):
        d1 = np.abs(P[i] - P[i - 1]).mean(); d2 = np.abs(P[i] - P[i + 1]).mean(); dn = np.abs(P[i - 1] - P[i + 1]).mean()
        if min(d1, d2) > 2 * max(dn, 1) and min(d1, d2) > 6: bad.append((i, e))
for i in range(n):
    f = glob.glob(f"{d}/frame_{i:04d}_L_*.rgba")[0]; w, h = map(int, f.rsplit("_", 1)[1][:-5].split("x"))
    a = np.fromfile(f, dtype=np.uint8).reshape(h, w, 4)[:, :, :3].astype(int); r, g, b = a[..., 0], a[..., 1], a[..., 2]
    ptr.append(int(((b > 170) & (r < 110) & (g > 90) & (g < 200) & (b - r > 90)).sum()) >= 40)
print(f"{n} frames; bad frames (index, eye): {sorted(bad)}")
print("pointer (left eye): " + "".join("#" if p else "." for p in ptr))

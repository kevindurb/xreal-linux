#!/usr/bin/env python3
"""How smoothly does the displayed image move in a --dump capture of a steady simulated head turn (--sim-pose --sim-yaw)?
Phase-correlates the left eye between consecutive refreshes and reports the horizontal shift per refresh. Smooth motion has
about the same shift every refresh; a repeated SteamVR frame shows as a stall (shift near 0) followed by a double shift.
Needs numpy.   usage: judder_report.py DIR"""
import glob, sys
import numpy as np
d = sys.argv[1].rstrip("/")
files = sorted(glob.glob(f"{d}/frame_*_L_*.rgba"))
def load(f):
    w, h = map(int, f.rsplit("_", 1)[1][:-5].split("x"))
    a = np.fromfile(f, dtype=np.uint8).reshape(h, w, 4)[:, :, :3].astype(np.float32).mean(axis=2)
    a = a[h // 4: 3 * h // 4, w // 8: 7 * w // 8]                 # centre band: avoids the panel edges and head-locked UI
    a = a[: a.shape[0] // 2 * 2, : a.shape[1] // 2 * 2]
    a = a.reshape(a.shape[0] // 2, 2, a.shape[1] // 2, 2).mean(axis=(1, 3))   # half resolution
    return (a - a.mean()) * np.outer(np.hanning(a.shape[0]), np.hanning(a.shape[1]))
def shift(a, b):
    F = np.fft.fft2(a) * np.conj(np.fft.fft2(b)); F /= np.abs(F) + 1e-6
    r = np.fft.ifft2(F).real; iy, ix = np.unravel_index(np.argmax(r), r.shape)
    def sub(m, i, n):                                            # parabolic refinement of the peak position
        l, c, rr = m[(i - 1) % n], m[i], m[(i + 1) % n]
        den = l - 2 * c + rr
        return (i + (0.5 * (l - rr) / den if den else 0)) % n
    sx = sub(r[iy], ix, r.shape[1]); sx = sx - r.shape[1] if sx > r.shape[1] / 2 else sx
    return 2 * sx                                                # back to full-resolution pixels
prev = load(files[0]); dx = []
for f in files[1:]:
    cur = load(f); dx.append(abs(shift(cur, prev))); prev = cur
dx = np.array(dx); m = float(np.median(dx[dx > 0.5])) if (dx > 0.5).any() else 0.0
stall = float((dx < 0.35 * m).mean()) if m else 1.0; dbl = float((dx > 1.65 * m).mean()) if m else 0.0
print(f"{len(dx)} refreshes; shift per refresh: median {m:.2f} px, p10 {np.percentile(dx, 10):.2f}, p90 {np.percentile(dx, 90):.2f}; "
      f"stalls {100 * stall:.0f}%, doubles {100 * dbl:.0f}%, spread (std/median) {dx.std() / m if m else 0:.2f}")

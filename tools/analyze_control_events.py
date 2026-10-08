#!/usr/bin/env python3
"""Find what precedes the glasses dropping from full side-by-side to 2D, from a presenter log.

usage: analyze_control_events.py ~/.local/state/xreal-linux/presenter.log [--window 30]

Reads the lines the presenter prints (both stamped with seconds since it started):
    [control +12.3s] event 10045 (2 bytes) 1802
    [display +12.3s] modes: 3840x1080
A single 3840x1080 mode is full side-by-side; any other mode list (or no connector) is a drop. For every drop it lists the control events in the
window before it, then compares how often each event (id and first bytes) appears in those windows with its rate over the whole log.
"""
import argparse
import re
import sys
from collections import Counter

EVENT = re.compile(r"\[control \+([\d.]+)s\] event (\d+) \((\d+) bytes\) ([0-9a-f]*)")
MODES = re.compile(r"\[display \+([\d.]+)s\] modes: (.*)")
SBS = ["3840x1080"]

# docs/xreal-link-messages.md sections 10 and 13, docs/findings.md
LABELS = {
    10002: "camera/anchor session start or stop",
    10030: "binary struct (twice per mode switch)",
    10045: "wearing/proximity-state notification (18 01 / 18 02)",
    10086: "display state notification (18 01 / 18 02), goes through 2 around display changes",
    10087: "NRPowerSaveEnter (sleep)",
    10122: "temperature",
}


def parse(lines):
    events, modes = [], []
    for line in lines:
        m = EVENT.search(line)
        if m:
            events.append((float(m.group(1)), int(m.group(2)), m.group(4)))
            continue
        m = MODES.search(line)
        if m:
            modes.append((float(m.group(1)), m.group(2).split()))
    return events, modes


def drops(modes):
    """Times at which the mode list stopped being exactly full side-by-side."""
    out, was_sbs = [], None
    for t, ms in modes:
        sbs = sorted(set(ms)) == SBS
        if was_sbs and not sbs:
            out.append((t, ms))
        was_sbs = sbs
    return out


def key(event):
    _, ident, head = event
    return (ident, head[:8] if ident != 10122 else "")


def label(ident):
    return LABELS.get(ident, "not identified")


def report(events, modes, window):
    drop_list = drops(modes)
    lines = []
    span = max([t for t, *_ in events] + [t for t, _ in modes] + [0.0])
    sbs_since = next((t for t, ms in modes if sorted(set(ms)) == SBS), None)
    lines.append(f"log covers {span:.0f} s, {len(events)} control events, {len(modes)} mode lists, {len(drop_list)} drop(s) to 2D")
    if sbs_since is None:
        lines.append("the glasses were never seen in full side-by-side (3840x1080 only)")
    before = Counter()
    for n, (t, ms) in enumerate(drop_list, 1):
        lines.append(f"\ndrop {n} at +{t:.1f}s, modes now: {' '.join(ms) or 'none'}")
        near = [e for e in events if t - window <= e[0] <= t + 2]
        if not near:
            lines.append(f"  no control events in the {window:.0f} s before it")
        for e in near:
            lines.append(f"  {e[0] - t:+7.1f}s  event {e[1]} {e[2][:16]}  ({label(e[1])})")
        for e in near:
            if e[0] <= t:
                before[key(e)] += 1
    if drop_list:
        total = Counter(key(e) for e in events)
        minutes = max(span / 60.0, 1e-9)
        lines.append(f"\nevents in the {window:.0f} s before a drop, against their rate over the whole log:")
        lines.append("  id     first bytes  before drops  per drop  per minute overall  meaning")
        for (ident, head), n in sorted(before.items(), key=lambda kv: -kv[1]):
            lines.append(f"  {ident:<6} {head or '-':<12} {n:>12}  {n / len(drop_list):>8.1f}  {total[(ident, head)] / minutes:>18.2f}  {label(ident)}")
        lines.append("an event that is common before drops but rare overall is the candidate cause; with one drop this is only a hint")
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--window", type=float, default=30.0)
    a = ap.parse_args()
    with open(a.log, errors="replace") as f:
        events, modes = parse(f)
    print(report(events, modes, a.window))
    return 0


if __name__ == "__main__":
    sys.exit(main())

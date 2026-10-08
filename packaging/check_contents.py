#!/usr/bin/env python3
"""Fail if a release tree contains captures or other personal data.

    check_contents.py DIR

A capture is anything under a `captures/` directory or named `*.rgba`; frame dumps, the camera/IMU capture formats (`*.bin`,
`*.chunks`, `*.raw`), and the glasses' cached config (`config-*.json`, which holds a serial number) do not belong in a release either.
"""
import os
import sys

BAD_DIRS = {"captures", "samples", "__pycache__", ".git"}
BAD_SUFFIXES = (".rgba", ".chunks", ".raw", ".pcap", ".bin")
BAD_PREFIXES = ("config-",)


def offenders(root):
    found = []
    for dirpath, dirnames, filenames in os.walk(root):
        for d in dirnames:
            if d in BAD_DIRS:
                found.append(os.path.join(dirpath, d))
        for f in filenames:
            if f.endswith(BAD_SUFFIXES) or (f.startswith(BAD_PREFIXES) and f.endswith(".json")):
                found.append(os.path.join(dirpath, f))
    return sorted(found)


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    bad = offenders(argv[1])
    for b in bad:
        print(f"FAIL {b}")
    if not bad:
        print(f"ok   {argv[1]}: no captures or personal data")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

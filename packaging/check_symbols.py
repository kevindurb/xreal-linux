#!/usr/bin/env python3
"""Fail when a binary needs a glibc (or libstdc++/libgcc) symbol version above the release baseline.

    check_symbols.py FILE...            run objdump -T on each file
    check_symbols.py --objdump-text F   read saved `objdump -T` output (used by the tests)

The baseline is the glibc of the Steam Runtime 3 (sniper) SDK the release is built in.
"""
import re
import subprocess
import sys

BASELINE_GLIBC = (2, 31)


def versions(objdump_text):
    return sorted({tuple(int(p) for p in m.group(1).split(".")) for m in re.finditer(r"GLIBC_([0-9]+(?:\.[0-9]+)+)", objdump_text)})


def too_new(objdump_text, baseline=BASELINE_GLIBC):
    return [v for v in versions(objdump_text) if v > baseline]


def main(argv):
    if len(argv) == 3 and argv[1] == "--objdump-text":
        texts = [(argv[2], open(argv[2]).read())]
    elif len(argv) >= 2:
        texts = [(f, subprocess.run(["objdump", "-T", f], capture_output=True, text=True, check=True).stdout) for f in argv[1:]]
    else:
        print(__doc__, file=sys.stderr)
        return 2
    bad = 0
    for name, text in texts:
        newer = too_new(text)
        top = ".".join(map(str, versions(text)[-1])) if versions(text) else "none"
        if newer:
            print(f"FAIL {name}: needs GLIBC_{'.'.join(map(str, newer[-1]))}, above the baseline GLIBC_{'.'.join(map(str, BASELINE_GLIBC))}")
            bad = 1
        else:
            print(f"ok   {name}: highest GLIBC_{top} (baseline GLIBC_{'.'.join(map(str, BASELINE_GLIBC))})")
    return bad


if __name__ == "__main__":
    sys.exit(main(sys.argv))

#!/usr/bin/env python3
"""Set values in SteamVR's steamvr.vrsettings. Stop SteamVR first (it rewrites the file on exit).
usage: vrsettings.py section.key=value ...     value is parsed as JSON (true, false, 1.5), else kept as a string
       vrsettings.py --unset section.key ..."""
import json, os, sys
p = os.path.expanduser("~/.local/share/Steam/config/steamvr.vrsettings")
j = json.load(open(p))
unset = sys.argv[1] == "--unset"
for a in sys.argv[2 if unset else 1:]:
    k, _, v = a.partition("=")
    sec, key = k.split(".", 1)
    if unset: j.get(sec, {}).pop(key, None)
    else:
        try: v = json.loads(v)
        except ValueError: pass
        j.setdefault(sec, {})[key] = v
json.dump(j, open(p, "w"), indent=3, sort_keys=True)

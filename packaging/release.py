#!/usr/bin/env python3
"""Release artifact helpers: a deterministic Decky plugin zip and a SHA256SUMS file.

    release.py bundle SRC_DIR OUT_ZIP     # zip the plugin's shippable files (plugin.json at the root)
    release.py sums OUT_SUMS FILE...

`SHA256SUMS` is `sha256sum`-style (`<hex>  <name>`), which the plugin parses to verify the AppImage it downloads.
"""
import hashlib
import os
import sys
import zipfile

# What a Decky plugin needs at runtime. `src/`, the tests, the build config and node_modules are development-only.
PLUGIN_FILES = ("plugin.json", "main.py", "package.json", "LICENSE", "README.md", "dist/index.js")
FIXED_TIME = (1980, 1, 1, 0, 0, 0)


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def sums_text(paths):
    return "".join(f"{sha256_file(p)}  {os.path.basename(p)}\n" for p in sorted(paths, key=os.path.basename))


def bundle_files(src):
    missing = [f for f in PLUGIN_FILES if not os.path.isfile(os.path.join(src, f))]
    if missing:
        raise FileNotFoundError(f"{src} is missing {', '.join(missing)} (run the plugin build first)")
    return list(PLUGIN_FILES)


def write_bundle(src, out_zip):
    """A zip whose entries are byte-for-byte reproducible (fixed timestamps, fixed order)."""
    parent = os.path.dirname(os.path.abspath(out_zip))
    os.makedirs(parent, exist_ok=True)
    with zipfile.ZipFile(out_zip, "w", zipfile.ZIP_DEFLATED) as z:
        for rel in bundle_files(src):
            with open(os.path.join(src, rel), "rb") as f:
                data = f.read()
            info = zipfile.ZipInfo(rel, date_time=FIXED_TIME)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            z.writestr(info, data)


def plugin_root_for(src):
    """The directory a Decky install expects: the zip's entries, or the source directory itself."""
    return src


def main(argv):
    if len(argv) == 4 and argv[1] == "bundle":
        write_bundle(argv[2], argv[3])
        print(f"wrote {argv[3]} ({', '.join(bundle_files(argv[2]))})")
        return 0
    if len(argv) >= 4 and argv[1] == "sums":
        text = sums_text(argv[3:])
        with open(argv[2], "w") as f:
            f.write(text)
        sys.stdout.write(text)
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))

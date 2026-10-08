#!/usr/bin/env bash
# Checks everything the XREAL VR setup depends on and says what to fix. Read-only. Run on the machine the glasses are plugged into.
#   doctor.sh
# A thin wrapper: the checks live in the presenter binary (`xreal-presenter check`, the same as `xreal-linux.AppImage check`).
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
for BIN in "${XREAL_PRESENTER:-}" "$HERE/dist/xreal-presenter" "$HERE/presenter/target/release/xreal-presenter" "$HOME/.local/share/xreal-linux/xreal-linux.AppImage"; do
  [ -n "$BIN" ] && [ -x "$BIN" ] && exec env XREAL_DRIVER_DIR="$HERE/driver/xreal" "$BIN" check
done
echo "no presenter binary found: build it (packaging/build.sh, or presenter/README.md) or install the AppImage" >&2
exit 1

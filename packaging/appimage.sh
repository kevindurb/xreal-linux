#!/usr/bin/env bash
# Assemble dist/ (from packaging/build.sh) into dist/xreal-linux-x86_64.AppImage.
#   packaging/appimage.sh
# The host's Vulkan, Wayland and xkbcommon libraries are used, not bundled. appimagetool is downloaded once, pinned by checksum, and run
# with --appimage-extract-and-run so building needs no FUSE.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -x dist/xreal-presenter ] || { echo "run packaging/build.sh first" >&2; exit 1; }
TOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-x86_64.AppImage"
TOOL_SHA256="46fdd785094c7f6e545b61afcfb0f3d98d8eab243f644b4b17698c01d06083d1"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/xreal-linux-build"
mkdir -p "$CACHE"
TOOL="$CACHE/appimagetool-1.9.0"
if ! echo "$TOOL_SHA256  $TOOL" | sha256sum -c --status 2>/dev/null; then
  curl -fsSL -o "$TOOL" "$TOOL_URL"
  echo "$TOOL_SHA256  $TOOL" | sha256sum -c --status || { echo "appimagetool checksum mismatch" >&2; rm -f "$TOOL"; exit 1; }
  chmod +x "$TOOL"
fi

APPDIR=build-out/AppDir
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/lib/xreal-linux"
cp -a dist/xreal-presenter dist/driver dist/VERSION "$APPDIR/usr/lib/xreal-linux/"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
# No arguments (a double click) runs the guided setup; the binary decides from $APPIMAGE.
exec "$APPDIR/usr/lib/xreal-linux/xreal-presenter" "$@"
RUN
chmod +x "$APPDIR/AppRun"
cat > "$APPDIR/xreal-linux.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=XREAL for SteamVR
Comment=Set up XREAL glasses as a SteamVR headset
Exec=xreal-linux
Icon=xreal-linux
Terminal=false
Categories=Utility;
DESKTOP
python3 packaging/make_icon.py "$APPDIR/xreal-linux.png"

python3 packaging/check_contents.py "$APPDIR"
OUT=dist/xreal-linux-x86_64.AppImage
ARCH=x86_64 "$TOOL" --appimage-extract-and-run --no-appstream "$APPDIR" "$OUT" 2>&1 | tail -3
chmod +x "$OUT"
ls -l "$OUT"

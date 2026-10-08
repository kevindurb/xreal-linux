#!/usr/bin/env bash
# Build the prototype SteamVR driver into driver/xreal/bin/linux64/driver_xreal.so
# The OpenVR driver header is fetched (not vendored) into driver/.openvr on first run.
set -euo pipefail
cd "$(dirname "$0")"
OPENVR_TAG="${OPENVR_TAG:-v2.15.6}"
if [ ! -f .openvr/headers/openvr_driver.h ]; then
  git clone --depth 1 --branch "$OPENVR_TAG" https://github.com/ValveSoftware/openvr .openvr
fi
mkdir -p xreal/bin/linux64
# Static libstdc++/libgcc and a plain-glibc dependency keep the library loadable inside SteamVR's runtime.
g++ -std=c++17 -O2 -Wall -Wextra -Wno-unused-parameter -fPIC -shared -fvisibility=hidden -fno-math-errno \
    -static-libstdc++ -static-libgcc -pthread \
    -I.openvr/headers -o xreal/bin/linux64/driver_xreal.so src/xreal_driver.cpp
echo "built xreal/bin/linux64/driver_xreal.so"
objdump -p xreal/bin/linux64/driver_xreal.so | grep NEEDED
objdump -T xreal/bin/linux64/driver_xreal.so | grep -o 'GLIBC_[0-9.]*\|GLIBCXX_[0-9.]*' | sort -V -u | tail -3

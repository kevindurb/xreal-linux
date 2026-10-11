#!/usr/bin/env bash
# Build the Decky plugin and package it as dist/xreal-control.zip (the shippable bundle the release attaches).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"

cd "$root/decky"
pnpm install --frozen-lockfile
pnpm run build

cd "$root"
mkdir -p dist
python3 packaging/release.py bundle decky dist/xreal-control.zip

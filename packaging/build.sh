#!/usr/bin/env bash
# Build the driver and the presenter in the pinned Steam Runtime 3 (sniper) SDK and collect the release files in dist/.
#   packaging/build.sh            (needs podman or docker and network access)
# dist/ gets: xreal-presenter, driver/xreal/ (manifest, settings, bin/linux64/driver_xreal.so), VERSION.
# The image is pinned by digest; change it deliberately and record why in docs/findings.md.
set -euo pipefail
cd "$(dirname "$0")/.."
IMAGE="${XREAL_BUILD_IMAGE:-registry.gitlab.steamos.cloud/steamrt/sniper/sdk@sha256:1c33c507bc75d012e77df5727f93b0d5b8c3f7c8d4142ba5f7a16882cc92e014}"
ENGINE="${CONTAINER_ENGINE:-$([ -x /usr/bin/podman ] && echo /usr/bin/podman || command -v podman || command -v docker)}"
[ -n "$ENGINE" ] || { echo "need podman or docker" >&2; exit 1; }
COMMIT="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
git diff --quiet HEAD 2>/dev/null || COMMIT="$COMMIT-dirty"
ROOT="$PWD"
rm -rf dist build-out
mkdir -p dist build-out

# The container user is root inside a rootless engine; files it writes are owned by the invoking user outside.
"$ENGINE" run --rm -v "$ROOT":/src:Z -v xreal-sniper-cargo:/root/.cargo:Z -v xreal-sniper-rustup:/root/.rustup:Z -w /src "$IMAGE" bash -euo pipefail -c '
  export PATH=/root/.cargo/bin:$PATH
  command -v cargo >/dev/null || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
  (cd driver && ./build.sh >/dev/null)
  (cd presenter && cargo build --release --locked --target-dir /src/build-out/target)
'

install -D -m 0755 build-out/target/release/xreal-presenter dist/xreal-presenter
install -D -m 0644 driver/xreal/driver.vrdrivermanifest dist/driver/xreal/driver.vrdrivermanifest
install -D -m 0644 driver/xreal/resources/settings/default.vrsettings dist/driver/xreal/resources/settings/default.vrsettings
install -D -m 0755 driver/xreal/bin/linux64/driver_xreal.so dist/driver/xreal/bin/linux64/driver_xreal.so
printf 'commit %s\n' "$COMMIT" > dist/VERSION

python3 packaging/check_symbols.py dist/driver/xreal/bin/linux64/driver_xreal.so dist/xreal-presenter
(cd dist && find . -type f | LC_ALL=C sort) | tee build-out/files.txt

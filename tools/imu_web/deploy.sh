#!/usr/bin/env bash
# Copy the viewer to the Deck. Usage: deploy.sh [ssh-host] [remote-dir]
# Then on the Deck:  python3 ~/xreal-linux/tools/imu_web/server.py
# and from your own machine:  ssh -L 8765:localhost:8765 steamdeck   ->  http://localhost:8765/
set -euo pipefail
HOST="${1:-steamdeck}"
DEST="${2:-xreal-linux}"                      # relative to the remote home directory
cd "$(dirname "$0")/../.."
ssh "$HOST" "mkdir -p '$DEST/tools/imu_web' '$DEST/captures'"
rsync -av --exclude '__pycache__' --exclude 'test_*' tools/imu_web/ "$HOST:$DEST/tools/imu_web/"
echo "copied to $HOST:~/$DEST/tools/imu_web (tests excluded)"

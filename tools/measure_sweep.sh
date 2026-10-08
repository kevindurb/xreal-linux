#!/usr/bin/env bash
# On the Deck: run measure_matrix.sh once per value of a driver setting. Results: /tmp/sweep-results.txt
#   measure_sweep.sh KEY "V1 V2 ..." [HOLD] [HOME] [REPS]      e.g. measure_sweep.sh driver_xreal.running_start_ms "2 4 8" true true 2
set -uo pipefail
KEY=${1:?}; VALUES=${2:?}; HOLD=${3:-true}; HOME_ON=${4:-true}; REPS=${5:-2}
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: >/tmp/sweep-results.txt
for v in $VALUES; do
  TAG="${KEY##*.}-$v-" HOLDS=$HOLD HOMES=$HOME_ON EXTRA="$KEY=$v" "$HERE/tools/measure_matrix.sh" "$REPS" >/dev/null 2>&1
  { echo "## $KEY=$v"; cat /tmp/matrix-results.txt; } >>/tmp/sweep-results.txt
done
echo DONE >>/tmp/sweep-results.txt

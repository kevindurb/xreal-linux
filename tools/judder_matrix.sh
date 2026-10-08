#!/usr/bin/env bash
# On the Deck: displayed-motion smoothness (tools/judder_report.py) for reprojection x hold x running start x Home, no dashboard.
#   judder_matrix.sh      results: /tmp/judder-results.txt
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: >/tmp/judder-results.txt
for reproject in 0 1; do
  for rs in 2 8; do
    for hold in true false; do
      { echo "## reproject=$reproject running_start=$rs hold=$hold"; } >>/tmp/judder-results.txt
      XREAL_REPROJECT=$reproject DASH=0 TAG="rp$reproject-rs$rs-" HOLDS=$hold HOMES="false true" EXTRA="driver_xreal.running_start_ms=$rs" \
        "$HERE/tools/measure_matrix.sh" 1 >/dev/null 2>&1
      grep -E "^###|refreshes; shift|bad frames" /tmp/matrix-results.txt | sed -E 's/\[\(.*//' >>/tmp/judder-results.txt
    done
  done
done
echo ALLDONE >>/tmp/judder-results.txt

#!/usr/bin/env bash
# On the Deck: measure bad frames for each combination of hold_after_present and Home, REPS captures each, then restore the settings.
#   measure_matrix.sh [REPS]      results: /tmp/matrix-results.txt (also each /tmp/measure-LABEL.txt)
set -uo pipefail
REPS=${1:-2}; TAG=${TAG:-}; HOMES=${HOMES:-false true}; HOLDS=${HOLDS:-true false}; EXTRA=${EXTRA:-}   # TAG prefixes the labels; HOMES/HOLDS pick the combinations
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RES=/tmp/matrix-results.txt; : >"$RES"
restart() {  # restart SteamVR and wait for the driver to activate
  "$HERE/tools/vr_session.sh" stop >/dev/null; sleep 4
  "$HERE/tools/vrsettings.py" "$@"
  "$HERE/tools/vr_session.sh" start >/dev/null
  for _ in $(seq 90); do
    n=$(grep -c "HMD activated" "$HOME/.local/share/Steam/logs/vrserver.txt" 2>/dev/null || echo 0)
    [ "$n" -gt "${ACT:-0}" ] && break; sleep 2
  done
  ACT=$(grep -c "HMD activated" "$HOME/.local/share/Steam/logs/vrserver.txt"); sleep 25
}
ACT=$(grep -c "HMD activated" "$HOME/.local/share/Steam/logs/vrserver.txt" 2>/dev/null || echo 0)
for home in $HOMES; do
  for hold in $HOLDS; do
    restart steamvr.enableHomeApp=$home driver_xreal.hold_after_present=$hold $EXTRA
    for r in $(seq "$REPS"); do
      label="${TAG}home-$home-hold-$hold-r$r"
      "$HERE/tools/measure_dashboard.sh" "$label" "${FRAMES:-480}" >/dev/null 2>&1
      { echo "### $label"; grep -E "hold after|gpu busy|new SteamVR|bad frames|refreshes; shift|steamvr-compositor-sync" "/tmp/measure-$label.txt" | tail -3; } >>"$RES"
    done
  done
done
restart steamvr.enableHomeApp=false driver_xreal.hold_after_present=true
echo DONE >>"$RES"

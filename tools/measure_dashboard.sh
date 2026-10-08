#!/usr/bin/env bash
# On the Deck, with SteamVR running: open the dashboard, restart the presenter with a simulated head sweep and a 480-frame dump,
# then count bad frames (tools/find_bad_frames.py, run in a container because the Deck has no numpy) and print the judder report.
#   measure_dashboard.sh LABEL [FRAMES]       results go to /tmp/measure-LABEL.txt; the dump is deleted afterwards
set -uo pipefail
LABEL=${1:?usage: measure_dashboard.sh LABEL [FRAMES]}; FRAMES=${2:-480}
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VRCMD="$HOME/.local/share/Steam/steamapps/common/SteamVR/bin/linux64/vrcmd"
DUMP=/tmp/dump-$LABEL; OUT=/tmp/measure-$LABEL.txt
export XDG_RUNTIME_DIR=/run/user/$(id -u)
pgrep -x vrserver >/dev/null || { echo "SteamVR is not running" >&2; exit 1; }
rm -rf "$DUMP"; mkdir -p "$DUMP"; : >"$OUT"
{
  echo "== $LABEL $(date -Is)"
  grep -h "HMD activated" "$HOME/.local/share/Steam/logs/vrserver.txt" | tail -1
  grep -h "steamvr-compositor-sync" "$HOME/.local/share/Steam/logs/vrcompositor-linux.txt" 2>/dev/null | tail -1
} >>"$OUT"
if [ "${DASH:-1}" = 0 ]; then "$VRCMD" --hidedashboard >/dev/null 2>&1; else "$VRCMD" --showdashboard >/dev/null 2>&1; fi; sleep 4   # DASH=0: no dashboard, for judder
XREAL_EXTRA_ARGS="--sim-pose --sim-yaw 40 --sim-pitch -30 --sim-pitch-amp 0 --dump $DUMP --dump-frames $FRAMES" "$HERE/tools/vr_session.sh" presenter >/dev/null
GPU=$(ls /sys/class/drm/card*/device/gpu_busy_percent 2>/dev/null | head -1)
( while :; do cat "$GPU" 2>/dev/null; sleep 0.2; done ) >"$DUMP.gpu" 2>/dev/null & SAMPLER=$!
sleep 6; touch "$DUMP/trigger"   # the presenter captures the next FRAMES frames
for _ in $(seq 120); do [ "$(ls "$DUMP"/*_R_*.rgba 2>/dev/null | wc -l)" -ge "$FRAMES" ] && break; sleep 1; done
sleep 3
kill "$SAMPLER" 2>/dev/null
echo "gpu busy during capture: $(sort -n "$DUMP.gpu" | awk '{a[NR]=$1; s+=$1} END{printf "avg %.0f%%, p90 %d%%, max %d%%", s/NR, a[int(NR*0.9)], a[NR]}')" >>"$OUT"; rm -f "$DUMP.gpu"
"$VRCMD" --stats 2>&1 | head -15 >>"$OUT"
grep "new SteamVR frames" /tmp/presenter.log | tail -6 >>"$OUT"
/usr/bin/podman run --rm -v "$DUMP":"$DUMP":ro,z -v "$HERE/tools":/tools:ro,z registry.fedoraproject.org/fedora:44 \
  bash -c "dnf -y -q install python3-numpy >/dev/null 2>&1; python3 /tools/find_bad_frames.py $DUMP; python3 /tools/judder_report.py $DUMP" >>"$OUT" 2>&1 && cp "$DUMP/meta.csv" "/tmp/meta-$LABEL.csv" && rm -rf "$DUMP"
grep -h "steamvr-compositor-sync" "$HOME/.local/share/Steam/logs/vrcompositor-linux.txt" 2>/dev/null | tail -2 >>"$OUT"
cat "$OUT"

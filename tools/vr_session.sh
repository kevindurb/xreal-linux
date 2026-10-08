#!/usr/bin/env bash
# Start/stop the XREAL VR session on the Deck: the presenter (owns the glasses' display and the IMU) and SteamVR.
#   vr_session.sh start | stop | presenter | status | log
#   reprojection is on unless XREAL_REPROJECT=0; XREAL_EXTRA_ARGS passes more presenter flags (--sim-pose, --dump DIR)
# The glasses must be in follow mode with the Stabilizer off; `start` first sets full SBS (a single 3840x1080 mode) with the presenter's one-shot --set-sbs-only if they are in the regular mode, and confirms it before starting anything else.
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${XREAL_PRESENTER:-$HERE/presenter/target/release/xreal-presenter}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/xreal-linux"
mkdir -p "$STATE"
LOG="$STATE/presenter.log"
SBS_LOG="$STATE/set_sbs.log"

# The installed socket unit owns the driver link's address; a manual presenter cannot bind it, so stop the units first and bring them back on `stop`.
units_stop()    { systemctl --user is-active --quiet xreal-linux.socket 2>/dev/null && { systemctl --user stop xreal-linux.service xreal-linux.socket 2>/dev/null; echo "stopped xreal-linux.socket (a manual session needs its address; 'vr_session.sh stop' starts it again)"; }; }
units_resume()  { systemctl --user is-enabled --quiet xreal-linux.socket 2>/dev/null && systemctl --user start xreal-linux.socket 2>/dev/null; }

stop() {
  for p in vrmonitor vrdashboard vrserver vrcompositor vrwebhelper; do pkill -TERM -x "$p" 2>/dev/null; done
  systemctl --user stop xreal-presenter 2>/dev/null
  systemctl --user reset-failed xreal-presenter 2>/dev/null
}

full_sbs() { [ "$(sort -u /sys/class/drm/card1-DP-1/modes 2>/dev/null | tr '\n' ' ')" = "3840x1080 " ]; }

launch_presenter() {
  ARGS="--monitor DP-1"
  [ "${XREAL_REPROJECT:-1}" = 1 ] && ARGS="$ARGS --reproject"   # rotational reprojection: hides SteamVR's missed frames (docs/findings.md)
  ARGS="$ARGS ${XREAL_EXTRA_ARGS:-}"
  systemd-run --user --unit=xreal-presenter --description="XREAL presenter" -p RuntimeMaxSec=14400 \
    --setenv=XDG_RUNTIME_DIR --setenv=WAYLAND_DISPLAY --setenv=RUST_BACKTRACE=1 \
    -p StandardOutput=file:$LOG -p StandardError=file:$LOG "$BIN" $ARGS >/dev/null
}

case "${1:-status}" in
  start)   units_stop
    # sets full SBS first (one-shot, only if the glasses are in 2D) and confirms it; the presenter and SteamVR start only once the glasses are in it
    stop; sleep 3; rm -f "$LOG"
    "$BIN" --set-sbs-only >"$SBS_LOG" 2>&1
    for _ in $(seq 30); do full_sbs && break; sleep 0.5; done
    if ! full_sbs; then
      modes=$(sort -u /sys/class/drm/card1-DP-1/modes 2>/dev/null | tr '\n' ' ')
      echo "the glasses did not reach full SBS within 15 s (modes: ${modes:-none}); the presenter and SteamVR were not started. Log:" >&2
      tail -8 "$SBS_LOG" >&2
      exit 1
    fi
    launch_presenter
    sleep 3
    steam steam://run/250820 >/tmp/steamvr_launch.log 2>&1 &
    echo "started; 'vr_session.sh status' in ~30 s" ;;
  presenter)   units_stop   # restart only the presenter (SteamVR keeps running and the driver reconnects)
    systemctl --user stop xreal-presenter 2>/dev/null; systemctl --user reset-failed xreal-presenter 2>/dev/null; rm -f "$LOG"
    launch_presenter
    echo "presenter restarted with: $ARGS" ;;
  stop) stop; "$BIN" --restore-display 2>&1 | tail -3; units_resume; echo stopped ;;   # puts the glasses back in the 2D mode they were in before `start`, if it switched them
  log) tail -n 40 "$LOG" ;;
  status)
    echo "presenter: $(systemctl --user is-active xreal-presenter)"
    echo "steamvr:   $(ps -eo comm | grep -c '^vrserver') vrserver process(es)"
    grep -v '^swapchain\|fps$' "$LOG" 2>/dev/null | tail -8; tail -1 "$LOG" 2>/dev/null ;;
  *) echo "usage: $0 start|stop|presenter|status|log" >&2; exit 2 ;;
esac

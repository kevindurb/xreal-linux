#!/usr/bin/env bash
# Start/stop the XREAL VR session on the Deck: the presenter (owns the glasses' display and the IMU) and SteamVR.
#   vr_session.sh start | stop | presenter | status | log
#   reprojection is on unless XREAL_REPROJECT=0; XREAL_EXTRA_ARGS passes more presenter flags (--sim-pose, --dump DIR)
# The glasses must be in follow mode with the Stabilizer off; `start` has the presenter set full SBS (a single 3840x1080 mode) when they are in the regular mode.
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
BIN="$HOME/xreal-linux/presenter/target/release/xreal-presenter"
LOG=/tmp/presenter.log

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
  start)   # the presenter sets full SBS itself (unless XREAL_EXTRA_ARGS has --no-set-sbs); SteamVR starts only once the glasses are in it
    stop; sleep 3; rm -f "$LOG"
    launch_presenter
    for _ in $(seq 30); do full_sbs && break; sleep 0.5; done
    if ! full_sbs; then
      modes=$(sort -u /sys/class/drm/card1-DP-1/modes 2>/dev/null | tr '\n' ' ')
      echo "the glasses did not reach full SBS within 15 s (modes: ${modes:-none}); SteamVR was not started. Presenter log:" >&2
      grep -E 'control|input mode|NRDpSetInputMode|setter' "$LOG" 2>/dev/null | tail -8 >&2
      systemctl --user stop xreal-presenter 2>/dev/null; systemctl --user reset-failed xreal-presenter 2>/dev/null
      exit 1
    fi
    sleep 3
    steam steam://run/250820 >/tmp/steamvr_launch.log 2>&1 &
    echo "started; 'vr_session.sh status' in ~30 s" ;;
  presenter)   # restart only the presenter (SteamVR keeps running and the driver reconnects)
    systemctl --user stop xreal-presenter 2>/dev/null; systemctl --user reset-failed xreal-presenter 2>/dev/null; rm -f "$LOG"
    launch_presenter
    echo "presenter restarted with: $ARGS" ;;
  stop) stop; echo stopped ;;
  log) tail -n 40 "$LOG" ;;
  status)
    echo "presenter: $(systemctl --user is-active xreal-presenter)"
    echo "steamvr:   $(ps -eo comm | grep -c '^vrserver') vrserver process(es)"
    grep -v '^swapchain\|fps$' "$LOG" 2>/dev/null | tail -8; tail -1 "$LOG" 2>/dev/null ;;
  *) echo "usage: $0 start|stop|presenter|status|log" >&2; exit 2 ;;
esac

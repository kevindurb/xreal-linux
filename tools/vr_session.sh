#!/usr/bin/env bash
# Start/stop the XREAL VR session on the Deck: the presenter (owns the glasses' display and the IMU) and SteamVR.
#   vr_session.sh start | stop | status | log
# The glasses must already be in full SBS (a single 3840x1080 mode), follow mode, Stabilizer off.
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

case "${1:-status}" in
  start)
    modes=$(sort /sys/class/drm/card1-DP-1/modes 2>/dev/null | uniq | tr '\n' ' ')
    if [ "$modes" != "3840x1080 " ]; then
      echo "glasses are not in full SBS (modes: ${modes:-none}); switch them first" >&2; exit 1
    fi
    stop; sleep 3; rm -f "$LOG"
    systemd-run --user --unit=xreal-presenter --description="XREAL presenter" -p RuntimeMaxSec=14400 \
      --setenv=XDG_RUNTIME_DIR --setenv=WAYLAND_DISPLAY --setenv=RUST_BACKTRACE=1 \
      -p StandardOutput=file:$LOG -p StandardError=file:$LOG "$BIN" --monitor DP-1 >/dev/null
    sleep 3
    steam steam://run/250820 >/tmp/steamvr_launch.log 2>&1 &
    echo "started; 'vr_session.sh status' in ~30 s" ;;
  stop) stop; echo stopped ;;
  log) tail -n 40 "$LOG" ;;
  status)
    echo "presenter: $(systemctl --user is-active xreal-presenter)"
    echo "steamvr:   $(ps -eo comm | grep -c '^vrserver') vrserver process(es)"
    grep -v '^swapchain\|fps$' "$LOG" 2>/dev/null | tail -8; tail -1 "$LOG" 2>/dev/null ;;
  *) echo "usage: $0 start|stop|status|log" >&2; exit 2 ;;
esac

#!/usr/bin/env bash
# Checks everything the XREAL VR setup depends on and says what to fix. Read-only. Run on the machine the glasses are plugged into.
#   doctor.sh
OK=0; WARN=0; BAD=0
ok()   { echo "  [ok]   $*"; OK=$((OK+1)); }
warn() { echo "  [warn] $*"; WARN=$((WARN+1)); }
bad()  { echo "  [FAIL] $*"; BAD=$((BAD+1)); }
STEAMVR="$HOME/.local/share/Steam/steamapps/common/SteamVR"
CFG="$HOME/.local/share/Steam/config/steamvr.vrsettings"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Glasses"
if lsusb 2>/dev/null | grep -qi "3318:"; then ok "$(lsusb | grep -i '3318:' | head -1 | sed 's/.*ID //')"; else bad "no XREAL USB device (cable, or the glasses are asleep: wake them)"; fi
GW=""
for ip in 169.254.1.1 169.254.2.1; do
  if timeout 2 bash -c "exec 3<>/dev/tcp/$ip/52998" 2>/dev/null; then GW=$ip; break; fi
done
if [ -n "$GW" ]; then
  n=$(timeout 2 bash -c "exec 3<>/dev/tcp/$GW/52998; head -c 20000 <&3" 2>/dev/null | wc -c)
  [ "${n:-0}" -ge 10000 ] && ok "IMU stream on $GW:52998 ($n bytes in under 2 s)" || bad "connected to $GW:52998 but no IMU data"
else bad "cannot reach the glasses' IMU port (169.254.x.1:52998); check the USB-C connection and that NetworkManager brought up the USB network interfaces"; fi

echo "Display"
CONN=$(ls -d /sys/class/drm/card*-DP-* 2>/dev/null | while read c; do [ "$(cat $c/status)" = connected ] && grep -q . $c/modes && { echo $c; break; }; done)
if [ -z "$CONN" ]; then bad "no connected DisplayPort output (glasses not showing video)"; else
  modes=$(sort "$CONN/modes" | uniq | tr '\n' ' ')
  case "$modes" in
    "3840x1080 ") ok "$(basename $CONN) is in full SBS (3840x1080): what we need" ;;
    "1920x1080 ") warn "$(basename $CONN) looks like half SBS (1920x1080 only); VR needs full SBS" ;;
    *) bad "$(basename $CONN) is in a normal 2D mode ($modes); switch the glasses to full SBS in their menu" ;;
  esac
fi
if command -v kscreen-doctor >/dev/null; then
  export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}
  export DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}
  out=$(QT_QPA_PLATFORM=wayland timeout 10 kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g')
  n=$(echo "$out" | grep -c '^Output')
  if [ "$n" -ge 2 ] && echo "$out" | awk '/^Output/{o=$3} /enabled/{e[o]=1} END{c=0; for(k in e)c++; exit !(c>=2)}'; then
    ok "more than one display is enabled, so desktop windows have somewhere to go other than the glasses"
  else warn "the glasses may be the only enabled display; Steam and SteamVR windows will appear in one eye (enable another display and make it primary)"; fi
fi

echo "SteamVR"
if [ -f "$STEAMVR/bin/version.txt" ] || [ -d "$STEAMVR" ]; then ok "SteamVR installed at $STEAMVR"; else bad "SteamVR not installed (Steam app 250820)"; fi
if command -v getcap >/dev/null && getcap "$STEAMVR/bin/linux64/vrcompositor-launcher" 2>/dev/null | grep -q cap_sys_nice; then ok "vrcompositor-launcher has cap_sys_nice"; else warn "vrcompositor-launcher lacks cap_sys_nice (SteamVR asks for it on first launch)"; fi
if grep -q "xreal" "$HOME/.config/openvr/openvrpaths.vrpath" 2>/dev/null; then ok "XREAL driver registered with SteamVR"; else bad "driver not registered: vrpathreg.sh adddriver $HERE/driver/xreal"; fi
[ -f "$HERE/driver/xreal/bin/linux64/driver_xreal.so" ] && ok "driver library built" || bad "driver not built (driver/build.sh)"
if [ -f "$CFG" ]; then
  python3 - "$CFG" <<'PY' || true
import json, sys
j = json.load(open(sys.argv[1])); st = j.get("steamvr", {}); pw = j.get("power", {})
def line(level, msg): print(f"  [{level}]".ljust(9) + msg)
line("ok" if st.get("forcedDriver") == "xreal" else "warn", f"steamvr.forcedDriver = {st.get('forcedDriver')!r} (want 'xreal')")
t = pw.get("turnOffScreensTimeout", 5.0)
line("ok" if t >= 600 else "warn", f"power.turnOffScreensTimeout = {t} s (the 5 s default puts the headset into standby and stutters)")
line("ok" if pw.get("pauseCompositorOnStandby", True) is False else "warn", f"power.pauseCompositorOnStandby = {pw.get('pauseCompositorOnStandby', True)}")
d = j.get("driver_xreal", {})
print("  [info] " + f"per-eye render size: {d.get('render_width', 1920)}x{d.get('render_height', 1080)}; head height {d.get('head_height', 1.5)} m")
# SteamVR's dashboard shows single bad frames unless it is paced: hold on and a running start of 8 ms or more (docs/findings.md).
line("ok" if st.get("enableHomeApp", True) is False else "warn", f"steamvr.enableHomeApp = {st.get('enableHomeApp', True)} (recommended false: Home takes about 10 ms of GPU per frame; steamvr.background = {st.get('background')!r})")
if d.get("hold_after_present", True) is False: line("warn", "driver_xreal.hold_after_present is false: dashboard frames may glitch (set it true)")
if d.get("running_start_ms", 8) < 8: line("warn", f"driver_xreal.running_start_ms = {d['running_start_ms']}: dashboard frames may glitch (8 or more)")
PY
else warn "no steamvr.vrsettings yet (SteamVR has not been run)"; fi

echo "Presenter"
[ -x "$HERE/presenter/target/release/xreal-presenter" ] && ok "presenter built" || bad "presenter not built (see presenter/README.md)"
systemctl --user is-active --quiet xreal-presenter 2>/dev/null && ok "presenter service running" || warn "presenter service not running (tools/vr_session.sh start)"
pgrep -x vrserver >/dev/null && ok "SteamVR running" || warn "SteamVR not running"

echo
echo "Glasses menu settings this tool cannot read (check them by hand): Follow mode, Stabilizer OFF, auto sleep OFF."
echo "Result: $OK ok, $WARN warnings, $BAD failures"
[ "$BAD" -eq 0 ]

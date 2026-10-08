//! `check`: report, without changing anything, whether the machine is ready. The observations are gathered into `Facts` and judged by
//! `evaluate`, so the judgement is tested on synthetic states.

use super::env::{Env, Version};
use super::{install, settings, units};
use crate::glasses::Previous;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub section: &'static str,
    pub level: Level,
    pub text: String,
}

impl Line {
    pub fn render(&self) -> String {
        let tag = match self.level {
            Level::Ok => "  [ok]   ",
            Level::Warn => "  [warn] ",
            Level::Fail => "  [FAIL] ",
            Level::Info => "  [info] ",
        };
        format!("{tag}{}", self.text)
    }
}

#[derive(Clone, Debug)]
pub enum Imu {
    Flowing { host: String, bytes: usize },
    NoData { host: String },
    Unreachable,
}

#[derive(Clone, Debug, Default)]
pub struct UnitFacts {
    pub socket_enabled: String,
    pub socket_active: String,
    pub service_active: String,
    /// Whether the user manager's environment has WAYLAND_DISPLAY; None if it could not be asked.
    pub wayland_in_manager: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub usb: Option<String>,
    pub imu: Option<Imu>,
    /// The glasses' connector and its sorted, de-duplicated mode list.
    pub output: Option<(String, Vec<String>)>,
    /// Whether more than one display is enabled; None when it could not be determined.
    pub more_than_one_display: Option<bool>,
    pub steamvr_dir: Option<String>,
    pub cap_sys_nice: Option<bool>,
    pub registered: Vec<String>,
    pub installed_driver_path: String,
    pub bundled_driver_path: String,
    pub driver_installed: bool,
    pub driver_bundled: bool,
    pub settings: Option<Value>,
    pub units: Option<UnitFacts>,
    pub installed_version: Option<Version>,
    pub bundle_version: Option<Version>,
    pub pending: Option<Previous>,
    pub presenter_running: bool,
    pub steamvr_running: bool,
}

fn line(section: &'static str, level: Level, text: impl Into<String>) -> Line {
    Line { section, level, text: text.into() }
}

pub fn evaluate(f: &Facts) -> Vec<Line> {
    use Level::*;
    let mut out = vec![];
    let g = "Glasses";
    match &f.usb {
        Some(d) => out.push(line(g, Ok, d.clone())),
        None => out.push(line(g, Fail, "no XREAL USB device (cable, or the glasses are asleep: wake them)")),
    }
    match &f.imu {
        Some(Imu::Flowing { host, bytes }) => out.push(line(g, Ok, format!("IMU stream on {host}:52998 ({bytes} bytes in under 2 s)"))),
        Some(Imu::NoData { host }) => out.push(line(g, Fail, format!("connected to {host}:52998 but no IMU data"))),
        _ => out.push(line(g, Fail, "cannot reach the glasses' IMU port (169.254.x.1:52998); check the USB-C connection and that NetworkManager brought up the USB network interfaces")),
    }
    let d = "Display";
    match &f.output {
        None => out.push(line(d, Fail, "no connected DisplayPort output (glasses not showing video)")),
        Some((name, modes)) => {
            let joined = modes.join(" ");
            match joined.as_str() {
                "3840x1080" => out.push(line(d, Ok, format!("{name} is in full SBS (3840x1080); the presenter switches the glasses to it when SteamVR starts"))),
                "1920x1080" => out.push(line(d, Warn, format!("{name} looks like half SBS (1920x1080 only); the presenter switches the glasses to full SBS when SteamVR starts"))),
                _ => out.push(line(d, Ok, format!("{name} is in a normal 2D mode ({joined}); the presenter will switch the glasses to full SBS when SteamVR starts and back afterwards"))),
            }
        }
    }
    match f.more_than_one_display {
        Some(true) => out.push(line(d, Ok, "more than one display is enabled, so desktop windows have somewhere to go other than the glasses")),
        Some(false) => out.push(line(d, Warn, "the glasses may be the only enabled display; Steam and SteamVR windows will appear in one eye (enable another display and make it primary)")),
        None => {}
    }
    let s = "SteamVR";
    match &f.steamvr_dir {
        Some(p) => out.push(line(s, Ok, format!("SteamVR installed at {p}"))),
        None => out.push(line(s, Fail, "SteamVR not installed (Steam app 250820)")),
    }
    match f.cap_sys_nice {
        Some(true) => out.push(line(s, Ok, "vrcompositor-launcher has cap_sys_nice")),
        Some(false) => out.push(line(s, Warn, "vrcompositor-launcher lacks cap_sys_nice (SteamVR asks for it on first launch)")),
        None => {}
    }
    let ours = f.registered.iter().any(|p| super::install::same_path(p, std::path::Path::new(&f.installed_driver_path)) || super::install::same_path(p, std::path::Path::new(&f.bundled_driver_path)));
    if ours {
        out.push(line(s, Ok, "XREAL driver registered with SteamVR"));
    } else if f.registered.is_empty() {
        out.push(line(s, Fail, "driver not registered: run setup (or fix) to register it with SteamVR"));
    } else {
        out.push(line(s, Fail, format!("an XREAL driver is registered from {} but it is not this package's ({}): run setup to replace it", f.registered.join(", "), f.installed_driver_path)));
    }
    if f.driver_installed || f.driver_bundled {
        out.push(line(s, Ok, "driver library present"));
    } else {
        out.push(line(s, Fail, "driver library not found (run setup)"));
    }
    match &f.settings {
        None => out.push(line(s, Warn, "no steamvr.vrsettings yet (SteamVR has not been run)")),
        Some(j) => out.extend(judge_settings(j)),
    }
    let p = "Package";
    match (&f.installed_version, &f.bundle_version) {
        (Some(i), Some(b)) if b.newer_than(i) => out.push(line(p, Warn, format!("installed version {} is older than this one ({}): run setup to update", i.commit, b.commit))),
        (Some(i), _) => out.push(line(p, Ok, format!("installed version {}", i.commit))),
        (None, _) => out.push(line(p, Warn, "this package is not installed (run setup)")),
    }
    match &f.units {
        None => out.push(line(p, Warn, "systemd user units: could not be queried")),
        Some(u) => {
            if u.socket_enabled == "enabled" && (u.socket_active == "active" || u.socket_active == "listening") {
                out.push(line(p, Ok, "xreal-linux.socket is enabled and listening; the presenter starts when SteamVR does"));
            } else if u.socket_enabled == "enabled" || u.socket_active == "active" {
                out.push(line(p, Warn, format!("xreal-linux.socket is {} / {}; run setup to repair it", u.socket_enabled, u.socket_active)));
            } else {
                out.push(line(p, Warn, "xreal-linux.socket is not installed (run setup, or use tools/vr_session.sh from a checkout)"));
            }
            match u.wayland_in_manager {
                Some(true) => out.push(line(p, Ok, "WAYLAND_DISPLAY is in the user manager's environment")),
                Some(false) => out.push(line(p, Fail, "WAYLAND_DISPLAY is not in the user manager's environment, so the service cannot open a window: run `systemctl --user import-environment WAYLAND_DISPLAY` or log in again")),
                None => {}
            }
        }
    }
    match f.pending {
        Some(Previous::TwoD) if !f.presenter_running => out.push(line(p, Warn, "the glasses were switched to full SBS and not restored to their 2D mode: run `xreal-linux restore-display`")),
        Some(Previous::TwoD) => {}
        Some(Previous::Sbs) | None => {}
    }
    out.push(line(p, if f.presenter_running { Ok } else { Info }, if f.presenter_running { "presenter running" } else { "presenter not running (it starts with SteamVR)" }));
    out.push(line(p, Info, if f.steamvr_running { "SteamVR running" } else { "SteamVR not running" }));
    out
}

/// The SteamVR settings the setup relies on and the pacing settings that let bad dashboard frames through.
pub fn judge_settings(j: &Value) -> Vec<Line> {
    use Level::*;
    let s = "SteamVR";
    let (st, pw, d) = (&j["steamvr"], &j["power"], &j["driver_xreal"]);
    let mut out = vec![];
    let forced = st.get("forcedDriver").and_then(Value::as_str);
    out.push(line(s, if forced == Some("xreal") { Ok } else { Warn }, format!("steamvr.forcedDriver = {} (want 'xreal')", forced.map(|f| format!("'{f}'")).unwrap_or_else(|| "None".into()))));
    let t = pw.get("turnOffScreensTimeout").and_then(Value::as_f64).unwrap_or(5.0);
    out.push(line(s, if t >= 600.0 { Ok } else { Warn }, format!("power.turnOffScreensTimeout = {t} s (the 5 s default puts the headset into standby and stutters)")));
    let pause = pw.get("pauseCompositorOnStandby").and_then(Value::as_bool).unwrap_or(true);
    out.push(line(s, if !pause { Ok } else { Warn }, format!("power.pauseCompositorOnStandby = {}", if pause { "True" } else { "False" })));
    let home = st.get("enableHomeApp").and_then(Value::as_bool).unwrap_or(true);
    out.push(line(s, if !home { Ok } else { Warn }, format!("steamvr.enableHomeApp = {} (recommended false: Home takes about 10 ms of GPU per frame)", if home { "True" } else { "False" })));
    if d.get("hold_after_present").and_then(Value::as_bool) == Some(false) {
        out.push(line(s, Warn, "driver_xreal.hold_after_present is false: dashboard frames may glitch (set it true)"));
    }
    if let Some(r) = d.get("running_start_ms").and_then(Value::as_f64).filter(|r| *r < 8.0) {
        out.push(line(s, Warn, format!("driver_xreal.running_start_ms = {r}: dashboard frames may glitch (8 or more)")));
    }
    out
}

/// What only the wearer can confirm: the host can neither read nor set these.
pub const BY_HAND: [&str; 3] = [
    "Follow mode (not Anchor): in the glasses' menu (double-click the X button), under Display.",
    "Stabilizer OFF: double-click the X button, then Display, then Stabilizer.",
    "Auto sleep OFF: in the glasses' settings menu (double-click the X button); the entry name varies with the firmware.",
];

pub fn render(lines: &[Line]) -> String {
    let mut text = String::new();
    let mut section = "";
    for l in lines {
        if l.section != section {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(l.section);
            text.push('\n');
            section = l.section;
        }
        text.push_str(&l.render());
        text.push('\n');
    }
    text.push_str("\nConfirm these in the glasses' own menu; this tool cannot read or set them:\n");
    for item in BY_HAND {
        text.push_str(&format!("  - {item}\n"));
    }
    let count = |lv| lines.iter().filter(|l| l.level == lv).count();
    text.push_str(&format!("\nResult: {} ok, {} warnings, {} failures\n", count(Level::Ok), count(Level::Warn), count(Level::Fail)));
    text
}

pub fn failed(lines: &[Line]) -> bool {
    lines.iter().any(|l| l.level == Level::Fail)
}

// ---- gathering --------------------------------------------------------------------------------------------------------------

fn usb_glasses() -> Option<String> {
    for e in std::fs::read_dir("/sys/bus/usb/devices").ok()?.flatten() {
        let read = |n: &str| std::fs::read_to_string(e.path().join(n)).ok().map(|s| s.trim().to_string());
        if read("idVendor").as_deref() == Some("3318") {
            return Some(format!("3318:{} {}", read("idProduct").unwrap_or_default(), read("product").unwrap_or_else(|| "XREAL".into())));
        }
    }
    None
}

fn imu() -> Imu {
    use std::io::Read;
    for host in ["169.254.1.1", "169.254.2.1"] {
        let Ok(mut s) = std::net::TcpStream::connect_timeout(&format!("{host}:52998").parse().unwrap(), std::time::Duration::from_secs(2)) else { continue };
        let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let (mut buf, deadline) = (vec![0u8; 20000], std::time::Instant::now() + std::time::Duration::from_secs(2));
        let mut got = 0;
        while got < buf.len() && std::time::Instant::now() < deadline {
            match s.read(&mut buf[got..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => got += n,
            }
        }
        return if got >= 10000 { Imu::Flowing { host: host.into(), bytes: got } } else { Imu::NoData { host: host.into() } };
    }
    Imu::Unreachable
}

fn more_than_one_display() -> Option<bool> {
    let out = std::process::Command::new("kscreen-doctor").arg("-o").env("QT_QPA_PLATFORM", "wayland").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).replace('\u{1b}', "");
    let (mut enabled, mut current) = (0, false);
    for l in text.lines() {
        if l.contains("Output:") {
            current = true;
        } else if current && l.trim() == "enabled" {
            enabled += 1;
            current = false;
        }
    }
    Some(enabled >= 2)
}

fn cap_sys_nice(env: &Env) -> Option<bool> {
    let out = std::process::Command::new("getcap").arg(env.steamvr_dir().join("bin/linux64/vrcompositor-launcher")).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).contains("cap_sys_nice"))
}

/// `DP-1: 1920x1080 1920x1200`, for `status`.
pub fn gather_output_summary() -> Option<String> {
    let c = crate::output::find_glasses_connector(std::path::Path::new("/sys/class/drm"))?;
    let modes = crate::glasses::read_modes(&c)?;
    Some(format!("{c}: {}", modes.join(" ")))
}

pub fn gather(env: &Env) -> Facts {
    let output = crate::output::find_glasses_connector(std::path::Path::new("/sys/class/drm")).and_then(|c| crate::glasses::read_modes(&c).map(|m| (c, m)));
    let u = |verb, unit| units::state(verb, unit);
    let manager_env = units::systemctl_user(&["show-environment"]).ok();
    Facts {
        usb: usb_glasses(),
        imu: Some(imu()),
        output,
        more_than_one_display: more_than_one_display(),
        steamvr_dir: env.steamvr_dir().is_dir().then(|| env.steamvr_dir().display().to_string()),
        cap_sys_nice: cap_sys_nice(env),
        registered: install::xreal_registrations(env),
        installed_driver_path: env.installed_driver().display().to_string(),
        bundled_driver_path: env.bundled_driver().display().to_string(),
        driver_installed: env.installed_driver().join("bin/linux64/driver_xreal.so").is_file(),
        driver_bundled: env.bundled_driver().join("bin/linux64/driver_xreal.so").is_file(),
        settings: std::fs::read_to_string(env.vrsettings()).ok().and_then(|t| serde_json::from_str(&t).ok()),
        units: Some(UnitFacts {
            socket_enabled: u("is-enabled", units::SOCKET),
            socket_active: u("is-active", units::SOCKET),
            service_active: u("is-active", units::SERVICE),
            wayland_in_manager: manager_env.map(|t| t.lines().any(|l| l.starts_with("WAYLAND_DISPLAY="))),
        }),
        installed_version: env.installed_version(),
        bundle_version: env.bundled_version(),
        pending: crate::glasses::read_record(&env.state_dir()),
        presenter_running: process_running("xreal-presenter"),
        steamvr_running: settings::steamvr_running(),
    }
}

/// Another process with this name (this process, which is also called xreal-presenter, does not count).
fn process_running(name: &str) -> bool {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy() != me.as_str() && std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Facts {
        Facts {
            usb: Some("3318:0424 XREAL One".into()),
            imu: Some(Imu::Flowing { host: "169.254.1.1".into(), bytes: 20000 }),
            output: Some(("DP-1".into(), vec!["1920x1080".into(), "1920x1200".into()])),
            more_than_one_display: Some(true),
            steamvr_dir: Some("/s/SteamVR".into()),
            cap_sys_nice: Some(true),
            registered: vec!["/d/xreal-linux/driver/xreal".into()],
            installed_driver_path: "/d/xreal-linux/driver/xreal".into(),
            bundled_driver_path: "/tmp/mnt/driver/xreal".into(),
            driver_installed: true,
            driver_bundled: true,
            settings: Some(json!({"steamvr": {"forcedDriver": "xreal", "enableHomeApp": false}, "power": {"turnOffScreensTimeout": 86400, "pauseCompositorOnStandby": false}})),
            units: Some(UnitFacts { socket_enabled: "enabled".into(), socket_active: "active".into(), service_active: "inactive".into(), wayland_in_manager: Some(true) }),
            installed_version: Version::parse("commit a\nbuilt 1\n"),
            bundle_version: Version::parse("commit a\nbuilt 1\n"),
            pending: None,
            presenter_running: false,
            steamvr_running: false,
        }
    }

    fn levels(f: &Facts) -> Vec<Level> {
        evaluate(f).iter().map(|l| l.level).collect()
    }

    #[test]
    fn everything_in_place_has_no_failure_and_a_2d_mode_is_not_one() {
        let lines = evaluate(&good());
        assert!(!failed(&lines), "{lines:#?}");
        assert!(!lines.iter().any(|l| l.level == Level::Warn), "{lines:#?}");
        let mode = lines.iter().find(|l| l.section == "Display" && l.text.starts_with("DP-1")).unwrap();
        assert_eq!(mode.level, Level::Ok);
        assert!(mode.text.contains("will switch the glasses to full SBS"));
    }

    #[test]
    fn missing_glasses_driver_or_steamvr_fail() {
        let mut f = good();
        f.usb = None;
        f.imu = Some(Imu::Unreachable);
        f.output = None;
        f.steamvr_dir = None;
        f.registered.clear();
        f.driver_installed = false;
        f.driver_bundled = false;
        let lines = evaluate(&f);
        assert!(failed(&lines));
        assert!(lines.iter().filter(|l| l.level == Level::Fail).count() >= 6, "{lines:#?}");
        assert!(lines.iter().any(|l| l.text.contains("run setup (or fix) to register")));
    }

    #[test]
    fn a_registration_from_somewhere_else_is_reported_with_its_path() {
        let mut f = good();
        f.registered = vec!["/home/u/xreal-linux/driver/xreal".into()];
        let lines = evaluate(&f);
        assert!(lines.iter().any(|l| l.level == Level::Fail && l.text.contains("/home/u/xreal-linux/driver/xreal")), "{lines:#?}");
    }

    #[test]
    fn a_pending_restore_warns_unless_the_presenter_is_running() {
        let mut f = good();
        f.pending = Some(Previous::TwoD);
        assert!(evaluate(&f).iter().any(|l| l.level == Level::Warn && l.text.contains("not restored")));
        f.presenter_running = true;
        assert!(!evaluate(&f).iter().any(|l| l.text.contains("not restored")));
        f.presenter_running = false;
        f.pending = Some(Previous::Sbs);
        assert!(!evaluate(&f).iter().any(|l| l.text.contains("not restored")));
    }

    #[test]
    fn units_and_the_session_environment_are_judged() {
        let mut f = good();
        f.units.as_mut().unwrap().wayland_in_manager = Some(false);
        assert!(levels(&f).contains(&Level::Fail));
        f.units = Some(UnitFacts::default());
        assert!(evaluate(&f).iter().any(|l| l.text.contains("not installed")));
    }

    #[test]
    fn settings_that_are_wrong_or_let_bad_frames_through_warn() {
        let lines = judge_settings(&json!({"driver_xreal": {"hold_after_present": false, "running_start_ms": 4}}));
        assert!(lines.iter().filter(|l| l.level == Level::Warn).count() >= 5, "{lines:#?}");
        assert!(lines.iter().any(|l| l.text.contains("hold_after_present") && l.level == Level::Warn));
        assert!(lines.iter().any(|l| l.text.contains("running_start_ms = 4")));
    }

    #[test]
    fn the_report_ends_with_the_three_hand_settings_and_not_the_display_mode() {
        let text = render(&evaluate(&good()));
        let tail = text.split("Confirm these").nth(1).unwrap();
        assert!(tail.contains("Follow mode") && tail.contains("Stabilizer OFF") && tail.contains("Auto sleep OFF"));
        assert!(!tail.contains("SBS") && !tail.contains("3840"));
        assert!(text.contains("Result:"));
    }
}

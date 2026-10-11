//! The systemd user units: the socket that owns the driver link's address, the service that runs the presenter when the driver
//! connects, and the opt-in head-look service. Written only with consent, marked as ours, removed by uninstall.

use super::env::Env;
use std::path::PathBuf;
use std::process::Command;

pub const SOCKET: &str = "xreal-linux.socket";
pub const SERVICE: &str = "xreal-linux.service";
pub const HEADLOOK: &str = "xreal-headlook.service";
const MARKER: &str = "# Managed by xreal-linux (setup writes it, uninstall removes it).";

/// The units `setup`'s main install writes and enables. The head-look unit is written separately, only when opted in.
pub const INSTALLED_UNITS: [&str; 2] = [SOCKET, SERVICE];
/// The units a provisioning install (the game-mode panel) writes, leaving all of them disabled for the panel to switch.
pub const PROVISIONED_UNITS: [&str; 3] = [SOCKET, SERVICE, HEADLOOK];
/// The units `uninstall` stops and removes.
pub const REMOVED_UNITS: [&str; 3] = [SOCKET, SERVICE, HEADLOOK];

pub fn socket_unit() -> String {
    format!(
        "{MARKER}\n[Unit]\nDescription=XREAL link for the SteamVR driver (starts the presenter when SteamVR connects)\n\n[Socket]\n# An abstract socket, so SteamVR's container can reach it.\nListenSequentialPacket=@xreal-presenter-%U\nAccept=no\n\n[Install]\nWantedBy=sockets.target\n"
    )
}

/// systemd splits ExecStart on spaces; quote the path when it has any.
fn quoted(path: &str) -> String {
    if path.contains(' ') { format!("\"{path}\"") } else { path.to_string() }
}

pub fn service_unit(app: &str, extract_and_run: bool) -> String {
    let app = quoted(app);
    let env = if extract_and_run { "Environment=APPIMAGE_EXTRACT_AND_RUN=1\n" } else { "" };
    format!(
        "{MARKER}\n[Unit]\nDescription=XREAL presenter for SteamVR\nRequires={SOCKET}\nAfter={SOCKET}\nStartLimitIntervalSec=60\nStartLimitBurst=5\n\n[Service]\nType=exec\n{env}EnvironmentFile=-%E/xreal-linux/service.env\nExecStart={app} serve\n# Puts the glasses back in the 2D mode recorded before the session, also after a crash or kill.\nExecStopPost={app} restore-display\nRestart=no\n"
    )
}

/// The opt-in head-look service: runs the presenter's head-look input as the user, with no terminal and no SteamVR. It reconnects
/// to the IMU on its own, so `Restart=on-failure` is enough (a dropped link must not restart the process). Disabled until asked for.
pub fn headlook_unit(app: &str, extract_and_run: bool) -> String {
    let app = quoted(app);
    let env = if extract_and_run { "Environment=APPIMAGE_EXTRACT_AND_RUN=1\n" } else { "" };
    format!(
        "{MARKER}\n[Unit]\nDescription=XREAL head-look input (drives a virtual mouse from head motion, without SteamVR)\nStartLimitIntervalSec=60\nStartLimitBurst=5\n\n[Service]\nType=exec\n{env}EnvironmentFile=-%E/xreal-linux/headlook.env\nExecStart={app} headlook --service\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n"
    )
}

pub fn unit_path(env: &Env, name: &str) -> PathBuf {
    env.unit_dir().join(name)
}

pub fn is_ours(text: &str) -> bool {
    text.lines().next() == Some(MARKER)
}

pub fn systemctl_user(args: &[&str]) -> Result<String, String> {
    let out = Command::new("systemctl").arg("--user").args(args).output().map_err(|e| format!("cannot run systemctl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() { Ok(text) } else { Err(format!("{text}{}", String::from_utf8_lossy(&out.stderr).trim())) }
}

/// `systemctl --user <verb> <unit>` as a state word (`active`, `inactive`, `enabled`, `not-found`...), never an error.
pub fn state(verb: &str, unit: &str) -> String {
    match Command::new("systemctl").args(["--user", verb, unit]).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string(),
        Err(_) => "unknown".into(),
    }
}

/// Write both units and enable the socket. Returns what was done, or the problems.
pub fn install(env: &Env, extract_and_run: bool, dry_run: bool) -> Result<Vec<String>, Vec<String>> {
    let app = env.installed_appimage();
    let files = [(SOCKET, socket_unit()), (SERVICE, service_unit(&app.to_string_lossy(), extract_and_run))];
    let (mut done, mut problems) = (vec![], vec![]);
    for (name, text) in &files {
        let path = unit_path(env, name);
        if let Ok(old) = std::fs::read_to_string(&path) {
            if !is_ours(&old) {
                problems.push(format!("{} exists and is not ours; left alone", path.display()));
                continue;
            }
        }
        if dry_run {
            done.push(format!("would write {}", path.display()));
            continue;
        }
        match std::fs::create_dir_all(env.unit_dir()).and_then(|_| std::fs::write(&path, text)) {
            Ok(()) => done.push(format!("wrote {}", path.display())),
            Err(e) => problems.push(format!("cannot write {}: {e}", path.display())),
        }
    }
    if problems.is_empty() && !dry_run {
        for args in [&["daemon-reload"][..], &["enable", "--now", SOCKET]] {
            match systemctl_user(args) {
                Ok(_) => done.push(format!("systemctl --user {}", args.join(" "))),
                Err(e) => problems.push(format!("systemctl --user {} failed: {e}", args.join(" "))),
            }
        }
    } else if dry_run {
        done.push(format!("would run systemctl --user enable --now {SOCKET}"));
    }
    if problems.is_empty() { Ok(done) } else { Err(problems) }
}

/// Write and enable the opt-in head-look unit (`--service`). Called only when the user opts in during interactive setup, so a declined
/// setup writes nothing of it. The game-mode panel does not use this: it provisions the unit disabled and enables it with its switch.
/// The unit is left alone if a foreign one is in the way.
pub fn install_headlook(env: &Env, extract_and_run: bool, dry_run: bool) -> Result<Vec<String>, Vec<String>> {
    let path = unit_path(env, HEADLOOK);
    if let Ok(old) = std::fs::read_to_string(&path) {
        if !is_ours(&old) {
            return Err(vec![format!("{} exists and is not ours; left alone", path.display())]);
        }
    }
    let app = env.installed_appimage();
    let text = headlook_unit(&app.to_string_lossy(), extract_and_run);
    let mut done = vec![];
    if dry_run {
        done.push(format!("would write {}", path.display()));
        done.push(format!("would run systemctl --user enable --now {HEADLOOK}"));
        return Ok(done);
    }
    std::fs::create_dir_all(env.unit_dir())
        .and_then(|_| std::fs::write(&path, text))
        .map_err(|e| vec![format!("cannot write {}: {e}", path.display())])?;
    done.push(format!("wrote {}", path.display()));
    for args in [&["daemon-reload"][..], &["enable", "--now", HEADLOOK]] {
        match systemctl_user(args) {
            Ok(_) => done.push(format!("systemctl --user {}", args.join(" "))),
            Err(e) => return Err(vec![format!("systemctl --user {} failed: {e}", args.join(" "))]),
        }
    }
    Ok(done)
}

/// Write the package's units but enable none: what the game-mode panel installs. Every unit is placed so the panel's mode switch can
/// turn one on, but nothing runs until the wearer picks a mode there. A foreign unit is left alone.
pub fn provision(env: &Env, extract_and_run: bool, dry_run: bool) -> Result<Vec<String>, Vec<String>> {
    let app = env.installed_appimage();
    let app = app.to_string_lossy();
    let files = [
        (SOCKET, socket_unit()),
        (SERVICE, service_unit(&app, extract_and_run)),
        (HEADLOOK, headlook_unit(&app, extract_and_run)),
    ];
    let (mut done, mut problems) = (vec![], vec![]);
    for (name, text) in &files {
        let path = unit_path(env, name);
        if let Ok(old) = std::fs::read_to_string(&path) {
            if !is_ours(&old) {
                problems.push(format!("{} exists and is not ours; left alone", path.display()));
                continue;
            }
        }
        if dry_run {
            done.push(format!("would write {}", path.display()));
            continue;
        }
        match std::fs::create_dir_all(env.unit_dir()).and_then(|_| std::fs::write(&path, text)) {
            Ok(()) => done.push(format!("wrote {}", path.display())),
            Err(e) => problems.push(format!("cannot write {}: {e}", path.display())),
        }
    }
    // Deliberately no `enable`: the panel enables the mode the wearer picks.
    if problems.is_empty() && !dry_run {
        match systemctl_user(&["daemon-reload"]) {
            Ok(_) => done.push("systemctl --user daemon-reload".into()),
            Err(e) => problems.push(format!("systemctl --user daemon-reload failed: {e}")),
        }
    } else if dry_run {
        done.push("would run systemctl --user daemon-reload (nothing enabled)".into());
    }
    if problems.is_empty() { Ok(done) } else { Err(problems) }
}

/// Stop and remove the units we wrote; a unit that is not ours is left alone.
pub fn remove(env: &Env, dry_run: bool) -> Vec<String> {
    let mut done = vec![];
    for name in REMOVED_UNITS {
        let path = unit_path(env, name);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if !is_ours(&text) {
            done.push(format!("{} is not ours; left alone", path.display()));
            continue;
        }
        if dry_run {
            done.push(format!("would stop and remove {}", path.display()));
            continue;
        }
        let _ = systemctl_user(&["disable", "--now", name]);
        let _ = systemctl_user(&["stop", name]);
        match std::fs::remove_file(&path) {
            Ok(()) => done.push(format!("removed {}", path.display())),
            Err(e) => done.push(format!("cannot remove {}: {e}", path.display())),
        }
    }
    if !dry_run {
        let _ = systemctl_user(&["daemon-reload"]);
        for unit in [SERVICE, HEADLOOK] {
            let _ = systemctl_user(&["reset-failed", unit]);
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_unit_owns_the_per_user_abstract_address() {
        let u = socket_unit();
        assert!(u.contains("ListenSequentialPacket=@xreal-presenter-%U") && u.contains("Accept=no") && is_ours(&u));
    }

    #[test]
    fn the_service_restores_the_display_mode_on_every_stop_and_is_rate_limited() {
        let u = service_unit("/home/u/.local/share/xreal-linux/xreal-linux.AppImage", false);
        assert!(u.contains("ExecStart=/home/u/.local/share/xreal-linux/xreal-linux.AppImage serve"));
        assert!(u.contains("ExecStopPost=/home/u/.local/share/xreal-linux/xreal-linux.AppImage restore-display"));
        assert!(u.contains("StartLimitBurst=5") && u.contains("Restart=no") && u.contains("Requires=xreal-linux.socket"));
        assert!(!u.contains("APPIMAGE_EXTRACT_AND_RUN"));
        assert!(service_unit("/a b/app", true).contains("ExecStart=\"/a b/app\" serve"));
        assert!(service_unit("/a/app", true).contains("Environment=APPIMAGE_EXTRACT_AND_RUN=1"));
    }

    #[test]
    fn only_our_marked_units_count_as_ours() {
        assert!(!is_ours("[Unit]\nDescription=someone else\n"));
        assert!(is_ours(&service_unit("/x", false)));
    }

    #[test]
    fn the_headlook_unit_is_opt_in_and_reads_its_own_environment_file() {
        let u = headlook_unit("/home/u/.local/share/xreal-linux/xreal-linux.AppImage", false);
        assert!(is_ours(&u), "carries the managed-by marker");
        assert!(u.contains("ExecStart=/home/u/.local/share/xreal-linux/xreal-linux.AppImage headlook --service"), "{u}");
        assert!(u.contains("EnvironmentFile=-%E/xreal-linux/headlook.env"), "{u}");
        assert!(u.contains("Restart=on-failure"), "{u}");
        assert!(u.contains("WantedBy=default.target"), "{u}");
        assert!(!u.contains("xreal-linux.socket"), "head look must not depend on the SteamVR socket");
        assert!(headlook_unit("/a b/app", true).contains("ExecStart=\"/a b/app\" headlook --service"));
        assert!(headlook_unit("/a/app", true).contains("Environment=APPIMAGE_EXTRACT_AND_RUN=1"));
    }

    #[test]
    fn the_main_install_writes_two_units_but_uninstall_removes_the_headlook_one_too() {
        assert_eq!(INSTALLED_UNITS, [SOCKET, SERVICE], "the head-look unit is written only when opted in");
        assert_eq!(REMOVED_UNITS, [SOCKET, SERVICE, HEADLOOK], "uninstall removes all three");
        assert!(!INSTALLED_UNITS.contains(&HEADLOOK));
        assert!(REMOVED_UNITS.contains(&HEADLOOK));
    }

    #[test]
    fn provisioning_places_every_unit_but_enables_none() {
        assert_eq!(PROVISIONED_UNITS, [SOCKET, SERVICE, HEADLOOK], "the panel install places all three so it can switch between them");
        assert_eq!(PROVISIONED_UNITS, REMOVED_UNITS, "uninstall removes exactly what provisioning placed");
        assert!(PROVISIONED_UNITS.contains(&HEADLOOK), "head mouse is placed disabled so the panel's switch can enable it");
        assert!(!INSTALLED_UNITS.contains(&HEADLOOK), "the interactive install does not touch it unless opted in");
    }
}

//! The two systemd user units: the socket that owns the driver link's address, and the service that runs the presenter when the
//! driver connects. Written only with consent, marked as ours, removed by uninstall.

use super::env::Env;
use std::path::PathBuf;
use std::process::Command;

pub const SOCKET: &str = "xreal-linux.socket";
pub const SERVICE: &str = "xreal-linux.service";
const MARKER: &str = "# Managed by xreal-linux (setup writes it, uninstall removes it).";

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

/// Stop and remove the units we wrote; a unit that is not ours is left alone.
pub fn remove(env: &Env, dry_run: bool) -> Vec<String> {
    let mut done = vec![];
    for name in [SOCKET, SERVICE] {
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
        let _ = systemctl_user(&["reset-failed", SERVICE]);
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
}

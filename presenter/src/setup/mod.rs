//! The user-facing commands of the package: `setup`, `check`, `fix`, `status`, `uninstall`, plus `serve` (what the service unit runs)
//! and `restore-display` (its ExecStopPost). All for the current user; nothing needs root.

pub mod check;
pub mod dialog;
pub mod env;
pub mod install;
pub mod settings;
pub mod units;

use dialog::Dialog;
use env::Env;

/// What `main` should do after looking at the first argument.
pub enum Dispatch {
    /// A command ran; exit with this status.
    Exit(i32),
    /// Run the presenter with these arguments.
    Present(Vec<String>),
    /// Not a subcommand: the arguments are the presenter's own flags.
    NotACommand,
}

pub const GLASSES_NOTICE: &str = "When SteamVR starts using the glasses they switch to full side-by-side (SBS), and back to the mode they were in when SteamVR stops. \
Each switch re-plugs the glasses' display for about two seconds, so the desktop may rearrange windows. Nothing is done to the glasses while SteamVR is not running, so they stay usable as a normal monitor.";

const USAGE: &str = "usage: xreal-linux [setup | check | fix | status | uninstall | restore-display] [--dry-run] [--yes]
  setup            install for this user (asks first), register the driver, enable the background service, fix SteamVR settings
  check            report whether everything is in place (changes nothing)
  fix              set the SteamVR settings the setup needs (asks for each; SteamVR must not be running)
  status           what is installed and what the service did last
  uninstall        undo setup
  restore-display  put the glasses back in the 2D mode recorded before a session";

pub fn dispatch(args: &[String]) -> Dispatch {
    let in_appimage = std::env::var_os("APPIMAGE").is_some();
    let cmd = match args.first().map(String::as_str) {
        Some(c) if !c.starts_with('-') => c,
        None if in_appimage => "setup",
        Some("--help") | Some("-h") if in_appimage => "help",
        _ => return Dispatch::NotACommand,
    };
    let rest: Vec<&str> = args.iter().skip(if args.is_empty() { 0 } else { 1 }).map(String::as_str).collect();
    let (dry_run, yes) = (rest.contains(&"--dry-run"), rest.contains(&"--yes") || rest.contains(&"-y"));
    match cmd {
        "serve" => return Dispatch::Present(serve_args(std::env::var("XREAL_REPROJECT").ok().as_deref(), std::env::var("XREAL_EXTRA_ARGS").ok().as_deref())),
        "restore-display" => {
            let r = crate::glasses::restore_previous_mode();
            let pending = matches!(r, crate::glasses::Restore::Pending(_));
            crate::report_restore(r);
            return Dispatch::Exit(pending as i32);
        }
        "help" => {
            println!("{USAGE}");
            return Dispatch::Exit(0);
        }
        "setup" | "check" | "fix" | "status" | "uninstall" => {}
        other if args.first().is_some_and(|a| a == other) => {
            eprintln!("unknown command '{other}'\n{USAGE}");
            return Dispatch::Exit(2);
        }
        _ => return Dispatch::NotACommand,
    }
    let env = match Env::from_process() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return Dispatch::Exit(1);
        }
    };
    if env.uid == 0 {
        eprintln!("Do not run this as root: it installs for one user. Run it as the user who runs SteamVR.");
        return Dispatch::Exit(1);
    }
    let dialog = Dialog::new(yes);
    let code = match cmd {
        "setup" => setup(&env, &dialog, dry_run),
        "check" => check(&env, &dialog),
        "fix" => fix(&env, &dialog, dry_run),
        "status" => status(&env),
        "uninstall" => uninstall(&env, &dialog, dry_run),
        _ => unreachable!(),
    };
    Dispatch::Exit(code)
}

/// The presenter arguments of the service: reprojection on unless `XREAL_REPROJECT=0`, then any `XREAL_EXTRA_ARGS`.
pub fn serve_args(reproject: Option<&str>, extra: Option<&str>) -> Vec<String> {
    let mut a = vec!["--service".to_string()];
    if reproject != Some("0") {
        a.push("--reproject".into());
    }
    a.extend(extra.unwrap_or("").split_whitespace().map(str::to_string));
    a
}

fn report(dialog: &Dialog, title: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let text = format!("{title}\n{}", lines.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"));
    println!("{text}");
    if dialog.is_graphical() && !dialog.assume_yes {
        dialog.info(&text);
    }
}

fn check(env: &Env, dialog: &Dialog) -> i32 {
    let lines = check::evaluate(&check::gather(env));
    let text = check::render(&lines);
    print!("{text}");
    if dialog.is_graphical() && !dialog.assume_yes {
        dialog.info(&text);
    }
    check::failed(&lines) as i32
}

fn fix(env: &Env, dialog: &Dialog, dry_run: bool) -> i32 {
    match settings::fix(env, dialog, dry_run) {
        settings::FixOutcome::NothingToDo => {
            println!("the SteamVR settings are already right");
            0
        }
        settings::FixOutcome::Applied(lines) => {
            report(dialog, if dry_run { "dry run: nothing changed" } else { "SteamVR settings changed" }, &lines);
            0
        }
        settings::FixOutcome::Declined => {
            println!("nothing was changed");
            0
        }
        settings::FixOutcome::Refused(why) => {
            eprintln!("{why}");
            if dialog.is_graphical() && !dialog.assume_yes {
                dialog.info(&why);
            }
            1
        }
    }
}

fn setup(env: &Env, dialog: &Dialog, dry_run: bool) -> i32 {
    let source = env.appimage.clone().or_else(|| std::env::current_exe().ok()).unwrap_or_default();
    if std::env::var_os("APPDIR").is_some_and(|d| !d.to_string_lossy().contains(".mount_")) {
        println!("note: running without FUSE (extracted to {}); the service will use the same way of starting", std::env::var("APPDIR").unwrap_or_default());
    }
    let mut intro = vec![
        format!("Install XREAL for SteamVR for your user only (no root):"),
        format!("  - copy the app and the SteamVR driver to {}", env.install_dir().display()),
        "  - register the driver with SteamVR".to_string(),
        "Then it asks separately about the background service and the SteamVR settings.".to_string(),
    ];
    if let Some((new, old)) = install::update_available(env) {
        intro.insert(1, format!("An older version ({}) is installed; this one ({}) will replace it in place.", old.commit, new.commit));
    }
    if dry_run {
        println!("dry run: nothing will be changed\n{}", intro.join("\n"));
    } else if !dialog.ask(&intro.join("\n")) {
        println!("nothing was installed");
        return 0;
    }
    let mut problems = vec![];
    let r = install::install_files(env, &source, dry_run);
    report(dialog, "Files", &r.lines);
    problems.extend(r.problems);
    problems.extend(register_driver(env, dialog, dry_run));
    // The service.
    let service_question = format!("Enable the background service?\n\n{GLASSES_NOTICE}");
    if dry_run {
        println!("{GLASSES_NOTICE}");
        match units::install(env, std::env::var_os("APPIMAGE_EXTRACT_AND_RUN").is_some(), true) {
            Ok(l) | Err(l) => report(dialog, "Service", &l),
        }
    } else if dialog.ask(&service_question) {
        match units::install(env, std::env::var_os("APPIMAGE_EXTRACT_AND_RUN").is_some(), false) {
            Ok(l) => report(dialog, "Service", &l),
            Err(p) => problems.extend(p),
        }
    } else {
        println!("the service was not enabled; start SteamVR from a checkout with tools/vr_session.sh, or run setup again");
    }
    // The SteamVR settings.
    if dry_run || dialog.ask("Check the SteamVR settings now and offer to change the ones the setup needs?\n(SteamVR must not be running; each change is shown and asked about, and a backup is kept.)") {
        match settings::fix(env, dialog, dry_run) {
            settings::FixOutcome::Applied(l) => report(dialog, "SteamVR settings", &l),
            settings::FixOutcome::Refused(why) => problems.push(why),
            settings::FixOutcome::NothingToDo => println!("the SteamVR settings are already right"),
            settings::FixOutcome::Declined => println!("no SteamVR setting was changed"),
        }
    }
    if !problems.is_empty() {
        eprintln!("\nProblems:");
        for p in &problems {
            eprintln!("  - {p}");
        }
        if dialog.is_graphical() && !dialog.assume_yes {
            dialog.info(&format!("Setup finished with problems:\n{}", problems.join("\n")));
        }
    }
    if dry_run {
        return 0;
    }
    let lines = check::evaluate(&check::gather(env));
    let text = check::render(&lines);
    print!("\n{text}");
    if dialog.is_graphical() && !dialog.assume_yes {
        dialog.info(&text);
    }
    (check::failed(&lines) || !problems.is_empty()) as i32
}

fn replaced_record(env: &Env) -> std::path::PathBuf {
    env.state_dir().join("replaced-driver")
}

/// Register the installed driver, replacing another `xreal` registration (the checkout's) only if the user agrees; the replaced path is
/// recorded so uninstall can put it back.
fn register_driver(env: &Env, dialog: &Dialog, dry_run: bool) -> Vec<String> {
    let ours = if dry_run && !env.installed_driver().exists() { env.bundled_driver() } else { env.installed_driver() };
    let registered = install::xreal_registrations(env);
    if registered.iter().any(|p| install::same_path(p, &ours)) {
        println!("the driver is already registered from {}", ours.display());
        return vec![];
    }
    let others: Vec<&String> = registered.iter().collect();
    if !others.is_empty() {
        let names = others.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        if dry_run {
            println!("would replace the XREAL driver registered from {names} with {}", ours.display());
        } else if !dialog.ask(&format!("An XREAL driver is already registered with SteamVR from:\n  {names}\nReplace it with the installed one? (uninstall puts it back)")) {
            return vec![format!("the driver registered from {names} was left in place; the installed one is not registered")];
        } else {
            let _ = std::fs::create_dir_all(env.state_dir());
            let _ = std::fs::write(replaced_record(env), others[0].as_str());
            for o in &others {
                if let Err(e) = install::vrpathreg(env, "removedriver", std::path::Path::new(o)) {
                    return vec![e];
                }
            }
        }
    } else if dry_run {
        println!("would register {} with SteamVR", ours.display());
    }
    if dry_run {
        return vec![];
    }
    match install::vrpathreg(env, "adddriver", &ours) {
        Ok(()) => {
            println!("registered {} with SteamVR", ours.display());
            vec![]
        }
        Err(e) => vec![e],
    }
}

/// `KEY=value` lines (systemd EnvironmentFile style: comments with # or ;, optional quotes).
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()))
        })
        .collect()
}

fn status(env: &Env) -> i32 {
    let u = |verb, unit| units::state(verb, unit);
    println!("installed:   {}", env.installed_version().map(|v| v.commit).unwrap_or_else(|| "no".into()));
    println!("bundle:      {}", env.bundled_version().map(|v| v.commit).unwrap_or_else(|| "none (not running from a release)".into()));
    println!("driver:      {}", { let r = install::xreal_registrations(env); if r.is_empty() { "not registered".into() } else { r.join(", ") } });
    println!("socket:      {} / {}", u("is-enabled", units::SOCKET), u("is-active", units::SOCKET));
    println!("service:     {}", u("is-active", units::SERVICE));
    println!("glasses:     {}", match (check::gather_output_summary(), ()) { (Some(s), _) => s, _ => "no connected glasses output found".into() });
    println!("recorded display mode: {}", match crate::glasses::read_record(&env.state_dir()) { Some(crate::glasses::Previous::TwoD) => "was 2D (not restored yet)", Some(crate::glasses::Previous::Sbs) => "was full SBS", None => "none" });
    // The service gets its options from this file (EnvironmentFile in the unit), not from the shell that runs `status`.
    let file = env.config_home.join("xreal-linux/service.env");
    let vars = parse_env_file(&std::fs::read_to_string(&file).unwrap_or_default());
    let get = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let args = serve_args(get("XREAL_REPROJECT"), get("XREAL_EXTRA_ARGS"));
    println!("service options: {} ({})", args.join(" "), if file.is_file() { file.display().to_string() } else { format!("defaults; put XREAL_REPROJECT=0 or XREAL_EXTRA_ARGS=... in {}", file.display()) });
    println!("reprojection: {}", if args.iter().any(|a| a == "--reproject") { "on" } else { "off" });
    println!("last session (journalctl --user -u {}):", units::SERVICE);
    let out = std::process::Command::new("journalctl").args(["--user", "-u", units::SERVICE, "-n", "5", "--no-pager", "-o", "cat"]).output();
    match out {
        Ok(o) if !o.stdout.is_empty() => print!("{}", String::from_utf8_lossy(&o.stdout)),
        _ => println!("  (nothing logged yet)"),
    }
    0
}

fn uninstall(env: &Env, dialog: &Dialog, dry_run: bool) -> i32 {
    if !dry_run && !dialog.ask("Uninstall XREAL for SteamVR?\n\nThis stops and removes the background service, puts the glasses back in their recorded mode, unregisters the driver, restores the SteamVR settings this package changed, and deletes the installed files. Records stay in the state directory.") {
        println!("nothing was changed");
        return 0;
    }
    let mut problems = vec![];
    report(dialog, "Service", &units::remove(env, dry_run));
    if dry_run {
        println!("would restore the glasses' display mode if a restore is pending");
    } else {
        match crate::glasses::restore_previous_mode() {
            crate::glasses::Restore::Pending(why) => problems.push(why),
            r => crate::report_restore(r),
        }
    }
    for o in install::xreal_registrations(env) {
        if install::same_path(&o, &env.installed_driver()) {
            if dry_run {
                println!("would deregister {o}");
            } else if let Err(e) = install::vrpathreg(env, "removedriver", std::path::Path::new(&o)) {
                problems.push(e);
            } else {
                println!("deregistered {o}");
            }
        } else {
            println!("the driver registered from {o} is not this package's; left registered");
        }
    }
    if let Ok(old) = std::fs::read_to_string(replaced_record(env)) {
        let old = old.trim();
        if dry_run {
            println!("would register {old} again");
        } else if std::path::Path::new(old).exists() {
            match install::vrpathreg(env, "adddriver", std::path::Path::new(old)) {
                Ok(()) => {
                    println!("registered {old} again");
                    let _ = std::fs::remove_file(replaced_record(env));
                }
                Err(e) => problems.push(e),
            }
        }
    }
    match settings::restore(env, dry_run) {
        Ok(lines) => report(dialog, "SteamVR settings", &lines),
        Err(e) => problems.push(e),
    }
    let r = install::remove_files(env, dry_run);
    report(dialog, "Files", &r.lines);
    problems.extend(r.problems);
    if problems.is_empty() {
        println!("uninstalled");
        0
    } else {
        eprintln!("Problems:");
        for p in &problems {
            eprintln!("  - {p}");
        }
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_runs_with_reprojection_unless_it_is_turned_off() {
        assert_eq!(serve_args(None, None), ["--service", "--reproject"]);
        assert_eq!(serve_args(Some("1"), None), ["--service", "--reproject"]);
        assert_eq!(serve_args(Some("0"), None), ["--service"]);
        assert_eq!(serve_args(None, Some("--no-set-sbs  --sim-pose")), ["--service", "--reproject", "--no-set-sbs", "--sim-pose"]);
    }

    #[test]
    fn the_service_environment_file_is_parsed_like_systemd_does() {
        let v = parse_env_file("# comment\nXREAL_REPROJECT=0\nXREAL_EXTRA_ARGS=\"--no-set-sbs --sim-pose\"\n\nbad line\n");
        assert_eq!(v, [("XREAL_REPROJECT".to_string(), "0".to_string()), ("XREAL_EXTRA_ARGS".to_string(), "--no-set-sbs --sim-pose".to_string())]);
    }

    #[test]
    fn the_glasses_notice_says_what_happens() {
        assert!(GLASSES_NOTICE.contains("full side-by-side") && GLASSES_NOTICE.contains("back to the mode") && GLASSES_NOTICE.contains("two seconds") && GLASSES_NOTICE.contains("not running"));
    }
}

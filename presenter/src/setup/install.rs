//! Self-install into `$XDG_DATA_HOME/xreal-linux/` and driver registration with SteamVR. Never overwrites a file it did not create,
//! and records every file it places so uninstall removes only those.

use super::env::{Env, Version};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Files of the driver, relative to the driver directory, with whether they are executable.
const DRIVER_FILES: [(&str, bool); 3] = [("driver.vrdrivermanifest", false), ("resources/settings/default.vrsettings", false), ("bin/linux64/driver_xreal.so", true)];

/// Relative to the install directory.
const MANIFEST: &str = ".installed-files";

#[derive(Debug, PartialEq)]
pub enum Placed {
    Wrote,
    Unchanged,
    /// A file that is not ours is in the way; left alone.
    NotOurs,
}

/// Paths (relative to the install directory) this package placed, one per line.
pub fn read_manifest(install_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(install_dir.join(MANIFEST)).map(|t| t.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

fn write_manifest(install_dir: &Path, files: &[String]) -> std::io::Result<()> {
    let mut sorted = files.to_vec();
    sorted.sort();
    sorted.dedup();
    std::fs::write(install_dir.join(MANIFEST), sorted.join("\n") + "\n")
}

/// Copy `from` to `to` via a temporary file, unless `to` exists, is not in `owned`, and differs.
fn place(from: &Path, to: &Path, rel: &str, owned: &[String], executable: bool, dry_run: bool) -> Result<Placed, String> {
    let new = std::fs::read(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    if let Ok(old) = std::fs::read(to) {
        if old == new {
            return Ok(Placed::Unchanged);
        }
        if !owned.iter().any(|o| o == rel) {
            return Ok(Placed::NotOurs);
        }
    }
    if dry_run {
        return Ok(Placed::Wrote);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let tmp = to.with_extension("xreal-tmp");
    std::fs::write(&tmp, &new).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }));
    }
    std::fs::rename(&tmp, to).map_err(|e| format!("cannot replace {}: {e}", to.display()))?;
    Ok(Placed::Wrote)
}

pub struct Report {
    pub lines: Vec<String>,
    pub problems: Vec<String>,
}

/// Place the app, the driver and the version file. `app_source` is the AppImage (or, from a build directory, the presenter binary).
pub fn install_files(env: &Env, app_source: &Path, dry_run: bool) -> Report {
    let mut report = Report { lines: vec![], problems: vec![] };
    let dir = env.install_dir();
    let mut owned = read_manifest(&dir);
    let mut jobs: Vec<(PathBuf, PathBuf, String, bool)> = vec![(app_source.to_path_buf(), env.installed_appimage(), "xreal-linux.AppImage".into(), true)];
    for (rel, exec) in DRIVER_FILES {
        jobs.push((env.bundled_driver().join(rel), env.installed_driver().join(rel), format!("driver/xreal/{rel}"), exec));
    }
    jobs.push((env.bundle.join("VERSION"), dir.join("VERSION"), "VERSION".into(), false));
    for (from, to, rel, exec) in jobs {
        match place(&from, &to, &rel, &owned, exec, dry_run) {
            Ok(Placed::Wrote) => {
                report.lines.push(format!("{} {}", if dry_run { "would place" } else { "placed" }, to.display()));
                if !owned.contains(&rel) {
                    owned.push(rel);
                }
            }
            Ok(Placed::Unchanged) => {
                report.lines.push(format!("already in place: {}", to.display()));
                if to.exists() && !owned.contains(&rel) && !dry_run {
                    // An identical file we did not place: leave it and do not claim it.
                }
            }
            Ok(Placed::NotOurs) => report.problems.push(format!("{} exists and is not a file this package placed; left alone", to.display())),
            Err(e) => report.problems.push(e),
        }
    }
    if !dry_run && !owned.is_empty() {
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| write_manifest(&dir, &owned)) {
            report.problems.push(format!("cannot write the file list in {}: {e}", dir.display()));
        }
    }
    report
}

/// Remove the files this package placed (and empty directories it leaves), leaving everything else.
pub fn remove_files(env: &Env, dry_run: bool) -> Report {
    let mut report = Report { lines: vec![], problems: vec![] };
    let dir = env.install_dir();
    let owned = read_manifest(&dir);
    if owned.is_empty() {
        report.lines.push("no installed files recorded".into());
    }
    for rel in &owned {
        let path = dir.join(rel);
        if !path.exists() {
            continue;
        }
        if dry_run {
            report.lines.push(format!("would remove {}", path.display()));
        } else if let Err(e) = std::fs::remove_file(&path) {
            report.problems.push(format!("cannot remove {}: {e}", path.display()));
        } else {
            report.lines.push(format!("removed {}", path.display()));
        }
    }
    if !dry_run {
        let _ = std::fs::remove_file(dir.join(MANIFEST));
        // Directories bottom-up; remove_dir only succeeds when empty, so anything of the user's keeps its directory.
        for sub in ["driver/xreal/bin/linux64", "driver/xreal/bin", "driver/xreal/resources/settings", "driver/xreal/resources", "driver/xreal", "driver"] {
            let _ = std::fs::remove_dir(dir.join(sub));
        }
        let _ = std::fs::remove_dir(&dir);
    }
    report
}

/// Whether the bundle we run from is newer than what is installed: Some(installed) when an update should be offered.
pub fn update_available(env: &Env) -> Option<(Version, Version)> {
    let (new, old) = (env.bundled_version()?, env.installed_version()?);
    new.newer_than(&old).then_some((new, old))
}

// ---- driver registration ------------------------------------------------------------------------------------------------------

/// The external driver directories registered with SteamVR, from `openvrpaths.vrpath`.
pub fn registered_drivers(env: &Env) -> Vec<String> {
    std::fs::read_to_string(env.openvrpaths())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["external_drivers"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()))
        .unwrap_or_default()
}

/// Registered paths whose driver is named `xreal` (the directory's last component).
pub fn xreal_registrations(env: &Env) -> Vec<String> {
    registered_drivers(env).into_iter().filter(|p| Path::new(p).file_name().is_some_and(|n| n == "xreal")).collect()
}

/// Run SteamVR's own `vrpathreg.sh` (adddriver / removedriver) so its file is edited by the tool that owns it.
pub fn vrpathreg(env: &Env, verb: &str, path: &Path) -> Result<(), String> {
    let script = env.steamvr_dir().join("bin/vrpathreg.sh");
    if !script.is_file() {
        return Err(format!("{} not found: is SteamVR installed (Steam app 250820)?", script.display()));
    }
    let out = Command::new(&script).arg(verb).arg(path).output().map_err(|e| format!("cannot run vrpathreg.sh: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("vrpathreg.sh {verb} failed: {}", String::from_utf8_lossy(&out.stdout).trim().to_string() + &String::from_utf8_lossy(&out.stderr)))
    }
}

pub fn same_path(a: &str, b: &Path) -> bool {
    Path::new(a) == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> (PathBuf, Env) {
        let root = std::env::temp_dir().join(format!("xreal-install-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bundle = root.join("bundle");
        for (rel, _) in DRIVER_FILES {
            let p = bundle.join("driver/xreal").join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, format!("driver {rel}")).unwrap();
        }
        std::fs::write(bundle.join("VERSION"), "commit aaa\nbuilt 100\n").unwrap();
        std::fs::write(bundle.join("app"), "the app").unwrap();
        let env = Env::for_root(&root.join("home"), &bundle);
        (root, env)
    }

    #[test]
    fn install_places_files_records_them_and_is_idempotent() {
        let (root, env) = fixture("a");
        let r = install_files(&env, &env.bundle.join("app"), false);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(std::fs::read_to_string(env.installed_appimage()).unwrap(), "the app");
        assert!(env.installed_driver().join("bin/linux64/driver_xreal.so").is_file());
        assert_eq!(read_manifest(&env.install_dir()).len(), 5);
        let again = install_files(&env, &env.bundle.join("app"), false);
        assert!(again.lines.iter().all(|l| l.starts_with("already in place")), "{:?}", again.lines);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let (root, env) = fixture("b");
        let r = install_files(&env, &env.bundle.join("app"), true);
        assert!(r.lines.iter().all(|l| l.starts_with("would place")), "{:?}", r.lines);
        assert!(!env.install_dir().exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_foreign_file_in_the_way_is_left_alone_and_survives_uninstall() {
        let (root, env) = fixture("c");
        std::fs::create_dir_all(env.install_dir()).unwrap();
        std::fs::write(env.installed_appimage(), "someone else's file").unwrap();
        std::fs::write(env.install_dir().join("notes.txt"), "mine").unwrap();
        let r = install_files(&env, &env.bundle.join("app"), false);
        assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
        assert_eq!(std::fs::read_to_string(env.installed_appimage()).unwrap(), "someone else's file");
        remove_files(&env, false);
        assert_eq!(std::fs::read_to_string(env.installed_appimage()).unwrap(), "someone else's file");
        assert_eq!(std::fs::read_to_string(env.install_dir().join("notes.txt")).unwrap(), "mine");
        assert!(!env.installed_driver().join("bin/linux64/driver_xreal.so").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_newer_bundle_updates_files_it_owns_in_place() {
        let (root, env) = fixture("d");
        install_files(&env, &env.bundle.join("app"), false);
        std::fs::write(env.bundle.join("VERSION"), "commit bbb\nbuilt 200\n").unwrap();
        std::fs::write(env.bundle.join("driver/xreal/bin/linux64/driver_xreal.so"), "driver v2").unwrap();
        let (new, old) = update_available(&env).unwrap();
        assert_eq!((new.commit.as_str(), old.commit.as_str()), ("bbb", "aaa"));
        let r = install_files(&env, &env.bundle.join("app"), false);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(std::fs::read_to_string(env.installed_driver().join("bin/linux64/driver_xreal.so")).unwrap(), "driver v2");
        assert!(update_available(&env).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_removes_exactly_the_recorded_files_and_empty_directories() {
        let (root, env) = fixture("e");
        install_files(&env, &env.bundle.join("app"), false);
        remove_files(&env, false);
        assert!(!env.install_dir().exists(), "everything we placed, and the directory, is gone");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registrations_are_read_from_the_vrpath_file() {
        let (root, env) = fixture("f");
        std::fs::create_dir_all(env.openvrpaths().parent().unwrap()).unwrap();
        std::fs::write(env.openvrpaths(), r#"{"external_drivers":["/a/other","/b/xreal"],"version":1}"#).unwrap();
        assert_eq!(xreal_registrations(&env), vec!["/b/xreal".to_string()]);
        std::fs::remove_dir_all(root).unwrap();
    }
}

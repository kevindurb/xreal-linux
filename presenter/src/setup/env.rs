//! Where things are: the user's directories, SteamVR and what this binary ships. Built from the process environment, or from a root
//! directory in tests and in the sandboxed runs used to check install and uninstall.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    pub data_home: PathBuf,
    pub state_home: PathBuf,
    pub config_home: PathBuf,
    /// The Steam directory that holds `config/`, `logs/` and `steamapps/`.
    pub steam_root: PathBuf,
    /// The directory this binary and its `driver/` and `VERSION` live in (the AppImage's payload, or `dist/`).
    pub bundle: PathBuf,
    /// The AppImage file we were started from, if any (`$APPIMAGE`).
    pub appimage: Option<PathBuf>,
    pub uid: u32,
}

impl Env {
    pub fn from_process() -> Result<Env, String> {
        let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
        let dir = |var: &str, rel: &str| std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(rel));
        let data_home = dir("XDG_DATA_HOME", ".local/share");
        let exe = std::env::current_exe().map_err(|e| format!("cannot find myself: {e}"))?;
        // Steam's usual directory, else the `~/.steam/steam` symlink it keeps pointing at its real one.
        let steam_root = [data_home.join("Steam"), home.join(".steam/steam")].into_iter().find(|p| p.join("steamapps").is_dir()).unwrap_or_else(|| data_home.join("Steam"));
        Ok(Env {
            state_home: dir("XDG_STATE_HOME", ".local/state"),
            config_home: dir("XDG_CONFIG_HOME", ".config"),
            bundle: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            appimage: std::env::var_os("APPIMAGE").map(PathBuf::from),
            steam_root,
            data_home,
            home,
            uid: unsafe { libc::getuid() },
        })
    }

    /// Everything under one root directory, with a bundle at `bundle` (tests).
    #[cfg(test)]
    pub fn for_root(root: &Path, bundle: &Path) -> Env {
        Env {
            home: root.to_path_buf(),
            data_home: root.join(".local/share"),
            state_home: root.join(".local/state"),
            config_home: root.join(".config"),
            steam_root: root.join(".local/share/Steam"),
            bundle: bundle.to_path_buf(),
            appimage: None,
            uid: 1000,
        }
    }

    /// `$XDG_DATA_HOME/xreal-linux`: the installed AppImage and driver.
    pub fn install_dir(&self) -> PathBuf {
        self.data_home.join("xreal-linux")
    }
    pub fn installed_appimage(&self) -> PathBuf {
        self.install_dir().join("xreal-linux.AppImage")
    }
    pub fn installed_driver(&self) -> PathBuf {
        self.install_dir().join("driver").join("xreal")
    }
    pub fn state_dir(&self) -> PathBuf {
        self.state_home.join("xreal-linux")
    }
    pub fn unit_dir(&self) -> PathBuf {
        self.config_home.join("systemd/user")
    }
    pub fn steamvr_dir(&self) -> PathBuf {
        self.steam_root.join("steamapps/common/SteamVR")
    }
    pub fn vrsettings(&self) -> PathBuf {
        self.steam_root.join("config/steamvr.vrsettings")
    }
    pub fn openvrpaths(&self) -> PathBuf {
        self.config_home.join("openvr/openvrpaths.vrpath")
    }
    /// The driver shipped in this bundle.
    pub fn bundled_driver(&self) -> PathBuf {
        // `XREAL_DRIVER_DIR`: a checkout's driver/xreal, for the scripts in tools/.
        if let Some(d) = std::env::var_os("XREAL_DRIVER_DIR").filter(|d| !d.is_empty()) {
            return PathBuf::from(d);
        }
        self.bundle.join("driver").join("xreal")
    }
    pub fn bundled_version(&self) -> Option<Version> {
        Version::read(&self.bundle.join("VERSION"))
    }
    pub fn installed_version(&self) -> Option<Version> {
        Version::read(&self.install_dir().join("VERSION"))
    }
}

/// The `VERSION` file: `commit <hash>` and `built <unix seconds of the commit>` lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub commit: String,
    pub built: u64,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let field = |name: &str| text.lines().find_map(|l| l.strip_prefix(name).map(|r| r.trim().to_string()));
        Some(Version { commit: field("commit ")?, built: field("built ").and_then(|b| b.parse().ok()).unwrap_or(0) })
    }
    pub fn read(path: &Path) -> Option<Version> {
        Version::parse(&std::fs::read_to_string(path).ok()?)
    }
    /// Strictly newer than `other`: a later build time. The same commit is never newer.
    pub fn newer_than(&self, other: &Version) -> bool {
        self.commit != other.commit && self.built > other.built
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare_by_build_time() {
        let a = Version::parse("commit abc\nbuilt 100\n").unwrap();
        let b = Version::parse("commit def\nbuilt 200\n").unwrap();
        assert!(b.newer_than(&a) && !a.newer_than(&b) && !a.newer_than(&a));
        assert_eq!(Version::parse("nonsense"), None);
        assert_eq!(Version::parse("commit abc\n").unwrap().built, 0);
    }
}

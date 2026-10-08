//! The SteamVR settings the setup depends on. SteamVR rewrites `steamvr.vrsettings` on exit, so nothing here edits it while SteamVR
//! runs. The first change makes a backup, and every changed key is recorded with its previous value so uninstall puts back only those.

use super::dialog::Dialog;
use super::env::Env;
use serde_json::{json, Value};
use std::path::Path;

pub struct Wanted {
    pub section: &'static str,
    pub key: &'static str,
    pub value: Value,
    pub why: &'static str,
}

/// SteamVR's safe mode writes this when a driver crashed or stalled; while it is true the driver is not loaded.
pub const BLOCKED_KEY: &str = "blocked_by_safe_mode";

/// `value` Null means the key must be absent (or false).
pub fn wanted() -> Vec<Wanted> {
    vec![
        Wanted { section: "steamvr", key: "forcedDriver", value: json!("xreal"), why: "makes SteamVR use the XREAL driver (this also means SteamVR will not use another headset until you undo it)" },
        Wanted { section: "power", key: "turnOffScreensTimeout", value: json!(86400), why: "the 5 s default puts the headset into standby after a few seconds of stillness" },
        Wanted { section: "power", key: "pauseCompositorOnStandby", value: json!(false), why: "keeps SteamVR drawing when it thinks the headset is idle" },
        Wanted { section: "steamvr", key: "motionSmoothing", value: json!(false), why: "the presenter does its own reprojection; SteamVR's smoothing fights it" },
        Wanted { section: "driver_xreal", key: BLOCKED_KEY, value: Value::Null, why: "SteamVR's safe mode disabled the XREAL driver after a crash (\"Headset Not Detected (108)\"); clearing it lets the driver load again" },
    ]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub section: String,
    pub key: String,
    pub old: Option<Value>,
    pub new: Value,
    pub why: String,
}

/// The wanted settings that the file does not have yet.
pub fn plan(settings: &Value) -> Vec<Change> {
    wanted()
        .into_iter()
        .filter_map(|w| {
            let old = settings.get(w.section).and_then(|s| s.get(w.key)).cloned();
            // A number setting is satisfied by a larger value (the timeout only has to be long).
            let satisfied = match (&old, &w.value) {
                (Some(Value::Number(have)), Value::Number(want)) if w.key == "turnOffScreensTimeout" => have.as_f64() >= want.as_f64().map(|v| v.min(600.0)),
                (Some(Value::Bool(false)) | None, Value::Null) => true,
                (Some(have), want) => have == want,
                (None, _) => false,
            };
            (!satisfied).then(|| Change { section: w.section.into(), key: w.key.into(), old, new: w.value, why: w.why.into() })
        })
        .collect()
}

pub fn describe(c: &Change) -> String {
    let old = c.old.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "not set".into());
    let new = if c.new.is_null() { "removed".to_string() } else { c.new.to_string() };
    format!("{}.{}: {} -> {}  ({})", c.section, c.key, old, new, c.why)
}

pub fn steamvr_running() -> bool {
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
        e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) && std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| matches!(c.trim(), "vrserver" | "vrcompositor"))
    })
}

fn load(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", path.display()))
}

/// Written the way SteamVR writes it (three-space indent, sorted keys), through a temporary file and a rename.
fn save(path: &Path, v: &Value) -> Result<(), String> {
    // serde_json indents by two spaces; SteamVR writes three.
    let pretty = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    let mut text: String = pretty
        .lines()
        .map(|l| {
            let indent = l.len() - l.trim_start_matches(' ').len();
            format!("{}{}", " ".repeat(indent / 2 * 3), &l[indent..])
        })
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    let buf = text.into_bytes();
    let tmp = path.with_extension("vrsettings.xreal-tmp");
    std::fs::write(&tmp, buf).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

fn manifest_path(env: &Env) -> std::path::PathBuf {
    env.state_dir().join("settings-changes.json")
}

/// Previously recorded changes (original values), if any.
pub fn read_manifest(env: &Env) -> Vec<Change> {
    let Ok(text) = std::fs::read_to_string(manifest_path(env)) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return vec![] };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| Some(Change { section: e["section"].as_str()?.into(), key: e["key"].as_str()?.into(), old: e.get("old").filter(|o| !o.is_null()).cloned(), new: e["new"].clone(), why: String::new() }))
                .collect()
        })
        .unwrap_or_default()
}

fn write_manifest(env: &Env, changes: &[Change]) -> Result<(), String> {
    let arr: Vec<Value> = changes.iter().map(|c| json!({"section": c.section, "key": c.key, "old": c.old, "new": c.new})).collect();
    std::fs::create_dir_all(env.state_dir()).map_err(|e| e.to_string())?;
    std::fs::write(manifest_path(env), serde_json::to_string_pretty(&arr).unwrap() + "\n").map_err(|e| format!("cannot write the change record: {e}"))
}

pub enum FixOutcome {
    NothingToDo,
    Applied(Vec<String>),
    Declined,
    Refused(String),
}

/// Show each change, ask, then back up, apply and record the accepted ones.
pub fn fix(env: &Env, dialog: &Dialog, dry_run: bool) -> FixOutcome {
    let path = env.vrsettings();
    if !path.is_file() {
        return FixOutcome::Refused(format!("{} does not exist yet: start SteamVR once, quit it, and run this again", path.display()));
    }
    let mut settings = match load(&path) {
        Ok(v) => v,
        Err(e) => return FixOutcome::Refused(e),
    };
    let changes = plan(&settings);
    if changes.is_empty() {
        return FixOutcome::NothingToDo;
    }
    if steamvr_running() && !dry_run {
        return FixOutcome::Refused("SteamVR is running and rewrites its settings when it quits: stop SteamVR, then run fix again".into());
    }
    let mut accepted = vec![];
    for c in changes {
        if dry_run {
            println!("would change {}", describe(&c));
        } else if dialog.ask(&format!("Change the SteamVR setting\n{}\n?", describe(&c))) {
            accepted.push(c);
        }
    }
    if dry_run {
        return FixOutcome::Applied(vec![]);
    }
    if accepted.is_empty() {
        return FixOutcome::Declined;
    }
    // Back up before the first change; later fixes keep the first backup's values in the record.
    let backup = env.state_dir().join(format!("steamvr.vrsettings.backup-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)));
    if let Err(e) = std::fs::create_dir_all(env.state_dir()).and_then(|_| std::fs::copy(&path, &backup).map(|_| ())) {
        return FixOutcome::Refused(format!("cannot back up {}: {e}; nothing was changed", path.display()));
    }
    let mut record = read_manifest(env);
    for c in &accepted {
        let section = settings.as_object_mut().unwrap().entry(c.section.clone()).or_insert_with(|| json!({}));
        if c.new.is_null() {
            if let Some(o) = section.as_object_mut() {
                o.remove(&c.key);
            }
        } else {
            section[&c.key] = c.new.clone();
        }
        match record.iter_mut().find(|r| r.section == c.section && r.key == c.key) {
            Some(r) => r.new = c.new.clone(), // keep the original old value
            None => record.push(c.clone()),
        }
    }
    if let Err(e) = write_manifest(env, &record).and_then(|_| save(&path, &settings)) {
        return FixOutcome::Refused(e);
    }
    let mut lines: Vec<String> = accepted.iter().map(describe).collect();
    lines.push(format!("backup: {}", backup.display()));
    FixOutcome::Applied(lines)
}

/// Put back only the keys the record names, and only if they still hold the value we set.
pub fn restore(env: &Env, dry_run: bool) -> Result<Vec<String>, String> {
    let record = read_manifest(env);
    if record.is_empty() {
        return Ok(vec!["no SteamVR settings were changed by this package".into()]);
    }
    let path = env.vrsettings();
    let mut settings = load(&path)?;
    if steamvr_running() && !dry_run {
        return Err("SteamVR is running and rewrites its settings when it quits: stop SteamVR first".into());
    }
    let mut lines = vec![];
    for r in &record {
        let current = settings.get(&r.section).and_then(|s| s.get(&r.key)).cloned();
        // A key we removed (new is null) is still "ours" while it is absent.
        let still_ours = if r.new.is_null() { current.is_none() } else { current.as_ref() == Some(&r.new) };
        if !still_ours {
            lines.push(format!("{}.{} was changed since: left as it is", r.section, r.key));
            continue;
        }
        let mut line = format!("{} {}.{} -> {}", if dry_run { "would restore" } else { "restored" }, r.section, r.key, r.old.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "not set".into()));
        if r.key == BLOCKED_KEY && r.old.is_some() {
            line.push_str(" (this puts SteamVR's safe-mode block back, as it was before)");
        }
        lines.push(line);
        if !dry_run {
            let section = settings.as_object_mut().unwrap().entry(r.section.clone()).or_insert_with(|| json!({})).as_object_mut().unwrap();
            match &r.old {
                Some(v) => {
                    section.insert(r.key.clone(), v.clone());
                }
                None => {
                    section.remove(&r.key);
                }
            }
        }
    }
    if !dry_run {
        save(&path, &settings)?;
        let _ = std::fs::remove_file(manifest_path(env));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::dialog::{Backend, Dialog};

    fn env(tag: &str, settings: &Value) -> (std::path::PathBuf, Env) {
        let root = std::env::temp_dir().join(format!("xreal-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let env = Env::for_root(&root, &root);
        std::fs::create_dir_all(env.vrsettings().parent().unwrap()).unwrap();
        std::fs::write(env.vrsettings(), serde_json::to_string_pretty(settings).unwrap()).unwrap();
        (root, env)
    }

    fn yes() -> Dialog { Dialog { backend: Backend::None, assume_yes: true } }
    fn no() -> Dialog { Dialog { backend: Backend::None, assume_yes: false } }

    #[test]
    fn only_missing_or_wrong_settings_are_planned() {
        let all_right = json!({"steamvr": {"forcedDriver": "xreal", "motionSmoothing": false}, "power": {"turnOffScreensTimeout": 86400, "pauseCompositorOnStandby": false}});
        assert!(plan(&all_right).is_empty());
        let keys: Vec<_> = plan(&json!({})).into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["forcedDriver", "turnOffScreensTimeout", "pauseCompositorOnStandby", "motionSmoothing"]);
        let short = plan(&json!({"power": {"turnOffScreensTimeout": 5.0}}));
        assert!(short.iter().any(|c| c.key == "turnOffScreensTimeout" && c.old == Some(json!(5.0))));
        assert!(!plan(&json!({"power": {"turnOffScreensTimeout": 1000}})).iter().any(|c| c.key == "turnOffScreensTimeout"), "a long enough timeout is left alone");
    }

    #[test]
    fn a_declined_change_writes_nothing() {
        let original = json!({"steamvr": {"forcedDriver": "other"}});
        let (root, env) = env("declined", &original);
        let before = std::fs::read(env.vrsettings()).unwrap();
        assert!(matches!(fix(&env, &no(), false), FixOutcome::Declined));
        assert_eq!(std::fs::read(env.vrsettings()).unwrap(), before);
        assert!(!env.state_dir().exists(), "no backup, no record");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fix_backs_up_applies_and_the_restore_changes_only_those_keys() {
        let original = json!({"steamvr": {"forcedDriver": "other", "installID": "42"}, "driver_null": {"enable": false}});
        let (root, env) = env("fix", &original);
        assert!(matches!(fix(&env, &yes(), false), FixOutcome::Applied(_)));
        let after: Value = serde_json::from_str(&std::fs::read_to_string(env.vrsettings()).unwrap()).unwrap();
        assert_eq!(after["steamvr"]["forcedDriver"], "xreal");
        assert_eq!(after["steamvr"]["installID"], "42");
        assert_eq!(after["power"]["pauseCompositorOnStandby"], false);
        let backups: Vec<_> = std::fs::read_dir(env.state_dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("backup")).collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(backups[0].path()).unwrap()).unwrap(), original);
        assert!(matches!(fix(&env, &yes(), false), FixOutcome::NothingToDo), "a second run has nothing to do");
        // The user changes an unrelated key, and one of ours, before uninstalling.
        let mut now = after.clone();
        now["driver_null"]["enable"] = json!(true);
        now["power"]["turnOffScreensTimeout"] = json!(7200);
        std::fs::write(env.vrsettings(), serde_json::to_string(&now).unwrap()).unwrap();
        let lines = restore(&env, false).unwrap();
        assert!(lines.iter().any(|l| l.contains("turnOffScreensTimeout was changed since")), "{lines:?}");
        let restored: Value = serde_json::from_str(&std::fs::read_to_string(env.vrsettings()).unwrap()).unwrap();
        assert_eq!(restored["steamvr"]["forcedDriver"], "other");
        assert!(restored["power"].get("pauseCompositorOnStandby").is_none(), "a key that did not exist is removed again");
        assert_eq!(restored["driver_null"]["enable"], true, "unrelated changes survive");
        assert_eq!(restored["power"]["turnOffScreensTimeout"], 7200);
        assert!(read_manifest(&env).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_first_backup_values_survive_a_second_fix() {
        let (root, env) = env("twice", &json!({"steamvr": {"forcedDriver": "other"}}));
        fix(&env, &yes(), false);
        let mut now: Value = serde_json::from_str(&std::fs::read_to_string(env.vrsettings()).unwrap()).unwrap();
        now["steamvr"]["motionSmoothing"] = json!(true);
        std::fs::write(env.vrsettings(), now.to_string()).unwrap();
        fix(&env, &yes(), false);
        let record = read_manifest(&env);
        assert_eq!(record.iter().find(|r| r.key == "forcedDriver").unwrap().old, Some(json!("other")));
        assert_eq!(record.iter().find(|r| r.key == "motionSmoothing").unwrap().old, None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_safe_mode_block_is_planned_for_removal_only_when_set() {
        assert!(!plan(&json!({"driver_xreal": {"blocked_by_safe_mode": false}})).iter().any(|c| c.key == BLOCKED_KEY));
        assert!(!plan(&json!({})).iter().any(|c| c.key == BLOCKED_KEY));
        let c = plan(&json!({"driver_xreal": {"blocked_by_safe_mode": true}})).into_iter().find(|c| c.key == BLOCKED_KEY).unwrap();
        assert_eq!(c.old, Some(json!(true)));
        assert!(describe(&c).contains("-> removed"));
    }

    #[test]
    fn fix_clears_the_block_and_uninstall_puts_it_back_with_a_note() {
        let original = json!({"driver_xreal": {"blocked_by_safe_mode": true, "running_start_ms": 8}, "steamvr": {"forcedDriver": "xreal", "motionSmoothing": false}, "power": {"turnOffScreensTimeout": 86400, "pauseCompositorOnStandby": false}});
        let (root, env) = env("blocked", &original);
        assert!(matches!(fix(&env, &yes(), false), FixOutcome::Applied(_)));
        let after: Value = serde_json::from_str(&std::fs::read_to_string(env.vrsettings()).unwrap()).unwrap();
        assert!(after["driver_xreal"].get(BLOCKED_KEY).is_none());
        assert_eq!(after["driver_xreal"]["running_start_ms"], 8, "other driver keys are untouched");
        assert!(matches!(fix(&env, &yes(), false), FixOutcome::NothingToDo));
        let backups = std::fs::read_dir(env.state_dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("backup")).count();
        assert_eq!(backups, 1);
        let lines = restore(&env, false).unwrap();
        assert!(lines.iter().any(|l| l.contains("safe-mode block back")), "{lines:?}");
        let restored: Value = serde_json::from_str(&std::fs::read_to_string(env.vrsettings()).unwrap()).unwrap();
        assert_eq!(restored["driver_xreal"][BLOCKED_KEY], true);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_declined_block_removal_changes_nothing() {
        let (root, env) = env("blocked-no", &json!({"driver_xreal": {"blocked_by_safe_mode": true}, "steamvr": {"forcedDriver": "xreal", "motionSmoothing": false}, "power": {"turnOffScreensTimeout": 86400, "pauseCompositorOnStandby": false}}));
        let before = std::fs::read(env.vrsettings()).unwrap();
        assert!(matches!(fix(&env, &no(), false), FixOutcome::Declined));
        assert_eq!(std::fs::read(env.vrsettings()).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }
}

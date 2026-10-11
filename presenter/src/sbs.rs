//! `sbs`: standalone flat side-by-side for games that render their own SBS, without SteamVR.
//!
//! `on` puts the glasses into full SBS and then supervises: if the glasses drop back to 2D it sets full SBS again, a
//! bounded number of times. `off` restores the mode recorded before the first set. `status` reads the mode and the
//! record. Nothing is sent while a SteamVR session is running, because SteamVR owns the mode then.
//!
//! See `openspec/changes/add-headlook-mode` (the flat-stereo-mode spec).

use crate::glasses::{self, Previous, Restore};
use std::time::Duration;

/// How many times `sbs on` re-sets full SBS after the glasses drop to 2D before it gives up.
pub const RE_SET_BOUND: u32 = 3;
/// How often the mode is polled while supervising.
const POLL: Duration = Duration::from_secs(5);
/// Consecutive polls that may fail to read the mode before the supervisor stops.
const UNREACHABLE_LIMIT: u32 = 6;

/// A name for a mode value from the getter: 0 is regular 2D, anything else is full SBS.
pub fn mode_name(mode: u64) -> &'static str {
    if mode == 0 {
        "2D"
    } else {
        "full side-by-side"
    }
}

/// Why `sbs` refuses to act, or `None` when it may.
pub fn conflict(steamvr_running: bool) -> Option<&'static str> {
    steamvr_running.then_some("SteamVR is running and owns the glasses' display mode; stop SteamVR before using `sbs`")
}

/// Is a SteamVR session running? It would fight `sbs` over the display mode.
pub fn steamvr_running() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else { return false };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
            let comm = comm.trim();
            if comm == "vrserver" || comm == "vrmonitor" || comm == "vrcompositor" {
                return true;
            }
        }
    }
    false
}

/// What a supervised poll says to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropAction {
    /// Full SBS is still set.
    StillSbs,
    /// The glasses dropped to 2D and the setter should be sent again.
    Resend,
    /// The re-set bound is reached.
    GaveUp,
    /// The mode could not be read this time, but not often enough to stop.
    Transient,
    /// The mode could not be read for several polls in a row.
    Lost,
}

/// Counts drops and re-sets while `sbs on` supervises. Pure, so a test can drive it.
pub struct DropWatcher {
    resets_left: u32,
    unreachable: u32,
}

impl DropWatcher {
    pub fn new(bound: u32) -> Self {
        DropWatcher { resets_left: bound, unreachable: 0 }
    }

    pub fn observe(&mut self, mode: Result<u64, String>) -> DropAction {
        match mode {
            Ok(0) if self.resets_left > 0 => {
                self.resets_left -= 1;
                self.unreachable = 0;
                DropAction::Resend
            }
            Ok(0) => DropAction::GaveUp,
            Ok(_) => {
                self.unreachable = 0;
                DropAction::StillSbs
            }
            Err(_) => {
                self.unreachable += 1;
                if self.unreachable >= UNREACHABLE_LIMIT {
                    DropAction::Lost
                } else {
                    DropAction::Transient
                }
            }
        }
    }
}

/// The `sbs` subcommand. Returns the process exit code.
pub fn command(args: &[&str]) -> i32 {
    match args.first().copied() {
        Some("on") => on(),
        Some("off") => off(),
        Some("status") => status(),
        _ => {
            eprintln!("usage: xreal-presenter sbs on|off|status");
            2
        }
    }
}

fn on() -> i32 {
    if let Some(why) = conflict(steamvr_running()) {
        eprintln!("{why}");
        return 1;
    }
    match glasses::read_input_mode() {
        Ok(m) if m != 0 => println!("the glasses are already in {}; supervising for drops", mode_name(m)),
        Ok(_) => {
            if !glasses::set_sbs_once() {
                eprintln!("could not reach the glasses' control port");
                return 1;
            }
            println!("set full side-by-side");
        }
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    }
    supervise()
}

fn supervise() -> i32 {
    let mut watcher = DropWatcher::new(RE_SET_BOUND);
    println!("sbs on: supervising the display mode (Ctrl-C to stop)");
    loop {
        std::thread::sleep(POLL);
        match watcher.observe(glasses::read_input_mode()) {
            DropAction::StillSbs | DropAction::Transient => {}
            DropAction::Resend => {
                println!("the glasses dropped to 2D; setting full side-by-side again");
                let _ = glasses::set_sbs_once();
            }
            DropAction::GaveUp => {
                eprintln!("the glasses keep dropping to 2D; giving up after {RE_SET_BOUND} re-sets");
                return 1;
            }
            DropAction::Lost => {
                eprintln!("lost the glasses' control port; stopping");
                return 1;
            }
        }
    }
}

fn off() -> i32 {
    if let Some(why) = conflict(steamvr_running()) {
        eprintln!("{why}");
        return 1;
    }
    match glasses::restore_previous_mode() {
        Restore::Restored => {
            println!("the glasses are back in the 2D mode recorded before the session");
            0
        }
        Restore::NothingToSend => {
            println!("nothing to restore: the glasses were already in full side-by-side when this was first set");
            0
        }
        Restore::NothingRecorded => {
            println!("nothing recorded to restore");
            0
        }
        Restore::Pending(why) => {
            eprintln!("{why}");
            1
        }
    }
}

fn status() -> i32 {
    let recorded = glasses::state_dir().and_then(|d| glasses::read_record(&d));
    let rec = match recorded {
        Some(Previous::TwoD) => "was 2D (restore pending)",
        Some(Previous::Sbs) => "was full side-by-side",
        None => "none",
    };
    println!("recorded mode:    {rec}");
    match glasses::read_input_mode() {
        Ok(m) => {
            println!("glasses input mode: {} ({m})", mode_name(m));
            0
        }
        Err(e) => {
            eprintln!("glasses input mode: unreachable ({e})");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_names_split_2d_from_sbs() {
        assert_eq!(mode_name(0), "2D");
        assert_eq!(mode_name(1), "full side-by-side");
        assert_eq!(mode_name(7), "full side-by-side");
    }

    #[test]
    fn a_steamvr_session_is_a_conflict_with_a_reason() {
        assert!(conflict(true).unwrap().contains("SteamVR"));
        assert_eq!(conflict(false), None);
    }

    #[test]
    fn the_drop_watcher_resets_a_bounded_number_of_times() {
        let mut w = DropWatcher::new(2);
        assert_eq!(w.observe(Ok(1)), DropAction::StillSbs);
        assert_eq!(w.observe(Ok(0)), DropAction::Resend, "first drop");
        assert_eq!(w.observe(Ok(1)), DropAction::StillSbs);
        assert_eq!(w.observe(Ok(0)), DropAction::Resend, "second drop");
        assert_eq!(w.observe(Ok(0)), DropAction::GaveUp, "the bound is reached");
    }

    #[test]
    fn the_drop_watcher_tolerates_a_few_unreachable_polls() {
        let mut w = DropWatcher::new(1);
        for _ in 0..UNREACHABLE_LIMIT - 1 {
            assert_eq!(w.observe(Err("down".into())), DropAction::Transient);
        }
        assert_eq!(w.observe(Err("down".into())), DropAction::Lost);
        // A good poll clears the streak.
        let mut w = DropWatcher::new(1);
        assert_eq!(w.observe(Err("down".into())), DropAction::Transient);
        assert_eq!(w.observe(Ok(1)), DropAction::StillSbs);
        for _ in 0..UNREACHABLE_LIMIT - 1 {
            assert_eq!(w.observe(Err("down".into())), DropAction::Transient);
        }
    }

    #[test]
    fn an_unknown_subcommand_is_a_usage_error() {
        assert_eq!(command(&[]), 2);
        assert_eq!(command(&["nonsense"]), 2);
    }
}

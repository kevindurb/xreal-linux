//! A service started right after login can run before the desktop session has put `WAYLAND_DISPLAY` into the systemd user manager's
//! environment. winit then panics at once, the unit hits its start limit, and the driver waits on a dead socket. So the service waits a
//! bounded time for a display variable, taking it from the manager's environment.

use std::time::{Duration, Instant};

pub const WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(200);
const VARS: [&str; 2] = ["WAYLAND_DISPLAY", "DISPLAY"];

pub const NOT_FOUND: &str = "no display found (neither WAYLAND_DISPLAY nor DISPLAY is set, here or in the systemd user manager's environment): the desktop session must be running before the presenter starts; log in to the desktop, then start SteamVR again";

/// The display variables out of `systemctl --user show-environment` output.
fn parse_manager(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, v)| VARS.contains(k) && !v.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Variables to set so a display can be opened: empty at once when the environment already has one, otherwise what the manager
/// reports once it has one. `Err` after `timeout`. Manager errors count as "not there yet".
pub fn wait_for_display(has_display: impl Fn() -> bool, manager_env: impl Fn() -> Result<String, String>, timeout: Duration, poll: Duration) -> Result<Vec<(String, String)>, ()> {
    if has_display() {
        return Ok(vec![]);
    }
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(vars) = manager_env().map(|t| parse_manager(&t)) {
            if !vars.is_empty() {
                return Ok(vars);
            }
        }
        if Instant::now() >= deadline {
            return Err(());
        }
        std::thread::sleep(poll);
    }
}

/// For the service path: wait, set what was found for this process, or print the remedy and exit non-zero.
pub fn ensure_display() {
    let found = wait_for_display(
        || VARS.iter().any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty())),
        || crate::setup::units::systemctl_user(&["show-environment"]),
        WAIT,
        POLL,
    );
    match found {
        Ok(vars) => {
            for (k, v) in vars {
                eprintln!("display: using {k}={v} from the user manager");
                std::env::set_var(k, v);
            }
        }
        Err(()) => {
            eprintln!("{NOT_FOUND}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const SHORT: Duration = Duration::from_millis(60);
    const TICK: Duration = Duration::from_millis(5);

    #[test]
    fn a_display_already_present_does_not_wait_or_query() {
        let queried = Cell::new(false);
        let r = wait_for_display(|| true, || { queried.set(true); Ok(String::new()) }, SHORT, TICK);
        assert_eq!(r, Ok(vec![]));
        assert!(!queried.get());
    }

    #[test]
    fn a_display_that_appears_late_is_taken_from_the_manager() {
        let calls = Cell::new(0);
        let r = wait_for_display(
            || false,
            || {
                calls.set(calls.get() + 1);
                Ok(if calls.get() < 3 { "PATH=/usr/bin\n".into() } else { "PATH=/usr/bin\nWAYLAND_DISPLAY=wayland-0\nHOME=/home/x\n".into() })
            },
            Duration::from_secs(5),
            TICK,
        );
        assert_eq!(r, Ok(vec![("WAYLAND_DISPLAY".into(), "wayland-0".into())]));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn no_display_ever_gives_up_after_the_deadline() {
        let start = Instant::now();
        assert_eq!(wait_for_display(|| false, || Ok("PATH=/usr/bin\nWAYLAND_DISPLAY=\n".into()), SHORT, TICK), Err(()));
        assert!(start.elapsed() >= SHORT);
    }

    #[test]
    fn a_failing_manager_query_is_not_found_not_a_panic() {
        assert_eq!(wait_for_display(|| false, || Err("Failed to connect to bus".into()), SHORT, TICK), Err(()));
        assert!(NOT_FOUND.contains("no display found") && NOT_FOUND.contains("desktop session"));
    }

    #[test]
    fn x11_display_counts_too() {
        let r = wait_for_display(|| false, || Ok("DISPLAY=:0\n".into()), SHORT, TICK);
        assert_eq!(r, Ok(vec![("DISPLAY".into(), ":0".into())]));
    }
}

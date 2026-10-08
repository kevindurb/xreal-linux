//! Asking the user: `kdialog`, then `zenity`, then the terminal, whichever is present and usable, so an AppImage double-clicked in a
//! file manager (no terminal) still shows its questions. The background service never asks.

use std::io::{BufRead, Write};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backend {
    Kdialog,
    Zenity,
    Terminal,
    /// No dialog tool with a display and no terminal: nothing can be asked.
    None,
}

/// What the choice depends on, as plain values so it can be tested.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub has_kdialog: bool,
    pub has_zenity: bool,
    /// A graphical session is reachable (`WAYLAND_DISPLAY` or `DISPLAY` is set).
    pub has_display: bool,
    pub stdin_is_terminal: bool,
}

pub fn choose_backend(f: &Facts) -> Backend {
    if f.has_display && f.has_kdialog {
        Backend::Kdialog
    } else if f.has_display && f.has_zenity {
        Backend::Zenity
    } else if f.stdin_is_terminal {
        Backend::Terminal
    } else {
        Backend::None
    }
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

pub fn facts_from_process() -> Facts {
    Facts {
        has_kdialog: on_path("kdialog"),
        has_zenity: on_path("zenity"),
        has_display: std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some(),
        stdin_is_terminal: unsafe { libc::isatty(0) } == 1,
    }
}

pub struct Dialog {
    pub backend: Backend,
    /// `--yes`: answer every question yes without asking.
    pub assume_yes: bool,
}

const TITLE: &str = "XREAL for SteamVR";

impl Dialog {
    pub fn new(assume_yes: bool) -> Dialog {
        Dialog { backend: choose_backend(&facts_from_process()), assume_yes }
    }

    /// Whether this dialog opens windows (as opposed to printing in the terminal).
    pub fn is_graphical(&self) -> bool {
        matches!(self.backend, Backend::Kdialog | Backend::Zenity)
    }

    /// Show text the user should read (a report, a notice).
    pub fn info(&self, text: &str) {
        match self.backend {
            Backend::Kdialog => {
                let _ = Command::new("kdialog").args(["--title", TITLE, "--msgbox", text]).status();
            }
            Backend::Zenity => {
                let _ = Command::new("zenity").args(["--info", "--title", TITLE, "--no-markup", "--text", text]).status();
            }
            Backend::Terminal | Backend::None => println!("{text}"),
        }
    }

    /// A yes/no question; `assume_yes` answers yes, and with no way to ask the answer is no.
    pub fn ask(&self, text: &str) -> bool {
        if self.assume_yes {
            println!("{text}\n  -> yes (--yes)");
            return true;
        }
        match self.backend {
            Backend::Kdialog => Command::new("kdialog").args(["--title", TITLE, "--yesno", text]).status().is_ok_and(|s| s.success()),
            Backend::Zenity => Command::new("zenity").args(["--question", "--title", TITLE, "--no-markup", "--text", text]).status().is_ok_and(|s| s.success()),
            Backend::Terminal => {
                print!("{text}\n[y/N] ");
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line).is_ok() && parse_yes(&line)
            }
            Backend::None => {
                eprintln!("{text}\n  -> no (nothing to ask with: no terminal, and neither kdialog nor zenity with a display)");
                false
            }
        }
    }
}

pub fn parse_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(kde: bool, zen: bool, display: bool, tty: bool) -> Facts {
        Facts { has_kdialog: kde, has_zenity: zen, has_display: display, stdin_is_terminal: tty }
    }

    #[test]
    fn kdialog_first_then_zenity_then_the_terminal() {
        assert_eq!(choose_backend(&facts(true, true, true, true)), Backend::Kdialog);
        assert_eq!(choose_backend(&facts(false, true, true, true)), Backend::Zenity);
        assert_eq!(choose_backend(&facts(false, false, true, true)), Backend::Terminal);
    }

    #[test]
    fn a_launch_from_a_file_manager_uses_the_dialog_tool() {
        assert_eq!(choose_backend(&facts(true, false, true, false)), Backend::Kdialog);
        assert_eq!(choose_backend(&facts(false, true, true, false)), Backend::Zenity);
    }

    #[test]
    fn a_dialog_tool_without_a_display_is_not_usable() {
        assert_eq!(choose_backend(&facts(true, true, false, true)), Backend::Terminal);
        assert_eq!(choose_backend(&facts(true, true, false, false)), Backend::None);
    }

    #[test]
    fn nothing_to_ask_with() {
        assert_eq!(choose_backend(&facts(false, false, false, false)), Backend::None);
        assert!(!Dialog { backend: Backend::None, assume_yes: false }.ask("?"));
        assert!(Dialog { backend: Backend::None, assume_yes: true }.ask("?"));
    }

    #[test]
    fn answers_are_parsed_strictly() {
        assert!(parse_yes("y\n") && parse_yes(" YES ") && !parse_yes("") && !parse_yes("sure") && !parse_yes("n"));
    }
}

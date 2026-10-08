//! The driver link's handshake: a protocol version both sides must share, and whether the glasses are usable.
//!
//!   type 8 HELLO       driver -> presenter  [8, protocol_version]   the first message on every connection
//!   type 9 HELLO_REPLY presenter -> driver  [9, protocol_version, glasses_present, reason]
//!
//! `reason` explains a false `glasses_present` (the `REASON_*` codes). A driver that sends no HELLO is older than the handshake.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Bump when a message of the driver link changes meaning; the driver (driver/src/xreal_driver.cpp, `kProtocolVersion`) must match.
pub const PROTOCOL_VERSION: u32 = 1;
pub const MSG_HELLO: u32 = 8;
pub const MSG_HELLO_REPLY: u32 = 9;

pub const REASON_OK: u32 = 0;
pub const REASON_GLASSES_UNREACHABLE: u32 = 1;
pub const REASON_VERSION_MISMATCH: u32 = 2;
pub const REASON_NOT_SBS: u32 = 3;

/// The protocol version a driver announced by its first message; 0 for a driver that predates the handshake.
pub fn driver_version(first_message: &[u32; 16]) -> u32 {
    if first_message[0] == MSG_HELLO { first_message[1] } else { 0 }
}

/// Ok when the driver speaks our protocol, else the sentence for the log and the journal.
pub fn check_version(driver: u32, ours: u32) -> Result<(), String> {
    if driver == ours {
        return Ok(());
    }
    let theirs = if driver == 0 { "none (a driver older than the handshake)".to_string() } else { driver.to_string() };
    Err(format!("driver protocol version {theirs}, presenter protocol version {ours}; restart SteamVR so it loads the matching driver"))
}

pub fn hello_reply(glasses_present: bool, reason: u32) -> [u32; 16] {
    let mut w = [0u32; 16];
    w[0] = MSG_HELLO_REPLY;
    w[1] = PROTOCOL_VERSION;
    w[2] = glasses_present as u32;
    w[3] = reason;
    w
}

pub fn reason_text(reason: u32) -> &'static str {
    match reason {
        REASON_OK => "ok",
        REASON_GLASSES_UNREACHABLE => "the glasses did not answer on their control port",
        REASON_VERSION_MISMATCH => "driver and presenter protocol versions differ",
        REASON_NOT_SBS => "the glasses did not reach full side by side",
        _ => "unknown",
    }
}

/// The listening socket systemd passed us (socket activation), if any: `LISTEN_PID` must be our own pid and `LISTEN_FDS` at least 1;
/// the first passed descriptor is always fd 3.
pub fn systemd_listen_fd(listen_pid: Option<&str>, listen_fds: Option<&str>, our_pid: u32) -> Option<i32> {
    let pid: u32 = listen_pid?.trim().parse().ok()?;
    let fds: i32 = listen_fds?.trim().parse().ok()?;
    (pid == our_pid && fds >= 1).then_some(3)
}

/// The abstract socket name the driver connects to for this user.
pub fn default_socket_name(uid: u32) -> String {
    format!("xreal-presenter-{uid}")
}

/// Whether the glasses answer on the control port, as the control thread last saw it.
#[derive(Default)]
pub struct GlassesStatus {
    state: Mutex<Option<bool>>,
    changed: Condvar,
}

impl GlassesStatus {
    pub fn set_reachable(&self, reachable: bool) {
        *self.state.lock().unwrap() = Some(reachable);
        self.changed.notify_all();
    }

    pub fn reachable(&self) -> Option<bool> {
        *self.state.lock().unwrap()
    }

    /// Wait until the glasses have answered (true) or `timeout` passes (false).
    pub fn wait_reachable(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.state.lock().unwrap();
        loop {
            if *g == Some(true) {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else { return false };
            g = self.changed.wait_timeout(g, left).unwrap().0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(words: &[u32]) -> [u32; 16] {
        let mut m = [0u32; 16];
        m[..words.len()].copy_from_slice(words);
        m
    }

    #[test]
    fn matching_versions_proceed() {
        assert_eq!(driver_version(&msg(&[MSG_HELLO, PROTOCOL_VERSION])), PROTOCOL_VERSION);
        assert!(check_version(PROTOCOL_VERSION, PROTOCOL_VERSION).is_ok());
    }

    #[test]
    fn a_different_version_names_both_numbers() {
        let e = check_version(7, 1).unwrap_err();
        assert!(e.contains("driver protocol version 7") && e.contains("presenter protocol version 1") && e.contains("restart SteamVR"), "{e}");
    }

    #[test]
    fn a_driver_without_a_hello_is_version_zero_and_refused() {
        let v = driver_version(&msg(&[1, 1, 1280, 720])); // a SET as the first message
        assert_eq!(v, 0);
        let e = check_version(v, PROTOCOL_VERSION).unwrap_err();
        assert!(e.contains("older than the handshake"), "{e}");
    }

    #[test]
    fn the_reply_carries_version_presence_and_reason() {
        assert_eq!(hello_reply(true, REASON_OK)[..4], [MSG_HELLO_REPLY, PROTOCOL_VERSION, 1, 0]);
        assert_eq!(hello_reply(false, REASON_GLASSES_UNREACHABLE)[..4], [MSG_HELLO_REPLY, PROTOCOL_VERSION, 0, 1]);
    }

    #[test]
    fn systemd_hands_over_fd_3_only_to_the_right_process() {
        assert_eq!(systemd_listen_fd(Some("42"), Some("1"), 42), Some(3));
        assert_eq!(systemd_listen_fd(Some("42"), Some("2"), 42), Some(3));
        assert_eq!(systemd_listen_fd(Some("41"), Some("1"), 42), None, "meant for another process");
        assert_eq!(systemd_listen_fd(Some("42"), Some("0"), 42), None);
        assert_eq!(systemd_listen_fd(None, None, 42), None);
        assert_eq!(systemd_listen_fd(Some("x"), Some("1"), 42), None);
    }

    #[test]
    fn the_default_socket_name_is_per_user() {
        assert_eq!(default_socket_name(1000), "xreal-presenter-1000");
    }

    #[test]
    fn waiting_for_the_glasses_ends_when_they_answer_or_time_runs_out() {
        let s = std::sync::Arc::new(GlassesStatus::default());
        assert!(!s.wait_reachable(Duration::from_millis(30)));
        let s2 = s.clone();
        let t = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(40)); s2.set_reachable(true); });
        assert!(s.wait_reachable(Duration::from_secs(2)));
        t.join().unwrap();
    }
}

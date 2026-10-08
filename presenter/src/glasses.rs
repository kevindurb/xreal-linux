//! The glasses' control port (TCP 52999): read the factory calibration, log the glasses' own events, and optionally set full
//! side-by-side. Framing and ids are in docs/xreal-link-messages.md section 13. The read-only getters 10015 (config) and 10273
//! (input mode) are always sent; the one setter, 10274, only when the getter says the glasses are in the regular mode, at most once
//! per connection and `MAX_SBS_SETS` times per run, and never with `--no-set-sbs`.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const GET_CONFIG: u16 = 10015;
const GET_INPUT_MODE: u16 = 10273;
const SET_INPUT_MODE: u16 = 10274;
const TEMPERATURE_EVENT: u16 = 10122;
const TX_TOP_BIT: u32 = 0x8000_0000;
const GETTER_BODY: [u8; 2] = [0x18, 0x00];
const SBS_BODY: [u8; 4] = [0x1a, 0x02, 0x08, 0x01];
const TWO_D_BODY: [u8; 4] = [0x1a, 0x02, 0x08, 0x00];
const HOSTS: [&str; 2] = ["169.254.2.1:52999", "169.254.1.1:52999"];
/// Bounds the setter for glasses that keep reverting to the regular mode.
const MAX_SBS_SETS: u32 = 3;

/// What the presenter takes from the glasses' factory calibration.
#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    /// OpenVR raw projection tangents (left, right, top, bottom), symmetric, the mean over both eyes.
    pub fov: [f32; 4],
    pub ipd_m: f32,
    /// Row-major 3x3 matrices applied to the raw gyro and accelerometer vectors.
    pub gyro_matrix: [f64; 9],
    pub accel_matrix: [f64; 9],
    /// The two displays' orientations (left, right), Hamilton w, x, y, z, in the IMU's frame.
    pub eye_orientations: Option<[[f32; 4]; 2]>,
}

pub type SharedCalibration = Arc<Mutex<Option<Calibration>>>;

static START: OnceLock<Instant> = OnceLock::new();

/// Seconds since the presenter started: the one clock the `[control ...]` and `[display ...]` log lines share, so tools/analyze_control_events.py can line them up.
pub fn uptime_s() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// The sorted, de-duplicated mode list the kernel reports for the connector named `monitor` (for example `DP-1`), or None if it has no entry.
fn read_modes(monitor: &str) -> Option<Vec<String>> {
    for card in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let name = card.file_name().to_string_lossy().into_owned();
        if name.starts_with("card") && name.ends_with(&format!("-{monitor}")) {
            let text = std::fs::read_to_string(card.path().join("modes")).ok()?;
            let mut modes: Vec<String> = text.lines().map(str::to_owned).collect();
            modes.sort();
            modes.dedup();
            return Some(modes);
        }
    }
    None
}

/// Log every change of the connector's mode list; a single `3840x1080` is full side-by-side, anything else means the glasses are in 2D (or gone).
pub fn watch_display_modes(monitor: String) {
    let mut last: Option<Vec<String>> = None;
    loop {
        let now = read_modes(&monitor);
        if now != last {
            let text = now.as_ref().map(|m| m.join(" ")).unwrap_or_else(|| "no connector".into());
            println!("[display +{:.1}s] modes: {text}", uptime_s());
            last = now;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// The only requests this client sends: the read-only getters 10015 and 10273, and the display input mode setter 10274 with value 0 (restore 2D) or 1 (full SBS).
fn is_allowed(id: u16, body: &[u8]) -> bool {
    match id {
        GET_CONFIG | GET_INPUT_MODE => body == GETTER_BODY,
        SET_INPUT_MODE => body == SBS_BODY || body == TWO_D_BODY,
        _ => false,
    }
}

pub fn build_request(msg_id: u16, txid: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + body.len());
    out.extend_from_slice(&msg_id.to_be_bytes());
    out.extend_from_slice(&((4 + body.len()) as u32).to_be_bytes());
    out.extend_from_slice(&(txid | TX_TOP_BIT).to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn read_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let byte = *b.get(*i)?;
        *i += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// The length-delimited field `field` of a protobuf message (the first one if repeated).
fn bytes_field(msg: &[u8], field: u64) -> Option<&[u8]> {
    let mut i = 0;
    while i < msg.len() {
        let tag = read_varint(msg, &mut i)?;
        match tag & 7 {
            0 => {
                read_varint(msg, &mut i)?;
            }
            1 => i += 8,
            5 => i += 4,
            2 => {
                let n = read_varint(msg, &mut i)? as usize;
                let end = i.checked_add(n).filter(|&e| e <= msg.len())?;
                if tag >> 3 == field {
                    return Some(&msg[i..end]);
                }
                i = end;
            }
            _ => return None,
        }
    }
    None
}

/// The numeric value (field 2, a varint) of a getter reply body; 0 when absent, as protobuf omits defaults.
fn reply_value(body: &[u8]) -> Option<u64> {
    let nested = bytes_field(body, 4)?;
    let mut i = 0;
    while i < nested.len() {
        let tag = read_varint(nested, &mut i)?;
        match tag & 7 {
            0 => {
                let v = read_varint(nested, &mut i)?;
                if tag >> 3 == 2 {
                    return Some(v);
                }
            }
            _ => return None,
        }
    }
    Some(0)
}

/// The JSON text of a GetConfig reply body.
fn reply_config_json(body: &[u8]) -> Option<&str> {
    let nested = bytes_field(body, 4)?;
    std::str::from_utf8(bytes_field(nested, 2)?).ok()
}

fn numbers(v: &Value, n: usize) -> Option<Vec<f64>> {
    let a: Vec<f64> = v.as_array()?.iter().map(|x| x.as_f64()).collect::<Option<_>>()?;
    (a.len() == n).then_some(a)
}

/// Field of view and IPD from the display intrinsics, assuming the 1080-row side-by-side picture sits unscaled and centred in the
/// panel's 1200 rows (not measured), and the IMU matrices. The serial number is not read here.
pub fn parse_calibration(json: &str) -> Result<Calibration, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("config is not JSON: {e}"))?;
    let d = &v["display"];
    let res = numbers(&d["resolution"], 2).ok_or("display.resolution missing")?;
    let (panel_w, panel_h) = (res[0], res[1]);
    let picture_h = 1080.0;
    let crop = (panel_h - picture_h) / 2.0;
    let (mut h_sum, mut v_sum) = (0.0, 0.0);
    for key in ["k_left_display", "k_right_display"] {
        let k = numbers(&d[key], 9).ok_or(format!("display.{key} missing"))?;
        let (fx, fy, cx, cy) = (k[0], k[4], k[2], k[5] - crop);
        h_sum += cx / fx + (panel_w - cx) / fx;
        v_sum += cy / fy + (picture_h - cy) / fy;
    }
    let h = (h_sum / 4.0) as f32;
    let t = (v_sum / 4.0) as f32;
    let xl = numbers(&d["target_p_left_display"], 3).ok_or("target_p_left_display missing")?[0];
    let xr = numbers(&d["target_p_right_display"], 3).ok_or("target_p_right_display missing")?[0];
    let orientation = |key: &str| numbers(&d[key], 4).map(|q| [q[3] as f32, q[0] as f32, q[1] as f32, q[2] as f32]);
    let eye_orientations = orientation("target_q_left_display").zip(orientation("target_q_right_display")).map(|(l, r)| [l, r]);
    let imu = &v["IMU"]["device_1"]["imu_intrinsics"];
    let m = |key: &str| -> Result<[f64; 9], String> {
        let a = numbers(&imu[key], 9).ok_or(format!("IMU intrinsics {key} missing"))?;
        Ok(a.try_into().unwrap())
    };
    Ok(Calibration { fov: [-h, h, -t, t], ipd_m: (xr - xl).abs() as f32, gyro_matrix: m("gyro_calib_mat")?, accel_matrix: m("accl_calib_mat")?, eye_orientations })
}

fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".cache").join("xreal-presenter"))
}

/// 64-bit FNV-1a, so a cache file can be named per unit without putting the serial number in the name.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn save_cache(json: &str) {
    let (Some(dir), Ok(v)) = (cache_dir(), serde_json::from_str::<Value>(json)) else { return };
    let serial = v["FSN"].as_str().unwrap_or("");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("config-{:016x}.json", fnv1a(serial))), json);
}

/// The most recently cached config, for when the control port cannot be reached.
fn load_cache() -> Option<String> {
    let newest = std::fs::read_dir(cache_dir()?)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("config-"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?;
    std::fs::read_to_string(newest.path()).ok()
}

struct Conn {
    stream: TcpStream,
    buf: Vec<u8>,
    last_temperature_log: Option<Instant>,
}

/// What waiting for the next frame produced: a quiet line is normal, only a closed or failed one ends the connection.
#[derive(Debug, PartialEq)]
enum Next {
    Frame(u16, Vec<u8>),
    Quiet,
    Closed,
}

/// What to do about the input mode after reading it.
#[derive(Debug, PartialEq)]
enum SbsPlan {
    Send,
    AlreadySet(u64),
    Skip(&'static str),
}

/// The setter goes out only for the regular mode (0), once per connection, and never more than `MAX_SBS_SETS` times in a run.
fn plan_sbs(mode: Option<u64>, sent_on_this_connection: bool, sent_in_run: u32) -> SbsPlan {
    match mode {
        None => SbsPlan::Skip("no usable input mode reply"),
        Some(0) if sent_on_this_connection => SbsPlan::Skip("already sent on this connection"),
        Some(0) if sent_in_run >= MAX_SBS_SETS => SbsPlan::Skip("the setter limit for this run is reached"),
        Some(0) => SbsPlan::Send,
        Some(v) => SbsPlan::AlreadySet(v),
    }
}

/// The result code of a setter reply (field 1 of the nested message); 0 is success, and protobuf omits it when 0.
fn reply_status(body: &[u8]) -> Option<u64> {
    let nested = bytes_field(body, 4)?;
    let mut i = 0;
    while i < nested.len() {
        let tag = read_varint(nested, &mut i)?;
        if tag & 7 != 0 {
            return None;
        }
        let v = read_varint(nested, &mut i)?;
        if tag >> 3 == 1 {
            return Some(v);
        }
    }
    Some(0)
}

impl Conn {
    /// A dead peer (the glasses unplugged) is noticed by keepalive probes instead of by the read timeout.
    fn enable_keepalive(&self) {
        use std::os::fd::AsRawFd;
        let fd = self.stream.as_raw_fd();
        for (level, name, value) in [(libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1), (libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, 10), (libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 5), (libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 3)] {
            unsafe { libc::setsockopt(fd, level, name, &value as *const i32 as *const _, std::mem::size_of::<i32>() as u32) };
        }
    }

    fn log_event(&mut self, id: u16, payload: &[u8]) {
        if id == TEMPERATURE_EVENT {
            if self.last_temperature_log.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
                return;
            }
            self.last_temperature_log = Some(Instant::now());
        }
        let head: String = payload.iter().take(16).map(|b| format!("{b:02x}")).collect();
        println!("[control +{:.1}s] event {id} ({} bytes) {head}", uptime_s(), payload.len());
    }

    /// The next whole frame (msg_id, payload after the 6-byte header), `Quiet` if none arrived within `timeout`, `Closed` if the connection ended.
    fn read_frame(&mut self, timeout: Duration) -> Next {
        let deadline = Instant::now() + timeout;
        loop {
            if self.buf.len() >= 6 {
                let id = u16::from_be_bytes([self.buf[0], self.buf[1]]);
                let len = u32::from_be_bytes(self.buf[2..6].try_into().unwrap()) as usize;
                if len > 4 << 20 {
                    self.buf.clear();
                    return Next::Closed;
                }
                if self.buf.len() >= 6 + len {
                    let payload = self.buf[6..6 + len].to_vec();
                    self.buf.drain(..6 + len);
                    return Next::Frame(id, payload);
                }
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else { return Next::Quiet };
            let _ = self.stream.set_read_timeout(Some(left.max(Duration::from_millis(1))));
            let mut chunk = [0u8; 16384];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Next::Closed,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => return Next::Closed,
            }
        }
    }

    /// Send one request and return the reply body (after the transaction id), logging any events that arrive first.
    fn request(&mut self, id: u16, txid: u32, body: &[u8], timeout: Duration) -> Option<Vec<u8>> {
        if !is_allowed(id, body) {
            eprintln!("[control +{:.1}s] refusing request {id}: not on the allowlist", uptime_s());
            return None;
        }
        self.stream.write_all(&build_request(id, txid, body)).ok()?;
        let deadline = Instant::now() + timeout;
        loop {
            let Next::Frame(rid, payload) = self.read_frame(deadline.checked_duration_since(Instant::now())?) else { return None };
            let is_reply = rid == id && payload.len() >= 4 && u32::from_be_bytes(payload[..4].try_into().unwrap()) == txid;
            if is_reply {
                return Some(payload[4..].to_vec());
            }
            self.log_event(rid, &payload);
        }
    }

    /// Read the input mode and, only if the plan says so, set full side-by-side once. Returns whether the setter was sent.
    fn ensure_sbs(&mut self, sent_in_run: u32) -> bool {
        let mode = self.request(GET_INPUT_MODE, 2, &GETTER_BODY, Duration::from_secs(5)).as_deref().and_then(reply_value);
        match plan_sbs(mode, false, sent_in_run) {
            SbsPlan::Send => {
                println!("[control +{:.1}s] input mode is regular; sending NRDpSetInputMode = side by side ({} of at most {} this run)", uptime_s(), sent_in_run + 1, MAX_SBS_SETS);
                match self.request(SET_INPUT_MODE, 3, &SBS_BODY, Duration::from_secs(5)).as_deref() {
                    Some(reply) => match reply_status(reply) {
                        Some(0) => println!("[control +{:.1}s] NRDpSetInputMode accepted", uptime_s()),
                        Some(code) => eprintln!("[control +{:.1}s] NRDpSetInputMode rejected with status {code}; not retrying", uptime_s()),
                        None => eprintln!("[control +{:.1}s] NRDpSetInputMode reply not understood; not retrying", uptime_s()),
                    },
                    None => eprintln!("[control +{:.1}s] no reply to NRDpSetInputMode; not retrying", uptime_s()),
                }
                true
            }
            SbsPlan::AlreadySet(v) => {
                println!("[control +{:.1}s] input mode is already {v}; not sending the setter", uptime_s());
                false
            }
            SbsPlan::Skip(why) => {
                eprintln!("[control +{:.1}s] not sending the setter: {why}", uptime_s());
                false
            }
        }
    }
}

fn publish(shared: &SharedCalibration, json: &str, source: &str) {
    match parse_calibration(json) {
        Ok(c) => {
            println!("calibration from {source}: fov half tangents {:.4} x {:.4}, ipd {:.1} mm", c.fov[1], c.fov[3], c.ipd_m * 1000.0);
            crate::warp::set_fov(c.fov);
            crate::warp::set_eye_orientations(c.eye_orientations);
            *shared.lock().unwrap() = Some(c);
        }
        Err(e) => eprintln!("calibration from {source} unusable: {e}"),
    }
}

/// Read the config once per connection, set full SBS if asked, then log the glasses' events until the connection really ends; reconnect forever.
pub fn run(shared: SharedCalibration, set_sbs: bool) {
    if let Some(json) = load_cache() {
        publish(&shared, &json, "cache");
    }
    let (mut which, mut sbs_sent) = (0, 0u32);
    loop {
        let addr = HOSTS[which % HOSTS.len()];
        which += 1;
        let Ok(stream) = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2)) else {
            std::thread::sleep(Duration::from_millis(1000));
            continue;
        };
        let mut c = Conn { stream, buf: Vec::new(), last_temperature_log: None };
        c.enable_keepalive();
        println!("[control +{:.1}s] connected to {addr}", uptime_s());
        match c.request(GET_CONFIG, 1, &GETTER_BODY, Duration::from_secs(8)).as_deref().and_then(reply_config_json) {
            Some(json) => {
                save_cache(json);
                publish(&shared, json, "glasses");
            }
            None => eprintln!("no usable GetConfig reply"),
        }
        if set_sbs && c.ensure_sbs(sbs_sent) {
            sbs_sent += 1;
        }
        loop {
            match c.read_frame(Duration::from_secs(30)) {
                Next::Frame(id, payload) => c.log_event(id, &payload),
                Next::Quiet => {}
                Next::Closed => break,
            }
        }
        println!("[control +{:.1}s] connection closed", uptime_s());
        std::thread::sleep(Duration::from_millis(1000));
    }
}

/// One-shot for the launcher: connect, set full SBS if the glasses are in 2D, and return whether a connection was made.
pub fn set_sbs_once() -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut which = 0;
    while std::time::Instant::now() < deadline {
        let addr = HOSTS[which % HOSTS.len()];
        which += 1;
        let Ok(stream) = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2)) else {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        };
        let mut c = Conn { stream, buf: Vec::new(), last_temperature_log: None };
        println!("[control +{:.1}s] connected to {addr}", uptime_s());
        c.ensure_sbs(0);
        return true;
    }
    eprintln!("could not reach the glasses' control port");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_json() -> String {
        serde_json::json!({
            "FSN": "TEST-SERIAL",
            "display": {
                "resolution": [1920, 1200],
                "k_left_display": [2490.3648940102466, 0, 962.0111858309965, 0, 2470.537608760116, 605.3586852624517, 0, 0, 1],
                "k_right_display": [2488.027738860976, 0, 961.2344180191407, 0, 2460.0983343970383, 604.6005837110847, 0, 0, 1],
                "target_p_left_display": [-0.05674006253578419, 0.021724029363008816, -0.025956102625676013],
                "target_p_right_display": [0.007232850140664456, 0.02183653092211818, -0.02656951674632919],
                "target_q_left_display": [-0.0027658853160565026, 0.00734393654157105, 9.288422001839559e-05, 0.999969203449293],
                "target_q_right_display": [-0.002752314081589499, -0.00026082712728034966, 0.0014252580421451003, 0.9999951626762598]
            },
            "IMU": {"device_1": {"imu_intrinsics": {
                "gyro_calib_mat": [1.0058, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.006],
                "accl_calib_mat": [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            }}}
        })
        .to_string()
    }

    #[test]
    fn fov_and_ipd_match_the_values_used_so_far() {
        let c = parse_calibration(&config_json()).unwrap();
        assert!((c.fov[1] - 0.3857).abs() < 5e-4, "{:?}", c.fov);
        assert!((c.fov[3] - 0.2190).abs() < 5e-4, "{:?}", c.fov);
        assert_eq!(c.fov[0], -c.fov[1]);
        assert_eq!(c.fov[2], -c.fov[3]);
        assert!((c.ipd_m - 0.064).abs() < 1e-4);
        assert_eq!(c.gyro_matrix[8], 1.006);
        let [l, r] = c.eye_orientations.unwrap();
        assert!((l[0] - 0.99997).abs() < 1e-5 && (l[2] - 0.0073439).abs() < 1e-6, "{l:?}"); // stored w, x, y, z; the config has x, y, z, w
        assert!((r[3] - 0.0014253).abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn missing_fields_are_an_error_not_a_panic() {
        assert!(parse_calibration("{}").is_err());
        assert!(parse_calibration("not json").is_err());
    }

    #[test]
    fn getconfig_request_matches_the_documented_packet() {
        assert_eq!(build_request(GET_CONFIG, 1, &GETTER_BODY), [0x27, 0x1f, 0, 0, 0, 6, 0x80, 0, 0, 1, 0x18, 0]);
    }

    #[test]
    fn set_input_mode_request_is_the_approved_packet() {
        assert_eq!(build_request(SET_INPUT_MODE, 3, &SBS_BODY), [0x28, 0x22, 0, 0, 0, 8, 0x80, 0, 0, 3, 0x1a, 2, 8, 1]);
    }

    #[test]
    fn set_input_mode_two_d_request_is_the_sbs_packet_with_value_zero() {
        assert_eq!(build_request(SET_INPUT_MODE, 4, &TWO_D_BODY), [0x28, 0x22, 0, 0, 0, 8, 0x80, 0, 0, 4, 0x1a, 2, 8, 0]);
    }

    #[test]
    fn only_the_listed_ids_and_values_are_allowed() {
        assert!(is_allowed(GET_CONFIG, &GETTER_BODY) && is_allowed(GET_INPUT_MODE, &GETTER_BODY));
        assert!(is_allowed(SET_INPUT_MODE, &SBS_BODY) && is_allowed(SET_INPUT_MODE, &TWO_D_BODY));
        assert!(!is_allowed(SET_INPUT_MODE, &[0x1a, 0x02, 0x08, 0x02]), "other values");
        assert!(!is_allowed(SET_INPUT_MODE, &GETTER_BODY), "a setter id with a getter body");
        assert!(!is_allowed(GET_INPUT_MODE, &SBS_BODY), "a getter id with a setter body");
        for id in [10009u16, 10047, 10053, 10054, 10036, 10031, 10275] {
            assert!(!is_allowed(id, &GETTER_BODY) && !is_allowed(id, &SBS_BODY), "{id}");
        }
    }

    #[test]
    fn a_refused_request_sends_nothing() {
        let (mut c, mut server) = pair();
        assert_eq!(c.request(10047, 9, &GETTER_BODY, Duration::from_millis(100)), None);
        server.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let mut b = [0u8; 8];
        assert!(server.read(&mut b).is_err(), "nothing should have been written");
    }

    #[test]
    fn reply_values_follow_protobuf_defaults() {
        assert_eq!(reply_value(&[0x22, 0x00]), Some(0));
        assert_eq!(reply_value(&[0x22, 0x02, 0x10, 0x01]), Some(1));
    }

    #[test]
    fn config_reply_string_is_extracted() {
        let json = "{\"a\":1}";
        let mut nested = vec![0x08, 0x00, 0x12, json.len() as u8];
        nested.extend_from_slice(json.as_bytes());
        let mut body = vec![0x22, nested.len() as u8];
        body.extend_from_slice(&nested);
        assert_eq!(reply_config_json(&body), Some(json));
    }

    fn pair() -> (Conn, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (Conn { stream: client, buf: Vec::new(), last_temperature_log: None }, server)
    }

    #[test]
    fn a_quiet_line_is_not_a_closed_connection() {
        let (mut c, mut server) = pair();
        assert_eq!(c.read_frame(Duration::from_millis(50)), Next::Quiet);
        server.write_all(&[0x27, 0x2a, 0, 0, 0, 2, 0xaa, 0xbb]).unwrap();
        assert_eq!(c.read_frame(Duration::from_secs(2)), Next::Frame(10026, vec![0xaa, 0xbb]));
        assert_eq!(c.read_frame(Duration::from_millis(50)), Next::Quiet);
    }

    #[test]
    fn a_closed_connection_ends_the_reader() {
        let (mut c, server) = pair();
        drop(server);
        assert_eq!(c.read_frame(Duration::from_secs(2)), Next::Closed);
    }

    #[test]
    fn a_frame_split_across_reads_is_reassembled() {
        let (mut c, mut server) = pair();
        server.write_all(&[0x27, 0x2a, 0, 0, 0]).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        server.write_all(&[3, 1, 2, 3]).unwrap();
        assert_eq!(c.read_frame(Duration::from_secs(2)), Next::Frame(10026, vec![1, 2, 3]));
    }

    #[test]
    fn the_setter_is_planned_once_and_only_from_the_regular_mode() {
        assert_eq!(plan_sbs(Some(0), false, 0), SbsPlan::Send);
        assert_eq!(plan_sbs(Some(1), false, 0), SbsPlan::AlreadySet(1));
        assert_eq!(plan_sbs(Some(0), true, 0), SbsPlan::Skip("already sent on this connection"));
        assert_eq!(plan_sbs(Some(0), false, MAX_SBS_SETS), SbsPlan::Skip("the setter limit for this run is reached"));
        assert_eq!(plan_sbs(Some(0), false, MAX_SBS_SETS - 1), SbsPlan::Send);
        assert!(matches!(plan_sbs(None, false, 0), SbsPlan::Skip(_)));
    }

    #[test]
    fn setter_replies_report_success_or_a_rejection_code() {
        assert_eq!(reply_status(&[0x22, 0x00]), Some(0));
        assert_eq!(reply_status(&[0x22, 0x03, 0x08, 0x91, 0x4e]), Some(10001));
        assert_eq!(reply_status(&[0x00]), None);
    }

    #[test]
    fn cache_names_do_not_contain_the_serial() {
        assert!(!format!("config-{:016x}.json", fnv1a("TEST-SERIAL")).contains("TEST-SERIAL"));
        assert_ne!(fnv1a("a"), fnv1a("b"));
    }
}

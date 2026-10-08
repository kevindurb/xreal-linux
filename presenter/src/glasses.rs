//! The glasses' control port (TCP 52999): read the factory calibration, log the glasses' own events, and optionally set full
//! side-by-side. Framing and ids are in docs/xreal-link-messages.md section 13. Only the read-only getters 10015 (config) and 10273
//! (input mode) are sent by default; the one setter, 10274, is sent only with `--set-sbs` and only when the getter says the glasses
//! are in the regular mode.

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
const HOSTS: [&str; 2] = ["169.254.2.1:52999", "169.254.1.1:52999"];

/// What the presenter takes from the glasses' factory calibration.
#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    /// OpenVR raw projection tangents (left, right, top, bottom), symmetric, the mean over both eyes.
    pub fov: [f32; 4],
    pub ipd_m: f32,
    /// Row-major 3x3 matrices applied to the raw gyro and accelerometer vectors.
    pub gyro_matrix: [f64; 9],
    pub accel_matrix: [f64; 9],
    pub distortion: Option<crate::warp::DistortionGrids>,
    /// A hash of the unit's serial number, to name per-unit files with; the serial itself is not kept.
    pub unit_id: u64,
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

/// The factory display-distortion grids, or None if the config has none or they are not the expected 32-pixel grid.
fn parse_distortion(v: &Value) -> Option<crate::warp::DistortionGrids> {
    let side = |key: &str| -> Option<(usize, usize, Vec<[f32; 2]>)> {
        let g = &v["display_distortion"][key];
        let (cols, rows) = (g["num_col"].as_u64()? as usize, g["num_row"].as_u64()? as usize);
        let data = numbers(&g["data"], cols * rows * 4)?;
        let step = crate::warp::GRID_STEP as f64;
        let mut out = Vec::with_capacity(cols * rows);
        for (i, p) in data.chunks_exact(4).enumerate() {
            if (p[0] - (i % cols) as f64 * step).abs() > 0.5 || (p[1] - (i / cols) as f64 * step).abs() > 0.5 {
                return None;
            }
            out.push([p[2] as f32, p[3] as f32]);
        }
        Some((cols, rows, out))
    };
    let (cols, rows, left) = side("left_display")?;
    let (c2, r2, right) = side("right_display")?;
    (cols == c2 && rows == r2 && cols >= 2 && rows >= 2).then_some(crate::warp::DistortionGrids { cols, rows, left, right })
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
    let imu = &v["IMU"]["device_1"]["imu_intrinsics"];
    let m = |key: &str| -> Result<[f64; 9], String> {
        let a = numbers(&imu[key], 9).ok_or(format!("IMU intrinsics {key} missing"))?;
        Ok(a.try_into().unwrap())
    };
    Ok(Calibration { fov: [-h, h, -t, t], ipd_m: (xr - xl).abs() as f32, gyro_matrix: m("gyro_calib_mat")?, accel_matrix: m("accl_calib_mat")?, distortion: parse_distortion(&v), unit_id: fnv1a(v["FSN"].as_str().unwrap_or("")) })
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
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

impl Conn {
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

    /// The next whole frame as (msg_id, payload after the 6-byte header), or None on timeout or a closed connection.
    fn read_frame(&mut self, timeout: Duration) -> Option<(u16, Vec<u8>)> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.buf.len() >= 6 {
                let id = u16::from_be_bytes([self.buf[0], self.buf[1]]);
                let len = u32::from_be_bytes(self.buf[2..6].try_into().unwrap()) as usize;
                if len > 4 << 20 {
                    self.buf.clear();
                    return None;
                }
                if self.buf.len() >= 6 + len {
                    let payload = self.buf[6..6 + len].to_vec();
                    self.buf.drain(..6 + len);
                    return Some((id, payload));
                }
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            let _ = self.stream.set_read_timeout(Some(left.max(Duration::from_millis(1))));
            let mut chunk = [0u8; 16384];
            match self.stream.read(&mut chunk) {
                Ok(0) | Err(_) if Instant::now() >= deadline => return None,
                Ok(0) => return None,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => return None,
            }
        }
    }

    /// Send one request and return the reply body (after the transaction id), logging any events that arrive first.
    fn request(&mut self, id: u16, txid: u32, body: &[u8], timeout: Duration) -> Option<Vec<u8>> {
        self.stream.write_all(&build_request(id, txid, body)).ok()?;
        let deadline = Instant::now() + timeout;
        loop {
            let (rid, payload) = self.read_frame(deadline.checked_duration_since(Instant::now())?)?;
            let is_reply = rid == id && payload.len() >= 4 && u32::from_be_bytes(payload[..4].try_into().unwrap()) == txid;
            if is_reply {
                return Some(payload[4..].to_vec());
            }
            self.log_event(rid, &payload);
        }
    }
}

fn publish(shared: &SharedCalibration, json: &str, source: &str) {
    match parse_calibration(json) {
        Ok(c) => {
            println!("calibration from {source}: fov half tangents {:.4} x {:.4}, ipd {:.1} mm", c.fov[1], c.fov[3], c.ipd_m * 1000.0);
            crate::warp::set_fov(c.fov);
            crate::warp::set_distortion(c.distortion.clone());
            if c.distortion.is_none() { eprintln!("the config has no usable display distortion grid"); }
            *shared.lock().unwrap() = Some(c);
        }
        Err(e) => eprintln!("calibration from {source} unusable: {e}"),
    }
}

/// Read the config once per connection, set full SBS if asked, then log the glasses' events until the connection drops; reconnect forever.
pub fn run(shared: SharedCalibration, set_sbs: bool) {
    if let Some(json) = load_cache() {
        publish(&shared, &json, "cache");
    }
    let mut which = 0;
    loop {
        let addr = HOSTS[which % HOSTS.len()];
        which += 1;
        let Ok(stream) = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2)) else {
            std::thread::sleep(Duration::from_millis(1000));
            continue;
        };
        let mut c = Conn { stream, buf: Vec::new(), last_temperature_log: None };
        println!("[control +{:.1}s] connected to {addr}", uptime_s());
        match c.request(GET_CONFIG, 1, &GETTER_BODY, Duration::from_secs(8)).as_deref().and_then(reply_config_json) {
            Some(json) => {
                save_cache(json);
                publish(&shared, json, "glasses");
            }
            None => eprintln!("no usable GetConfig reply"),
        }
        if set_sbs {
            match c.request(GET_INPUT_MODE, 2, &GETTER_BODY, Duration::from_secs(5)).as_deref().and_then(reply_value) {
                Some(0) => {
                    println!("input mode is regular; sending NRDpSetInputMode = side by side");
                    let reply = c.request(SET_INPUT_MODE, 3, &SBS_BODY, Duration::from_secs(5));
                    println!("NRDpSetInputMode reply: {:?}", reply.as_deref().map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>()));
                }
                Some(v) => println!("input mode is already {v}; not sending the setter"),
                None => eprintln!("no usable NRDpGetInputMode reply; not sending the setter"),
            }
        }
        while let Some((id, payload)) = c.read_frame(Duration::from_secs(30)) {
            c.log_event(id, &payload);
        }
        println!("[control +{:.1}s] closed or silent for 30 s", uptime_s());
        std::thread::sleep(Duration::from_millis(1000));
    }
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
                "target_p_right_display": [0.007232850140664456, 0.02183653092211818, -0.02656951674632919]
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
    }

    fn grid_json(cols: usize, rows: usize, shift: f64) -> serde_json::Value {
        let data: Vec<f64> = (0..rows).flat_map(|r| (0..cols).flat_map(move |c| [c as f64 * 32.0, r as f64 * 32.0, c as f64 * 32.0 + shift, r as f64 * 32.0 - shift])).collect();
        serde_json::json!({"num_col": cols, "num_row": rows, "type": 1, "data": data})
    }

    #[test]
    fn distortion_grids_are_read_per_eye() {
        let v = serde_json::json!({"display_distortion": {"left_display": grid_json(4, 3, 2.0), "right_display": grid_json(4, 3, -3.0)}});
        let g = parse_distortion(&v).unwrap();
        assert_eq!((g.cols, g.rows), (4, 3));
        assert_eq!(g.left[1], [34.0, -2.0]);
        assert_eq!(g.right[5], [32.0 - 3.0, 32.0 + 3.0]);
    }

    #[test]
    fn a_grid_with_other_spacing_or_missing_data_is_not_used() {
        let mut bad = grid_json(4, 3, 0.0);
        bad["data"][4] = serde_json::json!(40.0); // second point's input x is not 32
        assert!(parse_distortion(&serde_json::json!({"display_distortion": {"left_display": bad, "right_display": grid_json(4, 3, 0.0)}})).is_none());
        assert!(parse_distortion(&serde_json::json!({})).is_none());
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

    #[test]
    fn cache_names_do_not_contain_the_serial() {
        assert!(!format!("config-{:016x}.json", fnv1a("TEST-SERIAL")).contains("TEST-SERIAL"));
        assert_ne!(fnv1a("a"), fnv1a("b"));
    }
}

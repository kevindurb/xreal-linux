//! Head tracking from the glasses' IMU (TCP 52998): record parsing and an orientation filter.
//!
//! Frames. The glasses report gyro (rad/s) and accelerometer (m/s^2) in their own sensor frame D. Measured (see
//! docs/findings.md): gravity reads -9.8 on Y, yaw left is negative gyro Y, pitch up is positive gyro X and tilting to the
//! left shoulder is negative gyro Z. That is D = B rotated 180 degrees about X, where B is the OpenVR/OpenGL body frame
//! (x right, y up, z back), so a vector converts as (x, -y, -z). The filter works in B and its world frame is also x right,
//! y up, z back, with yaw zero at start.
//!
//! The filter is a Mahony-style complementary filter: integrate the gyro, and correct pitch and roll by nudging the
//! estimated "up" toward the accelerometer's. The IMU stream also carries an uncalibrated magnetometer (record type 4,
//! not used here; see docs/findings.md), so yaw currently drifts with the gyro bias; the bias is estimated whenever
//! the head is still.

use std::io::Read;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type Quat = [f64; 4]; // w, x, y, z

const G: f64 = 9.80665;
const RECORD: usize = 134;
const MAGIC: [u8; 6] = [0x28, 0x36, 0x00, 0x00, 0x00, 0x80];
const TYPE_IMU: u32 = 0x0B;

/// Latest orientation, shared with the thread that talks to the driver.
#[derive(Clone, Copy, Default)]
pub struct PoseState {
    pub q: [f32; 4], // w, x, y, z; world-from-body
    pub omega: [f32; 3], // angular velocity in the world frame, rad/s
    pub timestamp_ns: u64,
    pub host_ns: u64, // CLOCK_MONOTONIC when the sample reached the host, the clock the driver measures pose age against
    pub valid: bool,
}

/// CLOCK_MONOTONIC in nanoseconds (std's Instant does not expose it).
pub fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

fn qnorm(q: Quat) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n == 0.0 { [1.0, 0.0, 0.0, 0.0] } else { [q[0] / n, q[1] / n, q[2] / n, q[3] / n] }
}

/// Rotate a world-frame vector into the body frame: q* v q.
fn world_to_body(q: Quat, v: [f64; 3]) -> [f64; 3] {
    let conj = [q[0], -q[1], -q[2], -q[3]];
    let r = qmul(qmul(conj, [0.0, v[0], v[1], v[2]]), q);
    [r[1], r[2], r[3]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Shortest rotation taking unit vector `u` to unit vector `w`.
fn from_two_vectors(u: [f64; 3], w: [f64; 3]) -> Quat {
    let d = dot(u, w);
    if d < -0.999999 {
        return [0.0, 1.0, 0.0, 0.0]; // 180 degrees about any perpendicular axis; the exact choice only sets yaw
    }
    let c = cross(u, w);
    qnorm([1.0 + d, c[0], c[1], c[2]])
}

pub struct Fusion {
    pub q: Quat,
    bias: [f64; 3], // gyro bias in the body frame, rad/s
    last_ts: Option<u64>,
    initialised: bool,
    still_s: f64,
    kp: f64,
    omega_body: [f64; 3], // bias-corrected gyro, body frame
}

impl Default for Fusion {
    fn default() -> Self {
        Fusion { q: [1.0, 0.0, 0.0, 0.0], bias: [0.0; 3], last_ts: None, initialised: false, still_s: 0.0, kp: 2.0, omega_body: [0.0; 3] }
    }
}

impl Fusion {
    /// Feed one sample in the glasses' sensor frame (gyro rad/s, accel m/s^2).
    pub fn update(&mut self, ts_ns: u64, gyro_dev: [f32; 3], accel_dev: [f32; 3]) {
        let raw = [gyro_dev[0] as f64, -(gyro_dev[1] as f64), -(gyro_dev[2] as f64)];
        let acc = [accel_dev[0] as f64, -(accel_dev[1] as f64), -(accel_dev[2] as f64)];
        let dt = match self.last_ts {
            Some(l) if ts_ns > l => ((ts_ns - l) as f64 / 1e9).min(0.05),
            _ => 0.0,
        };
        self.last_ts = Some(ts_ns);

        let an = norm(acc);
        let gravity_ok = (an - G).abs() < 0.2 * G;

        if !self.initialised {
            if gravity_ok {
                // Start with pitch and roll from gravity and yaw zero.
                self.q = from_two_vectors([acc[0] / an, acc[1] / an, acc[2] / an], [0.0, 1.0, 0.0]);
                self.initialised = true;
            }
            return;
        }

        // Estimate the gyro bias while the head is still.
        let rate = norm([raw[0] - self.bias[0], raw[1] - self.bias[1], raw[2] - self.bias[2]]);
        if rate < 0.03 && (an - G).abs() < 0.3 {
            self.still_s += dt;
            if self.still_s > 0.5 {
                let a = (dt / 1.0).min(1.0);
                for i in 0..3 {
                    self.bias[i] += (raw[i] - self.bias[i]) * a;
                }
            }
        } else {
            self.still_s = 0.0;
        }

        let mut g = [raw[0] - self.bias[0], raw[1] - self.bias[1], raw[2] - self.bias[2]];
        self.omega_body = g;
        if gravity_ok {
            let measured = [acc[0] / an, acc[1] / an, acc[2] / an];
            let estimated = world_to_body(self.q, [0.0, 1.0, 0.0]);
            let e = cross(measured, estimated);
            for i in 0..3 {
                g[i] += self.kp * e[i];
            }
        }
        let dq = qmul(self.q, [0.0, g[0], g[1], g[2]]);
        self.q = qnorm([
            self.q[0] + 0.5 * dt * dq[0],
            self.q[1] + 0.5 * dt * dq[1],
            self.q[2] + 0.5 * dt * dq[2],
            self.q[3] + 0.5 * dt * dq[3],
        ]);
    }

    pub fn ready(&self) -> bool {
        self.initialised
    }

    /// Angular velocity in the world frame (the body-frame gyro rotated by the current orientation).
    pub fn omega_world(&self) -> [f64; 3] {
        let o = self.omega_body;
        let r = qmul(qmul(self.q, [0.0, o[0], o[1], o[2]]), [self.q[0], -self.q[1], -self.q[2], -self.q[3]]);
        [r[1], r[2], r[3]]
    }
}

/// Pull complete IMU records out of `buf`, returning (timestamp ns, gyro, accel) in the sensor frame.
pub fn parse_records(buf: &mut Vec<u8>) -> Vec<(u64, [f32; 3], [f32; 3])> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    loop {
        let found = buf[pos..].windows(MAGIC.len()).position(|w| w == MAGIC);
        let Some(rel) = found else {
            pos = buf.len().saturating_sub(MAGIC.len() - 1).max(pos);
            break;
        };
        let i = pos + rel;
        if buf.len() - i < RECORD {
            pos = i;
            break;
        }
        let rec = &buf[i..i + RECORD];
        pos = i + RECORD;
        let kind = u32::from_le_bytes(rec[30..34].try_into().unwrap());
        if kind != TYPE_IMU {
            continue;
        }
        let f = |o: usize| f32::from_le_bytes(rec[o..o + 4].try_into().unwrap());
        let v = [f(34), f(38), f(42), f(46), f(50), f(54)];
        if v.iter().any(|x| !x.is_finite()) {
            continue;
        }
        let ts = u64::from_le_bytes(rec[14..22].try_into().unwrap());
        out.push((ts, [v[0], v[1], v[2]], [v[3], v[4], v[5]]));
    }
    buf.drain(..pos);
    out
}

/// Debug: replace the IMU with a slow synthetic head sweep so SteamVR's gaze pointer crosses the UI without anyone wearing the
/// glasses. Yaw +-`yaw_amplitude_deg` over a 16 s period and pitch +-12 degrees over 7 s.
pub fn run_sim(pose: Arc<Mutex<PoseState>>, yaw_amplitude_deg: f64, pitch_offset_deg: f64, pitch_amplitude_deg: f64) {
    let start = std::time::Instant::now();
    let mut prev: Quat = [1.0, 0.0, 0.0, 0.0];
    let mut prev_t = 0.0f64;
    loop {
        std::thread::sleep(Duration::from_millis(2));
        let t = start.elapsed().as_secs_f64();
        // Positive yaw is a turn to the left. 16 s period: left extreme at 4 s, right extreme at 12 s.
        let yaw = yaw_amplitude_deg.to_radians() * (2.0 * std::f64::consts::PI * t / 16.0).sin();
        // pitch_offset_deg is a constant tilt (negative looks down); the amplitude adds a slow nod on top.
        let pitch = pitch_offset_deg.to_radians() + pitch_amplitude_deg.to_radians() * (2.0 * std::f64::consts::PI * t / 7.0).sin();
        let qy = [(yaw / 2.0).cos(), 0.0, (yaw / 2.0).sin(), 0.0];
        let qp = [(pitch / 2.0).cos(), (pitch / 2.0).sin(), 0.0, 0.0];
        let q = qmul(qy, qp);
        let dt = (t - prev_t).max(1e-6);
        // world-frame angular velocity from the change in orientation: 2 * vec(q_new * conj(q_old)) / dt
        let d = qmul(q, [prev[0], -prev[1], -prev[2], -prev[3]]);
        let sign = if d[0] < 0.0 { -1.0 } else { 1.0 };
        let w = [2.0 * sign * d[1] / dt, 2.0 * sign * d[2] / dt, 2.0 * sign * d[3] / dt];
        prev = q;
        prev_t = t;
        *pose.lock().unwrap() = PoseState {
            q: [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32],
            omega: [w[0] as f32, w[1] as f32, w[2] as f32],
            timestamp_ns: (t * 1e9) as u64,
            host_ns: monotonic_ns(),
            valid: true,
        };
    }
}

/// Read the IMU forever (reconnecting) and keep `pose` up to date.
pub fn run(pose: Arc<Mutex<PoseState>>) {
    let hosts = ["169.254.1.1:52998", "169.254.2.1:52998"];
    let mut which = 0;
    loop {
        let addr = hosts[which % hosts.len()];
        which += 1;
        let stream = match TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2)) {
            Ok(s) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        println!("IMU connected to {addr}");
        let mut stream = stream;
        let mut fusion = Fusion::default();
        let mut buf: Vec<u8> = Vec::with_capacity(32768);
        let mut chunk = [0u8; 16384];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
            let arrived = monotonic_ns();
            for (ts, g, a) in parse_records(&mut buf) {
                fusion.update(ts, g, a);
                if fusion.ready() {
                    let (q, w) = (fusion.q, fusion.omega_world());
                    *pose.lock().unwrap() = PoseState {
                        q: [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32],
                        omega: [w[0] as f32, w[1] as f32, w[2] as f32],
                        timestamp_ns: ts,
                        host_ns: arrived,
                        valid: true,
                    };
                }
            }
        }
        println!("IMU disconnected");
        pose.lock().unwrap().valid = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Angle in degrees between two quaternions.
    fn angle_between(a: Quat, b: Quat) -> f64 {
        let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs().min(1.0);
        2.0 * d.acos().to_degrees()
    }

    /// Convert a body-frame vector to the glasses' sensor frame.
    fn to_dev(v: [f64; 3]) -> [f32; 3] {
        [v[0] as f32, -v[1] as f32, -v[2] as f32]
    }

    /// Rotate about a body axis at constant `rate` for `deg` degrees, 1 kHz, after 3 s of rest. Returns the filter.
    fn rotate(axis: usize, rate: f64, deg: f64, bias: [f64; 3]) -> Fusion {
        let mut f = Fusion::default();
        let mut ts = 1_000_000_000u64;
        let rest = |f: &mut Fusion, ts: &mut u64, n: usize| {
            for _ in 0..n {
                let g = [bias[0], bias[1], bias[2]];
                f.update(*ts, to_dev(g), to_dev([0.0, G, 0.0]));
                *ts += 1_000_000;
            }
        };
        rest(&mut f, &mut ts, 3000);
        let total = deg.to_radians();
        let steps = (total / rate * 1000.0) as usize;
        let mut theta = 0.0;
        for _ in 0..steps {
            theta += rate / 1000.0;
            let mut w = [bias[0], bias[1], bias[2]];
            w[axis] += rate;
            // accelerometer reading (body frame) for a body rotated by theta about `axis`: R^T * (0, g, 0)
            let (s, c) = (theta.sin(), theta.cos());
            let up = match axis {
                0 => [0.0, c * G, -s * G],
                1 => [0.0, G, 0.0],
                _ => [s * G, c * G, 0.0],
            };
            f.update(ts, to_dev(w), to_dev(up));
            ts += 1_000_000;
        }
        f
    }

    #[test]
    fn still_stays_put_and_learns_bias() {
        let f = rotate(0, 0.4, 0.0001, [0.008, -0.002, 0.001]);
        assert!(angle_between(f.q, [1.0, 0.0, 0.0, 0.0]) < 0.5, "orientation drifted while still");
    }

    #[test]
    fn yaw_left_is_positive_about_up() {
        // Yaw left is negative gyro Y in the sensor frame, i.e. +y rotation in the body frame.
        let f = rotate(1, 0.5, 45.0, [0.0; 3]);
        let h = 22.5f64.to_radians();
        assert!(angle_between(f.q, [h.cos(), 0.0, h.sin(), 0.0]) < 1.5, "q = {:?}", f.q);
    }

    #[test]
    fn pitch_up_is_positive_about_x() {
        let f = rotate(0, 0.4, 30.0, [0.0; 3]);
        let h = 15.0f64.to_radians();
        assert!(angle_between(f.q, [h.cos(), h.sin(), 0.0, 0.0]) < 1.5, "q = {:?}", f.q);
    }

    #[test]
    fn roll_left_is_positive_about_z() {
        let f = rotate(2, 0.4, 30.0, [0.0; 3]);
        let h = 15.0f64.to_radians();
        assert!(angle_between(f.q, [h.cos(), 0.0, 0.0, h.sin()]) < 1.5, "q = {:?}", f.q);
    }

    #[test]
    fn yaw_with_bias_still_ends_up_close() {
        let f = rotate(1, 0.5, 45.0, [0.008, -0.002, 0.001]);
        let h = 22.5f64.to_radians();
        assert!(angle_between(f.q, [h.cos(), 0.0, h.sin(), 0.0]) < 2.5, "q = {:?}", f.q);
    }

    #[test]
    fn parses_a_real_record_layout() {
        let mut rec = vec![0u8; RECORD];
        rec[..6].copy_from_slice(&MAGIC);
        rec[6] = 0x28; // the varying header bytes must not matter
        rec[7] = 0xbe;
        rec[14..22].copy_from_slice(&123u64.to_le_bytes());
        rec[30..34].copy_from_slice(&TYPE_IMU.to_le_bytes());
        for (i, v) in [0.1f32, 0.2, 0.3, 1.0, -9.7, 0.5].iter().enumerate() {
            rec[34 + 4 * i..38 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        let mut buf = rec.clone();
        buf.extend_from_slice(&rec[..50]); // a partial record is kept for next time
        let out = parse_records(&mut buf);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 123);
        assert!((out[0].2[1] + 9.7).abs() < 1e-6);
        assert_eq!(buf.len(), 50);
    }
}

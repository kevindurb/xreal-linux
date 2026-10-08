//! Magnetometer calibration for `--mag-yaw`: `--mag-calibrate` collects field vectors while the wearer turns the glasses through many
//! directions, fits an offset and a per-axis scale, and saves them for this unit; `--mag-report` measures what the correction buys.
//! The fit models a hard-iron offset and a diagonal soft-iron scale only. None of it has been validated on real magnetometer data.

use crate::glasses::SharedCalibration;
use crate::tracking::{self, Fusion, Sample};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Offset and per-axis scale in the sensor frame: a calibrated field is `(raw - offset) * scale`, on a sphere of radius `radius` (microtesla).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagCalibration {
    pub offset: [f64; 3],
    pub scale: [f64; 3],
    pub radius: f64,
}

impl MagCalibration {
    pub fn apply(&self, m: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|i| (m[i] - self.offset[i]) * self.scale[i])
    }

    fn to_json(self) -> Value {
        json!({"offset": self.offset, "scale": self.scale, "radius": self.radius})
    }

    fn from_json(v: &Value) -> Option<MagCalibration> {
        let three = |key: &str| -> Option<[f64; 3]> {
            let a = v[key].as_array()?;
            Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
        };
        Some(MagCalibration { offset: three("offset")?, scale: three("scale")?, radius: v["radius"].as_f64()? })
    }

    /// Saved per unit under the presenter's cache directory, in a file named by a hash so the serial number never appears.
    fn path(unit_id: u64) -> Option<PathBuf> {
        Some(crate::glasses::cache_dir()?.join(format!("mag-{unit_id:016x}.json")))
    }

    pub fn save(&self, unit_id: u64) -> std::io::Result<PathBuf> {
        let path = Self::path(unit_id).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory"))?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, self.to_json().to_string())?;
        Ok(path)
    }

    pub fn load(unit_id: u64) -> Option<MagCalibration> {
        Self::from_json(&serde_json::from_str(&std::fs::read_to_string(Self::path(unit_id)?).ok()?).ok()?)
    }
}

/// Solve the N x N system `a x = b` by Gaussian elimination with partial pivoting.
pub fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let pivot = (col..N).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..N {
            let f = a[row][col] / a[col][col];
            for k in col..N {
                a[row][k] -= f * a[col][k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = [0.0; N];
    for row in (0..N).rev() {
        let tail: f64 = (row + 1..N).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    Some(x)
}

/// What the collector knows so far.
#[derive(Debug)]
pub struct Progress {
    pub samples: usize,
    pub bins_visited: usize,
    pub fit: Option<MagCalibration>,
    /// RMS distance of the calibrated samples from the fitted sphere, as a fraction of its radius.
    pub residual: f64,
}

impl Progress {
    pub const BINS: usize = 26;
    const MIN_SAMPLES: usize = 1000;
    /// Mean half-range of the samples below which they are just the sensor's noise around one resting value.
    const MIN_HALF_RANGE_UT: f64 = 10.0;
    const MIN_BINS: usize = 21;
    const MAX_RESIDUAL: f64 = 0.10;

    /// Enough coverage, enough samples and a sphere-like result, so the fit is worth saving.
    pub fn good_enough(&self) -> bool {
        let Some(f) = &self.fit else { return false };
        self.samples >= Self::MIN_SAMPLES
            && self.bins_visited >= Self::MIN_BINS
            && self.residual <= Self::MAX_RESIDUAL
            && (10.0..100.0).contains(&f.radius)
            && f.scale.iter().all(|s| (0.5..2.0).contains(s))
    }
}

/// Keeps a thinned copy of the field samples (the sensor reports at several hundred hertz) and fits them on request.
#[derive(Default)]
pub struct Calibrator {
    points: Vec<[f64; 3]>,
    seen: u64,
}

impl Calibrator {
    const KEEP_EVERY: u64 = 8;
    const MAX_POINTS: usize = 20_000;

    pub fn add(&mut self, m: [f64; 3]) {
        self.seen += 1;
        if self.seen % Self::KEEP_EVERY == 0 && self.points.len() < Self::MAX_POINTS {
            self.points.push(m);
        }
    }

    fn range(&self) -> Option<([f64; 3], f64)> {
        let first = self.points.first()?;
        let (mut lo, mut hi) = (*first, *first);
        for p in &self.points {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        let centre = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
        let half = (0..3).map(|i| (hi[i] - lo[i]) / 2.0).sum::<f64>() / 3.0;
        (half > 1e-6).then_some((centre, half))
    }

    /// How many of the 26 directions around the sample cloud (each axis negative, near zero or positive, not all near zero) have a sample.
    fn bins_visited(&self, centre: [f64; 3]) -> usize {
        let mut seen = [false; 27];
        for p in &self.points {
            let d = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if n < 1e-9 {
                continue;
            }
            let s = d.map(|c| match c / n {
                v if v > 0.38 => 2,
                v if v < -0.38 => 0,
                _ => 1,
            });
            seen[s[0] * 9 + s[1] * 3 + s[2]] = true;
        }
        seen[13] = false; // all three near zero is not a direction
        seen.iter().filter(|&&b| b).count()
    }

    pub fn progress(&self) -> Progress {
        let Some((c0, r0)) = self.range() else { return Progress { samples: 0, bins_visited: 0, fit: None, residual: 1.0 } };
        let bins_visited = if r0 < Progress::MIN_HALF_RANGE_UT { 0 } else { self.bins_visited(c0) }; // a cloud of sensor noise has every direction
        let fit = self.fit(c0, r0);
        let residual = fit.map_or(1.0, |f| {
            let sum: f64 = self.points.iter().map(|&p| {
                let c = f.apply(p);
                let r = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
                ((r - f.radius) / f.radius).powi(2)
            }).sum();
            (sum / self.points.len() as f64).sqrt()
        });
        Progress { samples: self.points.len(), bins_visited, fit, residual }
    }

    /// Least-squares fit of `a x^2 + b y^2 + c z^2 + d x + e y + f z = 1` in coordinates scaled to the cloud, then read off the ellipsoid.
    fn fit(&self, c0: [f64; 3], r0: f64) -> Option<MagCalibration> {
        let mut ata = [[0.0; 6]; 6];
        let mut atb = [0.0; 6];
        for p in &self.points {
            let u = [0, 1, 2].map(|i| (p[i] - c0[i]) / r0);
            let phi = [u[0] * u[0], u[1] * u[1], u[2] * u[2], u[0], u[1], u[2]];
            for i in 0..6 {
                for j in 0..6 {
                    ata[i][j] += phi[i] * phi[j];
                }
                atb[i] += phi[i];
            }
        }
        let x = solve(ata, atb)?;
        let (a, d) = ([x[0], x[1], x[2]], [x[3], x[4], x[5]]);
        if a.iter().any(|&v| v <= 1e-9) {
            return None;
        }
        let centre_u = [0, 1, 2].map(|i| -d[i] / (2.0 * a[i]));
        let k = 1.0 + (0..3).map(|i| a[i] * centre_u[i] * centre_u[i]).sum::<f64>();
        if k <= 0.0 {
            return None;
        }
        let radii = [0, 1, 2].map(|i| r0 * (k / a[i]).sqrt());
        let radius = (radii[0] * radii[1] * radii[2]).cbrt();
        Some(MagCalibration {
            offset: [0, 1, 2].map(|i| c0[i] + r0 * centre_u[i]),
            scale: [0, 1, 2].map(|i| radius / radii[i]),
            radius,
        })
    }
}

fn wait_for_unit(calibration: &SharedCalibration) -> Option<u64> {
    for _ in 0..150 {
        if let Some(c) = calibration.lock().unwrap().as_ref() {
            return Some(c.unit_id);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("no config from the glasses within 15 s (it identifies the unit the calibration is saved for)");
    None
}

/// `--mag-calibrate`: print progress until the fit is good enough, save it and return the exit code.
pub fn run_calibrate(calibration: SharedCalibration) -> i32 {
    let Some(unit_id) = wait_for_unit(&calibration) else { return 1 };
    println!("Turn the glasses slowly through every direction: look up, down, left, right, tilt each shoulder, a full turn each way. Ctrl-C quits.");
    let mut cal = Calibrator::default();
    let mut last_print = Instant::now();
    let mut result = None;
    tracking::stream_samples(|s| {
        let Sample::Mag { field, .. } = s else { return true };
        cal.add(field.map(f64::from));
        if last_print.elapsed() < Duration::from_millis(500) {
            return true;
        }
        last_print = Instant::now();
        let p = cal.progress();
        match &p.fit {
            Some(f) => print!("\r{} samples, directions {}/{}, centre ({:.1}, {:.1}, {:.1}) uT, radius {:.1} uT, scale ({:.2}, {:.2}, {:.2}), residual {:.1}%   ",
                p.samples, p.bins_visited, Progress::BINS, f.offset[0], f.offset[1], f.offset[2], f.radius, f.scale[0], f.scale[1], f.scale[2], p.residual * 100.0),
            None => print!("\r{} samples, directions {}/{}, no fit yet   ", p.samples, p.bins_visited, Progress::BINS),
        }
        let _ = std::io::Write::flush(&mut std::io::stdout());
        if p.good_enough() {
            result = p.fit;
            return false;
        }
        true
    });
    println!();
    let Some(fit) = result else { return 1 };
    match fit.save(unit_id) {
        Ok(path) => {
            println!("saved {} (offset {:?}, scale {:?}); `--mag-yaw` now uses it", path.display(), fit.offset, fit.scale);
            0
        }
        Err(e) => {
            eprintln!("could not save the calibration: {e}");
            1
        }
    }
}

/// Yaw about the world's up axis (degrees) of a world-from-body quaternion.
pub fn yaw_deg(q: [f64; 4]) -> f64 {
    let [w, x, y, z] = q;
    (2.0 * (x * z + w * y)).atan2(1.0 - 2.0 * (x * x + y * y)).to_degrees()
}

/// Signed difference `b - a` of two angles in degrees, wrapped to (-180, 180].
pub fn angle_diff_deg(a: f64, b: f64) -> f64 {
    let d = (b - a) % 360.0;
    if d > 180.0 { d - 360.0 } else if d <= -180.0 { d + 360.0 } else { d }
}

/// `--mag-report`: hold the glasses still, run the filter with and without the saved correction on the same samples, and print the yaw drift of each.
pub fn run_report(calibration: SharedCalibration, use_matrices: bool, seconds: f64) -> i32 {
    let Some(unit_id) = wait_for_unit(&calibration) else { return 1 };
    let Some(mag_cal) = MagCalibration::load(unit_id) else {
        eprintln!("no saved magnetometer calibration for this unit: run --mag-calibrate first");
        return 1;
    };
    let matrices = calibration.lock().unwrap().as_ref().filter(|_| use_matrices).map(|c| (c.gyro_matrix, c.accel_matrix));
    println!("Put the glasses on a table and do not touch them for {seconds:.0} s (after a 5 s settle).");
    let (mut plain, mut corrected) = (Fusion::default(), Fusion::default());
    corrected.enable_mag_yaw_calibrated(mag_cal);
    let (mut t0_ns, mut start, mut last) = (None::<u64>, None::<[f64; 2]>, [0.0f64; 2]);
    let (mut rate_sq, mut n) = (0.0f64, 0u64);
    let mut next_print = 0.0;
    tracking::stream_samples(|s| {
        match s {
            Sample::Mag { ts, field } => corrected.update_mag(ts, field),
            Sample::Imu { ts, gyro, accel } => {
                let (g, a) = match &matrices {
                    Some((gm, am)) => (tracking::apply_matrix(gm, gyro), tracking::apply_matrix(am, accel)),
                    None => (gyro, accel),
                };
                plain.update(ts, g, a);
                corrected.update(ts, g, a);
                if !(plain.ready() && corrected.ready()) {
                    return true;
                }
                let t = (ts - *t0_ns.get_or_insert(ts)) as f64 / 1e9;
                if t < 5.0 {
                    return true;
                }
                let yaws = [yaw_deg(plain.q), yaw_deg(corrected.q)];
                start.get_or_insert(yaws);
                last = yaws;
                rate_sq += (g[0] as f64).powi(2) + (g[1] as f64).powi(2) + (g[2] as f64).powi(2);
                n += 1;
                let held = t - 5.0;
                if held >= next_print {
                    print!("\r{held:.0} s ...   ");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    next_print += 5.0;
                }
                return held < seconds;
            }
        }
        true
    });
    println!();
    if n == 0 {
        eprintln!("no samples");
        return 1;
    }
    let minutes = seconds / 60.0;
    let start = start.unwrap_or(last);
    let d = [angle_diff_deg(start[0], last[0]), angle_diff_deg(start[1], last[1])];
    println!("gyro only:            yaw changed {:+.2} deg over {seconds:.0} s ({:+.2} deg/min)", d[0], d[0] / minutes);
    println!("with the magnetometer: yaw changed {:+.2} deg over {seconds:.0} s ({:+.2} deg/min)", d[1], d[1] / minutes);
    let rms = (rate_sq / n as f64).sqrt();
    if rms > 0.05 {
        println!("warning: the glasses were not still (RMS gyro rate {rms:.3} rad/s), so these numbers are not a fair comparison");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random numbers in [-1, 1) so the tests need no extra crate.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
        }
    }

    /// Points of the field as seen through hard-iron `offset` and per-axis gain `gain`, for body directions covering the sphere.
    fn cloud(offset: [f64; 3], gain: [f64; 3], radius: f64, noise: f64, n: usize, seed: u64) -> Vec<[f64; 3]> {
        let mut rng = Lcg(seed);
        let mut out = Vec::new();
        while out.len() < n {
            let v = [rng.next(), rng.next(), rng.next()];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if !(0.1..=1.0).contains(&l) {
                continue;
            }
            out.push([0, 1, 2].map(|i| offset[i] + gain[i] * (radius * v[i] / l + noise * rng.next())));
        }
        out
    }

    fn fed(points: &[[f64; 3]]) -> Calibrator {
        let mut c = Calibrator::default();
        for p in points {
            for _ in 0..Calibrator::KEEP_EVERY {
                c.add(*p);
            }
        }
        c
    }

    #[test]
    fn recovers_a_fixed_offset_and_scale_under_noise() {
        let (offset, gain) = ([10.0, -20.0, 5.0], [1.0, 0.6, 1.3]);
        let p = fed(&cloud(offset, gain, 48.0, 0.4, 4000, 7)).progress();
        let f = p.fit.expect("a fit");
        for i in 0..3 {
            assert!((f.offset[i] - offset[i]).abs() < 0.6, "offset {:?}", f.offset);
        }
        // After calibration the three axes must have the same gain: the scales undo the gains.
        let ratio = |i: usize, j: usize| (f.scale[i] * gain[i]) / (f.scale[j] * gain[j]);
        assert!((ratio(0, 1) - 1.0).abs() < 0.03 && (ratio(0, 2) - 1.0).abs() < 0.03, "scale {:?}", f.scale);
        assert!(p.residual < 0.03, "residual {}", p.residual);
        assert!(p.good_enough());
        // The offset applies as `(raw - offset) * scale`: the centre of the cloud maps to the origin.
        let c = f.apply(offset);
        assert!(c.iter().all(|v| v.abs() < 0.8), "{c:?}");
    }

    #[test]
    fn a_cloud_of_noise_is_not_good_enough() {
        let mut rng = Lcg(3);
        let pts: Vec<[f64; 3]> = (0..3000).map(|_| [rng.next() * 40.0, rng.next() * 40.0, rng.next() * 40.0]).collect();
        assert!(!fed(&pts).progress().good_enough());
    }

    #[test]
    fn coverage_counts_the_directions_visited() {
        let mut few = Calibrator::default();
        // Only a ring around the Z axis (a flat turn): the up and down directions are missing.
        for k in 0..2000 {
            let a = k as f64 * 0.05;
            few.add([30.0 * a.cos(), 30.0 * a.sin(), 0.0]);
        }
        let ring = few.progress();
        assert!(ring.bins_visited < 14, "{}", ring.bins_visited);
        assert!(!ring.good_enough());
        let all = fed(&cloud([0.0; 3], [1.0; 3], 40.0, 0.0, 4000, 11)).progress();
        assert_eq!(all.bins_visited, 26);
    }

    #[test]
    fn a_resting_sensor_covers_no_directions() {
        let mut rng = Lcg(9);
        let mut c = Calibrator::default();
        for _ in 0..4000 {
            c.add([-24.5 + 0.5 * rng.next(), 10.5 + 0.5 * rng.next(), -41.7 + 0.5 * rng.next()]);
        }
        let p = c.progress();
        assert_eq!(p.bins_visited, 0);
        assert!(!p.good_enough());
    }

    #[test]
    fn too_few_samples_is_not_good_enough() {
        assert!(!fed(&cloud([0.0; 3], [1.0; 3], 40.0, 0.0, 200, 5)).progress().good_enough());
    }

    #[test]
    fn solve_handles_a_known_system() {
        let x = solve([[2.0, 1.0, 0.0], [1.0, 3.0, 1.0], [0.0, 1.0, 4.0]], [3.0, 5.0, 5.0]).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-9 && (x[1] - 1.0).abs() < 1e-9 && (x[2] - 1.0).abs() < 1e-9, "{x:?}");
        assert!(solve([[1.0, 2.0], [2.0, 4.0]], [1.0, 2.0]).is_none());
    }

    #[test]
    fn a_saved_calibration_reads_back_the_same_and_has_no_serial() {
        let c = MagCalibration { offset: [1.5, -2.5, 3.0], scale: [1.0, 1.1, 0.9], radius: 47.0 };
        assert_eq!(MagCalibration::from_json(&serde_json::from_str(&c.to_json().to_string()).unwrap()), Some(c));
        assert!(!MagCalibration::path(0xabc).unwrap().to_string_lossy().contains("serial"));
        assert!(MagCalibration::from_json(&json!({"offset": [1.0, 2.0]})).is_none());
    }

    #[test]
    fn yaw_and_angle_differences_wrap_properly() {
        let h = 30f64.to_radians() / 2.0;
        assert!((yaw_deg([h.cos(), 0.0, h.sin(), 0.0]) - 30.0).abs() < 1e-9);
        assert!((angle_diff_deg(170.0, -170.0) - 20.0).abs() < 1e-9);
        assert!((angle_diff_deg(-170.0, 170.0) + 20.0).abs() < 1e-9);
    }
}

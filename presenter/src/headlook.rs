//! Head-look: turn the glasses' orientation into input for a game outside SteamVR.
//!
//! This module holds the pure mapping core — the mapping models, the tuning set, the neutral reference and
//! recenter. The service loop (reading the IMU) and the userspace input device are wired in by later tasks.
//! See `openspec/changes/add-headlook-mode`.
//!
//! Frames follow `tracking.rs`: `q` is world-from-body (x right, y up, z back) and `omega` is the world-frame
//! angular velocity in rad/s. A left turn is a positive rotation about up, and looking up is a positive
//! rotation about the body x axis.

use crate::tracking::{parse_records, Fusion, Quat};

fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

fn qconj(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

/// Rotate a body-frame vector into the world frame: `q v q*`.
fn rotate(q: Quat, v: [f64; 3]) -> [f64; 3] {
    let r = qmul(qmul(q, [0.0, v[0], v[1], v[2]]), qconj(q));
    [r[1], r[2], r[3]]
}

/// The mapping model. Rate uses the angular velocity only (tolerant of yaw drift); Stick and Absolute map the
/// head angle from a neutral pose, 1:1 (Absolute) or damped by the curve (Stick).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    Rate,
    Stick,
    Absolute,
}

impl Model {
    /// The models this build supports, for the error message when an unknown one is asked for.
    pub const SUPPORTED: [&'static str; 3] = ["rate", "stick", "absolute"];

    pub fn parse(s: &str) -> Option<Model> {
        match s {
            "rate" => Some(Model::Rate),
            "stick" => Some(Model::Stick),
            "absolute" => Some(Model::Absolute),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Model::Rate => "rate",
            Model::Stick => "stick",
            Model::Absolute => "absolute",
        }
    }

    fn is_position(self) -> bool {
        !matches!(self, Model::Rate)
    }
}

/// The tunable knobs. `axis` output is a normalised gamepad stick (x right, y up, each in -1..1); `mouse` output
/// is a relative pointer delta for one tick.
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    pub model: Model,
    /// Angle below which a position model emits nothing (degrees).
    pub deadzone_deg: f64,
    /// Angular rate below which the rate model emits nothing (degrees per second).
    pub deadzone_dps: f64,
    /// Overall gain.
    pub sensitivity: f64,
    /// Position-model response exponent (1.0 is linear; Absolute ignores it).
    pub curve: f64,
    /// Head angle at which a position model reaches full deflection (degrees).
    pub range_deg: f64,
    /// Angular rate at which the rate model reaches full deflection (degrees per second).
    pub rate_full_dps: f64,
    /// Pointer counts per degree of head turn (mouse output).
    pub mouse_counts_per_deg: f64,
    pub invert_x: bool,
    pub invert_y: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            model: Model::Stick,
            deadzone_deg: 2.0,
            deadzone_dps: 3.0,
            sensitivity: 1.0,
            curve: 1.0,
            range_deg: 30.0,
            rate_full_dps: 90.0,
            mouse_counts_per_deg: 20.0,
            invert_x: false,
            invert_y: false,
        }
    }
}

/// The pose that counts as "looking straight ahead". Recenter sets it to the current pose.
#[derive(Clone, Copy, Debug)]
pub struct Neutral {
    q: Quat,
}

impl Neutral {
    pub fn from_pose(q: Quat) -> Self {
        Neutral { q }
    }

    pub fn pose(self) -> Quat {
        self.q
    }
}

impl Default for Neutral {
    fn default() -> Self {
        Neutral { q: [1.0, 0.0, 0.0, 0.0] }
    }
}

/// The mapping output for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Output {
    /// Normalised gamepad stick: x right, y up, each in -1..1.
    pub axis: [f64; 2],
    /// Relative pointer delta (x right, y down), in counts.
    pub mouse: [f64; 2],
    /// True when a position model's raw deflection was beyond its range.
    pub saturated: bool,
}

/// Yaw and pitch of where the head points, in degrees. Yaw is positive to the left, pitch positive upward.
pub fn heading(q: Quat) -> (f64, f64) {
    let f = rotate(q, [0.0, 0.0, -1.0]);
    let yaw = (-f[0]).atan2(-f[2]);
    let pitch = f[1].clamp(-1.0, 1.0).asin();
    (yaw.to_degrees(), pitch.to_degrees())
}

/// Wrap a yaw difference into -180..180 degrees.
fn wrap_deg(d: f64) -> f64 {
    let mut d = d % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

/// Remove a deadzone: values within it become zero, values beyond keep their sign and lose the deadzone.
fn deadzone(v: f64, dz: f64) -> f64 {
    if v.abs() <= dz {
        0.0
    } else {
        v - dz * v.signum()
    }
}

/// One position-model axis from an angle: deadzone, scale to the half-range, clamp, then curve.
fn position_axis(a: f64, t: &Tuning) -> (f64, bool) {
    let a = deadzone(a, t.deadzone_deg);
    let raw = a / t.range_deg;
    let clamped = raw.clamp(-1.0, 1.0);
    let curve = if t.model == Model::Absolute { 1.0 } else { t.curve };
    let shaped = clamped.signum() * clamped.abs().powf(curve);
    (shaped, (raw.abs() - 1.0) > 1e-9)
}

/// Map one pose to input. `omega` is the world-frame angular velocity (rad/s) and `dt` the tick length in
/// seconds, used only for the mouse output.
pub fn map(t: &Tuning, neutral: &Neutral, q: Quat, omega: [f32; 3], dt: f64) -> Output {
    let (yaw, pitch) = heading(q);
    let (ny, np) = heading(neutral.q);
    let dyaw = wrap_deg(yaw - ny);
    let dpitch = pitch - np;

    let rate_yaw = (omega[1] as f64).to_degrees();
    let rate_pitch = (omega[0] as f64).to_degrees();

    let mut saturated = false;
    let (mut ax, mut ay) = if t.model.is_position() {
        let (sx, sx_sat) = position_axis(dyaw, t);
        let (sy, sy_sat) = position_axis(dpitch, t);
        saturated = sx_sat || sy_sat;
        (-sx, sy)
    } else {
        // Rate: the axis follows the angular velocity, so a still head emits nothing.
        let ry = deadzone(rate_yaw, t.deadzone_dps);
        let rp = deadzone(rate_pitch, t.deadzone_dps);
        (-ry / t.rate_full_dps, rp / t.rate_full_dps)
    };
    ax *= t.sensitivity;
    ay *= t.sensitivity;
    if t.invert_x {
        ax = -ax;
    }
    if t.invert_y {
        ay = -ay;
    }

    // Mouse is always a rate device: integrate the angular velocity over the tick.
    let mut mx = -t.mouse_counts_per_deg * rate_yaw * dt;
    let mut my = -t.mouse_counts_per_deg * rate_pitch * dt;
    if t.invert_x {
        mx = -mx;
    }
    if t.invert_y {
        my = -my;
    }

    Output {
        axis: [ax.clamp(-1.0, 1.0), ay.clamp(-1.0, 1.0)],
        mouse: [mx, my],
        saturated,
    }
}

/// Summary of one capture run through the fusion and the mapping core. Used by the offline simulator and its
/// tests, which need no glasses.
#[derive(Clone, Copy, Debug)]
pub struct SimResult {
    pub samples: usize,
    pub duration_s: f64,
    pub axis_min: [f64; 2],
    pub axis_max: [f64; 2],
    pub saturated: usize,
    /// Change in the fused heading yaw over the whole capture, in degrees per minute (the drift a position model
    /// would track, and the rate a rate model would command).
    pub yaw_drift_deg_per_min: f64,
    pub mean_rate_dps: [f64; 2],
    /// Fraction of samples whose rate axis was exactly zero (the deadzone held a still head).
    pub zero_fraction: f64,
}

/// Run a recorded IMU capture (a raw dump of the 134-byte stream) through the same fusion the presenter uses and
/// the mapping core, and summarise the result. Returns `None` if the file is missing or holds no IMU records.
/// The neutral reference is taken from the first valid pose, so a position model starts centred.
pub fn simulate_capture(path: &std::path::Path, tuning: &Tuning) -> Option<SimResult> {
    let bytes = std::fs::read(path).ok()?;
    let mut buf = bytes;
    let mut fusion = Fusion::default();
    let mut neutral: Option<Neutral> = None;
    let (mut first_yaw, mut last_yaw) = (None, 0.0f64);
    let (mut t0, mut t1) = (None, 0u64);
    let mut prev_ts: Option<u64> = None;
    let mut axis_min = [f64::INFINITY; 2];
    let mut axis_max = [f64::NEG_INFINITY; 2];
    let (mut saturated, mut zero, mut n) = (0usize, 0usize, 0usize);
    let mut sum_rate = [0.0f64; 2];

    for (ts, gyro, accel) in parse_records(&mut buf) {
        fusion.update(ts, gyro, accel);
        if !fusion.ready() {
            continue;
        }
        let q = fusion.q;
        let omega = fusion.omega_world();
        if neutral.is_none() {
            neutral = Some(Neutral::from_pose(q));
            first_yaw = Some(heading(q).0);
        }
        let dt = match prev_ts {
            Some(p) if ts > p => ((ts - p) as f64 / 1e9).min(0.1),
            _ => 0.01,
        };
        prev_ts = Some(ts);
        let o = map(
            tuning,
            neutral.as_ref().unwrap(),
            q,
            [omega[0] as f32, omega[1] as f32, omega[2] as f32],
            dt,
        );
        axis_min[0] = axis_min[0].min(o.axis[0]);
        axis_min[1] = axis_min[1].min(o.axis[1]);
        axis_max[0] = axis_max[0].max(o.axis[0]);
        axis_max[1] = axis_max[1].max(o.axis[1]);
        if o.saturated {
            saturated += 1;
        }
        if o.axis == [0.0, 0.0] {
            zero += 1;
        }
        sum_rate[0] += (omega[0] as f64).to_degrees();
        sum_rate[1] += (omega[1] as f64).to_degrees();
        last_yaw = heading(q).0;
        if t0.is_none() {
            t0 = Some(ts);
        }
        t1 = ts;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let duration_s = (t1 - t0.unwrap_or(t1)) as f64 / 1e9;
    let drift = if duration_s > 0.0 {
        wrap_deg(last_yaw - first_yaw.unwrap_or(0.0)) / duration_s * 60.0
    } else {
        0.0
    };
    Some(SimResult {
        samples: n,
        duration_s,
        axis_min,
        axis_max,
        saturated,
        yaw_drift_deg_per_min: drift,
        mean_rate_dps: [sum_rate[0] / n as f64, sum_rate[1] / n as f64],
        zero_fraction: zero as f64 / n as f64,
    })
}

/// Print a one-line trace per model for one capture. The offline simulator's user-facing output.
pub fn print_trace(path: &std::path::Path) {
    println!("capture: {}", path.display());
    for model in [Model::Rate, Model::Stick, Model::Absolute] {
        let t = Tuning { model, ..Tuning::default() };
        match simulate_capture(path, &t) {
            Some(r) => println!(
                "  {:8} samples {:6}  {:.1}s  axis.x [{:+.2},{:+.2}] axis.y [{:+.2},{:+.2}]  sat {:<6} zero {:.0}%  yaw {:+.2} deg/min  rate [{:+.2},{:+.2}] dps",
                model.name(),
                r.samples,
                r.duration_s,
                r.axis_min[0],
                r.axis_max[0],
                r.axis_min[1],
                r.axis_max[1],
                r.saturated,
                r.zero_fraction * 100.0,
                r.yaw_drift_deg_per_min,
                r.mean_rate_dps[0],
                r.mean_rate_dps[1],
            ),
            None => println!("  {:8} (no data)", model.name()),
        }
    }
}

/// The capture files the offline simulator and its tests use, resolved relative to this crate.
pub fn capture_files() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../captures");
    ["imu_yaw.bin", "imu_move.bin", "xreal_52998_still.bin"]
        .iter()
        .map(|n| root.join(n))
        .collect()
}

// ---- the service ----------------------------------------------------------------------------------------------

/// Options for the `headlook` subcommand.
#[derive(Clone, Debug)]
pub struct Options {
    pub tuning: Tuning,
    pub device: crate::uinput::DeviceKind,
    pub recenter_on_start: bool,
    pub capture: Option<std::path::PathBuf>,
    pub replay: Option<std::path::PathBuf>,
    /// `--service`: the unit's mode, which also reads `headlook.env` (the systemd `EnvironmentFile`).
    pub service: bool,
}

/// The arguments the `xreal-headlook.service` unit supplies through `headlook.env` (systemd injects the file into the environment).
/// Device defaults to `mouse` and model to `stick` in `parse_args`, so only overrides appear here. A foreground run ignores the file.
pub fn env_service_args(get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let clean = |s: String| s.trim().to_string();
    let mut a = vec![];
    if let Some(v) = get("XREAL_HEADLOOK_DEVICE").map(clean).filter(|s| !s.is_empty()) {
        a.push("--device".to_string());
        a.push(v);
    }
    if let Some(v) = get("XREAL_HEADLOOK_MODEL").map(clean).filter(|s| !s.is_empty()) {
        a.push("--model".to_string());
        a.push(v);
    }
    // Anything else (the tuning knobs) as a free string, like the service's XREAL_EXTRA_ARGS.
    if let Some(extra) = get("XREAL_HEADLOOK_ARGS") {
        a.extend(extra.split_whitespace().map(str::to_string));
    }
    a
}

/// Merge the environment file's arguments (when running as the service) with the command line, so a command-line flag overrides the
/// file. `--service` itself is kept so `parse_args` records it.
pub fn service_args(service: bool, args: &[&str], get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut merged = if service { env_service_args(get) } else { vec![] };
    merged.extend(args.iter().map(|s| s.to_string()));
    merged
}

fn num(s: &str, flag: &str) -> Result<f64, String> {
    s.parse().map_err(|_| format!("{flag} needs a number, got '{s}'"))
}

/// Parse the `headlook` arguments (the ones after the subcommand name).
pub fn parse_args(args: &[&str]) -> Result<Options, String> {
    use crate::uinput::DeviceKind;
    let mut tuning = Tuning::default();
    let mut device = DeviceKind::Mouse;
    let mut recenter_on_start = true;
    let mut capture = None;
    let mut replay = None;
    let mut service = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        let val = |i: usize| args.get(i + 1).copied().ok_or_else(|| format!("{a} needs a value"));
        match a {
            "--model" => {
                let v = val(i)?;
                tuning.model = Model::parse(v).ok_or_else(|| format!("unknown model '{v}'; supported models: {}", Model::SUPPORTED.join(", ")))?;
                i += 2;
            }
            "--device" => {
                let v = val(i)?;
                device = match v {
                    "gamepad" => DeviceKind::Gamepad,
                    "mouse" => DeviceKind::Mouse,
                    _ => return Err(format!("unknown device '{v}'; use gamepad or mouse")),
                };
                i += 2;
            }
            "--deadzone" => { tuning.deadzone_deg = num(val(i)?, a)?; i += 2; }
            "--deadzone-rate" => { tuning.deadzone_dps = num(val(i)?, a)?; i += 2; }
            "--sensitivity" => { tuning.sensitivity = num(val(i)?, a)?; i += 2; }
            "--curve" => { tuning.curve = num(val(i)?, a)?; i += 2; }
            "--range" => { tuning.range_deg = num(val(i)?, a)?; i += 2; }
            "--rate-full" => { tuning.rate_full_dps = num(val(i)?, a)?; i += 2; }
            "--mouse-gain" => { tuning.mouse_counts_per_deg = num(val(i)?, a)?; i += 2; }
            "--invert-x" => { tuning.invert_x = true; i += 1; }
            "--invert-y" => { tuning.invert_y = true; i += 1; }
            "--no-recenter" => { recenter_on_start = false; i += 1; }
            "--service" => { service = true; i += 1; }
            "--capture" => { capture = Some(std::path::PathBuf::from(val(i)?)); i += 2; }
            "--replay" => { replay = Some(std::path::PathBuf::from(val(i)?)); i += 2; }
            other => return Err(format!("unknown option '{other}'")),
        }
    }
    Ok(Options { tuning, device, recenter_on_start, capture, replay, service })
}

/// One event to write to the device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Abs(u16, i32),
    Rel(u16, i32),
    Syn,
}

/// The events for one mapped output: a gamepad stick or a relative pointer, ending with the frame's SYN.
pub fn events(kind: crate::uinput::DeviceKind, o: &Output) -> Vec<Event> {
    use crate::uinput::DeviceKind;
    match kind {
        // The gamepad drives its left stick (SDL maps ABS_X/ABS_Y to leftx/lefty for an unknown pad; it drops the
        // right stick). See `docs/findings.md`, task 5.1.
        DeviceKind::Gamepad => vec![
            Event::Abs(crate::uinput::ABS_X, crate::uinput::gamepad_value(o.axis[0])),
            Event::Abs(crate::uinput::ABS_Y, crate::uinput::gamepad_value(-o.axis[1])),
            Event::Syn,
        ],
        DeviceKind::Mouse => vec![
            Event::Rel(0, crate::uinput::mouse_value(o.mouse[0])),
            Event::Rel(1, crate::uinput::mouse_value(o.mouse[1])),
            Event::Syn,
        ],
    }
}

/// A running head-look session: the neutral reference, the last tick time, and the mapping step. Kept separate from
/// the device and the network so a test can drive it with synthetic poses.
pub struct Session {
    tuning: Tuning,
    kind: crate::uinput::DeviceKind,
    neutral: Neutral,
    have_neutral: bool,
    recenter_on_start: bool,
    last_ns: u64,
    /// Fractional mouse counts not yet emitted. A slow turn at the capture's ~1 kHz yields far less than one
    /// count per tick; rounding each tick would discard it, so the remainder is carried to the next tick.
    mouse_accum: [f64; 2],
}

impl Session {
    pub fn new(tuning: Tuning, kind: crate::uinput::DeviceKind, recenter_on_start: bool) -> Self {
        Session {
            tuning,
            kind,
            neutral: Neutral::default(),
            have_neutral: false,
            recenter_on_start,
            last_ns: 0,
            mouse_accum: [0.0; 2],
        }
    }

    /// Make the current pose look straight ahead.
    pub fn recenter(&mut self, q: Quat) {
        self.neutral = Neutral::from_pose(q);
        self.have_neutral = true;
    }

    /// Turn one pose into device events, or `None` when the pose is not valid (nothing is emitted).
    pub fn tick(&mut self, p: &crate::tracking::PoseState, now_ns: u64) -> Option<Vec<Event>> {
        if !p.valid {
            return None;
        }
        let q: Quat = [p.q[0] as f64, p.q[1] as f64, p.q[2] as f64, p.q[3] as f64];
        if !self.have_neutral {
            if self.recenter_on_start {
                self.recenter(q);
            } else {
                self.have_neutral = true;
            }
        }
        let dt = if self.last_ns == 0 {
            0.005
        } else {
            ((now_ns.saturating_sub(self.last_ns)) as f64 / 1e9).clamp(0.0, 0.05)
        };
        self.last_ns = now_ns;
        let o = map(&self.tuning, &self.neutral, q, p.omega, dt);
        Some(self.emit_events(&o))
    }

    /// Turn one mapped output into device events. The mouse path accumulates fractional counts across ticks so a
    /// high-rate, low-count stream still moves the pointer.
    fn emit_events(&mut self, o: &Output) -> Vec<Event> {
        match self.kind {
            crate::uinput::DeviceKind::Mouse => {
                self.mouse_accum[0] += o.mouse[0];
                self.mouse_accum[1] += o.mouse[1];
                let dx = self.mouse_accum[0].trunc();
                let dy = self.mouse_accum[1].trunc();
                self.mouse_accum[0] -= dx;
                self.mouse_accum[1] -= dy;
                vec![
                    Event::Rel(0, crate::uinput::mouse_value(dx)),
                    Event::Rel(1, crate::uinput::mouse_value(dy)),
                    Event::Syn,
                ]
            }
            _ => events(self.kind, o),
        }
    }
}

/// The status line the service prints on start, on a state change and every ten seconds.
pub fn status_line(kind: crate::uinput::DeviceKind, model: Model, imu: bool, recenter_age_s: Option<f64>) -> String {
    use crate::uinput::DeviceKind;
    let (dev, name) = match kind {
        DeviceKind::Gamepad => ("gamepad", crate::uinput::GAMEPAD_NAME),
        DeviceKind::Mouse => ("mouse", crate::uinput::MOUSE_NAME),
    };
    format!(
        "headlook: {dev} '{name}', model {}, IMU {}, last recenter {}",
        model.name(),
        if imu { "connected" } else { "unavailable" },
        recenter_age_s.map(|s| format!("{s:.0}s ago")).unwrap_or_else(|| "not yet".into()),
    )
}

const USAGE: &str = "usage: xreal-presenter headlook [--model rate|stick|absolute] [--device gamepad|mouse] \
[--deadzone DEG] [--deadzone-rate DPS] [--sensitivity X] [--curve N] [--range DEG] [--rate-full DPS] \
[--mouse-gain COUNTS_PER_DEG] [--invert-x] [--invert-y] [--no-recenter] [--capture FILE] [--replay FILE] [--service]
  --service      the unit's mode: read XREAL_HEADLOOK_DEVICE / XREAL_HEADLOOK_MODEL / XREAL_HEADLOOK_ARGS from the environment (headlook.env)";

static RECENTER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_sigusr1(_sig: libc::c_int) {
    RECENTER.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The `headlook` subcommand. Returns the process exit code. In `--service` mode the options in `headlook.env` are applied first, so
/// the unit can be configured without editing the unit file, and a command-line flag still wins.
pub fn command(args: &[&str]) -> i32 {
    let service = args.iter().any(|a| *a == "--service");
    let merged = service_args(service, args, |k| std::env::var(k).ok());
    let refs: Vec<&str> = merged.iter().map(String::as_str).collect();
    let opts = match parse_args(&refs) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            eprintln!("{USAGE}");
            return 2;
        }
    };
    if let Some(path) = &opts.capture {
        print_trace(path);
        return 0;
    }
    run(opts)
}

fn emit(device: &mut crate::uinput::UinputDevice, e: Event) {
    let _ = match e {
        Event::Abs(axis, v) => device.emit_abs(axis, v),
        Event::Rel(axis, v) => device.emit_rel(axis, v),
        Event::Syn => device.syn(),
    };
}

/// Drive the device from a recorded capture instead of the live IMU: Phase 1's no-wearer check. The capture loops,
/// paced to its own timestamps, so a game sees continuous head motion with no glasses movement.
fn replay_device(opts: &Options, path: &std::path::Path, device: &mut crate::uinput::UinputDevice) -> i32 {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {}: {e}", path.display());
            return 1;
        }
    };
    let mut buf = bytes;
    let records = parse_records(&mut buf);
    if records.is_empty() {
        eprintln!("no IMU records in {}", path.display());
        return 1;
    }
    println!("{}", status_line(opts.device, opts.tuning.model, true, None));
    println!("replaying {} ({} samples, looping)", path.display(), records.len());
    let mut session = Session::new(opts.tuning, opts.device, opts.recenter_on_start);
    loop {
        let mut fusion = Fusion::default();
        let mut prev: Option<u64> = None;
        for (ts, gyro, accel) in &records {
            fusion.update(*ts, *gyro, *accel);
            if !fusion.ready() {
                continue;
            }
            if let Some(p) = prev {
                let gap = (*ts).saturating_sub(p);
                if gap > 0 {
                    std::thread::sleep(std::time::Duration::from_nanos(gap.min(50_000_000)));
                }
            }
            prev = Some(*ts);
            let q = fusion.q;
            let w = fusion.omega_world();
            let p = crate::tracking::PoseState {
                q: [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32],
                omega: [w[0] as f32, w[1] as f32, w[2] as f32],
                timestamp_ns: *ts,
                host_ns: crate::tracking::monotonic_ns(),
                valid: true,
            };
            if let Some(evs) = session.tick(&p, crate::tracking::monotonic_ns()) {
                for e in evs {
                    emit(device, e);
                }
            }
        }
    }
}

fn run(opts: Options) -> i32 {
    use crate::uinput::DeviceKind;
    let device = match opts.device {
        DeviceKind::Gamepad => crate::uinput::UinputDevice::create_gamepad(crate::uinput::GAMEPAD_NAME),
        DeviceKind::Mouse => crate::uinput::UinputDevice::create_mouse(crate::uinput::MOUSE_NAME),
    };
    let mut device = match device {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot create the virtual input device: {e}");
            eprintln!("/dev/uinput must exist and be writable by this user; the session's uaccess ACL usually allows it, so no root is needed.");
            return 1;
        }
    };
    if let Some(path) = opts.replay.clone() {
        return replay_device(&opts, &path, &mut device);
    }
    unsafe { libc::signal(libc::SIGUSR1, on_sigusr1 as extern "C" fn(libc::c_int) as libc::sighandler_t); }

    let pose = std::sync::Arc::new(std::sync::Mutex::new(crate::tracking::PoseState::default()));
    {
        let p = pose.clone();
        let cal: crate::glasses::SharedCalibration = std::sync::Arc::new(std::sync::Mutex::new(None));
        std::thread::spawn(move || crate::tracking::run(p, cal, false));
    }

    let mut session = Session::new(opts.tuning, opts.device, opts.recenter_on_start);
    let started = std::time::Instant::now();
    let mut last_recenter: Option<f64> = None;
    let mut was_valid = false;
    let mut last_status = started;
    let mut latencies: Vec<f64> = Vec::new();
    println!("{}", status_line(opts.device, opts.tuning.model, false, None));
    println!("recenter with: kill -USR1 {}", std::process::id());
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        if RECENTER.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let p = *pose.lock().unwrap();
            if p.valid {
                session.recenter([p.q[0] as f64, p.q[1] as f64, p.q[2] as f64, p.q[3] as f64]);
                last_recenter = Some(started.elapsed().as_secs_f64());
                println!("recentered");
            }
        }
        let now = crate::tracking::monotonic_ns();
        let p = *pose.lock().unwrap();
        if !p.valid {
            if was_valid {
                for e in events(opts.device, &Output::default()) {
                    emit(&mut device, e);
                }
                println!("IMU unavailable; not emitting head motion");
                was_valid = false;
            }
        } else {
            if !was_valid {
                println!("IMU connected");
                was_valid = true;
            }
            if let Some(evs) = session.tick(&p, now) {
                if p.host_ns != 0 {
                    let lat = now.saturating_sub(p.host_ns) as f64 / 1e6;
                    if lat < 1000.0 {
                        latencies.push(lat);
                    }
                }
                for e in evs {
                    emit(&mut device, e);
                }
            }
        }
        if last_status.elapsed() >= std::time::Duration::from_secs(10) {
            let age = last_recenter.map(|s| started.elapsed().as_secs_f64() - s);
            println!("{}", status_line(opts.device, opts.tuning.model, was_valid, age));
            if let Some((p50, p95, n)) = latency_summary(&mut latencies) {
                println!("  IMU-arrival to emit: p50 {p50:.1} ms, p95 {p95:.1} ms over {n} samples");
            }
            last_status = std::time::Instant::now();
        }
    }
}

/// Percentiles (p50, p95) of a latency window in ms, clearing it. `None` when the window is empty.
fn latency_summary(samples: &mut Vec<f64>) -> Option<(f64, f64, usize)> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = samples.len();
    let at = |p: f64| samples[(((n as f64 - 1.0) * p).round() as usize).min(n - 1)];
    let out = (at(0.5), at(0.95), n);
    samples.clear();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world-from-body quaternion for a yaw (left positive) then a pitch (up positive), both in degrees.
    fn pose(yaw_deg: f64, pitch_deg: f64) -> Quat {
        let y = yaw_deg.to_radians() / 2.0;
        let p = pitch_deg.to_radians() / 2.0;
        qmul([y.cos(), 0.0, y.sin(), 0.0], [p.cos(), p.sin(), 0.0, 0.0])
    }

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn heading_reports_left_yaw_and_up_pitch_as_positive() {
        let (y, p) = heading(pose(20.0, 15.0));
        assert!(close(y, 20.0, 0.5), "yaw {y}");
        assert!(close(p, 15.0, 0.5), "pitch {p}");
    }

    #[test]
    fn rate_axis_follows_omega_with_left_positive_then_negated() {
        let t = Tuning { model: Model::Rate, ..Tuning::default() };
        let o = map(&t, &Neutral::default(), pose(0.0, 0.0), [0.0, 1.0, 0.0], 0.01);
        // 1 rad/s yaw-left = 57.30 dps; minus 3 dps deadzone = 54.30; /90 = 0.603, negated for the stick.
        assert!(close(o.axis[0], -0.603, 0.02), "axis.x {}", o.axis[0]);
        let still = map(&t, &Neutral::default(), pose(0.0, 0.0), [0.0, 0.0, 0.0], 0.01);
        assert_eq!(still.axis, [0.0, 0.0]);
    }

    #[test]
    fn rate_deadzone_suppresses_small_bias() {
        let t = Tuning { model: Model::Rate, ..Tuning::default() };
        // 0.01 rad/s = 0.57 dps, below the 3 dps deadzone.
        let o = map(&t, &Neutral::default(), pose(0.0, 0.0), [0.0, 0.01, 0.0], 0.01);
        assert_eq!(o.axis, [0.0, 0.0], "bias leaked through the deadzone");
    }

    #[test]
    fn stick_axis_tracks_heading_and_clamps() {
        let t = Tuning { model: Model::Stick, sensitivity: 1.0, ..Tuning::default() };
        let o = map(&t, &Neutral::default(), pose(10.0, 0.0), [0.0; 3], 0.01);
        // (10 - 2 deadzone) / 30 range = 0.2667, negated for the stick.
        assert!(close(o.axis[0], -0.2667, 0.01), "axis.x {}", o.axis[0]);
        let big = map(&t, &Neutral::default(), pose(60.0, 0.0), [0.0; 3], 0.01);
        assert_eq!(big.axis[0], -1.0);
        assert!(big.saturated);
        let zero = map(&t, &Neutral::default(), pose(0.0, 0.0), [0.0; 3], 0.01);
        assert_eq!(zero.axis, [0.0, 0.0]);
    }

    #[test]
    fn absolute_is_linear_where_stick_curves() {
        let t = Tuning { model: Model::Absolute, curve: 2.0, ..Tuning::default() };
        let o = map(&t, &Neutral::default(), pose(15.0, 0.0), [0.0; 3], 0.01);
        // Absolute ignores the curve: (15 - 2) / 30 = 0.4333.
        assert!(close(o.axis[0], -0.4333, 0.01), "axis.x {}", o.axis[0]);

        let s = Tuning { model: Model::Stick, curve: 2.0, ..Tuning::default() };
        let os = map(&s, &Neutral::default(), pose(15.0, 0.0), [0.0; 3], 0.01);
        assert!(close(os.axis[0].abs(), 0.4333f64.powi(2), 0.01), "curved axis {}", os.axis[0]);
    }

    #[test]
    fn recenter_zeroes_the_output_for_the_current_pose() {
        let t = Tuning::default();
        let turned = pose(25.0, 10.0);
        let before = map(&t, &Neutral::default(), turned, [0.0; 3], 0.01);
        assert!(before.axis[0].abs() > 0.1 && before.axis[1].abs() > 0.1);
        let after = map(&t, &Neutral::from_pose(turned), turned, [0.0; 3], 0.01);
        assert_eq!(after.axis, [0.0, 0.0], "recenter did not zero the output");
    }

    #[test]
    fn sensitivity_scales_the_axis() {
        let base = Tuning { sensitivity: 1.0, ..Tuning::default() };
        let double = Tuning { sensitivity: 2.0, ..Tuning::default() };
        let a = map(&base, &Neutral::default(), pose(10.0, 0.0), [0.0; 3], 0.01);
        let b = map(&double, &Neutral::default(), pose(10.0, 0.0), [0.0; 3], 0.01);
        assert!(close(b.axis[0], a.axis[0] * 2.0, 1e-6));
    }

    #[test]
    fn mouse_delta_is_a_negative_rate_times_dt() {
        let t = Tuning { model: Model::Rate, ..Tuning::default() };
        // 0.5 rad/s yaw-left for 0.02 s = 0.573 deg; * 20 counts/deg = 11.46, negated for the pointer.
        let o = map(&t, &Neutral::default(), pose(0.0, 0.0), [0.0, 0.5, 0.0], 0.02);
        assert!(close(o.mouse[0], -11.46, 0.1), "mouse.x {}", o.mouse[0]);
    }

    #[test]
    fn invert_flags_flip_the_signs() {
        let t = Tuning { invert_x: true, invert_y: true, ..Tuning::default() };
        let o = map(&t, &Neutral::default(), pose(10.0, 10.0), [0.0; 3], 0.01);
        assert!(o.axis[0] > 0.0 && o.axis[1] < 0.0, "invert did not flip: {:?}", o.axis);
    }

    #[test]
    fn unknown_model_is_not_parsed_and_the_list_is_named() {
        assert!(Model::parse("nonsense").is_none());
        assert_eq!(Model::SUPPORTED, ["rate", "stick", "absolute"]);
    }

    #[test]
    fn simulator_traces_each_capture() {
        // Exercises the offline harness on the recorded captures; run with --nocapture to see the trace.
        let mut any = false;
        for path in capture_files() {
            if !path.exists() {
                eprintln!("skip (no capture at {})", path.display());
                continue;
            }
            print_trace(&path);
            any = true;
        }
        if !any {
            eprintln!("no captures present; the simulator trace test had nothing to run");
        }
    }

    /// Run one capture through every model, or `None` when the capture is not on this machine (it is git-ignored).
    fn sim_all(path: &std::path::Path) -> Option<Vec<(Model, SimResult)>> {
        if !path.exists() {
            eprintln!("skip (no capture at {})", path.display());
            return None;
        }
        Some(
            [Model::Rate, Model::Stick, Model::Absolute]
                .iter()
                .map(|m| (*m, simulate_capture(path, &Tuning { model: *m, ..Tuning::default() }).unwrap()))
                .collect(),
        )
    }

    fn capture(name: &str) -> std::path::PathBuf {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../captures");
        root.join(name)
    }

    #[test]
    fn every_axis_stays_in_range_on_every_capture() {
        for path in capture_files() {
            let Some(runs) = sim_all(&path) else { continue };
            for (model, r) in runs {
                for i in 0..2 {
                    assert!(r.axis_min[i] >= -1.0 - 1e-9, "{} {:?} axis_min", path.display(), model);
                    assert!(r.axis_max[i] <= 1.0 + 1e-9, "{} {:?} axis_max", path.display(), model);
                }
            }
        }
    }

    #[test]
    fn the_deadzone_holds_a_still_head() {
        let Some(runs) = sim_all(&capture("xreal_52998_still.bin")) else { return };
        for (model, r) in runs {
            assert!(r.zero_fraction > 0.95, "{model:?} let a still head through: zero {}", r.zero_fraction);
            assert!(r.axis_min[0].abs() < 0.05 && r.axis_max[0].abs() < 0.05, "{model:?} moved on a still capture");
        }
    }

    #[test]
    fn a_yaw_sweep_drives_the_rate_and_saturates_a_position_model() {
        let Some(runs) = sim_all(&capture("imu_yaw.bin")) else { return };
        let by = |m: Model| runs.iter().find(|(x, _)| *x == m).unwrap().1;
        let rate = by(Model::Rate);
        assert!(rate.axis_min[0] < -0.3 || rate.axis_max[0] > 0.3, "rate model ignored the sweep: {:?}", rate);
        let stick = by(Model::Stick);
        assert!(stick.saturated > 1000, "a position model never clamped on a large sweep: {}", stick.saturated);
    }

    #[test]
    fn a_move_capture_drives_the_position_axes() {
        let Some(runs) = sim_all(&capture("imu_move.bin")) else { return };
        let by = |m: Model| runs.iter().find(|(x, _)| *x == m).unwrap().1;
        let stick = by(Model::Stick);
        assert!(stick.axis_min[1] < -0.5 && stick.axis_max[1] > 0.5, "position model did not follow the move capture: {:?}", stick);
    }

    // ---- service tests ----------------------------------------------------------------------------------------

    use crate::uinput::{gamepad_value, DeviceKind};

    fn state(yaw: f64, pitch: f64, omega: [f32; 3]) -> crate::tracking::PoseState {
        let q = pose(yaw, pitch);
        crate::tracking::PoseState {
            q: [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32],
            omega,
            timestamp_ns: 0,
            host_ns: 0,
            valid: true,
        }
    }

    #[test]
    fn parse_args_reads_every_option_and_defaults() {
        let o = parse_args(&[]).unwrap();
        assert_eq!(o.tuning.model, Model::Stick);
        assert_eq!(o.device, DeviceKind::Mouse);
        assert!(o.recenter_on_start);
        assert!(o.capture.is_none());

        let o = parse_args(&[
            "--model", "rate", "--device", "mouse", "--deadzone", "4", "--deadzone-rate", "5", "--sensitivity", "1.5",
            "--curve", "2", "--range", "45", "--rate-full", "120", "--mouse-gain", "30", "--invert-x", "--invert-y",
            "--no-recenter", "--capture", "captures/imu_yaw.bin", "--replay", "captures/imu_move.bin",
        ]).unwrap();
        assert_eq!(o.tuning.model, Model::Rate);
        assert_eq!(o.device, DeviceKind::Mouse);
        assert_eq!(o.tuning.deadzone_deg, 4.0);
        assert_eq!(o.tuning.deadzone_dps, 5.0);
        assert_eq!(o.tuning.sensitivity, 1.5);
        assert_eq!(o.tuning.curve, 2.0);
        assert_eq!(o.tuning.range_deg, 45.0);
        assert_eq!(o.tuning.rate_full_dps, 120.0);
        assert_eq!(o.tuning.mouse_counts_per_deg, 30.0);
        assert!(o.tuning.invert_x && o.tuning.invert_y);
        assert!(!o.recenter_on_start);
        assert_eq!(o.capture.unwrap().to_str().unwrap(), "captures/imu_yaw.bin");
        assert_eq!(o.replay.unwrap().to_str().unwrap(), "captures/imu_move.bin");
    }

    #[test]
    fn the_service_reads_headlook_env_and_a_cli_flag_overrides_it() {
        let get = |k: &str| match k {
            "XREAL_HEADLOOK_DEVICE" => Some("gamepad".to_string()),
            "XREAL_HEADLOOK_MODEL" => Some("rate".to_string()),
            "XREAL_HEADLOOK_ARGS" => Some("--sensitivity 1.5  --range 45".to_string()),
            _ => None,
        };
        let merged = service_args(true, &["--service"], get);
        let refs: Vec<&str> = merged.iter().map(String::as_str).collect();
        let o = parse_args(&refs).unwrap();
        assert!(o.service);
        assert_eq!(o.device, DeviceKind::Gamepad);
        assert_eq!(o.tuning.model, Model::Rate);
        assert_eq!(o.tuning.sensitivity, 1.5);
        assert_eq!(o.tuning.range_deg, 45.0);

        // A command-line flag is applied after the file, so it wins.
        let merged = service_args(true, &["--service", "--device", "mouse", "--sensitivity", "2"], get);
        let refs: Vec<&str> = merged.iter().map(String::as_str).collect();
        let o = parse_args(&refs).unwrap();
        assert_eq!(o.device, DeviceKind::Mouse);
        assert_eq!(o.tuning.sensitivity, 2.0);

        // The unit's ExecStart flag parses on its own (no environment file present).
        assert!(parse_args(&["--service"]).unwrap().service);
    }

    #[test]
    fn a_foreground_run_ignores_the_environment_file() {
        let get = |k: &str| (k == "XREAL_HEADLOOK_DEVICE").then(|| "gamepad".to_string());
        assert!(service_args(false, &[], get).is_empty(), "without --service the file is not read");
        assert!(env_service_args(|_| None).is_empty());
        assert!(env_service_args(|k| (k == "XREAL_HEADLOOK_DEVICE").then(|| "  ".to_string())).is_empty(), "blank values leave the defaults");
    }

    #[test]
    fn parse_args_refuses_an_unknown_model_and_names_the_supported_ones() {
        let e = parse_args(&["--model", "bogus"]).unwrap_err();
        assert!(e.contains("bogus") && e.contains("rate") && e.contains("stick") && e.contains("absolute"), "{e}");
        assert!(parse_args(&["--device", "trackball"]).is_err());
        assert!(parse_args(&["--tuning"]).is_err());
        assert!(parse_args(&["--model"]).is_err(), "a flag without its value");
    }

    #[test]
    fn events_have_the_right_shape_for_each_device() {
        let o = Output { axis: [0.5, -0.25], mouse: [3.0, -4.0], saturated: false };
        assert_eq!(
            events(DeviceKind::Gamepad, &o),
            vec![
                Event::Abs(crate::uinput::ABS_X, gamepad_value(0.5)),
                Event::Abs(crate::uinput::ABS_Y, gamepad_value(0.25)),
                Event::Syn
            ]
        );
        assert_eq!(events(DeviceKind::Mouse, &o), vec![Event::Rel(0, 3), Event::Rel(1, -4), Event::Syn]);
    }

    #[test]
    fn session_recenters_on_start_then_follows_the_head() {
        let t = Tuning { model: Model::Stick, ..Tuning::default() };
        let mut s = Session::new(t, DeviceKind::Gamepad, true);
        // The first pose becomes the neutral, so the stick starts centred.
        let first = s.tick(&state(20.0, 0.0, [0.0; 3]), 1_000_000_000).unwrap();
        assert_eq!(first, vec![Event::Abs(crate::uinput::ABS_X, 0), Event::Abs(crate::uinput::ABS_Y, 0), Event::Syn]);
        // A turn away from that neutral gives a nonzero axis.
        let moved = s.tick(&state(8.0, 0.0, [0.0; 3]), 1_005_000_000).unwrap();
        assert!(matches!(moved[0], Event::Abs(crate::uinput::ABS_X, v) if v > 1000), "{moved:?}");
    }

    #[test]
    fn session_emits_nothing_when_the_imu_is_unavailable() {
        let mut s = Session::new(Tuning::default(), DeviceKind::Gamepad, true);
        let mut p = state(0.0, 0.0, [0.0; 3]);
        p.valid = false;
        assert_eq!(s.tick(&p, 1), None, "an invalid pose must emit no head-motion events");
    }

    #[test]
    fn mouse_counts_accumulate_across_ticks() {
        // A slow turn at ~1 kHz gives well under one count per tick; rounding each tick would drop it all.
        let t = Tuning { model: Model::Rate, mouse_counts_per_deg: 20.0, ..Tuning::default() };
        let mut s = Session::new(t, DeviceKind::Mouse, false);
        let p = state(0.0, 0.0, [0.0, 0.005, 0.0]);
        let mut moved = 0i32;
        for i in 0..1000u64 {
            let evs = s.tick(&p, 1_000_000_000 + i * 1_000_000).unwrap();
            if let Event::Rel(0, v) = evs[0] {
                moved += v;
            }
        }
        assert!(moved != 0, "fractional mouse counts were rounded away");
    }

    #[test]
    fn recenter_zeroes_the_emitted_events_for_the_current_pose() {
        let mut s = Session::new(Tuning::default(), DeviceKind::Gamepad, false);
        let p = state(15.0, 8.0, [0.0; 3]);
        s.tick(&p, 1_000_000_000).unwrap();
        let before = s.tick(&p, 1_005_000_000).unwrap();
        assert!(before.iter().any(|e| matches!(e, Event::Abs(_, v) if *v != 0)), "{before:?}");
        s.recenter(pose(15.0, 8.0));
        let after = s.tick(&p, 1_010_000_000).unwrap();
        assert_eq!(after, vec![Event::Abs(crate::uinput::ABS_X, 0), Event::Abs(crate::uinput::ABS_Y, 0), Event::Syn]);
    }

    #[test]
    fn the_status_line_names_the_device_and_model() {
        let line = status_line(DeviceKind::Gamepad, Model::Stick, true, Some(3.0));
        assert!(line.contains("gamepad") && line.contains("XREAL") && line.contains("stick") && line.contains("connected"), "{line}");
        let line = status_line(DeviceKind::Mouse, Model::Rate, false, None);
        assert!(line.contains("mouse") && line.contains("rate") && line.contains("unavailable") && line.contains("not yet"), "{line}");
    }

    #[test]
    fn latency_summary_reports_percentiles_and_clears_the_window() {
        assert!(latency_summary(&mut vec![]).is_none());
        let mut v: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let (p50, p95, n) = latency_summary(&mut v).unwrap();
        assert_eq!(n, 100);
        assert!((p50 - 50.0).abs() <= 1.0, "p50 {p50}");
        assert!((p95 - 95.0).abs() <= 1.0, "p95 {p95}");
        assert!(v.is_empty(), "the latency window is cleared after reporting");
    }
}

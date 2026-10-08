//! Rotational reprojection ("timewarp") maths. The GPU version is in shaders/warp.frag; this CPU copy exists so the maths
//! can be unit tested and so the two can be compared.
//!
//! SteamVR renders each frame for the head pose at the time it started. By the time we show it the head has turned, so for
//! every output pixel we ask: which pixel of the rendered frame lies along this pixel's current line of sight?
//!
//! View space is OpenVR's: x right, y up, -z forward. `fov` holds OpenVR's raw projection tangents
//! (left, right, top, bottom) where left and top are negative (top is -tan(up), as GetProjectionRaw returns it).

pub type Quat = [f32; 4]; // w, x, y, z

/// Field of view in OpenVR's raw projection form used until the glasses' own calibration arrives; the driver falls back to the same values.
pub const DEFAULT_FOV: [f32; 4] = [-0.3857, 0.3857, -0.2190, 0.2190];
pub const FOV: [f32; 4] = DEFAULT_FOV;

static FOV_BITS: [std::sync::atomic::AtomicU32; 4] = [const { std::sync::atomic::AtomicU32::new(0) }; 4];

/// Replace the field of view used by the reprojection; must match what the driver reports to SteamVR.
pub fn set_fov(fov: [f32; 4]) {
    for (slot, v) in FOV_BITS.iter().zip(fov) {
        slot.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// The field of view in force: the glasses' calibration once set, else the default.
pub fn fov() -> [f32; 4] {
    let bits = FOV_BITS.each_ref().map(|b| b.load(std::sync::atomic::Ordering::Relaxed));
    if bits[1] == 0 { DEFAULT_FOV } else { bits.map(f32::from_bits) }
}

pub fn conj(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

pub fn mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

/// Row-major 3x3 rotation matrix of a unit quaternion.
pub fn to_matrix(q: Quat) -> [[f32; 3]; 3] {
    let [w, x, y, z] = q;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// The rotation that maps a ray in the *current* view frame into the *render* view frame:
/// R_render^T * R_now, i.e. the quaternion conj(q_render) * q_now. Both quaternions are world-from-head.
pub fn view_delta(q_render: Quat, q_now: Quat) -> [[f32; 3]; 3] {
    to_matrix(mul(conj(q_render), q_now))
}

/// Where in the rendered frame (uv in 0..1, v down) the output pixel `uv` should sample from, or None if it falls outside.
pub fn source_uv(m: &[[f32; 3]; 3], uv: [f32; 2], fov: [f32; 4]) -> Option<[f32; 2]> {
    let (l, r, t, b) = (fov[0], fov[1], fov[2], fov[3]);
    let x = l + (r - l) * uv[0];
    let y = -t + (-b + t) * uv[1]; // v = 0 is the top of the picture: +tan(up)
    let d = [
        m[0][0] * x + m[0][1] * y - m[0][2],
        m[1][0] * x + m[1][1] * y - m[1][2],
        m[2][0] * x + m[2][1] * y - m[2][2],
    ];
    if d[2] >= -1e-4 {
        return None; // behind the render camera
    }
    let (px, py) = (d[0] / -d[2], d[1] / -d[2]);
    let u = (px - l) / (r - l);
    let v = (py + t) / (-b + t);
    if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) { Some([u, v]) } else { None }
}

static EYE_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static EYE_ORIENTATIONS: std::sync::Mutex<Option<[Quat; 2]>> = std::sync::Mutex::new(None);

/// 0: the eyes' images are drawn as SteamVR rendered them; 1: each is rotated by its display's factory orientation; 2: the same with the opposite sign convention.
pub fn set_eye_rotation_mode(mode: u8) {
    EYE_MODE.store(mode, std::sync::atomic::Ordering::Relaxed);
}

pub fn eye_rotation_mode() -> u8 {
    EYE_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The two displays' factory orientations (left, right) as Hamilton w, x, y, z in the IMU's frame, from the glasses' config.
pub fn set_eye_orientations(q: Option<[Quat; 2]>) {
    *EYE_ORIENTATIONS.lock().unwrap() = q;
}

pub fn mat_mul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// Rotation taking a ray in each eye's own view frame into the frame both eyes share (OpenVR's x right, y up, -z forward), after taking out
/// the rotation the two displays have in common, so the picture is split evenly between them. `q` are the display orientations in the IMU's
/// frame (x right, y down, z forward), which is OpenVR's frame turned 180 degrees about x, so a rotation's y and z components change sign.
/// `reversed` reads them with the opposite convention (rotation from the IMU into the display instead of from the display into the IMU).
pub fn eye_matrices_from(q: [Quat; 2], reversed: bool) -> [[[f32; 3]; 3]; 2] {
    let q = q.map(|e| if reversed { conj(e) } else { e });
    let sign = if q[0].iter().zip(q[1].iter()).map(|(a, b)| a * b).sum::<f32>() < 0.0 { -1.0 } else { 1.0 };
    let sum = [0, 1, 2, 3].map(|i| q[0][i] + sign * q[1][i]);
    let norm = sum.iter().map(|v| v * v).sum::<f32>().sqrt();
    let mid = sum.map(|v| v / norm);
    q.map(|e| {
        let r = mul(conj(mid), e);
        to_matrix([r[0], r[1], -r[2], -r[3]])
    })
}

/// The per-eye rotations in force: identity unless an eye-rotation mode is set and the glasses' orientations are known.
pub fn eye_matrices() -> [[[f32; 3]; 3]; 2] {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    match (eye_rotation_mode(), *EYE_ORIENTATIONS.lock().unwrap()) {
        (0, _) | (_, None) => [identity; 2],
        (mode, Some(q)) => eye_matrices_from(q, mode == 2),
    }
}

/// Push-constant rows for each eye: the head rotation since SteamVR rendered, then the eye's own display rotation.
pub fn eye_rows(delta: &[[f32; 3]; 3]) -> [[f32; 12]; 2] {
    eye_matrices().map(|e| push_rows(&mat_mul(delta, &e)))
}

/// Pack a rotation matrix as three vec4 rows for push constants.
pub fn push_rows(m: &[[f32; 3]; 3]) -> [f32; 12] {
    [m[0][0], m[0][1], m[0][2], 0.0, m[1][0], m[1][1], m[1][2], 0.0, m[2][0], m[2][1], m[2][2], 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis_angle(axis: [f32; 3], deg: f32) -> Quat {
        let h = deg.to_radians() / 2.0;
        [h.cos(), axis[0] * h.sin(), axis[1] * h.sin(), axis[2] * h.sin()]
    }

    #[test]
    fn no_head_motion_samples_the_same_pixel() {
        let q = axis_angle([0.0, 1.0, 0.0], 30.0);
        let m = view_delta(q, q);
        for uv in [[0.1, 0.2], [0.5, 0.5], [0.9, 0.8]] {
            let s = source_uv(&m, uv, FOV).unwrap();
            assert!((s[0] - uv[0]).abs() < 1e-5 && (s[1] - uv[1]).abs() < 1e-5, "{uv:?} -> {s:?}");
        }
    }

    #[test]
    fn turning_left_pulls_content_from_the_left_of_the_render() {
        // The head turned left (+90 degrees would be a quarter turn; use a small angle) after the frame was rendered, so the
        // line of sight through the centre pixel now points further left in the rendered picture: u < 0.5.
        let m = view_delta([1.0, 0.0, 0.0, 0.0], axis_angle([0.0, 1.0, 0.0], 5.0));
        let s = source_uv(&m, [0.5, 0.5], FOV).unwrap();
        assert!(s[0] < 0.5 && (s[1] - 0.5).abs() < 1e-4, "{s:?}");
        // The centre ray moves left by tan(5 degrees) of the picture's tangent width.
        let expected = 5f32.to_radians().tan() / (FOV[1] - FOV[0]);
        assert!(((0.5 - s[0]) - expected).abs() < 0.01, "shift {}", 0.5 - s[0]);
    }

    #[test]
    fn looking_up_pulls_content_from_the_top() {
        let m = view_delta([1.0, 0.0, 0.0, 0.0], axis_angle([1.0, 0.0, 0.0], 3.0));
        let s = source_uv(&m, [0.5, 0.5], FOV).unwrap();
        assert!(s[1] < 0.5 && (s[0] - 0.5).abs() < 1e-4, "{s:?}"); // v = 0 is the top
    }

    #[test]
    fn rolling_rotates_the_picture_and_large_motion_leaves_the_frame() {
        let m = view_delta([1.0, 0.0, 0.0, 0.0], axis_angle([0.0, 0.0, 1.0], 10.0));
        let centre = source_uv(&m, [0.5, 0.5], FOV).unwrap();
        assert!((centre[0] - 0.5).abs() < 1e-4 && (centre[1] - 0.5).abs() < 1e-4);
        let corner = source_uv(&m, [0.05, 0.05], FOV).unwrap();
        assert!((corner[0] - 0.05).abs() > 0.001, "a roll must move off-centre pixels");
        let far = view_delta([1.0, 0.0, 0.0, 0.0], axis_angle([0.0, 1.0, 0.0], 60.0));
        assert!(source_uv(&far, [0.5, 0.5], FOV).is_none(), "60 degrees is outside the rendered field of view");
    }

    // Orientations of the 2026-10-08 unit's two displays (Hamilton w, x, y, z in the IMU frame): the left is turned 0.84 degrees about y, the right about 0.03.
    const LEFT: Quat = [0.999969, -0.0027659, 0.0073439, 0.0000929];
    const RIGHT: Quat = [0.9999952, -0.0027523, -0.0002608, 0.0014253];

    fn yaw_of(m: &[[f32; 3]; 3]) -> f32 {
        // The straight-ahead ray (0, 0, -1) goes to minus the matrix's third column; yaw to the right is its x over its forward part.
        (-m[0][2]).atan2(m[2][2])
    }

    #[test]
    fn the_eyes_split_the_factory_yaw_difference_evenly_and_oppositely() {
        let [l, r] = eye_matrices_from([LEFT, RIGHT], false);
        let (yl, yr) = (yaw_of(&l).to_degrees(), yaw_of(&r).to_degrees());
        assert!((yl + yr).abs() < 0.01, "not opposite: {yl} {yr}");
        // 0.872 degrees apart in total, 0.436 each: the left display's centre ray points right of the shared forward, the right one's left of it.
        assert!(yl > 0.43 && yl < 0.44 && yr < -0.43 && yr > -0.44, "{yl} {yr}");
    }

    #[test]
    fn the_reversed_convention_flips_the_direction() {
        let [l, _] = eye_matrices_from([LEFT, RIGHT], false);
        let [lr, _] = eye_matrices_from([LEFT, RIGHT], true);
        assert!((yaw_of(&l) + yaw_of(&lr)).abs() < 1e-4);
    }

    #[test]
    fn identical_displays_get_no_rotation() {
        for m in eye_matrices_from([LEFT, LEFT], false) {
            for i in 0..3 {
                for j in 0..3 {
                    assert!((m[i][j] - if i == j { 1.0 } else { 0.0 }).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn eye_rotation_changes_the_sampled_pixel_by_the_expected_amount() {
        // A 0.436 degree turn at the focal length of the panel (2490 px) is 18.9 px; on the 1920 px picture whose half width is 0.3857 tangents that is about 0.0147 of the width.
        let [l, _] = eye_matrices_from([LEFT, RIGHT], false);
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let s = source_uv(&mat_mul(&id, &l), [0.5, 0.5], FOV).unwrap();
        let shift_px = (s[0] - 0.5).abs() * 1920.0;
        assert!((shift_px - 0.436f32.to_radians() * 2490.0).abs() < 1.5, "{shift_px}");
    }

    #[test]
    fn a_pitch_difference_between_the_eyes_would_be_vertical_not_horizontal() {
        let up = |deg: f32| { let h = deg.to_radians() / 2.0; [h.cos(), h.sin(), 0.0, 0.0] };
        let [l, _] = eye_matrices_from([up(0.4), up(0.0)], false);
        assert!(yaw_of(&l).abs() < 1e-4);
        assert!((l[1][2]).abs() > 1e-3, "no vertical component");
    }
}

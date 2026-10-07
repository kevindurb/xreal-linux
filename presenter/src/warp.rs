//! Rotational reprojection ("timewarp") maths. The GPU version is in shaders/warp.frag; this CPU copy exists so the maths
//! can be unit tested and so the two can be compared.
//!
//! SteamVR renders each frame for the head pose at the time it started. By the time we show it the head has turned, so for
//! every output pixel we ask: which pixel of the rendered frame lies along this pixel's current line of sight?
//!
//! View space is OpenVR's: x right, y up, -z forward. `fov` holds OpenVR's raw projection tangents
//! (left, right, top, bottom) where left and top are negative (top is -tan(up), as GetProjectionRaw returns it).

pub type Quat = [f32; 4]; // w, x, y, z

/// Field of view in OpenVR's raw projection form; must match the driver's GetProjectionRaw.
pub const FOV: [f32; 4] = [-0.45, 0.45, -0.253, 0.253];

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
        // 5 degrees at +-0.45 tangent half-width: the centre ray moves left by tan(5 deg) = 0.0875 of 0.9 -> about 0.097 of the width
        assert!(((0.5 - s[0]) - 0.0875 / 0.9).abs() < 0.01, "shift {}", 0.5 - s[0]);
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
}

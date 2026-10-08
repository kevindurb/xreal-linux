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

/// The factory display-distortion grids: for each grid point, `step` panel pixels apart, where the glasses' optics put that panel pixel
/// in the ideal (undistorted) picture, in panel pixels. Row-major, `cols` x `rows`, left eye then right eye.
#[derive(Clone, Debug, PartialEq)]
pub struct DistortionGrids {
    pub cols: usize,
    pub rows: usize,
    pub left: Vec<[f32; 2]>,
    pub right: Vec<[f32; 2]>,
}

pub const PANEL: [f32; 2] = [1920.0, 1200.0];
/// Rows of each eye's picture; assumed centred in the panel's rows (not measured), so panel y = picture y + (1200 - 1080) / 2.
pub const PICTURE_ROWS: f32 = 1080.0;
pub const GRID_STEP: f32 = 32.0;

/// Where to sample the ideal picture for the output pixel at picture position `uv` (0..1, v down). The grid says panel pixel P is seen at
/// grid(P), so the pixel at P shows the picture at grid(P); `inverse` uses 2P - grid(P) instead, in case the grid runs the other way.
pub fn corrected_uv(grid: &[[f32; 2]], cols: usize, rows: usize, uv: [f32; 2], inverse: bool) -> [f32; 2] {
    let crop = (PANEL[1] - PICTURE_ROWS) / 2.0;
    let p = [uv[0] * PANEL[0], uv[1] * PICTURE_ROWS + crop];
    let g = [(p[0] / GRID_STEP).clamp(0.0, (cols - 1) as f32), (p[1] / GRID_STEP).clamp(0.0, (rows - 1) as f32)];
    let (x0, y0) = (g[0].floor() as usize, g[1].floor() as usize);
    let (x1, y1) = ((x0 + 1).min(cols - 1), (y0 + 1).min(rows - 1));
    let (fx, fy) = (g[0] - x0 as f32, g[1] - y0 as f32);
    let at = |x: usize, y: usize| grid[y * cols + x];
    let mut q = [0.0; 2];
    for k in 0..2 {
        let top = at(x0, y0)[k] * (1.0 - fx) + at(x1, y0)[k] * fx;
        let bottom = at(x0, y1)[k] * (1.0 - fx) + at(x1, y1)[k] * fx;
        q[k] = top * (1.0 - fy) + bottom * fy;
    }
    if inverse {
        q = [2.0 * p[0] - q[0], 2.0 * p[1] - q[1]];
    }
    [q[0] / PANEL[0], (q[1] - crop) / PICTURE_ROWS]
}

static DISTORTION: std::sync::Mutex<Option<std::sync::Arc<DistortionGrids>>> = std::sync::Mutex::new(None);
static DISTORTION_VERSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Publish the factory grids (or None when the config has none) for the renderer to pick up.
pub fn set_distortion(grids: Option<DistortionGrids>) {
    *DISTORTION.lock().unwrap() = grids.map(std::sync::Arc::new);
    DISTORTION_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// The current grids and a version that changes whenever they are replaced.
pub fn distortion() -> (u32, Option<std::sync::Arc<DistortionGrids>>) {
    (DISTORTION_VERSION.load(std::sync::atomic::Ordering::Relaxed), DISTORTION.lock().unwrap().clone())
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

    fn grid_with(cols: usize, rows: usize, f: impl Fn(f32, f32) -> [f32; 2]) -> Vec<[f32; 2]> {
        (0..rows).flat_map(|r| (0..cols).map(move |c| (c, r))).map(|(c, r)| f(c as f32 * GRID_STEP, r as f32 * GRID_STEP)).collect()
    }

    #[test]
    fn an_identity_grid_leaves_the_picture_position_alone() {
        let g = grid_with(61, 39, |x, y| [x, y]);
        for uv in [[0.0, 0.0], [0.3, 0.7], [1.0, 1.0], [0.5, 0.5]] {
            for inverse in [false, true] {
                let c = corrected_uv(&g, 61, 39, uv, inverse);
                assert!((c[0] - uv[0]).abs() < 1e-5 && (c[1] - uv[1]).abs() < 1e-5, "{uv:?} -> {c:?}");
            }
        }
    }

    #[test]
    fn a_grid_displacement_is_applied_forward_and_reversed_for_inverse() {
        // Every panel pixel is seen 20 px to the right and 10 px lower: the pixel shows the picture 20 px right and 10 px down of itself.
        let g = grid_with(61, 39, |x, y| [x + 20.0, y + 10.0]);
        let uv = [0.5, 0.5];
        let fwd = corrected_uv(&g, 61, 39, uv, false);
        assert!((fwd[0] - (0.5 + 20.0 / 1920.0)).abs() < 1e-5 && (fwd[1] - (0.5 + 10.0 / 1080.0)).abs() < 1e-5, "{fwd:?}");
        let inv = corrected_uv(&g, 61, 39, uv, true);
        assert!((inv[0] - (0.5 - 20.0 / 1920.0)).abs() < 1e-5 && (inv[1] - (0.5 - 10.0 / 1080.0)).abs() < 1e-5, "{inv:?}");
    }

    #[test]
    fn the_picture_is_centred_in_the_panel_rows() {
        // A grid that maps panel pixels to themselves except that it adds 60 rows: the picture's top row (panel row 60) lands on picture row 60.
        let g = grid_with(61, 39, |x, y| [x, y + 60.0]);
        let c = corrected_uv(&g, 61, 39, [0.5, 0.0], false);
        assert!((c[1] - 60.0 / 1080.0).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn lookups_interpolate_between_grid_points() {
        let g = grid_with(61, 39, |x, y| [x + x / 32.0, y]); // displacement grows by 1 px per grid column
        let a = corrected_uv(&g, 61, 39, [16.0 / 1920.0, 60.0 / 1080.0], false); // half a cell in: displacement 0.5 px
        assert!((a[0] * 1920.0 - 16.5).abs() < 1e-3, "{a:?}");
    }
}

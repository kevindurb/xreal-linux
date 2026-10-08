#version 450
// Rotational reprojection: for each output pixel, find where its current line of sight falls in the frame SteamVR
// rendered for an earlier head pose. Optionally first moves the pixel through the glasses' factory display-distortion grid.
// Keep in sync with src/warp.rs (source_uv, corrected_uv), which is unit tested.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 outColor;
layout(set = 0, binding = 0) uniform sampler2D eye;
layout(set = 1, binding = 0, std430) readonly buffer Grid { vec2 q[]; } grid;
layout(push_constant) uniform PC {
    vec4 r0;   // rotation taking a ray in the current view frame into the render view frame (rows)
    vec4 r1;
    vec4 r2;
    vec4 fov;  // OpenVR raw projection tangents: left, right, top, bottom (left and top negative)
    vec4 bounds; // valid region of this eye's texture: umin, vmin, umax, vmax (SteamVR may render into a sub-rectangle)
    vec4 dist; // x: 0 no distortion grid, 1 grid, 2 reversed grid; y: 1 draw a test grid instead of the eye image; z: this eye's first grid entry; w: grid columns
    vec4 dist2; // x: grid rows
} pc;

const vec2 PANEL = vec2(1920.0, 1200.0);
const float PICTURE_ROWS = 1080.0;
const float GRID_STEP = 32.0;

// Where in the ideal picture (uv, v down) the output pixel at `p` should look.
vec2 corrected(vec2 p) {
    float crop = (PANEL.y - PICTURE_ROWS) * 0.5;
    vec2 pp = vec2(p.x * PANEL.x, p.y * PICTURE_ROWS + crop);
    int cols = int(pc.dist.w), rows = int(pc.dist2.x);
    vec2 g = clamp(pp / GRID_STEP, vec2(0.0), vec2(float(cols - 1), float(rows - 1)));
    ivec2 g0 = ivec2(floor(g));
    ivec2 g1 = min(g0 + 1, ivec2(cols - 1, rows - 1));
    vec2 f = g - vec2(g0);
    int base = int(pc.dist.z);
    vec2 a = grid.q[base + g0.y * cols + g0.x];
    vec2 b = grid.q[base + g0.y * cols + g1.x];
    vec2 c = grid.q[base + g1.y * cols + g0.x];
    vec2 d = grid.q[base + g1.y * cols + g1.x];
    vec2 q = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    if (pc.dist.x > 1.5) q = 2.0 * pp - q;
    return vec2(q.x / PANEL.x, (q.y - crop) / PICTURE_ROWS);
}

void main() {
    vec2 pic = pc.dist.x > 0.5 ? corrected(uv) : uv;
    if (pc.dist.y > 0.5) {
        // Straight lines every 120 picture pixels, a bright frame and centre cross: they look straight only if the correction is right.
        if (pic.x < 0.0 || pic.x > 1.0 || pic.y < 0.0 || pic.y > 1.0) { outColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
        vec2 px = pic * vec2(1920.0, PICTURE_ROWS);
        vec2 cell = abs(fract(px / 120.0 + 0.5) - 0.5) * 120.0;
        bool frame = px.x < 3.0 || px.x > 1917.0 || px.y < 3.0 || px.y > 1077.0;
        bool cross = abs(px.x - 960.0) < 1.5 || abs(px.y - 540.0) < 1.5;
        float line = (min(cell.x, cell.y) < 1.0 || frame || cross) ? 1.0 : 0.0;
        outColor = vec4(mix(vec3(0.05), vec3(1.0), line), 1.0);
        return;
    }
    float l = pc.fov.x, r = pc.fov.y, t = pc.fov.z, b = pc.fov.w;
    float x = l + (r - l) * pic.x;
    float y = -t + (-b + t) * pic.y;
    vec3 d = vec3(x, y, -1.0);
    vec3 s = vec3(dot(pc.r0.xyz, d), dot(pc.r1.xyz, d), dot(pc.r2.xyz, d));
    if (s.z >= -1e-4) { outColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
    vec2 p = s.xy / -s.z;
    vec2 st = vec2((p.x - l) / (r - l), (p.y + t) / (-b + t));
    if (st.x < 0.0 || st.x > 1.0 || st.y < 0.0 || st.y > 1.0) { outColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
    // st is a position inside the rendered picture; the picture occupies only `bounds` of the texture (which may be flipped).
    vec2 tex = mix(pc.bounds.xy, pc.bounds.zw, st);
    vec2 half_texel = 0.5 / vec2(textureSize(eye, 0));
    vec2 lo = min(pc.bounds.xy, pc.bounds.zw) + half_texel;
    vec2 hi = max(pc.bounds.xy, pc.bounds.zw) - half_texel;
    outColor = texture(eye, clamp(tex, lo, hi));
}

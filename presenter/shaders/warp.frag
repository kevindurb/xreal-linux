#version 450
// Rotational reprojection: for each output pixel, find where its current line of sight falls in the frame SteamVR
// rendered for an earlier head pose. Keep in sync with src/warp.rs (source_uv), which is unit tested.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 outColor;
layout(set = 0, binding = 0) uniform sampler2D eye;
layout(push_constant) uniform PC {
    vec4 r0;   // rotation taking a ray in the current view frame into the render view frame (rows)
    vec4 r1;
    vec4 r2;
    vec4 fov;  // OpenVR raw projection tangents: left, right, top, bottom (left and top negative)
    vec4 bounds; // valid region of this eye's texture: umin, vmin, umax, vmax (SteamVR may render into a sub-rectangle)
} pc;
void main() {
    float l = pc.fov.x, r = pc.fov.y, t = pc.fov.z, b = pc.fov.w;
    float x = l + (r - l) * uv.x;
    float y = -t + (-b + t) * uv.y;
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

// Shared by every scene pass: the per-frame uniform and the sun's shadow
// map (docs/renderer.md §"Uniform layout"). Concatenated in front of
// mesh.wgsl, sky.wgsl, debug.wgsl and post.wgsl.

// Per-object shading budget. The frame light pool is a separate storage buffer.
const MAX_LIGHTS: u32 = 8u;
const LIGHT_POINT: f32 = 0.0;
const LIGHT_SPOT: f32 = 1.0;
const PI: f32 = 3.14159265358979;
// Shadow cascades (fynkel_render::shadow::CASCADES), a 2×2 atlas inside
// the one shadow map.
const CASCADES: u32 = 4u;

struct Cascade {
    // World → this cascade's shadow clip space (orthographic, depth 0..1).
    view_proj: mat4x4<f32>,
    // xy: the cascade's quadrant origin in atlas uv; zw: its size.
    rect: vec4<f32>,
    // x: receiver normal offset (world units); yzw: unused.
    params: vec4<f32>,
};

struct Light {
    // xyz: world position; w: range (metres).
    position: vec4<f32>,
    // rgb: colour × intensity (candela); w: LIGHT_POINT or LIGHT_SPOT.
    color: vec4<f32>,
    // xyz: unit direction the light shines along; w: cos(outer_angle).
    direction: vec4<f32>,
    // x: cos(inner_angle); yzw: unused.
    params: vec4<f32>,
};

struct Grade {
    // x: the contrast exponent (log space around middle grey); y: 1 when
    // the grade runs (0: neutral, skipped); z: the tone mapper (0 ACES,
    // 1 neutral, 2 none); w unused.
    params: vec4<f32>,
    // Columns (xyz) of the matrix before the contrast curve: the
    // post-exposure and the white balance.
    pre: array<vec4<f32>, 3>,
    // After it: the colour filter, the hue shift, the saturation.
    post: array<vec4<f32>, 3>,
    // rgb: lift, 1 / gamma, gain (display space, after the tone mapper).
    lift: vec4<f32>,
    inv_gamma: vec4<f32>,
    gain: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // The sun's shadow cascades, nearest first.
    cascades: array<Cascade, CASCADES>,
    // xyz: camera position; w: exposure factor 1 / L_max.
    camera_pos: vec4<f32>,
    // xyz: unit vector toward the sun; w: 1 when a sun is present.
    sun_dir: vec4<f32>,
    // rgb: sun colour × illuminance (lux).
    sun_color: vec4<f32>,
    // rgb: ambient colour × illuminance (lux); w: L_max (nits at white).
    ambient: vec4<f32>,
    // x: sky radiance scale (nits); y: sky rotation about +Y (radians);
    // z: 1 when the environment lights the scene (E1); w: the specular
    // cube's last mip level.
    sky: vec4<f32>,
    // x: 1 when the shadow map holds this frame's sun; y: one atlas texel
    // in map uv; z: the fraction of a cascade blended into the next;
    // w: cascades in the atlas.
    shadow: vec4<f32>,
    // The frame's ambient occlusion as the volumes resolved it (R3a):
    // x: hemisphere radius (m); y: the exponent the visible fraction is
    // raised to (0: off, the red channel of `ssao_tex` is 1); zw unused.
    ao: vec4<f32>,
    // The frame's contact shadows as the volumes resolved them (R3d):
    // x: opacity (0: off, the green channel of `ssao_tex` is 1); y: the
    // march length toward the sun (m); z: the thickness a depth-buffer
    // surface is taken to have (m); w: the camera distance they end at (m).
    contact: vec4<f32>,
    // The frame's bloom as the volumes resolved it (R3b): x: threshold;
    // y: the soft knee (half the threshold); z: how much of each coarser
    // level is blended over the finer one (scatter, 0.05..0.95); w: the
    // intensity the post pass adds it with — 0 when the stage did not run.
    bloom: vec4<f32>,
    // rgb: the glow's tint (linear); w unused.
    bloom_tint: vec4<f32>,
    // The frame's colour grading as the volumes resolved it (R3c).
    grade: Grade,
    // x: lights in the pool; y: 1 when the sky is drawn; z: 1 when the
    // post pass must sRGB-encode (the target is a Unorm format); w: 1
    // when `ssao_tex` holds this frame's screen-space stage.
    counts: vec4<u32>,
    // x: the world's time (s), still while paused; y: seconds since the
    // previous frame; zw: the internal target's size in pixels (ADR-0031).
    time: vec4<f32>,
    // The frame's lights. A mesh reads only the ones its instance lists
    // (E1 light culling); the index is a byte of `Instance.lights`.
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
// Image-based lighting (E1): black 1×1 cubes when the scene has none.
@group(0) @binding(3) var cube_sampler: sampler;
@group(0) @binding(4) var irradiance_cube: texture_cube<f32>;
@group(0) @binding(5) var specular_cube: texture_cube<f32>;
@group(0) @binding(6) var brdf_lut: texture_2d<f32>;
// The screen-space stage at the internal size: red the ambient occlusion
// (R2), green the contact shadows (R3d); a 1×1 white texture when the
// profile has both off. The stage's own passes do not bind this group.
@group(0) @binding(7) var ssao_tex: texture_2d<f32>;

// A world direction in the environment's frame: the sky's rotation about
// +Y, so what a mirror reflects is what the sky pass drew.
fn rotate_sky(d: vec3<f32>) -> vec3<f32> {
    let c = cos(frame.sky.y);
    let s = sin(frame.sky.y);
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// A clip-space triangle covering the viewport: vertex ids 0, 1, 2.
fn fullscreen(id: u32) -> vec2<f32> {
    let x = f32(i32(id & 1u) * 4 - 1);
    let y = f32(i32(id >> 1u) * 4 - 1);
    return vec2<f32>(x, y);
}

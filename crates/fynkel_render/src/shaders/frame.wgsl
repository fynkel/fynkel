// Shared by every scene pass: the per-frame uniform and the sun's shadow
// map (docs/renderer.md §"Uniform layout"). Concatenated in front of
// mesh.wgsl, sky.wgsl, debug.wgsl and post.wgsl.

// The frame's light pool and what one object may take from it
// (fynkel_render::LIGHT_POOL / MAX_LIGHTS).
const LIGHT_POOL: u32 = 64u;
const MAX_LIGHTS: u32 = 8u;
const LIGHT_POINT: f32 = 0.0;
const LIGHT_SPOT: f32 = 1.0;
const PI: f32 = 3.14159265358979;

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

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // World → the sun's shadow clip space (orthographic, depth 0..1).
    sun_view_proj: mat4x4<f32>,
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
    // x: 1 when the shadow map holds this frame's sun; y: one shadow
    // texel in map uv; z: receiver normal offset (world units); w: unused.
    shadow: vec4<f32>,
    // x: lights in the pool; y: 1 when the sky is drawn; z: 1 when the
    // post pass must sRGB-encode (the target is a Unorm format);
    // w: unused.
    counts: vec4<u32>,
    // The frame's lights. A mesh reads only the ones its instance lists
    // (E1 light culling); the index is a byte of `Instance.lights`.
    lights: array<Light, LIGHT_POOL>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
// Image-based lighting (E1): black 1×1 cubes when the scene has none.
@group(0) @binding(3) var cube_sampler: sampler;
@group(0) @binding(4) var irradiance_cube: texture_cube<f32>;
@group(0) @binding(5) var specular_cube: texture_cube<f32>;
@group(0) @binding(6) var brdf_lut: texture_2d<f32>;

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

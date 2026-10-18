// A screen effect (stage 10): a game's `fn effect(in: EffectInput) ->
// vec4<f32>`, run for each pixel of the finished picture, which it reads
// with `picture`. A camera's effects run in order, each over what the one
// before it made; the last draws onto the view (renderer.rs). Colours are
// as the screen shows them: tone mapped, the fog and the glow in, linear.
// What the shader returns is put on the screen as it is; its alpha is
// dropped. An effect can have textures of its own, as a material's shader
// can (a colour grade's table: `color_lut`), and read the solid scene's
// depth (`scene_depth`), which a frame it's in keeps.

// A pixel of the picture, as an effect gets it.
struct EffectInput {
    // 0 to 1 across the view, from its top left.
    uv: vec2<f32>,
    // The pixel's centre, in the view's pixels from its top left.
    pixel: vec2<f32>,
    // The view's size, in pixels.
    size: vec2<f32>,
    // Seconds since the world began, as `frame_time()` in a material's.
    time: f32,
};

struct FyEffect {
    // xy: the view's size; z: the time.
    frame: vec4<f32>,
    // The camera's: from the screen to the world, where it is (w: 1 for a
    // perspective camera) and the way it looks.
    inverse_view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    forward: vec4<f32>,
    // The effect's values, as its `Values` lays them out.
    values: array<vec4<f32>, 32>,
};

@group(0) @binding(0) var fy_picture: texture_2d<f32>;
@group(0) @binding(1) var fy_linear: sampler;
@group(0) @binding(2) var<uniform> fy_effect: FyEffect;

// The picture at `uv` (0 to 1 from the top left), blended between its
// pixels; outside it, its nearest edge.
fn picture(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(fy_picture, fy_linear, uv, 0.0);
}

// The picture's pixel `at`, in the view's pixels from its top left, as it
// is; outside it, its nearest edge.
fn picture_at(at: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(fy_picture));
    return textureLoad(fy_picture, clamp(at, vec2<i32>(0), size - 1), 0);
}

struct FyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// The effect's own textures' sampler: smooth, repeating, as a material's.
@group(1) @binding(5) var fy_texture_sampler: sampler;

// `t` at `uv`, smoothly, repeating past 0 and 1.
fn sample(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(t, fy_texture_sampler, uv, 0.0);
}

// `color` looked up in a colour grade's table, `lut`: a strip of square
// tiles side by side, as many as each is wide (256 × 16, or 1024 × 32),
// red across each tile, green down it, blue from tile to tile, as Unity
// and most image editors make them. Declare it `color_texture`. A table
// that changes nothing gives `color` back.
fn color_lut(lut: texture_2d<f32>, color: vec3<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(lut));
    let n = size.y;
    let at = fy_to_srgb(saturate(color)) * (n - 1.0);
    let tile = min(floor(at.b), n - 2.0);
    let y = (at.g + 0.5) / n;
    let below = textureSampleLevel(lut, fy_texture_sampler, vec2<f32>((tile * n + at.r + 0.5) / size.x, y), 0.0);
    let above = textureSampleLevel(lut, fy_texture_sampler, vec2<f32>(((tile + 1.0) * n + at.r + 0.5) / size.x, y), 0.0);
    return mix(below.rgb, above.rgb, at.b - tile);
}

// Linear light as sRGB stores it.
fn fy_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

// The solid scene's depth: see-through surfaces and the sky aren't in it.
@group(2) @binding(0) var fy_effect_depth: texture_depth_2d;

// Where the solid scene is at `screen` (0 to 1 from the top left, as
// `EffectInput.uv`), in the world: on the camera's far end where only the
// sky is.
fn scene_position(screen: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(fy_effect_depth));
    let pixel = clamp(vec2<i32>(screen * vec2<f32>(size)), vec2<i32>(0), size - 1);
    let depth = textureLoad(fy_effect_depth, pixel, 0);
    let seen = fy_effect.inverse_view_proj * vec4<f32>(screen.x * 2.0 - 1.0, 1.0 - screen.y * 2.0, depth, 1.0);
    return seen.xyz / seen.w;
}

// Where the camera is, in the world.
fn camera_position() -> vec3<f32> {
    return fy_effect.eye.xyz;
}

// How far along the view the solid scene is at `screen`, in metres: the
// camera's far end where only the sky is. Fog, an outline where the
// depth jumps.
fn scene_depth(screen: vec2<f32>) -> f32 {
    return dot(scene_position(screen) - fy_effect.eye.xyz, fy_effect.forward.xyz);
}

// One triangle over the whole view.
@vertex
fn vs_fy_effect(@builtin(vertex_index) i: u32) -> FyOut {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return FyOut(vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(at.x, 1.0 - at.y));
}


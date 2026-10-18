// A screen effect (stage 10): a game's `fn effect(in: EffectInput) ->
// vec4<f32>`, run for each pixel of the finished picture, which it reads
// with `picture`. A camera's effects run in order, each over what the one
// before it made; the last draws onto the view (renderer.rs). Colours are
// as the screen shows them: tone mapped, the fog and the glow in, linear.
// What the shader returns is put on the screen as it is; its alpha is
// dropped.

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

// One triangle over the whole view.
@vertex
fn vs_fy_effect(@builtin(vertex_index) i: u32) -> FyOut {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return FyOut(vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(at.x, 1.0 - at.y));
}


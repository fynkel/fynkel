// The last pass: the scene's half-float image onto the view, one pixel
// for one pixel, with bloom's glow added (fs_bloom). Comes after
// neutral.wgsl.
//
// The image holds each sample tone mapped, as the scene pass writes it, so
// the MSAA resolve averages what the screen shows and edges stay as clean
// as when the scene was drawn straight to the view. Tone mapping is undone
// (it is invertible) where an effect needs the light values.

@group(0) @binding(0) var image: texture_2d<f32>;

// Bloom's first image, half the view's size, its glow added up
// (bloom.wgsl), and how strong it is.
struct Params {
    strength: f32,
    threshold: f32,
    knee: f32,
    levels: f32,
}

@group(1) @binding(0) var glow: texture_2d<f32>;
@group(1) @binding(1) var linear: sampler;
@group(1) @binding(2) var<uniform> params: Params;

// One triangle over the whole view.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(textureLoad(image, vec2<i32>(at.xy), 0).rgb, 1.0);
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(glow, linear, uv, 0.0).rgb;
}

// The glow added to the scene's light, and tone mapped again. Where there
// is none, the pixel is as it was: it's the change tone mapping makes
// that's added, so undoing and redoing it costs no precision.
@fragment
fn fs_bloom(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let mapped = textureLoad(image, vec2<i32>(at.xy), 0).rgb;
    let uv = at.xy / vec2<f32>(textureDimensions(image));
    let t = 1.0 / vec2<f32>(textureDimensions(glow));
    var sum = tap(uv) * 4.0;
    sum += (tap(uv + vec2<f32>(-t.x, 0.0)) + tap(uv + vec2<f32>(t.x, 0.0))) * 2.0;
    sum += (tap(uv + vec2<f32>(0.0, -t.y)) + tap(uv + vec2<f32>(0.0, t.y))) * 2.0;
    sum += tap(uv - t) + tap(uv + t) + tap(uv + vec2<f32>(-t.x, t.y)) + tap(uv + vec2<f32>(t.x, -t.y));
    let added = sum / 16.0 * (params.strength / params.levels);
    let light = untonemap(mapped);
    return vec4<f32>(mapped + tonemap(light + added) - tonemap(light), 1.0);
}

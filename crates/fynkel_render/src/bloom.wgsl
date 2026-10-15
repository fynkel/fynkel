// Bloom's passes (`bloom.rs`), each from one image of the chain into the
// next, half or twice its size. Comes after neutral.wgsl.
//
// The prefilter: the scene image's light, from half white up (by
// `Params`), into the first, half-size image. Down: each into the next,
// smaller. Up: each smaller one, blurred, added into the larger.

struct Params {
    strength: f32,
    threshold: f32,
    knee: f32,
    levels: f32,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle over the whole target.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Out(vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(at.x, 1.0 - at.y));
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(source, linear, uv, 0.0).rgb;
}

// The scene image's light at `uv`: its alpha says how much of it is light
// (not a colour shown as picked).
fn light(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(source, linear, uv, 0.0);
    return untonemap(c.rgb) * clamp(c.a, 0.0, 1.0);
}

// The part of `c` that glows: what's above the threshold, eased in over
// `knee` above it, so light just over it glows only a little.
fn bright(c: vec3<f32>) -> vec3<f32> {
    let brightness = max(c.r, max(c.g, c.b));
    let over = max(brightness - params.threshold, 0.0);
    var glows = over - 0.5 * params.knee;
    if over < params.knee {
        glows = over * over / (2.0 * params.knee);
    }
    return c * glows / max(brightness, 1e-5);
}

// Light from the scene image: four taps, each the average of four pixels,
// weighed down where bright (Karis), so a lone bright pixel doesn't
// flicker as it moves.
@fragment
fn fs_prefilter(in: Out) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let offset = vec2<f32>(f32(k & 1u), f32(k >> 1u)) * 2.0 - 1.0;
        let c = bright(light(in.uv + offset * texel));
        let w = 1.0 / (1.0 + max(c.r, max(c.g, c.b)));
        sum += c * w;
        weights += w;
    }
    return vec4<f32>(sum / weights, 1.0);
}

// Thirteen taps, as Call of Duty: Advanced Warfare's bloom: a box round
// the centre and four round it, weighed so edges don't shimmer.
@fragment
fn fs_down(in: Out) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let uv = in.uv;
    let a = tap(uv + t * vec2<f32>(-2.0, -2.0));
    let b = tap(uv + t * vec2<f32>(0.0, -2.0));
    let c = tap(uv + t * vec2<f32>(2.0, -2.0));
    let d = tap(uv + t * vec2<f32>(-2.0, 0.0));
    let e = tap(uv);
    let f = tap(uv + t * vec2<f32>(2.0, 0.0));
    let g = tap(uv + t * vec2<f32>(-2.0, 2.0));
    let h = tap(uv + t * vec2<f32>(0.0, 2.0));
    let i = tap(uv + t * vec2<f32>(2.0, 2.0));
    let j = tap(uv + t * vec2<f32>(-1.0, -1.0));
    let k = tap(uv + t * vec2<f32>(1.0, -1.0));
    let l = tap(uv + t * vec2<f32>(-1.0, 1.0));
    let m = tap(uv + t * vec2<f32>(1.0, 1.0));
    let sum = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
    return vec4<f32>(sum, 1.0);
}

// Nine taps, a tent a texel of the smaller image wide, added (by the
// pipeline's blending) to the larger.
fn tent(uv: vec2<f32>) -> vec3<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    var sum = tap(uv) * 4.0;
    sum += (tap(uv + vec2<f32>(-t.x, 0.0)) + tap(uv + vec2<f32>(t.x, 0.0))) * 2.0;
    sum += (tap(uv + vec2<f32>(0.0, -t.y)) + tap(uv + vec2<f32>(0.0, t.y))) * 2.0;
    sum += tap(uv - t) + tap(uv + t) + tap(uv + vec2<f32>(-t.x, t.y)) + tap(uv + vec2<f32>(t.x, -t.y));
    return sum / 16.0;
}

@fragment
fn fs_up(in: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(tent(in.uv), 1.0);
}

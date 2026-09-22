
// Bloom (R3b): light brighter than the threshold spreads into its
// neighbourhood. A bright-pass takes the HDR frame into the first level
// of a half-size mip chain; each further level is a 13-tap downsample of
// the one above; then the chain is walked back up, each level a 9-tap
// tent of the coarser one blended over it by `scatter` (the pipeline's
// blend constant). The post pass adds level 0 to the exposed radiance
// before tone mapping. Everything here is exposed radiance: 1 is white.

@group(1) @binding(0) var bloom_sampler: sampler;
@group(1) @binding(1) var bloom_source: texture_2d<f32>;

// Exposed radiance is clamped here so a specular spike cannot carry an
// f16 sum to infinity.
const BLOOM_CLAMP: f32 = 1024.0;

struct BloomOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> BloomOut {
    var out: BloomOut;
    let p = fullscreen(id);
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// One HDR texel's exposed, scene-referred radiance. The alpha is the post
// pass's tag: the background (0) and display-referred texels (> 1.5, the
// unlit and the debug lines) do not glow; a partly covered texel's
// radiance is premultiplied by its coverage, which is what it emits.
fn exposed(p: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(bloom_source)) - vec2<i32>(1);
    let t = textureLoad(bloom_source, clamp(p, vec2<i32>(0), last), 0);
    if t.a < 0.001 || t.a > 1.5 {
        return vec3<f32>(0.0);
    }
    return min(t.rgb * frame.camera_pos.w, vec3<f32>(BLOOM_CLAMP));
}

// The bright-pass: the 2×2 HDR texels under this half-size texel,
// averaged with Karis's 1 / (1 + luma) weights so a lone bright texel
// cannot flicker as it moves, then the soft threshold — nothing below
// `threshold − knee`, a quadratic ramp through the knee, and everything
// above `threshold` kept less the threshold.
@fragment
fn fs_prefilter(in: BloomOut) -> @location(0) vec4<f32> {
    let base = vec2<i32>(in.clip.xy) * 2;
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    for (var i = 0; i < 4; i++) {
        let c = exposed(base + vec2<i32>(i & 1, i >> 1u));
        let w = 1.0 / (1.0 + luma(c));
        sum += c * w;
        weights += w;
    }
    let c = sum / weights;
    let threshold = frame.bloom.x;
    let knee = frame.bloom.y;
    let brightness = max(c.r, max(c.g, c.b));
    var soft = clamp(brightness - threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 1e-5);
    let keep = max(soft, brightness - threshold) / max(brightness, 1e-5);
    return vec4<f32>(c * keep, 1.0);
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(bloom_source, bloom_sampler, uv, 0.0).rgb;
}

// Jimenez's 13-tap downsample (Call of Duty: Advanced Warfare): four
// overlapping 2×2 boxes and a centre one, weighted so the result has no
// box-filter pulsing as the image moves.
@fragment
fn fs_down(in: BloomOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(bloom_source));
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
    let down = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625
        + (j + k + l + m) * 0.125;
    return vec4<f32>(down, 1.0);
}

// A 3×3 tent over the coarser level, one of its texels wide; the blend
// state mixes it over this level by `scatter`.
@fragment
fn fs_up(in: BloomOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(bloom_source));
    let uv = in.uv;
    var up = tap(uv) * 4.0;
    up += (tap(uv + vec2<f32>(-t.x, 0.0)) + tap(uv + vec2<f32>(t.x, 0.0))
        + tap(uv + vec2<f32>(0.0, -t.y)) + tap(uv + vec2<f32>(0.0, t.y))) * 2.0;
    up += tap(uv + vec2<f32>(-t.x, -t.y)) + tap(uv + vec2<f32>(t.x, -t.y))
        + tap(uv + vec2<f32>(-t.x, t.y)) + tap(uv + vec2<f32>(t.x, t.y));
    return vec4<f32>(up / 16.0, 1.0);
}

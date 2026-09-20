// Presentation pass: the tone-mapped frame, rendered at the quality
// profile's render scale, resampled (bilinear) to the host's target.
// Skipped at scale 1, where the post pass writes the target directly.

@group(1) @binding(0) var ldr_sampler: sampler;
@group(1) @binding(1) var ldr_tex: texture_2d<f32>;

struct PresentOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> PresentOut {
    var out: PresentOut;
    let p = fullscreen(id);
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

@fragment
fn fs_main(in: PresentOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(ldr_tex, ldr_sampler, in.uv).rgb, 1.0);
}

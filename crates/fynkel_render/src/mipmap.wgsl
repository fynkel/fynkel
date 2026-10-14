// Draws a texture's next mip level from the one above it: one triangle
// covering the level, sampling the level above with a linear filter.
//
// Each pixel of the new level sits where four pixels of the level above
// meet, so one bilinear sample averages them. In an sRGB texture the
// hardware decodes to linear before filtering and encodes after, so the
// average is taken in linear light.
//
// Alpha is then scaled by the level's `alpha_scale` (texture.rs,
// `alpha_scales`), so a cut-out keeps its coverage as it shrinks.

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
// x: what this level's alpha is multiplied by.
@group(0) @binding(2) var<uniform> alpha_scale: vec4<f32>;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    // (0, 0), (2, 0), (0, 2): a triangle twice the screen's size, so the
    // screen is inside it.
    let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOut;
    out.clip = vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    out.uv = p;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(source, source_sampler, in.uv, 0.0);
    return vec4<f32>(c.rgb, min(c.a * alpha_scale.x, 1.0));
}

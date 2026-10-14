// Shadow maps: casters' depth as a light sees them (shadows.rs). Opaque
// casters write depth only, with no fragment shader; cut-outs drop what
// their alpha cuts away, as the forward pass does.

struct View {
    // World to map: x and y from -1 to 1 across, depth from 0 to 1.
    matrix: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;

// A material's textures (forward.wgsl's group 1): only the base colour's
// alpha is read.
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(5) var material_sampler: sampler;

struct In {
    @location(0) position: vec3<f32>,
    @location(3) uv: vec2<f32>,
    // Per caster (instance step):
    @location(4) model_0: vec4<f32>,
    @location(5) model_1: vec4<f32>,
    @location(6) model_2: vec4<f32>,
    @location(7) model_3: vec4<f32>,
    // xy: UV scale, zw: UV offset.
    @location(8) uv_transform: vec4<f32>,
    // x: the base colour's alpha, y: the alpha cutoff.
    @location(9) alpha: vec4<f32>,
};

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) alpha: vec2<f32>,
};

// A caster's vertex in the map.
fn caster_vertex(in: In) -> Out {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    var out: Out;
    out.clip = view.matrix * model * vec4<f32>(in.position, 1.0);
    out.uv = in.uv * in.uv_transform.xy + in.uv_transform.zw;
    out.alpha = in.alpha.xy;
    return out;
}

@vertex
fn vs_main(in: In) -> Out {
    return caster_vertex(in);
}

// A cascade's: as `vs_main`, but a caster nearer the sun than the map's
// depth range reaches is flattened onto its near end, so it still casts
// (pancaking; the map is orthographic, so w is 1).
@vertex
fn vs_cascade(in: In) -> Out {
    var out = caster_vertex(in);
    out.clip.z = max(out.clip.z, 0.0);
    return out;
}

// Mips average alpha, so where the texture is small in the map its alpha
// is the share of each texel's area the caster covers, and may sit below
// the cutoff everywhere. From the second mip on, that share of the map's
// texels is kept instead, by a dither, and the filter softens it into
// about that much shadow (forward.wgsl's `fs_cutout` does as much).
@fragment
fn fs_cutout(in: Out) {
    let alpha = in.alpha.x * textureSample(base_color_texture, material_sampler, in.uv).a;
    let size = vec2<f32>(textureDimensions(base_color_texture));
    let dx = dpdx(in.uv * size);
    let dy = dpdy(in.uv * size);
    let mip = max(0.0, 0.5 * log2(max(dot(dx, dx), dot(dy, dy))));
    // Interleaved gradient noise (Jimenez): an even spread from 0 to 1.
    let dither = fract(52.9829189 * fract(dot(in.clip.xy, vec2<f32>(0.06711056, 0.00583715))));
    if alpha < mix(in.alpha.y, dither, saturate(mip - 1.0)) {
        discard;
    }
}

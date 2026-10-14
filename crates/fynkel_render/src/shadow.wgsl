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

@vertex
fn vs_main(in: In) -> Out {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    var out: Out;
    out.clip = view.matrix * model * vec4<f32>(in.position, 1.0);
    out.uv = in.uv * in.uv_transform.xy + in.uv_transform.zw;
    out.alpha = in.alpha.xy;
    return out;
}

@fragment
fn fs_cutout(in: Out) {
    let alpha = in.alpha.x * textureSample(base_color_texture, material_sampler, in.uv).a;
    if alpha < in.alpha.y {
        discard;
    }
}

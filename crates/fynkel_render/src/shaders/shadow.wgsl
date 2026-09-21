// Shadow pass (docs/renderer.md §"Passes"): depth only, from the sun's
// orthographic view, for every caster instance. `alphaMode` mask casters
// discard through their base colour; the material bind group is the
// mesh pass's (only the sampler and base colour map are read).

struct ShadowFrame {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> shadow_frame: ShadowFrame;
@group(1) @binding(0) var material_sampler: sampler;
@group(1) @binding(1) var base_color_tex: texture_2d<f32>;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(3) uv0: vec2<f32>,
    @location(5) color: vec4<f32>,
    @location(6) m0: vec4<f32>,
    @location(7) m1: vec4<f32>,
    @location(8) m2: vec4<f32>,
    @location(9) m3: vec4<f32>,
    @location(10) tint: vec4<f32>,
    // Only `w` is read: a skinned instance's first joint.
    @location(11) emissive: vec4<f32>,
    @location(13) flags: vec4<f32>,
};

// The joint palettes and the skinned mesh's weights, as in mesh.wgsl.
@group(2) @binding(0) var<storage, read> joints: array<mat4x4<f32>>;

struct VsSkin {
    @location(14) joints: vec4<u32>,
    @location(15) weights: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // x: alpha factor (tint × vertex colour); y: cutoff or -1.
    @location(1) mask: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    return vertex(in, model);
}

@vertex
fn vs_skinned(in: VsIn, skin: VsSkin) -> VsOut {
    let base = u32(in.emissive.w);
    let last = arrayLength(&joints) - 1u;
    let pose = skin.weights.x * joints[min(base + skin.joints.x, last)]
        + skin.weights.y * joints[min(base + skin.joints.y, last)]
        + skin.weights.z * joints[min(base + skin.joints.z, last)]
        + skin.weights.w * joints[min(base + skin.joints.w, last)];
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3) * pose;
    return vertex(in, model);
}

fn vertex(in: VsIn, model: mat4x4<f32>) -> VsOut {
    var out: VsOut;
    out.clip = shadow_frame.view_proj * model * vec4<f32>(in.position, 1.0);
    out.uv = in.uv0;
    out.mask = vec2<f32>(in.tint.a * in.color.a, in.flags.x);
    return out;
}

@fragment
fn fs_main(in: VsOut) {
    if in.mask.y >= 0.0 {
        let a = textureSample(base_color_tex, material_sampler, in.uv).a * in.mask.x;
        if a < in.mask.y {
            discard;
        }
    }
}

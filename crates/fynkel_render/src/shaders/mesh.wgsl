// The mesh vertex stage (docs/renderer.md): the cooked vertex and its
// instance (`mesh_input.wgsl`) placed in the world, static or skinned.
// Every mesh surface — the built-in (`pbr.wgsl`) and each shader asset
// (ADR-0031) — is drawn over this one stage, lit by `light.wgsl`.

struct VsOut {
    // Invariant so the depth prepass and the scene pass compute the same
    // clip position to the bit: they are two pipelines over this one
    // vertex stage, and the scene pass's depth compare is `LessEqual`
    // against what the prepass wrote (R2).
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) emissive: vec4<f32>,
    @location(6) params: vec4<f32>,
    @location(7) flags: vec4<f32>,
    @location(8) @interpolate(flat) lights: vec4<u32>,
    @location(9) uv1: vec2<f32>,
};

// Every instance's light list (E1 light culling): the pool indices that
// reach it, one per byte — the first four in `x`, the next four in `y` —
// and how many in `z`. An instance's own slot is `flags.z`; there is no
// room for this among the vertex attributes (the pipeline's sixteen are
// all taken), and the stage reads it once per vertex.
@group(2) @binding(1) var<storage, read> instance_lights: array<vec4<u32>>;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    return fynkel_place(in, model);
}

// The skinned stage: the vertex posed in mesh space by its weighted
// joint matrices (`fynkel_pose`), then placed like any other.
@vertex
fn vs_skinned(in: VsIn, skin: VsSkin) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3) * fynkel_pose(in, skin);
    return fynkel_place(in, model);
}

fn fynkel_place(in: VsIn, model: mat4x4<f32>) -> VsOut {
    var out: VsOut;
    let world = model * vec4<f32>(in.position, 1.0);
    out.clip = frame.view_proj * world;
    out.world_pos = world.xyz;
    // Normals under non-uniform scale: normalising after the linear part
    // (no inverse transpose) — V1 accepts the skew.
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    let t = (model * vec4<f32>(in.tangent.xyz, 0.0)).xyz;
    out.world_tangent = vec4<f32>(t, in.tangent.w);
    out.uv = in.uv0;
    out.uv1 = in.uv1;
    out.color = in.tint * in.color;
    out.emissive = in.emissive;
    out.params = in.params;
    out.flags = in.flags;
    out.lights = instance_lights[u32(in.flags.z)];
    return out;
}

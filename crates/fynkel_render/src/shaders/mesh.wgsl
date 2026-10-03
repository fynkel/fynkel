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

// Each instance owns three vec4<u32> records: eight full-width light indices,
// then count plus padding. The vertex forwards the offset and count; fragment
// shading reads only that object's bounded list from storage.
@group(2) @binding(1) var<storage, read> instance_lights: array<vec4<u32>>;
@group(2) @binding(2) var<storage, read> local_lights: array<Light>;

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
    let list_offset = u32(in.flags.z) * 3u;
    out.lights = vec4<u32>(list_offset, 0u, instance_lights[list_offset + 2u].x, 0u);
    return out;
}

// The mesh vertex input (docs/renderer.md): the cooked vertex, its
// instance and, for a skinned mesh, its weights and the frame's joint
// palettes — shared by the scene's stages (`mesh.wgsl`) and a shader
// asset's shadow stage (ADR-0031).

struct VsIn {
    // The cooked 72-byte vertex (fynkel_cooked::payload::Vertex).
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv0: vec2<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) color: vec4<f32>,
    // Per-instance (fynkel_render::Instance): model matrix columns, base
    // colour factor, emissive (rgb × strength, nits-relative), params
    // (metallic, roughness, normal_scale, occlusion_strength), flags
    // (alpha cutoff, -1 opaque or -2 blend; unlit; the slot of this
    // instance's light list; bits: 1 receives shadows, 2 grey — an extra
    // world drawn greyed).
    @location(6) m0: vec4<f32>,
    @location(7) m1: vec4<f32>,
    @location(8) m2: vec4<f32>,
    @location(9) m3: vec4<f32>,
    @location(10) tint: vec4<f32>,
    @location(11) emissive: vec4<f32>,
    @location(12) params: vec4<f32>,
    @location(13) flags: vec4<f32>,
};

// The frame's joint palettes (M3-19): every skinned instance's matrices
// back to back, mesh space → posed joint → mesh space; an instance's
// first joint is `emissive.w`.
@group(2) @binding(0) var<storage, read> joints: array<mat4x4<f32>>;

// A skinned mesh's third vertex buffer: the cooked SkinWeight (four
// joint indices into the instance's palette, four weights summing to 1).
struct VsSkin {
    @location(14) joints: vec4<u32>,
    @location(15) weights: vec4<f32>,
};

// The skinned pose of a vertex in mesh space: its weighted joint
// matrices. An index past the palette (a mesh cooked against another
// skin) reads the last matrix instead of out of bounds.
fn fynkel_pose(in: VsIn, skin: VsSkin) -> mat4x4<f32> {
    let base = u32(in.emissive.w);
    let last = arrayLength(&joints) - 1u;
    return skin.weights.x * joints[min(base + skin.joints.x, last)]
        + skin.weights.y * joints[min(base + skin.joints.y, last)]
        + skin.weights.z * joints[min(base + skin.joints.z, last)]
        + skin.weights.w * joints[min(base + skin.joints.w, last)];
}

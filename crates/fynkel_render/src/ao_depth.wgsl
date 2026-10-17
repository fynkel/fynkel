// Light ambient occlusion's depth (ao.rs): the solid scene's, at half the
// view's size, with no fragment shader.

struct Frame {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

struct In {
    @location(0) position: vec3<f32>,
    // The model matrix's three rows (forward.wgsl's).
    @location(4) model_0: vec4<f32>,
    @location(5) model_1: vec4<f32>,
    @location(6) model_2: vec4<f32>,
    // w: where a skinned draw's joints start.
    @location(8) params: vec4<f32>,
};

fn depth(model: array<vec4<f32>, 3>, position: vec3<f32>) -> vec4<f32> {
    let p = vec4<f32>(position, 1.0);
    let world = vec4<f32>(dot(model[0], p), dot(model[1], p), dot(model[2], p), 1.0);
    return frame.view_proj * world;
}

@vertex
fn vs_depth(in: In) -> @builtin(position) vec4<f32> {
    return depth(array<vec4<f32>, 3>(in.model_0, in.model_1, in.model_2), in.position);
}

// A skinned mesh's (skin.wgsl).
@vertex
fn vs_depth_skinned(in: In, skin: SkinIn) -> @builtin(position) vec4<f32> {
    return depth(skin_rows(skin, u32(in.params.w)), in.position);
}

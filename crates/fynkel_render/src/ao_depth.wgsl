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
};

@vertex
fn vs_depth(in: In) -> @builtin(position) vec4<f32> {
    let p = vec4<f32>(in.position, 1.0);
    let world = vec4<f32>(dot(in.model_0, p), dot(in.model_1, p), dot(in.model_2, p), 1.0);
    return frame.view_proj * world;
}

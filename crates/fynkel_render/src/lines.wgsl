// Debug lines: drawn after the scene, on top of it (no depth test), unlit,
// in their own linear colour, whose alpha blends over what's there.

struct Frame {
    view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    to_sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> Out {
    return Out(frame.view_proj * vec4(position, 1.0), color);
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return in.color;
}

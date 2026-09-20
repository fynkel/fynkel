// Debug lines (fynkel_render::debug): world-space segments, depth tested
// against the scene and not written, display-referred colour (HDR alpha
// tag 2: the post pass clamps and encodes, no exposure).

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = frame.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(clamp(in.color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 2.0);
}

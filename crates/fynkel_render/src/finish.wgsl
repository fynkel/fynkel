// The last pass: the scene's half-float image onto the view, one pixel
// for one pixel. Later effects (bloom) work here, on the light values.
//
// The image holds each sample tone mapped, as the scene pass writes it, so
// the MSAA resolve averages what the screen shows and edges stay as clean
// as when the scene was drawn straight to the view. Tone mapping is undone
// (it is invertible) where an effect needs the light values.

@group(0) @binding(0) var image: texture_2d<f32>;

// One triangle over the whole view.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(textureLoad(image, vec2<i32>(at.xy), 0).rgb, 1.0);
}

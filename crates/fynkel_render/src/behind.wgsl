// What's behind see-through surfaces, for shaders that read it
// (renderer.rs): in a frame that has one, the solid scene's depth is kept
// and one sample of it copied into an image they read (fs_depth); its
// colour, resolved into another, is put back into the multisampled target
// for the see-through draws to go over (fs_restore).

// One triangle over the whole view.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

// As floats, not depth: Mali can't compile a read of multisampled depth.
// Drawn with one sample, it's a texture_2d<f32> (renderer.rs).
@group(0) @binding(0) var scene_depth: texture_multisampled_2d<f32>;

@fragment
fn fs_depth(@builtin(position) at: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(scene_depth, vec2<i32>(at.xy), 0).r;
}

@group(0) @binding(0) var scene_color: texture_2d<f32>;

@fragment
fn fs_restore(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(scene_color, vec2<i32>(at.xy), 0);
}

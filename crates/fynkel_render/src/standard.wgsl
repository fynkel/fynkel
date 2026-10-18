// The built-in material: its factors times its textures, lit
// (forward.wgsl). A material's shader has its own (shader.rs).
fn shade(in: VertexOut, front_facing: bool) -> vec4<f32> {
    return lit(in, front_facing, standard_surface(in, front_facing));
}

// It doesn't move its vertices.
fn fy_moved(in: VertexIn, model: array<vec4<f32>, 3>, out: VertexOut) -> VertexOut {
    return out;
}

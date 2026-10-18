// A jelly that wobbles: its surface pushed out and in along the way it
// faces, as a wave runs up it, more after a poke (each jelly its own).
struct PerObject { poke: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let wave = sin(in.local.y * 9.0 - frame_time() * 6.0);
    v.position += in.normal * wave * (0.02 + 0.12 * per_object.poke);
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.roughness = 0.15;
    s.emission = s.color * fresnel(in, 3.0) * 0.4;
    return s;
}

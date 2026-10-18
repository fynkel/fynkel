// Grass in the wind: each blade bends from its foot, more towards its
// tip, with a sway of its own from where it stands, and gusts that run
// across the field.
struct Values { wind: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let up = saturate(in.local.y);
    let t = frame_time();
    let gust = sin(t * 0.9 - in.origin.x * 0.35) * 0.5 + 0.5;
    let sway = sin(t * 2.3 + in.origin.x * 1.7 + in.origin.z * 1.3) * 0.35 + gust * 0.65;
    let bend = sway * values.wind * up * up;
    v.position.x += bend;
    v.position.z += bend * 0.3;
    v.position.y -= bend * bend * 0.4;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    // Darker at the root, lighter at the tip.
    s.color *= mix(1.0, 0.3, in.uv.y);
    return s;
}

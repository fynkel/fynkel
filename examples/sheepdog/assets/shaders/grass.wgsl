// The field's grass in the wind. A patch is one mesh of many blades:
// each bends from its foot, more towards its tip, with a sway of its own
// from where it stands, and gusts run across the field. v is 1 at a
// blade's foot and 0 at its tip.
struct Values { wind: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let up = 1.0 - saturate(in.uv.y);
    let t = frame_time();
    let p = in.position.xz;
    let gust = sin(t * 0.8 - p.x * 0.3 - p.y * 0.1) * 0.5 + 0.5;
    // Smooth across the field, so a blade's two sides sway together.
    let sway = sin(t * 2.1 + noise(p * 1.5) * 12.0) * 0.3 + gust * 0.7;
    let bend = sway * values.wind * up * up;
    v.position.x += bend;
    v.position.z += bend * 0.3;
    v.position.y -= bend * bend * 0.5;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    // Lit as the ground is, from whichever side it's seen.
    s.normal = vec3<f32>(0.0, 1.0, 0.0);
    // Darker at the root, lighter at the tip.
    let tint = mix(vec3<f32>(1.0), vec3<f32>(1.15, 1.1, 0.55), noise(in.position.xz * 0.7));
    s.color *= mix(1.7, 0.75, in.uv.y) * tint;
    return s;
}

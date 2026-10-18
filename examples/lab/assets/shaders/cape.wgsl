// A cape in the wind, moved in its own space so it blows out behind its
// wearer whichever way they face: nothing at the shoulders, most at the
// hem, rippling across.
struct Values { wind: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    // KayKit's cape hangs from y = 0 down to about -1.1.
    let hang = saturate(-in.local.y / 1.1);
    let t = frame_time();
    let ripple = sin(t * 3.0 + in.local.x * 4.0 - in.local.y * 3.0);
    v.local.z -= hang * hang * (0.2 + 0.1 * ripple) * values.wind;
    v.local.y += hang * hang * 0.08 * values.wind;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    return standard(in);
}

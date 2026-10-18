// A sheep bitten by a wolf: it flashes white and red (its own `flash`, 0
// to 1), the rest of the flock as it was.
struct PerObject { flash: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.color = mix(s.color, vec3<f32>(1.0), per_object.flash);
    s.emission += vec3<f32>(1.0, 0.2, 0.1) * per_object.flash * 2.5;
    return s;
}

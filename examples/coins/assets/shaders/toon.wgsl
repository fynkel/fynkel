// The toon style: each sun and lamp lights in a few flat bands, with no
// highlight, the shadow side never quite black. The surface is the
// material's own; the outline is the material's.
struct Values { bands: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.metallic = 0.0;
    return s;
}

fn light(in: LightInput) -> vec3<f32> {
    let lit = max(dot(in.normal, in.direction), 0.0) * in.shadow;
    let steps = max(values.bands, 1.0);
    let band = mix(0.35, 1.0, floor(lit * steps + 0.5) / steps);
    // Divided by pi, as the engine's own diffuse light is.
    return in.surface.color * in.color * band / 3.14159265;
}

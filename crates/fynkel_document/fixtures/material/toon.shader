// A toon shader: the colour in bands, from a ramp texture.
shader "Studio/Toon" {
    alpha opaque
    properties {
        bands   int    = 3  range(1, 8)
        tint    color3 = (1, 1, 1)  range(0, 1)
        outline bool   = true
        ramp    texture linear required
    }
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    let light = floor(max(dot(in.world_normal, vec3<f32>(0.0, 1.0, 0.0)), 0.0) * f32(material.bands));
    s.base_color = material.tint * sample_ramp(vec2<f32>(light / f32(material.bands), 0.5)).rgb;
    return s;
}

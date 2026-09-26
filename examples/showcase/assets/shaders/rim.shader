// Rim light: the edges of the object glow, as if lit from behind. The
// glow is the fresnel term — how far the surface turns away from the
// camera — put in the emission, so it glows in the dark and blooms.
shader "Examples/Rim Light" {
    lighting lit
    alpha opaque
    properties {
        base_color    color3  = (0.04, 0.05, 0.08)
        rim_color     color3  = (0.25, 0.65, 1)
        rim_power     float   = 3     range(0.5, 10)  "Higher: a thinner rim"
        rim_strength  float   = 4     range(0, 50)    "The rim's brightness"
        pulse         float   = 0     range(0, 5)     "Pulses per second; 0 holds it steady"
        metallic      float   = 0.2   range(0, 1)
        roughness     float   = 0.35  range(0, 1)
    }
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    s.base_color = material.base_color;
    s.metallic = material.metallic;
    s.roughness = material.roughness;

    let facing = clamp(dot(in.world_normal, in.view_dir), 0.0, 1.0);
    let rim = pow(1.0 - facing, material.rim_power);
    let beat = 0.75 + 0.25 * cos(globals.time * material.pulse * 6.2831853);
    s.emission = material.rim_color * material.rim_strength * rim * beat;
    return s;
}

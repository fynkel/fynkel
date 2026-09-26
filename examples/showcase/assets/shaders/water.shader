// Water: a see-through surface whose ripples move with time. Two copies
// of one tiling normal map (textures/ripples.png, from tools/ripples.py)
// scroll against each other, so the pattern never repeats as it moves
// (a normal map is data: its sidecar says `"colorSpace": "linear"`);
// vertex() lifts the plane into a slow swell. The engine lights it as
// any lit surface: the sky and the sun are what shine on it.
shader "Examples/Water" {
    lighting lit
    alpha opaque, blend
    shadows off
    properties {
        deep       color   = (0.02, 0.10, 0.16, 0.8)  "Colour of deep water; alpha is how opaque it is"
        shallow    color3  = (0.05, 0.32, 0.36)        "Colour where it is seen from above"
        ripples    texture = normal
        tiling     float   = 2     range(0.1, 50)      "Ripple tiles across the surface"
        flow       vec2    = (0.02, 0.012)             "How fast the ripples move (tiles per second)"
        strength   float   = 0.6   range(0, 2)         "How strong the ripples are"
        swell      float   = 0.05  range(0, 1)         "Height of the swell in metres"
        roughness  float   = 0.08  range(0, 1)
    }
}

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let t = globals.time;
    let p = in.position.xz;
    v.position.y += material.swell * (sin(p.x * 0.9 + t * 1.1) + 0.6 * sin(p.y * 1.3 - t * 0.8));
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    let uv = in.uv0 * material.tiling;
    let t = globals.time;
    let a = sample_ripples(uv + t * material.flow).xyz * 2.0 - 1.0;
    let b = sample_ripples(uv * 1.37 + vec2(0.31, 0.77) - t * material.flow.yx * 1.4).xyz * 2.0 - 1.0;
    // Add the slopes, keep the up: the two ripple patterns together.
    let n = normalize(vec3((a.xy + b.xy) * material.strength, a.z * b.z));
    s.normal = n;

    // Seen from above it is lighter; toward the horizon, deep (and the
    // sky's reflection, from the lighting, takes over).
    let facing = clamp(dot(in.world_normal, in.view_dir), 0.0, 1.0);
    s.base_color = mix(material.deep.rgb, material.shallow, facing * facing);
    s.alpha = mix(1.0, material.deep.a, facing);
    s.metallic = 0.0;
    s.roughness = material.roughness;
    return s;
}

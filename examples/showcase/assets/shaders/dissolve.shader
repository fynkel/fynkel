// Dissolve: the surface burns away through noise, with a glowing edge.
// `amount` goes from 0 (whole) to 1 (gone); game.Dissolve
// (lib/game/dissolve.dart) drives it from code with
// world.setMaterialValue. A mask material: a pixel whose alpha is under
// the cutoff is not drawn, so the holes are real holes and cast no shadow.
shader "Examples/Dissolve" {
    lighting lit
    alpha mask
    properties {
        base_color  color   = (0.8, 0.8, 0.8, 1)   "Multiplies the base map"
        base_map    texture = white
        amount      float   = 0     range(0, 1)    "0 whole, 1 gone"
        scale       float   = 3     range(0.1, 20) "Noise cells per metre"
        edge_width  float   = 0.08  range(0, 0.5)  "How far the glow reaches into the surface"
        edge_color  color3  = (1, 0.35, 0.05)
        edge_glow   float   = 6     range(0, 50)   "The edge's brightness (bloom picks it up)"
        metallic    float   = 0     range(0, 1)
        roughness   float   = 0.6   range(0, 1)
    }
}

// A value noise in 3D: a random number at every lattice corner, blended
// smoothly between them.
fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * 0.1031);
    let r = q + dot(q, q.yzx + 33.33);
    return fract((r.x + r.y) * r.z);
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash3(i + vec3(0.0, 0.0, 0.0)), hash3(i + vec3(1.0, 0.0, 0.0)), u.x),
                mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), u.x),
                mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + vec3(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

// Three octaves: large blotches with a ragged border.
fn fractal_noise(p: vec3<f32>) -> f32 {
    return value_noise(p) * 0.55 + value_noise(p * 2.03) * 0.3 + value_noise(p * 4.1) * 0.15;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    let c = material.base_color * sample_base_map(in.uv0);
    s.base_color = c.rgb;
    s.metallic = material.metallic;
    s.roughness = material.roughness;

    // The noise is over the world position, so it runs across the faces
    // without seams (the object stands still while it dissolves). Spread
    // the threshold a little past 0 and 1 so 0 is whole and 1 is gone.
    let n = fractal_noise(in.world_pos * material.scale);
    let d = n - (material.amount * 1.2 - 0.1);
    s.alpha = select(0.0, 1.0, d > 0.0);

    // The rim of each hole glows, hottest at the border.
    if material.amount > 0.0 && d < material.edge_width {
        let heat = 1.0 - d / max(material.edge_width, 1e-4);
        s.emission = material.edge_color * material.edge_glow * heat * heat;
        s.base_color = s.base_color * (1.0 - heat);
    }
    return s;
}

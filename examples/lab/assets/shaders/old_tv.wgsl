// An old TV: the picture bent like a tube's glass, dark at its edges,
// with scan lines and a faint flicker.
struct Values { lines: f32, curve: f32 }

fn effect(in: EffectInput) -> vec4<f32> {
    let c = in.uv - 0.5;
    let uv = 0.5 + c * (1.0 + values.curve * dot(c, c));
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let lines = 0.75 + 0.25 * cos(uv.y * values.lines * 6.2831853);
    let dark = 1.0 - 1.6 * dot(c, c);
    let flicker = 0.97 + 0.03 * sin(in.time * 60.0);
    return vec4<f32>(picture(uv).rgb * lines * dark * flicker, 1.0);
}

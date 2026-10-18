// A colour grade: each colour looked up in a table, as a film's colourist
// or an image editor makes them (lib/scenes/effects.dart makes this one in
// code: teal shadows, warm highlights). `amount` mixes it in.

struct Values { amount: f32 }
var table: color_texture;

fn effect(in: EffectInput) -> vec4<f32> {
    let seen = picture(in.uv).rgb;
    return vec4<f32>(mix(seen, color_lut(table, seen), values.amount), 1.0);
}

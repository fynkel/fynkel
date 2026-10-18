// A hit: the picture's edges flash red and it shakes a little sideways,
// as `amount` goes from 1 back to 0.
struct Values { amount: f32 }

fn effect(in: EffectInput) -> vec4<f32> {
    let shake = sin(in.time * 90.0) * 0.008 * values.amount;
    let seen = picture(in.uv + vec2<f32>(shake, 0.0)).rgb;
    let c = in.uv - 0.5;
    let edge = smoothstep(0.1, 0.5, length(c)) * values.amount;
    return vec4<f32>(mix(seen, vec3<f32>(0.9, 0.05, 0.05), edge), 1.0);
}

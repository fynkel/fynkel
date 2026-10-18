// A scanner's pulse: a ring of light sweeping out from the camera over
// everything solid, read from the scene's depth. The sky isn't touched.

struct Values { color: vec4<f32>, speed: f32, every: f32 }

fn effect(in: EffectInput) -> vec4<f32> {
    let seen = picture(in.uv).rgb;
    let far = distance(scene_position(in.uv), camera_position());
    // Where the ring is now, in metres from the camera.
    let ring = fract(in.time / values.every) * values.every * values.speed;
    let behind = ring - far;
    // A sharp front, and a trail fading behind it.
    let front = smoothstep(0.15, 0.0, abs(behind));
    let trail = select(0.0, exp(-behind * 2.0) * 0.2, behind > 0.0);
    // Not the sky, which is as far as the camera sees.
    let glow = (front + trail) * step(scene_depth(in.uv), 500.0);
    return vec4<f32>(seen + values.color.rgb * glow, 1.0);
}

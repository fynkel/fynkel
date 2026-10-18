// A hologram: unlit, lines running up it and its edges bright. Drawn
// additive, so black adds nothing.
fn unlit(in: SurfaceInput) -> vec4<f32> {
    let lines = step(0.5, fract(in.position.y * 14.0 - frame_time() * 1.5));
    let rim = fresnel(in, 2.0);
    let flicker = 0.85 + 0.15 * noise(vec2<f32>(frame_time() * 9.0, 0.0));
    return vec4<f32>(in.color.rgb * (rim * 1.6 + 0.25 * lines) * flicker, 1.0);
}

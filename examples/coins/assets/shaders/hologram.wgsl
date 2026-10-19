// A coin as a hologram: unlit, its colour in lines running up it, its
// edges bright, flickering a little. Drawn additive, so black adds
// nothing and what's behind shows through.
fn unlit(in: SurfaceInput) -> vec4<f32> {
    let lines = smoothstep(0.3, 0.7, fract(in.position.y * 12.0 - frame_time() * 1.2));
    let rim = fresnel(in, 2.5);
    let flicker = 0.9 + 0.1 * noise(vec2<f32>(frame_time() * 8.0, in.position.x));
    return vec4<f32>(in.color.rgb * (0.2 + rim * 1.0 + 0.3 * lines) * flicker, 1.0);
}

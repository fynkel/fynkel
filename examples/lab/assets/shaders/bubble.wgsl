// A force field: glows at its edges as seen, and brightest where it cuts
// through something (the floor, the Knight), since it's shallow there.
struct Values { color: vec4<f32>, touch: f32 }

fn unlit(in: SurfaceInput) -> vec4<f32> {
    let touching = 1.0 - smoothstep(0.0, values.touch, depth_behind(in));
    let pulse = 0.8 + 0.2 * sin(frame_time() * 3.0 + in.position.y * 8.0);
    let glow = saturate(fresnel(in, 3.0) * pulse + touching);
    return vec4<f32>(values.color.rgb * glow * 2.0, glow);
}

// Lava: dark rock with hot cracks that flow. `hot` is the cracks' colour,
// `scale` how many there are, `flow` how fast they move.
struct Values { hot: vec4<f32>, scale: f32, flow: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let n = fbm(in.uv * values.scale + vec2<f32>(0.0, frame_time() * values.flow));
    // Where the noise crosses the middle: thin, winding lines.
    let crack = 1.0 - smoothstep(0.0, 0.06, abs(n - 0.5));
    let warm = 1.0 - smoothstep(0.0, 0.2, abs(n - 0.5));
    s.color = mix(vec3<f32>(0.04, 0.035, 0.035), vec3<f32>(0.25, 0.07, 0.02), warm);
    s.roughness = 0.95;
    s.emission = values.hot.rgb * (crack * 4.0 + warm * 0.4);
    return s;
}

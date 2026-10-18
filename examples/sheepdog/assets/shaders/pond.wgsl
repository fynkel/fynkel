// The pond: small waves, the sandy bed seen through them, bent by the
// ripples and lost in the murk the deeper it is, and foam where it meets
// the bank.
struct Values { deep: vec4<f32>, foam: f32, bend: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let t = frame_time();
    let p = in.position.xz;
    let a = dot(p, vec2<f32>(0.9, 0.4)) * 3.0 + t * 1.4;
    let b = dot(p, vec2<f32>(-0.5, 0.8)) * 5.0 + t * 2.1;
    v.position.y += sin(a) * 0.02 + sin(b) * 0.01;
    let dx = cos(a) * 3.0 * 0.9 * 0.02 + cos(b) * 5.0 * -0.5 * 0.01;
    let dz = cos(a) * 3.0 * 0.4 * 0.02 + cos(b) * 5.0 * 0.8 * 0.01;
    v.normal = normalize(vec3<f32>(-dx, 1.0, -dz));
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let depth = depth_behind(in);
    var screen = in.screen + in.normal.xz * values.bend * saturate(depth * 2.0);
    // Bent onto something in front of the water: straight through.
    if scene_depth(screen) < view_depth(in.position) {
        screen = in.screen;
    }
    let murk = saturate(depth / 0.35);
    let edge = values.foam * (0.6 + 0.8 * noise(in.position.xz * 8.0 + frame_time() * 0.4));
    let foam = 1.0 - smoothstep(0.0, edge, depth);
    s.color = mix(values.deep.rgb * murk, vec3<f32>(0.95), foam);
    s.emission = scene_color(screen) * vec3<f32>(0.55, 0.8, 0.92) * (1.0 - murk * 0.85) * (1.0 - foam);
    s.roughness = mix(0.06, 0.8, foam);
    s.alpha = 1.0;
    return s;
}

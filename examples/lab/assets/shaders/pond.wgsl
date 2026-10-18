// A pond: phase 2's waves, and what's under them. Foam where the water
// meets the stones, the bank and the Knight's boots (it's shallow there);
// the bed seen through it, bent by the ripples and lost in the murk the
// deeper it is; the sun's highlight on top.
struct Values { height: f32, deep: vec4<f32>, foam: f32, bend: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let t = frame_time();
    let p = in.position.xz;
    let a = dot(p, vec2<f32>(0.9, 0.4)) * 3.0 + t * 1.6;
    let b = dot(p, vec2<f32>(-0.5, 0.8)) * 5.0 + t * 2.3;
    let h = values.height;
    v.position.y += (sin(a) * 0.03 + sin(b) * 0.015) * h;
    let dx = (cos(a) * 3.0 * 0.9 * 0.03 + cos(b) * 5.0 * -0.5 * 0.015) * h;
    let dz = (cos(a) * 3.0 * 0.4 * 0.03 + cos(b) * 5.0 * 0.8 * 0.015) * h;
    v.normal = normalize(vec3<f32>(-dx, 1.0, -dz));
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let depth = depth_behind(in);
    // The ripples bend what's under them, more the deeper it is.
    var screen = in.screen + in.normal.xz * values.bend * saturate(depth * 2.0);
    // Bent onto something in front of the water: straight through.
    if scene_depth(screen) < view_depth(in.position) {
        screen = in.screen;
    }
    let murk = saturate(depth / 0.6);
    // Foam in a band at the edge, broken up so it isn't a line.
    let edge = values.foam * (0.6 + 0.8 * noise(in.position.xz * 9.0 + frame_time() * 0.4));
    let foam = 1.0 - smoothstep(0.0, edge, depth);
    s.color = mix(values.deep.rgb * murk, vec3<f32>(0.95), foam);
    s.emission = scene_color(screen) * vec3<f32>(0.7, 0.88, 0.92) * (1.0 - murk * 0.85) * (1.0 - foam);
    s.roughness = mix(0.06, 0.8, foam);
    s.alpha = 1.0;
    return s;
}

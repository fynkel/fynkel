// Glass that bends what's behind it: the scene seen through it moved by
// the way its surface faces, tinted, and the sky's light at its edges.
struct Values { bend: f32, tint: vec4<f32> }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let facing = in.normal - in.view * dot(in.normal, in.view);
    let screen = in.screen + vec2<f32>(facing.x, -facing.y) * values.bend;
    let rim = fresnel(in, 4.0);
    s.color = vec3<f32>(rim * 0.6);
    s.emission = scene_color(screen) * values.tint.rgb * (1.0 - rim);
    s.metallic = 0.0;
    s.roughness = 0.04;
    s.alpha = 1.0;
    return s;
}

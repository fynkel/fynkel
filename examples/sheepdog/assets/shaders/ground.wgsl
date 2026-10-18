// The field: the material's grass, and sand where the ground dips under
// the pond's water.
struct Values { sand: vec4<f32> }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let under = 1.0 - smoothstep(-0.12, -0.02, in.position.y);
    s.color = mix(s.color, values.sand.rgb, under);
    return s;
}

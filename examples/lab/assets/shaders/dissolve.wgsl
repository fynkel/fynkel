// A dissolve: gone where the noise is under `amount`, with an edge that
// glows `edge`. The material has an alpha cutoff, so its shadow goes too.
struct Values { edge: vec4<f32>, amount: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let n = noise3(in.position * 4.0);
    s.alpha = step(values.amount, n);
    let near = 1.0 - smoothstep(0.0, 0.08, n - values.amount);
    s.emission = values.edge.rgb * near * 6.0;
    return s;
}

// A wolf burning away: gone where the noise is under its own `burnt`
// (0 to 1), with a glowing edge. Every wolf shares the material and burns
// on its own. The material has an alpha cutoff, so its shadow goes too.
struct Values { edge: vec4<f32> }
struct PerObject { burnt: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    // From its texture coordinates, so the pattern goes with the wolf.
    let n = noise(in.uv * 5.0) * 0.7 + noise(in.uv * 13.0) * 0.3;
    let amount = per_object.burnt * 1.1 - 0.05;
    s.alpha = step(amount, n);
    let near = 1.0 - smoothstep(0.0, 0.08, n - amount);
    s.emission += values.edge.rgb * near * 6.0;
    return s;
}

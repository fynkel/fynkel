// Waves on water: two trains of them crossing, the surface turned to
// face the way each slope does, deep blue looking down and the sky's
// colour at a glance.
struct Values { height: f32, deep: vec4<f32>, shallow: vec4<f32> }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let t = frame_time();
    let p = in.position.xz;
    let a = dot(p, vec2<f32>(0.9, 0.4)) * 3.0 + t * 1.6;
    let b = dot(p, vec2<f32>(-0.5, 0.8)) * 5.0 + t * 2.3;
    let h = values.height;
    v.position.y += (sin(a) * 0.05 + sin(b) * 0.025) * h;
    // The slope each way, from the waves' own derivatives.
    let dx = (cos(a) * 3.0 * 0.9 * 0.05 + cos(b) * 5.0 * -0.5 * 0.025) * h;
    let dz = (cos(a) * 3.0 * 0.4 * 0.05 + cos(b) * 5.0 * 0.8 * 0.025) * h;
    v.normal = normalize(vec3<f32>(-dx, 1.0, -dz));
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.color = mix(values.deep.rgb, values.shallow.rgb, fresnel(in, 3.0));
    s.roughness = 0.08;
    return s;
}

// A hit flash: every enemy shares the material, and each flashes on its
// own (its MeshRenderer's `flash`, 0 to 1).
struct PerObject { flash: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.color = mix(s.color, vec3<f32>(1.0), per_object.flash);
    s.emission = vec3<f32>(1.0, 0.25, 0.15) * per_object.flash * 2.0;
    return s;
}

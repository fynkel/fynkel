
// A material's own shader drawing its shadow (renderer.rs), when it's cut
// out or moves its vertices: placed and moved as the forward pass places
// it, seen from the map's light, and cut where its own alpha falls below
// the material's cutoff. As shadow.wgsl, a cascade's
// casters nearer the sun than its depth range are flattened onto its near
// end.
@group(2) @binding(8) var<uniform> fy_shadow_view: mat4x4<f32>;

fn fy_shadow_vertex(out: VertexOut, cascade: bool) -> VertexOut {
    var seen = out;
    seen.clip = fy_shadow_view * vec4<f32>(out.world_position, 1.0);
    if cascade {
        seen.clip.z = max(seen.clip.z, 0.0);
    }
    return seen;
}

@vertex
fn vs_fy_shadow(in: VertexIn) -> VertexOut {
    return fy_shadow_vertex(place(in, array<vec4<f32>, 3>(in.model_0, in.model_1, in.model_2)), false);
}

@vertex
fn vs_fy_shadow_cascade(in: VertexIn) -> VertexOut {
    return fy_shadow_vertex(place(in, array<vec4<f32>, 3>(in.model_0, in.model_1, in.model_2)), true);
}

@vertex
fn vs_fy_shadow_skinned(in: VertexIn, skin: SkinIn) -> VertexOut {
    return fy_shadow_vertex(place(in, skin_rows(skin, u32(in.params.w))), false);
}

@vertex
fn vs_fy_shadow_cascade_skinned(in: VertexIn, skin: SkinIn) -> VertexOut {
    return fy_shadow_vertex(place(in, skin_rows(skin, u32(in.params.w))), true);
}

@fragment
fn fs_fy_shadow(in: VertexOut, @builtin(front_facing) front_facing: bool) {
    if fy_alpha(in, front_facing) < in.params.z {
        discard;
    }
}

// Its depth for ambient occlusion (ao.rs), when it moves its vertices.
@vertex
fn vs_fy_depth(in: VertexIn) -> @builtin(position) vec4<f32> {
    return place(in, array<vec4<f32>, 3>(in.model_0, in.model_1, in.model_2)).clip;
}

@vertex
fn vs_fy_depth_skinned(in: VertexIn, skin: SkinIn) -> @builtin(position) vec4<f32> {
    return place(in, skin_rows(skin, u32(in.params.w))).clip;
}

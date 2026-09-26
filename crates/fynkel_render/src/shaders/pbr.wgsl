// The built-in `fynkel.pbr@1` surface (docs/renderer.md § Shading): every
// material map — base colour, normal from the vertex tangent,
// metallic-roughness, occlusion, emissive — and `alphaMode` mask by
// discard, handed to the engine's light (`light.wgsl`). The HDR alpha
// tells the post pass what the texel is: 0 background, 1 scene-referred
// radiance (nits) — a `blend` material writes its alpha instead, so the
// blended result is radiance premultiplied by coverage — 2
// display-referred colour (unlit).

@group(1) @binding(0) var material_sampler: sampler;
@group(1) @binding(1) var base_color_tex: texture_2d<f32>;
@group(1) @binding(2) var normal_tex: texture_2d<f32>;
@group(1) @binding(3) var metallic_roughness_tex: texture_2d<f32>;
@group(1) @binding(4) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(5) var emissive_tex: texture_2d<f32>;

// The camera depth prepass (R2): the scene pass's own vertex stage, so
// the depth it writes is the depth the scene pass computes, with only the
// alpha-mask discard behind it. No colour target.
@fragment
fn fs_depth(in: VsOut) {
    let cutoff = in.flags.x;
    if cutoff >= 0.0 {
        let a = textureSample(base_color_tex, material_sampler, in.uv).a * in.color.a;
        if a < cutoff {
            discard;
        }
    }
}

// The selection outline's mask (docs/renderer.md § The overlay): the
// silhouette of an outlined instance, with the prepass's alpha-mask
// discard (a blend material covers its whole surface). The outline's own
// copy of the instance carries the mask value — 1 selected, 0.5 a child —
// in `params.x`, which nothing else reads here; the target keeps the
// larger of what lands on a texel (a `Max` blend), so selected wins.
@fragment
fn fs_mask(in: VsOut) -> @location(0) vec4<f32> {
    let cutoff = in.flags.x;
    if cutoff >= 0.0 {
        let a = textureSample(base_color_tex, material_sampler, in.uv).a * in.color.a;
        if a < cutoff {
            discard;
        }
    }
    return vec4<f32>(in.params.x, 0.0, 0.0, 1.0);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let base = textureSample(base_color_tex, material_sampler, in.uv) * in.color;
    let cutoff = in.flags.x;
    if cutoff >= 0.0 && base.a < cutoff {
        discard;
    }
    // Opaque and mask texels are tagged 1; a blend material writes its
    // alpha (the transparent pipeline blends by it).
    var tag = 1.0;
    if cutoff < -1.5 {
        tag = clamp(base.a, 0.0, 1.0);
    }
    let albedo = base.rgb;
    let gray = (u32(in.flags.w) & 2u) != 0u;
    if in.flags.y > 0.5 {
        // Unlit: display-referred, the post pass only encodes it.
        if gray {
            return vec4<f32>(grayed(albedo), 2.0);
        }
        return vec4<f32>(albedo, 2.0);
    }

    var s: LitSurface;
    s.albedo = albedo;
    let tn = textureSample(normal_tex, material_sampler, in.uv).xyz * 2.0 - 1.0;
    s.normal = vec3<f32>(tn.xy * in.params.z, tn.z);
    let mr = textureSample(metallic_roughness_tex, material_sampler, in.uv);
    s.metallic = in.params.x * mr.b;
    s.roughness = in.params.y * mr.g;
    s.occlusion = 1.0 + in.params.w * (textureSample(occlusion_tex, material_sampler, in.uv).r - 1.0);
    s.emission = in.emissive.rgb * textureSample(emissive_tex, material_sampler, in.uv).rgb;
    var radiance = light_surface(in, front_facing, s);
    if gray {
        radiance = grayed(radiance);
    }
    return vec4<f32>(radiance, tag);
}

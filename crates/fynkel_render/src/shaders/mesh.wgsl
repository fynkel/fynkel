
// Opaque and transparent mesh stages (docs/renderer.md): Cook-Torrance
// (GGX, height-correlated Smith, Schlick) in physical units into the
// linear HDR target — one sun (lux) shadowed by its PCF-filtered map,
// constant ambient (lux), the object's own MAX_LIGHTS point/spot lights
// out of the frame's pool (candela),
// every material map, normal mapping from the vertex tangent, `alphaMode`
// mask by discard. The HDR alpha tells the post pass what the texel is:
// 0 background, 1 scene-referred radiance (nits) — a `blend` material
// writes its alpha instead, so the blended result is radiance
// premultiplied by coverage — 2 display-referred colour (unlit).

@group(1) @binding(0) var material_sampler: sampler;
@group(1) @binding(1) var base_color_tex: texture_2d<f32>;
@group(1) @binding(2) var normal_tex: texture_2d<f32>;
@group(1) @binding(3) var metallic_roughness_tex: texture_2d<f32>;
@group(1) @binding(4) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(5) var emissive_tex: texture_2d<f32>;

struct VsIn {
    // The cooked 72-byte vertex (fynkel_cooked::payload::Vertex).
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv0: vec2<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) color: vec4<f32>,
    // Per-instance (fynkel_render::Instance): model matrix columns, base
    // colour factor, emissive (rgb × strength, nits-relative), params
    // (metallic, roughness, normal_scale, occlusion_strength), flags
    // (alpha cutoff, -1 opaque or -2 blend; unlit; the slot of this
    // instance's light list; bits: 1 receives shadows, 2 grey — an extra
    // world drawn greyed).
    @location(6) m0: vec4<f32>,
    @location(7) m1: vec4<f32>,
    @location(8) m2: vec4<f32>,
    @location(9) m3: vec4<f32>,
    @location(10) tint: vec4<f32>,
    @location(11) emissive: vec4<f32>,
    @location(12) params: vec4<f32>,
    @location(13) flags: vec4<f32>,
};

struct VsOut {
    // Invariant so the depth prepass and the scene pass compute the same
    // clip position to the bit: they are two pipelines over this one
    // vertex stage, and the scene pass's depth compare is `LessEqual`
    // against what the prepass wrote (R2).
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) emissive: vec4<f32>,
    @location(6) params: vec4<f32>,
    @location(7) flags: vec4<f32>,
    @location(8) @interpolate(flat) lights: vec4<u32>,
};

// The frame's joint palettes (M3-19): every skinned instance's matrices
// back to back, mesh space → posed joint → mesh space; an instance's
// first joint is `emissive.w`.
@group(2) @binding(0) var<storage, read> joints: array<mat4x4<f32>>;

// Every instance's light list (E1 light culling): the pool indices that
// reach it, one per byte — the first four in `x`, the next four in `y` —
// and how many in `z`. An instance's own slot is `flags.z`; there is no
// room for this among the vertex attributes (the pipeline's sixteen are
// all taken), and the stage reads it once per vertex.
@group(2) @binding(1) var<storage, read> instance_lights: array<vec4<u32>>;

// A skinned mesh's third vertex buffer: the cooked SkinWeight (four
// joint indices into the instance's palette, four weights summing to 1).
struct VsSkin {
    @location(14) joints: vec4<u32>,
    @location(15) weights: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    return vertex(in, model);
}

// The skinned stage: the vertex is posed in mesh space by its weighted
// joint matrices, then placed like any other. An index past the palette
// (a mesh cooked against another skin) reads the last matrix instead of
// out of bounds.
@vertex
fn vs_skinned(in: VsIn, skin: VsSkin) -> VsOut {
    let base = u32(in.emissive.w);
    let last = arrayLength(&joints) - 1u;
    let pose = skin.weights.x * joints[min(base + skin.joints.x, last)]
        + skin.weights.y * joints[min(base + skin.joints.y, last)]
        + skin.weights.z * joints[min(base + skin.joints.z, last)]
        + skin.weights.w * joints[min(base + skin.joints.w, last)];
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3) * pose;
    return vertex(in, model);
}

fn vertex(in: VsIn, model: mat4x4<f32>) -> VsOut {
    var out: VsOut;
    let world = model * vec4<f32>(in.position, 1.0);
    out.clip = frame.view_proj * world;
    out.world_pos = world.xyz;
    // Normals under non-uniform scale: normalising after the linear part
    // (no inverse transpose) — V1 accepts the skew.
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    let t = (model * vec4<f32>(in.tangent.xyz, 0.0)).xyz;
    out.world_tangent = vec4<f32>(t, in.tangent.w);
    out.uv = in.uv0;
    out.color = in.tint * in.color;
    out.emissive = in.emissive;
    out.params = in.params;
    out.flags = in.flags;
    out.lights = instance_lights[u32(in.flags.z)];
    return out;
}

// GGX / Trowbridge-Reitz normal distribution.
fn d_ggx(ndoth: f32, a2: f32) -> f32 {
    let f = ndoth * ndoth * (a2 - 1.0) + 1.0;
    return a2 / (PI * f * f);
}

// Height-correlated Smith visibility (Heitz 2014), G / (4 NdotL NdotV).
fn v_smith(ndotv: f32, ndotl: f32, a2: f32) -> f32 {
    let gv = ndotl * sqrt(ndotv * ndotv * (1.0 - a2) + a2);
    let gl = ndotv * sqrt(ndotl * ndotl * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

fn f_schlick(vdoth: f32, f0: vec3<f32>) -> vec3<f32> {
    let f = pow(1.0 - vdoth, 5.0);
    return f0 + (vec3<f32>(1.0) - f0) * f;
}

// Radiance from one light of illuminance `e` (lux, rgb) along `l`.
fn shade(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, e: vec3<f32>,
         diffuse: vec3<f32>, f0: vec3<f32>, a2: f32) -> vec3<f32> {
    let ndotl = dot(n, l);
    if ndotl <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(l + v);
    let ndotv = max(dot(n, v), 1e-4);
    let ndoth = max(dot(n, h), 0.0);
    let vdoth = max(dot(v, h), 0.0);
    let spec = d_ggx(ndoth, a2) * v_smith(ndotv, ndotl, a2) * f_schlick(vdoth, f0);
    return (diffuse / PI + spec) * e * ndotl;
}

// Illuminance (lux, rgb) at `p` from a point or spot light, and the unit
// vector toward it.
fn punctual(light: Light, p: vec3<f32>) -> vec4<f32> {
    let to_light = light.position.xyz - p;
    let d2 = max(dot(to_light, to_light), 1e-4);
    let d = sqrt(d2);
    let l = to_light / d;
    // Inverse square, windowed to zero at `range` (Karis 2013).
    let ratio = d / max(light.position.w, 1e-3);
    let r4 = ratio * ratio * ratio * ratio;
    let window = clamp(1.0 - r4, 0.0, 1.0);
    var e = window * window / d2;
    if light.color.w == LIGHT_SPOT {
        let cos_angle = dot(-l, light.direction.xyz);
        e *= smoothstep(light.direction.w, light.params.x, cos_angle);
    }
    return vec4<f32>(l, e);
}

// Where a point lands in one cascade: its place in the atlas, the depth
// to compare, whether the cascade covers it at all, and how close to the
// cascade's border it is (0 at the centre, 1 at the edge).
struct CascadeHit {
    inside: bool,
    uv: vec2<f32>,
    depth: f32,
    edge: f32,
};

fn cascade_project(i: u32, p: vec3<f32>, n: vec3<f32>) -> CascadeHit {
    let c = frame.cascades[i];
    // The normal offset is per cascade: it is a texel of *that* cascade,
    // so the near ones barely move the sample and stay in contact.
    let clip = c.view_proj * vec4<f32>(p + n * c.params.x, 1.0);
    let ndc = clip.xyz / clip.w;
    let local = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    var hit: CascadeHit;
    hit.inside = all(local >= vec2<f32>(0.0)) && all(local <= vec2<f32>(1.0))
        && ndc.z >= 0.0 && ndc.z <= 1.0;
    // The quadrant this cascade was drawn into.
    hit.uv = c.rect.xy + local * c.rect.zw;
    hit.depth = ndc.z;
    // Only the sides: the cascade's depth range reaches far back toward
    // the sun, so a receiver sits near its far plane and a depth term
    // here would blend everywhere.
    hit.edge = max(abs(local.x * 2.0 - 1.0), abs(local.y * 2.0 - 1.0));
    return hit;
}

// 3×3 PCF over the atlas, each tap hardware-filtered (2×2) by the
// comparison sampler. A cascade's quadrant is inset by a texel and the
// border stays at the cleared far depth, so a tap that leaves the
// cascade reads "lit" rather than its neighbour's depths.
fn shadow_pcf(uv: vec2<f32>, depth: f32) -> f32 {
    let texel = frame.shadow.y;
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, depth);
        }
    }
    return lit / 9.0;
}

// The sun's visibility at `p` (surface normal `n`): the first cascade
// that covers the point, cross-faded into the next over the outer band
// so the change of resolution is a gradient and not a seam. A point no
// cascade covers — past the shadow distance — is lit.
fn sun_visibility(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let count = u32(frame.shadow.w);
    let blend = max(frame.shadow.z, 1e-4);
    for (var i = 0u; i < count; i++) {
        let hit = cascade_project(i, p, n);
        if !hit.inside {
            continue;
        }
        var lit = shadow_pcf(hit.uv, hit.depth);
        let t = (hit.edge - (1.0 - blend)) / blend;
        if t > 0.0 && i + 1u < count {
            let next = cascade_project(i + 1u, p, n);
            if next.inside {
                lit = mix(lit, shadow_pcf(next.uv, next.depth), clamp(t, 0.0, 1.0));
            }
        }
        return lit;
    }
    return 1.0;
}

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

// An extra world drawn greyed (Unity's Prefab Mode context in "Gray",
// `fynkel_render::ExtraWorld::gray`): the shaded colour desaturated to its
// Rec. 709 luminance, then dimmed.
const GRAY_DIM: f32 = 0.5;

fn grayed(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)) * GRAY_DIM);
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
    let bits = u32(in.flags.w);
    let gray = (bits & 2u) != 0u;
    if in.flags.y > 0.5 {
        // Unlit: display-referred, the post pass only encodes it.
        if gray {
            return vec4<f32>(grayed(albedo), 2.0);
        }
        return vec4<f32>(albedo, 2.0);
    }

    var n = normalize(in.world_normal);
    if !front_facing {
        n = -n;
    }
    let geometric_n = n;
    let t_raw = in.world_tangent.xyz;
    if dot(t_raw, t_raw) > 0.25 {
        // Normal map in tangent space; a zero tangent (no UVs at cook)
        // keeps the vertex normal.
        let t = normalize(t_raw - n * dot(n, t_raw));
        let b = cross(n, t) * in.world_tangent.w;
        var s = textureSample(normal_tex, material_sampler, in.uv).xyz * 2.0 - 1.0;
        s = vec3<f32>(s.xy * in.params.z, s.z);
        n = normalize(t * s.x + b * s.y + n * s.z);
    }

    let mr = textureSample(metallic_roughness_tex, material_sampler, in.uv);
    let metallic = clamp(in.params.x * mr.b, 0.0, 1.0);
    let roughness = clamp(in.params.y * mr.g, 0.045, 1.0);
    let a2 = roughness * roughness * roughness * roughness;
    var ao = 1.0 + in.params.w * (textureSample(occlusion_tex, material_sampler, in.uv).r - 1.0);
    // Screen-space ambient occlusion (R2) on top of the material's own,
    // for the instances the depth prepass drew: the blended ones are not
    // in that depth, so the AO under them belongs to what is behind.
    if frame.counts.w == 1u && cutoff > -1.5 {
        ao *= textureLoad(ssao_tex, vec2<i32>(in.clip.xy), 0).r;
    }
    let diffuse = albedo * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let v = normalize(frame.camera_pos.xyz - in.world_pos);

    var radiance = vec3<f32>(0.0);
    if frame.sun_dir.w > 0.5 {
        var visibility = 1.0;
        if frame.shadow.x > 0.5 && (bits & 1u) != 0u {
            visibility = sun_visibility(in.world_pos, geometric_n);
        }
        radiance += visibility * shade(n, v, frame.sun_dir.xyz, frame.sun_color.rgb, diffuse, f0, a2);
    }
    // Only the lights this object was given (E1 light culling): its list
    // is the pool indices packed a byte each, strongest first.
    let n_lights = min(in.lights.z, MAX_LIGHTS);
    for (var i = 0u; i < n_lights; i++) {
        let word = select(in.lights.y, in.lights.x, i < 4u);
        let light = frame.lights[(word >> ((i & 3u) * 8u)) & 0xffu];
        let le = punctual(light, in.world_pos);
        radiance += shade(n, v, le.xyz, light.color.rgb * le.w, diffuse, f0, a2);
    }
    // Environment light. Without an HDR environment: constant ambient,
    // the diffuse and the specular base each seeing E/π. With one (E1):
    // the split-sum approximation — the irradiance cube holds E/π for the
    // normal, the prefiltered cube the GGX-convolved radiance along the
    // reflection at `roughness · last mip`, and the BRDF table the scale
    // and bias on F0.
    let ambient_term = (diffuse + f0) * frame.ambient.rgb / PI;
    let n_env = rotate_sky(n);
    let r_env = rotate_sky(reflect(-v, n));
    let irradiance = textureSample(irradiance_cube, cube_sampler, n_env).rgb;
    let prefiltered = textureSampleLevel(specular_cube, cube_sampler, r_env, roughness * frame.sky.w).rgb;
    let ndotv = max(dot(n, v), 1e-4);
    let ab = textureSampleLevel(brdf_lut, cube_sampler, vec2<f32>(ndotv, roughness), 0.0).rg;
    let ibl_term = (diffuse * irradiance + prefiltered * (f0 * ab.x + ab.y)) * frame.sky.x;
    radiance += mix(ambient_term, ibl_term, frame.sky.z) * ao;
    // Emissive is relative to the exposure's white (L_max).
    let emissive = in.emissive.rgb * textureSample(emissive_tex, material_sampler, in.uv).rgb;
    radiance += emissive * frame.ambient.w;
    if gray {
        radiance = grayed(radiance);
    }
    return vec4<f32>(radiance, tag);
}

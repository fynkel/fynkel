// The engine's light (ADR-0031): everything the built-in PBR does once
// its surface is known — Cook-Torrance (GGX, height-correlated Smith,
// Schlick) in physical units, one sun (lux) shadowed by its PCF-filtered
// cascades and the contact shadows, the object's own MAX_LIGHTS
// point/spot lights out of the frame's pool (candela), constant ambient
// or image-based light under the screen-space AO, and emission. The
// built-in surface (`pbr.wgsl`) and every `lit` shader call
// `light_surface`, so they are lit the same to the bit.

// What a surface gives the light: the built-in's textures and factors,
// or a shader's `surface()`.
struct LitSurface {
    // Linear RGB.
    albedo: vec3<f32>,
    // Clamped here to 0..1.
    metallic: f32,
    // Clamped here to 0.045..1.
    roughness: f32,
    // Tangent space: (0, 0, 1) is the mesh's own normal. Ignored where
    // the mesh has no tangents (no UVs at cook).
    normal: vec3<f32>,
    // The material's own occlusion; the screen-space AO multiplies in.
    occlusion: f32,
    // Linear RGB relative to the exposure's white (L_max).
    emission: vec3<f32>,
};

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

// An extra world drawn greyed (Unity's Prefab Mode context in "Gray",
// `fynkel_render::ExtraWorld::gray`): the shaded colour desaturated to its
// Rec. 709 luminance, then dimmed.
const GRAY_DIM: f32 = 0.5;

fn grayed(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)) * GRAY_DIM);
}

// The radiance (nits × 1 / L_max) the surface sends toward the camera.
fn light_surface(in: VsOut, front_facing: bool, s: LitSurface) -> vec3<f32> {
    let cutoff = in.flags.x;
    let bits = u32(in.flags.w);
    let albedo = s.albedo;

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
        n = normalize(t * s.normal.x + b * s.normal.y + n * s.normal.z);
    }

    let metallic = clamp(s.metallic, 0.0, 1.0);
    let roughness = clamp(s.roughness, 0.045, 1.0);
    let a2 = roughness * roughness * roughness * roughness;
    var ao = s.occlusion;
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
        // Contact shadows (R3d) for the instances that receive shadows and
        // are in the prepass's depth: the nearer of the two shadows wins.
        if frame.contact.x > 0.0 && (bits & 1u) != 0u && cutoff > -1.5 {
            visibility = min(visibility, textureLoad(ssao_tex, vec2<i32>(in.clip.xy), 0).g);
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
    let emissive = s.emission;
    radiance += emissive * frame.ambient.w;
    return radiance;
}

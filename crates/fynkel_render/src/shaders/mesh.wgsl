
// Opaque mesh pass (docs/renderer.md): Cook-Torrance (GGX, height-
// correlated Smith, Schlick) in physical units into the linear HDR
// target — one sun (lux), constant ambient (lux), up to MAX_LIGHTS
// point/spot lights (candela), every material map, normal mapping from
// the vertex tangent, `alphaMode` mask by discard. The HDR alpha tells
// the post pass what the texel is: 0 background, 1 scene-referred
// radiance (nits), 2 display-referred colour (unlit; encode only).

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
    // (alpha cutoff or -1, unlit, double-sided, unused).
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
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) emissive: vec4<f32>,
    @location(6) params: vec4<f32>,
    @location(7) flags: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
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

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let base = textureSample(base_color_tex, material_sampler, in.uv) * in.color;
    let cutoff = in.flags.x;
    if cutoff >= 0.0 && base.a < cutoff {
        discard;
    }
    let albedo = base.rgb;
    if in.flags.y > 0.5 {
        // Unlit: display-referred, the post pass only encodes it.
        return vec4<f32>(albedo, 2.0);
    }

    var n = normalize(in.world_normal);
    if !front_facing {
        n = -n;
    }
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
    let ao = 1.0 + in.params.w * (textureSample(occlusion_tex, material_sampler, in.uv).r - 1.0);
    let diffuse = albedo * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let v = normalize(frame.camera_pos.xyz - in.world_pos);

    var radiance = vec3<f32>(0.0);
    if frame.sun_dir.w > 0.5 {
        radiance += shade(n, v, frame.sun_dir.xyz, frame.sun_color.rgb, diffuse, f0, a2);
    }
    for (var i = 0u; i < frame.counts.x; i++) {
        let le = punctual(frame.lights[i], in.world_pos);
        radiance += shade(n, v, le.xyz, frame.lights[i].color.rgb * le.w, diffuse, f0, a2);
    }
    // Constant ambient: the diffuse and the specular base each see E/π.
    radiance += (diffuse + f0) * frame.ambient.rgb / PI * ao;
    // Emissive is relative to the exposure's white (L_max).
    let emissive = in.emissive.rgb * textureSample(emissive_tex, material_sampler, in.uv).rgb;
    radiance += emissive * frame.ambient.w;
    return vec4<f32>(radiance, 1.0);
}

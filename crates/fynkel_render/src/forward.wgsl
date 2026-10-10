// The forward pass: metallic-roughness PBR lit by one sun and an ambient
// term, tonemapped (Khronos PBR Neutral) in the same pass. Materials are
// glTF's: factors times textures, a tangent-space normal map, occlusion of
// the ambient light, emission, and cut-outs (MASK).
//
// Units (stage 1 plan §3.6): `sun_color` is colour × intensity. The sun's
// irradiance is π × that, so with Lambert's albedo / π a rough surface
// facing a sun of intensity 1 shows its albedo. Ambient is radiance from
// every direction, which also gives the albedo at 1.0.
//
// The BRDF is Filament's: GGX distribution, height-correlated Smith
// visibility, Schlick Fresnel, and a plain Lambert diffuse.

struct Frame {
    view_proj: mat4x4<f32>,
    // w = 1: the camera's position. w = 0 (orthographic): the unit vector
    // towards the camera, the same everywhere.
    camera_position: vec4<f32>,
    // xyz: unit vector towards the sun. w: 1 if there is a sun.
    to_sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

// Set by the pipeline for double-sided materials, whose triangles are
// drawn from both sides: seen from behind, the normal is turned round so
// the back is lit as the front.
override double_sided: bool = false;

// A material's textures. A slot without a texture holds a 1×1 one that
// changes nothing: white, or a flat normal. Colour slots (base colour,
// emissive) are sRGB textures, so sampling decodes them to linear before
// filtering; the others are linear data.
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var normal_texture: texture_2d<f32>;
// glTF's packing: roughness in G, metallic in B.
@group(1) @binding(2) var metallic_roughness_texture: texture_2d<f32>;
// In R.
@group(1) @binding(3) var occlusion_texture: texture_2d<f32>;
@group(1) @binding(4) var emissive_texture: texture_2d<f32>;
@group(1) @binding(5) var material_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Along +u. w is 1 or -1: cross(normal, tangent) * w points up the
    // image (fynkel_mesh's convention, glTF's).
    @location(2) tangent: vec4<f32>,
    // (0, 0) is the image's top-left.
    @location(3) uv: vec2<f32>,
    // Per draw (instance step):
    @location(4) model_0: vec4<f32>,
    @location(5) model_1: vec4<f32>,
    @location(6) model_2: vec4<f32>,
    @location(7) model_3: vec4<f32>,
    // The inverse-transpose of the model matrix's upper 3×3, so normals
    // stay perpendicular under non-uniform scale.
    @location(8) normal_0: vec3<f32>,
    @location(9) normal_1: vec3<f32>,
    @location(10) normal_2: vec3<f32>,
    @location(11) base_color: vec4<f32>,
    // x: metallic, y: roughness, z: alpha cutoff (0 for opaque), w: 1, or
    // -1 when the model matrix mirrors (which flips the bitangent).
    @location(12) params: vec4<f32>,
    // rgb: linear emission.
    @location(13) emissive: vec4<f32>,
    // xy: UV scale, zw: UV offset (glTF's texture transform).
    @location(14) uv_transform: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) base_color: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) tangent: vec4<f32>,
    @location(5) uv: vec2<f32>,
    @location(6) @interpolate(flat) emissive: vec3<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    let normal_matrix = mat3x3<f32>(in.normal_0, in.normal_1, in.normal_2);
    let world = model * vec4<f32>(in.position, 1.0);
    var out: VertexOut;
    out.clip = frame.view_proj * world;
    out.world_position = world.xyz;
    out.normal = normal_matrix * in.normal;
    // A tangent lies along the surface, so it turns with the model matrix
    // itself (unlike the normal).
    out.tangent = vec4<f32>(mat3x3<f32>(in.model_0.xyz, in.model_1.xyz, in.model_2.xyz) * in.tangent.xyz,
        in.tangent.w * in.params.w);
    out.uv = in.uv * in.uv_transform.xy + in.uv_transform.zw;
    out.base_color = in.base_color;
    out.params = in.params;
    out.emissive = in.emissive.rgb;
    return out;
}

const PI: f32 = 3.14159265359;

fn distribution_ggx(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn visibility_smith(n_dot_v: f32, n_dot_l: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let gv = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    let gl = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / (gv + gl);
}

fn fresnel_schlick(f0: vec3<f32>, v_dot_h: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - v_dot_h, 5.0);
}

// The split-sum environment BRDF, approximated analytically (Karis,
// "Physically Based Shading on Mobile"), for the ambient's specular part.
fn env_brdf(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

// Khronos PBR Neutral: nearly the identity up to 0.76, so base colours
// stay true, then a smooth roll-off to white.
fn tonemap_neutral(color_in: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;
    var color = color_in;
    let x = min(color.r, min(color.g, color.b));
    var offset = 0.04;
    if x < 0.08 {
        offset = x - 6.25 * x * x;
    }
    color -= offset;
    let peak = max(color.r, max(color.g, color.b));
    if peak < start_compression {
        return color;
    }
    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / (peak + d - start_compression);
    color *= new_peak / peak;
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(color, vec3<f32>(new_peak), g);
}

// The surface's normal: the interpolated one, bent by the normal map.
// The map stores tangent-space x (along +u), y (up the image) and z (out
// of the surface) as 0–1. Without a usable tangent, the map is ignored.
fn surface_normal(in: VertexOut, mapped: vec3<f32>) -> vec3<f32> {
    let n = normalize(in.normal);
    let t_raw = in.tangent.xyz - n * dot(n, in.tangent.xyz);
    if dot(t_raw, t_raw) < 1e-12 {
        return n;
    }
    let t = normalize(t_raw);
    let b = cross(n, t) * in.tangent.w;
    let m = mapped * 2.0 - 1.0;
    return normalize(m.x * t + m.y * b + m.z * n);
}

// Linear radiance and alpha. `front_facing` is the builtin: whether the
// triangle is counter-clockwise on screen.
fn shade(in: VertexOut, front_facing: bool) -> vec4<f32> {
    // Every texture is sampled first, where control flow is still uniform
    // (WGSL requires it for implicit mip selection).
    let base_sample = textureSample(base_color_texture, material_sampler, in.uv);
    let normal_sample = textureSample(normal_texture, material_sampler, in.uv).xyz;
    let mr_sample = textureSample(metallic_roughness_texture, material_sampler, in.uv);
    let occlusion = textureSample(occlusion_texture, material_sampler, in.uv).r;
    let emissive_sample = textureSample(emissive_texture, material_sampler, in.uv).rgb;

    let base = in.base_color * base_sample;
    let albedo = base.rgb;
    let metallic = in.params.x * mr_sample.b;
    // Below ~0.045 the highlight gets thinner than a pixel and aliases.
    let roughness = clamp(in.params.y * mr_sample.g, 0.045, 1.0);
    let alpha = roughness * roughness;

    var n = surface_normal(in, normal_sample);
    // Mirroring turns triangles clockwise, so for a mirrored object the
    // front is the clockwise side. Turning the final normal round turns
    // the whole tangent frame, as the back's normal map needs.
    if double_sided && front_facing == (in.params.w < 0.0) {
        n = -n;
    }
    // w = 0: an orthographic camera, and xyz is the way to it.
    let v = select(normalize(frame.camera_position.xyz - in.world_position), frame.camera_position.xyz, frame.camera_position.w == 0.0);
    let n_dot_v = max(dot(n, v), 1e-4);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let diffuse_color = albedo * (1.0 - metallic);

    // Occlusion darkens only the ambient light: the sun's shadows are
    // another matter (glTF's rule).
    var color = frame.ambient.rgb * (diffuse_color + env_brdf(f0, roughness, n_dot_v)) * occlusion;

    if frame.to_sun.w > 0.5 {
        let l = frame.to_sun.xyz;
        let n_dot_l = max(dot(n, l), 0.0);
        if n_dot_l > 0.0 {
            let h = normalize(l + v);
            let n_dot_h = max(dot(n, h), 0.0);
            let v_dot_h = max(dot(v, h), 0.0);
            let specular = distribution_ggx(n_dot_h, alpha) * visibility_smith(n_dot_v, n_dot_l, alpha)
                * fresnel_schlick(f0, v_dot_h);
            let diffuse = diffuse_color / PI;
            let irradiance = frame.sun_color.rgb * PI * n_dot_l;
            color += (diffuse + specular) * irradiance;
        }
    }
    color += in.emissive * emissive_sample;
    return vec4<f32>(color, base.a);
}

// Opaque surfaces: alpha is ignored. The target is always an sRGB view,
// so the hardware encodes, and the MSAA resolve averages in linear light.
@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return vec4<f32>(tonemap_neutral(shade(in, front_facing).rgb), 1.0);
}

// Cut-outs (glTF's MASK): the surface is there where alpha reaches the
// cutoff. Alpha-to-coverage turns the returned alpha into how many of the
// 4 MSAA samples are covered, so the edge is antialiased. Alpha is first
// sharpened to go from 0 to 1 across about one pixel at the cutoff, so the
// edge stays crisp at any distance instead of turning into a dither
// (Castaño, "Anti-aliased alpha test"). The pipeline writes colour only,
// so the target stays opaque.
@fragment
fn fs_cutout(in: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let shaded = shade(in, front_facing);
    let cutoff = in.params.z;
    let sharp = (shaded.a - cutoff) / max(fwidth(shaded.a), 1e-4) + 0.5;
    return vec4<f32>(tonemap_neutral(shaded.rgb), clamp(sharp, 0.0, 1.0));
}

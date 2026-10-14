// The forward pass: metallic-roughness PBR lit by suns, point and spot
// lights and an ambient term, tonemapped (Khronos PBR Neutral) in the same
// pass. Materials are
// glTF's: factors times textures, a tangent-space normal map, occlusion of
// the ambient light, emission, and cut-outs (MASK).
//
// Units (stage 1 plan §3.6): a light's colour is colour × intensity. A
// sun's irradiance is π × that, so with Lambert's albedo / π a rough
// surface facing a sun of intensity 1 shows its albedo. A point or spot
// light's is π × that / distance², the same at 1 m, and fades smoothly to
// nothing at its range (Karis, "Real Shading in Unreal Engine 4"). Ambient
// is radiance from every direction, which also gives the albedo at 1.0.
//
// The BRDF is Filament's: GGX distribution, height-correlated Smith
// visibility, Schlick Fresnel, and a plain Lambert diffuse.

struct Sun {
    // xyz: unit vector towards the sun.
    to_sun: vec4<f32>,
    color: vec4<f32>,
};

// A point or spot light.
struct Lamp {
    // w: 1 / range².
    position: vec4<f32>,
    color: vec4<f32>,
    // xyz: the unit vector a spot light shines along.
    direction: vec4<f32>,
    // The cone's fade: saturate(cos(angle from the axis) * x + y), squared.
    // A point light's (0, 1) is 1 every way. z: its shadow map, or -1. w: a
    // texel of that map, per metre from the light.
    cone: vec4<f32>,
};

// Where the shadow maps look from (shadows.rs).
struct Shadows {
    // World to each cascade's map, then to each spot map: x and y from -1
    // to 1 across, depth from 0 (nearest the light) to 1.
    cascades: array<mat4x4<f32>, 3>,
    spots: array<mat4x4<f32>, 4>,
    // xyz: how far along the view each cascade ends. w: where the sun's
    // shadows start fading out.
    splits: vec4<f32>,
    // x: cascades, y: the filter (0 hard, 1 soft, 2 softer), z: 1 / the
    // cascades' size, w: 1 / the spot maps'.
    params: vec4<f32>,
    // Each cascade's texel, in metres.
    texels: vec4<f32>,
    // xyz: the way the camera looks. w: how far the sun's shadows reach.
    camera_forward: vec4<f32>,
    camera_at: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    // w = 1: the camera's position. w = 0 (orthographic): the unit vector
    // towards the camera, the same everywhere.
    camera_position: vec4<f32>,
    ambient: vec4<f32>,
    // x: how many suns there are. y: the one that casts shadows, or
    // 0xFFFFFFFF.
    counts: vec4<u32>,
    suns: array<Sun, 4>,
    lamps: array<Lamp, 64>,
    shadows: Shadows,
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

// The shadow maps: the sun's cascades, the spot lights' maps, each a layer,
// and a sampler that compares a depth against them, smoothed across texels.
@group(2) @binding(0) var cascade_maps: texture_depth_2d_array;
@group(2) @binding(1) var spot_maps: texture_depth_2d_array;
@group(2) @binding(2) var shadow_sampler: sampler_comparison;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Along +u. w is 1 or -1: cross(normal, tangent) * w points up the
    // image (fynkel_mesh's convention, glTF's).
    @location(2) tangent: vec4<f32>,
    // (0, 0) is the image's top-left.
    @location(3) uv: vec2<f32>,
    // Per draw (instance step):
    // The model matrix's three rows (its last is always 0, 0, 0, 1).
    @location(4) model_0: vec4<f32>,
    @location(5) model_1: vec4<f32>,
    @location(6) model_2: vec4<f32>,
    @location(7) base_color: vec4<f32>,
    // x: metallic, y: roughness, z: alpha cutoff (0 for opaque), w: unused
    // (the vertex shader puts the mirroring sign there).
    @location(8) params: vec4<f32>,
    // rgb: linear emission. w: 1 if shadows fall on it.
    @location(9) emissive: vec4<f32>,
    // xy: UV scale, zw: UV offset (glTF's texture transform).
    @location(10) uv_transform: vec4<f32>,
    // The lamps that light it: indices into `frame.lamps`, a byte each,
    // nearest first, 0xFF after the last.
    @location(11) lamps: vec2<u32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) base_color: vec4<f32>,
    // As the instance's, with w 1, or -1 when the model matrix mirrors
    // (which flips the bitangent and the front face).
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) tangent: vec4<f32>,
    @location(5) uv: vec2<f32>,
    @location(6) @interpolate(flat) emissive: vec4<f32>,
    @location(7) @interpolate(flat) lamps: vec2<u32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let p = vec4<f32>(in.position, 1.0);
    let world = vec4<f32>(dot(in.model_0, p), dot(in.model_1, p), dot(in.model_2, p), 1.0);
    // The upper 3×3's columns.
    let c0 = vec3<f32>(in.model_0.x, in.model_1.x, in.model_2.x);
    let c1 = vec3<f32>(in.model_0.y, in.model_1.y, in.model_2.y);
    let c2 = vec3<f32>(in.model_0.z, in.model_1.z, in.model_2.z);
    // Normals turn by the inverse-transpose, so they stay perpendicular
    // under non-uniform scale. The cofactor matrix is that times the
    // determinant: its sign keeps a mirrored object's normals pointing
    // out, and the fragment shader normalises away its size.
    let cofactor = mat3x3<f32>(cross(c1, c2), cross(c2, c0), cross(c0, c1));
    let mirror = select(1.0, -1.0, dot(c0, cross(c1, c2)) < 0.0);
    var out: VertexOut;
    out.clip = frame.view_proj * world;
    out.world_position = world.xyz;
    out.normal = cofactor * in.normal * mirror;
    // A tangent lies along the surface, so it turns with the model matrix
    // itself (unlike the normal).
    out.tangent = vec4<f32>(mat3x3<f32>(c0, c1, c2) * in.tangent.xyz, in.tangent.w * mirror);
    out.uv = in.uv * in.uv_transform.xy + in.uv_transform.zw;
    out.base_color = in.base_color;
    out.params = vec4<f32>(in.params.xyz, mirror);
    out.emissive = in.emissive;
    out.lamps = in.lamps;
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
    // (1 - v·h)⁵, multiplied out: cheaper than pow on phones' GPUs.
    let x = 1.0 - v_dot_h;
    let x2 = x * x;
    return f0 + (vec3<f32>(1.0) - f0) * (x2 * x2 * x);
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

// What a surface reflects towards `v` of light arriving from `l` with
// `irradiance` (on a surface facing it).
struct Surface {
    n: vec3<f32>,
    v: vec3<f32>,
    n_dot_v: f32,
    f0: vec3<f32>,
    diffuse: vec3<f32>,
    alpha: f32,
};

fn reflected(s: Surface, l: vec3<f32>, irradiance: vec3<f32>) -> vec3<f32> {
    let n_dot_l = dot(s.n, l);
    if n_dot_l <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(l + s.v);
    let n_dot_h = max(dot(s.n, h), 0.0);
    let v_dot_h = max(dot(s.v, h), 0.0);
    let specular = distribution_ggx(n_dot_h, s.alpha) * visibility_smith(s.n_dot_v, n_dot_l, s.alpha)
        * fresnel_schlick(s.f0, v_dot_h);
    return (s.diffuse / PI + specular) * irradiance * n_dot_l;
}

// How lit `uv` is at `depth` in `layer` of `map`, 0 to 1, by the frame's
// filter: one compare (smoothed across a texel by the sampler), 4 half a
// texel apart, or 9 a texel apart. `texel` is 1 / the map's size. Outside
// the map is lit.
fn filtered(map: texture_depth_2d_array, layer: u32, uv: vec2<f32>, depth: f32, texel: f32) -> f32 {
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || depth >= 1.0 {
        return 1.0;
    }
    let taps = u32(frame.shadows.params.y);
    if taps == 0u {
        return textureSampleCompareLevel(map, shadow_sampler, uv, layer, depth);
    }
    if taps == 1u {
        let d = 0.5 * texel;
        return 0.25 * (textureSampleCompareLevel(map, shadow_sampler, uv + vec2<f32>(-d, -d), layer, depth)
            + textureSampleCompareLevel(map, shadow_sampler, uv + vec2<f32>(d, -d), layer, depth)
            + textureSampleCompareLevel(map, shadow_sampler, uv + vec2<f32>(-d, d), layer, depth)
            + textureSampleCompareLevel(map, shadow_sampler, uv + vec2<f32>(d, d), layer, depth));
    }
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += textureSampleCompareLevel(map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * texel, layer, depth);
        }
    }
    return sum / 9.0;
}

// Where a lookup is made from: out along the surface's own normal `ng` by
// about a texel (`texel`, in metres), more where the light grazes it, so a
// surface doesn't shadow itself (acne).
fn offset_position(p: vec3<f32>, ng: vec3<f32>, l: vec3<f32>, texel: f32) -> vec3<f32> {
    return p + ng * texel * (0.75 + 1.5 * (1.0 - saturate(dot(ng, l))));
}

// How much of the sun reaches `p`, 0 to 1, from its cascades. Past the
// shadow distance all of it does; the last tenth fades there.
// Reads `frame.shadows` in place: copying it into a local first (`let s =
// frame.shadows`) loses the Android emulator's device.
fn sun_shadow(p: vec3<f32>, ng: vec3<f32>, l: vec3<f32>) -> f32 {
    let along = dot(p - frame.shadows.camera_at.xyz, frame.shadows.camera_forward.xyz);
    if along >= frame.shadows.camera_forward.w {
        return 1.0;
    }
    let count = u32(frame.shadows.params.x);
    var c = 0u;
    while c + 1u < count && along > frame.shadows.splits[c] {
        c++;
    }
    let q = frame.shadows.cascades[c] * vec4<f32>(offset_position(p, ng, l, frame.shadows.texels[c]), 1.0);
    let lit = filtered(cascade_maps, c, q.xy * vec2<f32>(0.5, -0.5) + 0.5, q.z, frame.shadows.params.z);
    let fading = frame.shadows.splits.w;
    let fade = saturate((along - fading) / max(frame.shadows.camera_forward.w - fading, 1e-4));
    return mix(lit, 1.0, fade);
}

// How much of a spot light reaches `p`, 0 to 1, from its map. `distance`
// is how far the light is.
fn spot_shadow(lamp: Lamp, p: vec3<f32>, ng: vec3<f32>, l: vec3<f32>, distance: f32) -> f32 {
    let k = u32(lamp.cone.z);
    let q = frame.shadows.spots[k] * vec4<f32>(offset_position(p, ng, l, lamp.cone.w * distance), 1.0);
    if q.w <= 0.0 {
        return 1.0;
    }
    let m = q.xyz / q.w;
    return filtered(spot_maps, k, m.xy * vec2<f32>(0.5, -0.5) + 0.5, m.z, frame.shadows.params.w);
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
    // The surface's own normal, without the map: shadow lookups move out
    // along it.
    var ng = normalize(in.normal);
    // Mirroring turns triangles clockwise, so for a mirrored object the
    // front is the clockwise side. Turning the final normal round turns
    // the whole tangent frame, as the back's normal map needs.
    if double_sided && front_facing == (in.params.w < 0.0) {
        n = -n;
        ng = -ng;
    }
    let receives = in.emissive.w > 0.5;
    // w = 0: an orthographic camera, and xyz is the way to it.
    let v = select(normalize(frame.camera_position.xyz - in.world_position), frame.camera_position.xyz, frame.camera_position.w == 0.0);
    let n_dot_v = max(dot(n, v), 1e-4);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let diffuse_color = albedo * (1.0 - metallic);

    // Occlusion darkens only the ambient light: the sun's shadows are
    // another matter (glTF's rule).
    var color = frame.ambient.rgb * (diffuse_color + env_brdf(f0, roughness, n_dot_v)) * occlusion;

    let surface = Surface(n, v, n_dot_v, f0, diffuse_color, alpha);
    for (var i = 0u; i < frame.counts.x; i++) {
        let sun = frame.suns[i];
        var lit = 1.0;
        if i == frame.counts.y && receives {
            lit = sun_shadow(in.world_position, ng, sun.to_sun.xyz);
        }
        color += reflected(surface, sun.to_sun.xyz, sun.color.rgb * (PI * lit));
    }
    for (var i = 0u; i < 8u; i++) {
        let index = (in.lamps[i / 4u] >> ((i % 4u) * 8u)) & 0xFFu;
        if index == 0xFFu {
            break;
        }
        // Its position first: past its range (the object is reached, not
        // every pixel of it), the rest isn't read.
        let position = frame.lamps[index].position;
        let to_lamp = position.xyz - in.world_position;
        let distance2 = dot(to_lamp, to_lamp);
        let near = distance2 * position.w;
        if near >= 1.0 {
            continue;
        }
        let lamp = frame.lamps[index];
        let l = to_lamp * inverseSqrt(max(distance2, 1e-8));
        // Closer than 10 cm counts as 10 cm, so a surface touching a lamp
        // isn't infinitely bright.
        let reach = 1.0 - near * near;
        let cone = saturate(dot(-l, lamp.direction.xyz) * lamp.cone.x + lamp.cone.y);
        var falloff = reach * reach * cone * cone / max(distance2, 0.01);
        if lamp.cone.z >= 0.0 && receives && falloff > 0.0 {
            falloff *= spot_shadow(lamp, in.world_position, ng, l, sqrt(distance2));
        }
        color += reflected(surface, l, lamp.color.rgb * (PI * falloff));
    }
    color += in.emissive.rgb * emissive_sample;
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

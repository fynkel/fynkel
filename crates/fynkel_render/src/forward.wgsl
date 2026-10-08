// The stage 1 forward pass: metallic-roughness PBR lit by one sun and an
// ambient term, tonemapped (Khronos PBR Neutral) in the same pass.
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
    camera_position: vec4<f32>,
    // xyz: unit vector towards the sun. w: 1 if there is a sun.
    to_sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Locations 2 and 3 are the tangent and UV, unused until textures
    // (stage 2 step 3).
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
    // x: metallic, y: roughness.
    @location(12) params: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) base_color: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
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
    out.base_color = in.base_color;
    out.params = in.params;
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

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let albedo = in.base_color.rgb;
    let metallic = in.params.x;
    // Below ~0.045 the highlight gets thinner than a pixel and aliases.
    let roughness = clamp(in.params.y, 0.045, 1.0);
    let alpha = roughness * roughness;

    let n = normalize(in.normal);
    let v = normalize(frame.camera_position.xyz - in.world_position);
    let n_dot_v = max(dot(n, v), 1e-4);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let diffuse_color = albedo * (1.0 - metallic);

    var color = frame.ambient.rgb * (diffuse_color + env_brdf(f0, roughness, n_dot_v));

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

    // The target is always an sRGB view, so the hardware encodes, and the
    // MSAA resolve averages in linear light.
    return vec4<f32>(tonemap_neutral(color), 1.0);
}

// V1 opaque pass (blueprint §8.5 seed): one directional light, Lambert
// diffuse with a Blinn-Phong highlight shaped by roughness/metallic, base
// colour = factor × texture × vertex colour, sRGB-encoded on output (the
// targets are Unorm, the maths is linear).

struct Frame {
    view_proj: mat4x4<f32>,
    // xyz: unit vector toward the light; w: unused.
    light_dir: vec4<f32>,
    // rgb: light colour × exposure; w: ambient fraction.
    light_color: vec4<f32>,
    camera_pos: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var base_color_tex: texture_2d<f32>;
@group(1) @binding(1) var base_color_sampler: sampler;

struct VsIn {
    // The cooked 72-byte vertex (fynkel_cooked::payload::Vertex).
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv0: vec2<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) color: vec4<f32>,
    // Per-instance: model matrix columns, base colour factor, params.
    @location(6) m0: vec4<f32>,
    @location(7) m1: vec4<f32>,
    @location(8) m2: vec4<f32>,
    @location(9) m3: vec4<f32>,
    @location(10) tint: vec4<f32>,
    // x: metallic, y: roughness, z: unlit (1 = flat colour), w: unused.
    @location(11) params: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) params: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    var out: VsOut;
    let world = model * vec4<f32>(in.position, 1.0);
    out.clip = frame.view_proj * world;
    out.world_pos = world.xyz;
    // Normals under non-uniform scale: normalising after the linear part is
    // the inverse-transpose-free approximation; fine for V1.
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv = in.uv0;
    out.color = in.tint * in.color;
    out.params = in.params;
    return out;
}

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let albedo = in.color.rgb * textureSample(base_color_tex, base_color_sampler, in.uv).rgb;
    if in.params.z > 0.5 {
        return vec4<f32>(srgb_encode(albedo), 1.0);
    }
    let n = normalize(in.world_normal);
    let l = frame.light_dir.xyz;
    let v = normalize(frame.camera_pos.xyz - in.world_pos);
    let h = normalize(l + v);
    let ndotl = max(dot(n, l), 0.0);
    let metallic = in.params.x;
    let roughness = clamp(in.params.y, 0.04, 1.0);
    let ambient = frame.light_color.w;
    let light = frame.light_color.rgb;
    // No environment lighting yet, so a metal keeps part of its diffuse
    // (a pure metal would be black except for the highlight); dielectrics
    // reflect ~4%, metals their albedo.
    let diffuse = albedo * (1.0 - metallic * 0.6);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let shininess = mix(256.0, 2.0, roughness * roughness);
    let spec = f0 * pow(max(dot(n, h), 0.0), shininess) * (1.0 - roughness * 0.7);
    let lit = albedo * ambient + diffuse * light * ndotl + spec * light * ndotl;
    return vec4<f32>(srgb_encode(clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}

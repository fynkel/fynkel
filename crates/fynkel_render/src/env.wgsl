// The sky's light, worked out once when it's set (sky.rs), into cubes the
// forward pass reads: each pass draws one face of one mip.
//
// - The radiance cube: the sky itself, then each mip the average of the
//   one above.
// - The specular cube: the radiance blurred as a GGX highlight of each
//   mip's roughness would see it (mip 0 a mirror, the last fully rough),
//   importance-sampled (Karis, "Real Shading in Unreal Engine 4"), each
//   sample read from the radiance mip whose texels are its size.
// - The irradiance cube: the light a surface facing each way gets,
//   cosine-weighted over its half of the sky, divided by π: a plain sky
//   of colour c gives c.

struct Pass {
    // 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z: the cube's layers.
    face: u32,
    // The target's side in texels.
    size: f32,
    // The specular pass's roughness; the others' radiance mip to read.
    roughness: f32,
    // The radiance cube's mip 0's side.
    source_size: f32,
    // The gradient's colours; w of the ground's: 1 to read the image, 0
    // the gradient. An image is read unturned at intensity 1: the forward
    // pass turns and scales the cubes as it reads them.
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    ground: vec4<f32>,
};

@group(0) @binding(0) var<uniform> pass_: Pass;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var radiance: texture_cube<f32>;
@group(0) @binding(3) var linear_sampler: sampler;

const PI: f32 = 3.14159265359;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

// The direction through `pixel` (framebuffer coordinates) of face `face`
// of a cube `size` texels a side, as cube sampling reads it.
fn direction(face: u32, pixel: vec2<f32>, size: f32) -> vec3<f32> {
    let uv = pixel / size * 2.0 - 1.0;
    let u = uv.x;
    let v = uv.y;
    var d: vec3<f32>;
    switch face {
        case 0u: { d = vec3<f32>(1.0, -v, -u); }
        case 1u: { d = vec3<f32>(-1.0, -v, u); }
        case 2u: { d = vec3<f32>(u, 1.0, v); }
        case 3u: { d = vec3<f32>(u, -1.0, -v); }
        case 4u: { d = vec3<f32>(u, -v, 1.0); }
        default: { d = vec3<f32>(-u, -v, -1.0); }
    }
    return normalize(d);
}

// The gradient: zenith straight up, horizon level, ground straight down.
fn gradient(d: vec3<f32>) -> vec3<f32> {
    if d.y >= 0.0 {
        return mix(pass_.horizon.rgb, pass_.zenith.rgb, sqrt(d.y));
    }
    return mix(pass_.horizon.rgb, pass_.ground.rgb, sqrt(-d.y));
}

// Where `d` is in the image: −Z the middle, +X three quarters across,
// straight up the top row.
fn equirect(d: vec3<f32>) -> vec2<f32> {
    let u = 0.5 + atan2(d.x, -d.z) / (2.0 * PI);
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return vec2<f32>(fract(u), v);
}

// The sky itself, as mip 0 of the radiance cube. An image is read over
// the texel's footprint (6 × 6 taps), so a big one isn't sampled sparsely.
@fragment
fn fs_source(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    if pass_.ground.w < 0.5 {
        return vec4<f32>(gradient(direction(pass_.face, at.xy, pass_.size)), 1.0);
    }
    var sum = vec3<f32>(0.0);
    for (var y = 0; y < 6; y++) {
        for (var x = 0; x < 6; x++) {
            let offset = (vec2<f32>(f32(x), f32(y)) + 0.5) / 6.0 - 0.5;
            let d = direction(pass_.face, at.xy + offset, pass_.size);
            sum += textureSampleLevel(image, linear_sampler, equirect(d), 0.0).rgb;
        }
    }
    return vec4<f32>(sum / 36.0, 1.0);
}

// A radiance mip from the one above: the bilinear sample at a texel's
// centre, which falls between the four texels above it, is their average.
@fragment
fn fs_downsample(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let d = direction(pass_.face, at.xy, pass_.size);
    return vec4<f32>(textureSampleLevel(radiance, linear_sampler, d, pass_.roughness).rgb, 1.0);
}

fn radical_inverse(bits_in: u32) -> f32 {
    var bits = (bits_in << 16u) | (bits_in >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}

// Turns `v`, given about +Z, to be about `n`.
fn about(n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return t * v.x + b * v.y + n * v.z;
}

const SPECULAR_SAMPLES: u32 = 64u;

// The radiance seen in a GGX highlight of `pass_.roughness`, about the
// texel's direction (taken as the normal, the view and the reflection).
@fragment
fn fs_specular(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let n = direction(pass_.face, at.xy, pass_.size);
    // A mirror: the radiance mip as big as this one.
    if pass_.roughness == 0.0 {
        let mip = log2(pass_.source_size / pass_.size);
        return vec4<f32>(textureSampleLevel(radiance, linear_sampler, n, mip).rgb, 1.0);
    }
    let alpha = pass_.roughness * pass_.roughness;
    let a2 = alpha * alpha;
    // The solid angle of a radiance texel at mip 0.
    let texel = 4.0 * PI / (6.0 * pass_.source_size * pass_.source_size);
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < SPECULAR_SAMPLES; i++) {
        let xi = vec2<f32>(f32(i) / f32(SPECULAR_SAMPLES), radical_inverse(i));
        let phi = 2.0 * PI * xi.x;
        let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a2 - 1.0) * xi.y));
        let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
        let h = about(n, vec3<f32>(sin_theta * cos(phi), sin_theta * sin(phi), cos_theta));
        let l = 2.0 * dot(n, h) * h - n;
        let n_dot_l = dot(n, l);
        if n_dot_l > 0.0 {
            // The sample's share of the sphere, by its probability (the
            // distribution's, as the view is the normal), read from the mip
            // whose texels are that big.
            let n_dot_h = cos_theta;
            let k = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
            let pdf = a2 / (PI * k * k) / 4.0;
            let share = 1.0 / (f32(SPECULAR_SAMPLES) * pdf + 1e-4);
            let mip = max(0.5 * log2(share / texel) + 1.0, 0.0);
            sum += textureSampleLevel(radiance, linear_sampler, l, mip).rgb * n_dot_l;
            weight += n_dot_l;
        }
    }
    return vec4<f32>(sum / max(weight, 1e-4), 1.0);
}

const IRRADIANCE_SAMPLES: u32 = 128u;

// The light a surface facing the texel's direction gets from the sky,
// divided by π, from a small radiance mip (`pass_.roughness`).
@fragment
fn fs_irradiance(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let n = direction(pass_.face, at.xy, pass_.size);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < IRRADIANCE_SAMPLES; i++) {
        // Cosine-weighted: the average of what they see is the answer.
        let xi = vec2<f32>((f32(i) + 0.5) / f32(IRRADIANCE_SAMPLES), radical_inverse(i));
        let r = sqrt(xi.y);
        let phi = 2.0 * PI * xi.x;
        let l = about(n, vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(1.0 - xi.y, 0.0))));
        sum += textureSampleLevel(radiance, linear_sampler, l, pass_.roughness).rgb;
    }
    return vec4<f32>(sum / f32(IRRADIANCE_SAMPLES), 1.0);
}

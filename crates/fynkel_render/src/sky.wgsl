// The sky, behind everything: one triangle over the view, at the far plane
// (reversed-Z's 0), drawn after the scene where nothing was. A gradient's
// colours are shown as picked; an image's are light, tone mapped as the
// scene is.

// forward.wgsl's `Sky`.
struct Sky {
    // Clip space back to the world.
    inverse_view_proj: mat4x4<f32>,
    // rgb: the colours. zenith.w: 1 gradient, 2 image. horizon.w: the
    // image's intensity. ground.w: its turn, in radians.
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    ground: vec4<f32>,
};

// The start of forward.wgsl's `Frame`.
struct Frame {
    view_proj: mat4x4<f32>,
    sky: Sky,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

const PI: f32 = 3.14159265359;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var out: Out;
    out.clip = vec4<f32>(at, 0.0, 1.0);
    out.ndc = at;
    return out;
}

// Khronos PBR Neutral, as forward.wgsl's.
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
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    // The way through the pixel: from its point on the near plane to its
    // point on the far one, so an orthographic camera works too.
    // Read in place: copying a struct out of the uniform first loses the
    // Android emulator's device (forward.wgsl's sun_shadow).
    let near = frame.sky.inverse_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let far = frame.sky.inverse_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let d = normalize(far.xyz / far.w - near.xyz / near.w);
    // A gradient's colours are shown as picked, so its alpha is 0: it's
    // not light that glows (renderer.rs, the scene image).
    if frame.sky.zenith.w < 1.5 {
        if d.y >= 0.0 {
            return vec4<f32>(mix(frame.sky.horizon.rgb, frame.sky.zenith.rgb, sqrt(d.y)), 0.0);
        }
        return vec4<f32>(mix(frame.sky.horizon.rgb, frame.sky.ground.rgb, sqrt(-d.y)), 0.0);
    }
    let u = 0.5 + (atan2(d.x, -d.z) - frame.sky.ground.w) / (2.0 * PI);
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    let light = textureSampleLevel(image, image_sampler, vec2<f32>(fract(u), v), 0.0).rgb;
    return vec4<f32>(tonemap_neutral(light * frame.sky.horizon.w), 1.0);
}

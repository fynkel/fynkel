
// Post pass: HDR (Rgba16Float, nits) → exposure → ACES (Hill fit) →
// sRGB encode (unless the target format encodes itself) into the host's
// target (or the render-scale intermediate). The HDR alpha is a tag with
// coverage: 0 background (the exact clear colour), (0, 1] scene-referred
// radiance premultiplied by coverage — a multisample resolve or a blended
// transparent over the background — composited over the clear colour by
// that coverage, ~2 display-referred (unlit): clamped and encoded only.
// Bloom (R3b), when the stage ran: its level 0 is added to the exposed
// scene-referred radiance before ACES, and over the uncovered part of a
// pixel it lights the clear colour (a screen blend, in the target's
// encoding); display-referred texels are left alone.

@group(1) @binding(0) var hdr_sampler: sampler;
@group(1) @binding(1) var hdr_tex: texture_2d<f32>;

struct PostParams {
    clear_color: vec4<f32>,
};
@group(1) @binding(2) var<uniform> post: PostParams;
// The bloom chain's level 0 at half the internal size (a 1×1 black
// placeholder while the quality profile has bloom off).
@group(1) @binding(3) var bloom_tex: texture_2d<f32>;

struct PostOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> PostOut {
    var out: PostOut;
    out.clip = vec4<f32>(fullscreen(id), 0.0, 1.0);
    return out;
}

fn rrt_odt_fit(v: vec3<f32>) -> vec3<f32> {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

// ACES filmic (Stephen Hill's fit of the RRT + sRGB ODT), sRGB primaries in
// and out, unit white.
fn aces(c: vec3<f32>) -> vec3<f32> {
    let acescg = vec3<f32>(
        dot(vec3<f32>(0.59719, 0.35458, 0.04823), c),
        dot(vec3<f32>(0.07600, 0.90834, 0.01566), c),
        dot(vec3<f32>(0.02840, 0.13383, 0.83777), c),
    );
    let fit = rrt_odt_fit(acescg);
    let out = vec3<f32>(
        dot(vec3<f32>(1.60475, -0.53108, -0.07367), fit),
        dot(vec3<f32>(-0.10208, 1.10813, -0.00605), fit),
        dot(vec3<f32>(-0.00327, -0.07276, 1.07602), fit),
    );
    return clamp(out, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The exposed glow at this pixel, tinted and scaled; 0 without bloom.
fn glow(p: vec2<f32>) -> vec3<f32> {
    if frame.bloom.w <= 0.0 {
        return vec3<f32>(0.0);
    }
    let uv = p / vec2<f32>(textureDimensions(hdr_tex));
    let b = textureSampleLevel(bloom_tex, hdr_sampler, uv, 0.0).rgb;
    return b * frame.bloom.w * frame.bloom_tint.rgb;
}

@fragment
fn fs_main(in: PostOut) -> @location(0) vec4<f32> {
    let hdr = textureLoad(hdr_tex, vec2<i32>(in.clip.xy), 0);
    let bloom = glow(in.clip.xy);
    // The clear colour is already in the target's encoding.
    var background = post.clear_color.rgb;
    if frame.bloom.w > 0.0 {
        var lit = aces(bloom);
        if frame.counts.z == 1u {
            lit = srgb_encode(lit);
        }
        background += (vec3<f32>(1.0) - background) * lit;
    }
    if hdr.a < 0.001 {
        // Written as-is: CLEAR_COLOR names the bytes in the target.
        return vec4<f32>(background, 1.0);
    }
    var color: vec3<f32>;
    var coverage: f32;
    if hdr.a > 1.5 {
        coverage = min(hdr.a * 0.5, 1.0);
        color = clamp(hdr.rgb / coverage, vec3<f32>(0.0), vec3<f32>(1.0));
    } else {
        coverage = min(hdr.a, 1.0);
        color = aces(hdr.rgb / coverage * frame.camera_pos.w + bloom);
    }
    if frame.counts.z == 1u {
        color = srgb_encode(color);
    }
    color = mix(background, color, coverage);
    return vec4<f32>(color, 1.0);
}

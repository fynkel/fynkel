
// Post pass: HDR (Rgba16Float, nits) → exposure → the grade (R3c) → the
// tone mapper (ACES, Hill fit, unless a volume picks another) → lift,
// gamma, gain → sRGB encode (unless the target format encodes itself) into the host's
// target (or the render-scale intermediate). The HDR alpha is a tag with
// coverage: 0 background (the exact clear colour), (0, 1] scene-referred
// radiance premultiplied by coverage — a multisample resolve or a blended
// transparent over the background — composited over the clear colour by
// that coverage, ~2 display-referred (unlit): clamped and encoded only.
// Bloom (R3b), when the stage ran: its level 0 is added to the exposed
// scene-referred radiance before ACES, and over the uncovered part of a
// pixel it lights the clear colour (a screen blend, in the target's
// encoding); display-referred texels are left alone. The grade (R3c)
// applies to what the scene drew, the sky included; the clear colour and
// display-referred texels are not graded.

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

// Unity's Neutral tone mapper (John Hable's curve with its constants,
// white at 5.3, clipped at 1).
fn neutral_curve(x: vec3<f32>) -> vec3<f32> {
    let a = 0.2;
    let b = 0.29;
    let c = 0.24;
    let d = 0.272;
    let e = 0.02;
    let f = 0.3;
    return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f;
}

fn neutral(c: vec3<f32>) -> vec3<f32> {
    let white_scale = vec3<f32>(1.0) / neutral_curve(vec3<f32>(5.3));
    return clamp(neutral_curve(c * white_scale) * white_scale, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Exposed scene radiance → the display's 0..1 by the frame's tone mapper.
fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let which = u32(frame.grade.params.z);
    if which == 1u {
        return neutral(max(c, vec3<f32>(0.0)));
    }
    if which == 2u {
        return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return aces(c);
}

// The grade on exposed scene radiance, before the tone mapper (R3c):
// post-exposure and white balance, contrast in log space around middle
// grey, then colour filter, hue shift and saturation.
fn grade_scene(c: vec3<f32>) -> vec3<f32> {
    if frame.grade.params.y == 0.0 {
        return c;
    }
    let g = frame.grade;
    let pre = mat3x3<f32>(g.pre[0].xyz, g.pre[1].xyz, g.pre[2].xyz);
    let post = mat3x3<f32>(g.post[0].xyz, g.post[1].xyz, g.post[2].xyz);
    var x = max(pre * c, vec3<f32>(0.0));
    x = 0.18 * pow(x / 0.18, vec3<f32>(g.params.x));
    return max(post * x, vec3<f32>(0.0));
}

// Lift, gamma and gain on the tone-mapped 0..1 (R3c): lift raises the
// darks with white held, gain scales, gamma bends the midtones.
fn grade_display(c: vec3<f32>) -> vec3<f32> {
    if frame.grade.params.y == 0.0 {
        return c;
    }
    let g = frame.grade;
    let lifted = g.gain.rgb * (c + g.lift.rgb * (vec3<f32>(1.0) - c));
    return clamp(pow(max(lifted, vec3<f32>(0.0)), g.inv_gamma.rgb), vec3<f32>(0.0), vec3<f32>(1.0));
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
        var lit = tonemap(bloom);
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
        color = grade_display(tonemap(grade_scene(hdr.rgb / coverage * frame.camera_pos.w + bloom)));
    }
    if frame.counts.z == 1u {
        color = srgb_encode(color);
    }
    color = mix(background, color, coverage);
    return vec4<f32>(color, 1.0);
}

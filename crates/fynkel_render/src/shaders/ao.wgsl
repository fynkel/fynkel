
// Ambient occlusion (R2, docs/renderer.md §"Ambient occlusion"): the
// depth prepass has written the frame's opaque depth, and this pass turns
// it into how much of the sky each pixel can see. `fs_main` is the SSAO
// estimate, `fs_blur` removes its per-pixel rotation; the mesh pass
// multiplies the result into its ambient and IBL term only (direct light
// is not occluded by it).
//
// Group 0 is the frame uniform alone — not the mesh pass's group 0, whose
// last binding is the texture this pass writes.

// `DEPTH_TEXTURE` is replaced with the multisampled type when the scene
// passes are multisampled; `textureLoad`'s last argument is then the
// sample index instead of the mip level, and 0 is right either way.
@group(1) @binding(0) var depth_tex: DEPTH_TEXTURE;
// The SSAO estimate, read by `fs_blur` (`fs_main` never reads it: the
// pass writes the other half of the pair).
@group(1) @binding(1) var ao_source: texture_2d<f32>;

// Samples per pixel of the hemisphere, its radius in metres, how far a
// sample must be in front of the surface to occlude it, and the exponent
// the result is shaped by (1 = linear).
const AO_SAMPLES: u32 = 12u;
const AO_RADIUS: f32 = 0.6;
const AO_BIAS: f32 = 0.02;
const AO_INTENSITY: f32 = 1.35;
// Half the blur's square side, and how far apart two taps may be along
// the view ray (a fraction of their distance) before they are taken to be
// different surfaces.
const AO_BLUR: i32 = 2;
const AO_BLUR_DEPTH: f32 = 0.02;

struct AoOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> AoOut {
    var out: AoOut;
    out.clip = vec4<f32>(fullscreen(id), 0.0, 1.0);
    return out;
}

fn depth_at(c: vec2<i32>) -> f32 {
    return textureLoad(depth_tex, c, 0);
}

/// The world point a pixel's depth stands for.
fn world_at(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec3<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth);
    let p = frame.inv_view_proj * vec4<f32>(ndc, 1.0);
    return p.xyz / p.w;
}

/// One hemisphere sample about +Z: a golden-angle spiral whose points
/// crowd toward the origin, so near contact is sampled densely.
fn kernel(i: u32, rotation: f32) -> vec3<f32> {
    let a = (f32(i) + 0.5) / f32(AO_SAMPLES);
    let phi = rotation + f32(i) * 2.39996323;
    let sin_t = sqrt(a);
    let cos_t = sqrt(1.0 - a);
    return vec3<f32>(cos(phi) * sin_t, sin(phi) * sin_t, cos_t) * mix(0.2, 1.0, a * a);
}

/// Interleaved gradient noise: the per-pixel rotation the blur averages
/// back out over its square.
fn dither(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fs_main(in: AoOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_tex));
    let c = vec2<i32>(in.clip.xy);
    let depth = depth_at(c);
    // The sky: nothing occludes it, and it must not darken what a blur
    // tap next to it reads.
    if depth >= 1.0 {
        return vec4<f32>(1.0);
    }
    let texel = 1.0 / vec2<f32>(size);
    let uv = (vec2<f32>(c) + 0.5) * texel;
    let p = world_at(uv, depth);
    let to_cam = normalize(frame.camera_pos.xyz - p);

    // The surface normal from the depth alone: on each axis the nearer
    // neighbour, which is the one still on this surface at a silhouette.
    let dl = depth_at(max(c - vec2<i32>(1, 0), vec2<i32>(0)));
    let dr = depth_at(min(c + vec2<i32>(1, 0), size - 1));
    let du = depth_at(max(c - vec2<i32>(0, 1), vec2<i32>(0)));
    let dd = depth_at(min(c + vec2<i32>(0, 1), size - 1));
    var hx = world_at(uv + vec2<f32>(texel.x, 0.0), dr) - p;
    if abs(dl - depth) < abs(dr - depth) {
        hx = p - world_at(uv - vec2<f32>(texel.x, 0.0), dl);
    }
    var hy = world_at(uv + vec2<f32>(0.0, texel.y), dd) - p;
    if abs(du - depth) < abs(dd - depth) {
        hy = p - world_at(uv - vec2<f32>(0.0, texel.y), du);
    }
    var n = cross(hx, hy);
    if dot(n, n) < 1e-16 {
        return vec4<f32>(1.0);
    }
    n = normalize(n);
    // The reconstruction's winding is whatever the projection makes it;
    // the visible surface always faces the camera.
    if dot(n, to_cam) < 0.0 {
        n = -n;
    }
    // A frame about the normal, turned by this pixel's dither.
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    let rotation = dither(in.clip.xy) * 6.28318531;

    let eye = frame.camera_pos.xyz;
    var occlusion = 0.0;
    for (var i = 0u; i < AO_SAMPLES; i++) {
        let k = kernel(i, rotation);
        let q = p + (t * k.x + b * k.y + n * k.z) * AO_RADIUS;
        let clip = frame.view_proj * vec4<f32>(q, 1.0);
        if clip.w <= 0.0 {
            continue;
        }
        let ndc = clip.xyz / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0)) {
            continue;
        }
        let sc = clamp(vec2<i32>(suv * vec2<f32>(size)), vec2<i32>(0), size - 1);
        let sd = depth_at(sc);
        if sd >= 1.0 {
            continue;
        }
        // The sample point and the surface drawn at its pixel are on the
        // same ray from the eye, so their distances order them.
        let s = world_at((vec2<f32>(sc) + 0.5) * texel, sd);
        if distance(s, eye) >= distance(q, eye) - AO_BIAS {
            continue;
        }
        // An occluder beyond the hemisphere is a different surface, not a
        // near one: fade it out rather than darken across the gap.
        let away = length(s - p);
        occlusion += clamp(AO_RADIUS / max(away, 1e-4), 0.0, 1.0);
    }
    let ao = pow(clamp(1.0 - occlusion / f32(AO_SAMPLES), 0.0, 1.0), AO_INTENSITY);
    return vec4<f32>(ao, 0.0, 0.0, 1.0);
}

@fragment
fn fs_blur(in: AoOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_tex));
    let c = vec2<i32>(in.clip.xy);
    let texel = 1.0 / vec2<f32>(size);
    let eye = frame.camera_pos.xyz;
    let d0 = depth_at(c);
    let r0 = distance(world_at((vec2<f32>(c) + 0.5) * texel, d0), eye);
    var sum = 0.0;
    var weight = 0.0;
    for (var y = -AO_BLUR; y < AO_BLUR; y++) {
        for (var x = -AO_BLUR; x < AO_BLUR; x++) {
            let s = c + vec2<i32>(x, y);
            if any(s < vec2<i32>(0)) || any(s >= size) {
                continue;
            }
            let d = depth_at(s);
            let r = distance(world_at((vec2<f32>(s) + 0.5) * texel, d), eye);
            if abs(r - r0) > AO_BLUR_DEPTH * max(r0, 1.0) {
                continue;
            }
            sum += textureLoad(ao_source, s, 0).r;
            weight += 1.0;
        }
    }
    if weight <= 0.0 {
        return vec4<f32>(textureLoad(ao_source, c, 0).r, 0.0, 0.0, 1.0);
    }
    return vec4<f32>(sum / weight, 0.0, 0.0, 1.0);
}

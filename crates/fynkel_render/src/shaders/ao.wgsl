
// The screen-space stage: the depth prepass has written the frame's
// opaque depth, and this pass turns it into two answers a pixel.
// Red, ambient occlusion (R2, docs/renderer.md §"Ambient occlusion"): how
// much of the sky the pixel can see; the mesh pass multiplies it into its
// ambient and IBL term only (direct light is not occluded by it).
// Green, contact shadows (R3d, §"Contact shadows"): whether a short march
// toward the sun meets something the shadow map is too coarse to see; the
// mesh pass darkens the sun's light by it. `fs_main` is the estimate,
// The tiled compute blur removes its per-pixel dither. An effect that is off (its
// intensity 0 in `frame`) leaves its channel at 1.
//
// Group 0 is the frame uniform alone — not the mesh pass's group 0, whose
// last binding is the texture this pass writes.

// MSAA sample zero is copied once into R32Float; single-sample scenes
// read their depth texture directly. Both paths preserve the same depth.
@group(1) @binding(0) var depth_tex: DEPTH_TEXTURE;
// Samples per pixel of the hemisphere and how far a sample must be in
// front of the surface to occlude it. The hemisphere's radius and the
// exponent the result is shaped by are the volumes' (R3a): `frame.ao`.
const AO_SAMPLES: u32 = 12u;
const AO_BIAS: f32 = 0.02;
// Steps of the contact-shadow march, and the smallest gap (in pixel
// footprints at the point) a step must be behind the drawn surface
// before it counts: the depth is a pixel's centre, the step anywhere in
// it. The march starts a footprint off the surface, so the surface
// itself stays behind every step.
const CONTACT_STEPS: u32 = 12u;
const CONTACT_BIAS: f32 = 1.0;

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
    return DEPTH_LOAD;
}

/// The world point a pixel's depth stands for.
fn world_at(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec3<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth);
    let p = frame.inv_view_proj * vec4<f32>(ndc, 1.0);
    return p.xyz / p.w;
}

// The same twelve golden-angle hemisphere points, evaluated once rather than
// rebuilding their fixed angle/radius at every pixel. Only the pixel's dither
// rotation varies; apply it with one shared sine/cosine pair.
const AO_KERNEL = array<vec3<f32>, 12>(
    vec3<f32>(0.0411083348, 0.0000000000, 0.1971486479),
    vec3<f32>(-0.0553985942, 0.0507496503, 0.1987755487),
    vec3<f32>(0.0093664029, -0.1067253289, 0.2088456278),
    vec3<f32>(0.0880815921, 0.1148868395, 0.2256023673),
    vec3<f32>(-0.1884410610, -0.0333325706, 0.2470529422),
    vec3<f32>(0.2102425478, -0.1337390990, 0.2708815543),
    vec3<f32>(-0.0830595911, 0.3089775369, 0.2943083357),
    vec3<f32>(-0.1867442352, -0.3595646630, 0.3138408733),
    vec3<f32>(0.4754319985, 0.1736270045, 0.3247871206),
    vec3<f32>(-0.5768520222, 0.2381161275, 0.3201387634),
    vec3<f32>(0.3221331968, -0.6883799258, 0.2872621299),
    vec3<f32>(0.2738572002, 0.8730999171, 0.1907993746),
);

fn kernel(i: u32, rotation: vec2<f32>) -> vec3<f32> {
    let k = AO_KERNEL[i];
    return vec3<f32>(
        rotation.x * k.x - rotation.y * k.y,
        rotation.y * k.x + rotation.x * k.y,
        k.z,
    );
}

/// Interleaved gradient noise: the per-pixel rotation the blur averages
/// back out over its square.
fn dither(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fs_main(in: AoOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_tex));
    // The target is half the depth's size: each of its pixels stands for
    // the depth texel at its top-left, and the blur upsamples against it.
    let c = min(vec2<i32>(in.clip.xy) * 2, size - 1);
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

    var ao = 1.0;
    if frame.ao.y > 0.0 {
        ao = ambient_occlusion(p, n, t, b, vec2<f32>(cos(rotation), sin(rotation)), size, texel);
    }
    var contact = 1.0;
    if frame.contact.x > 0.0 {
        contact = contact_shadow(p, n, uv, depth, dither(in.clip.xy), size, texel);
    }
    return vec4<f32>(ao, contact, 0.0, 1.0);
}

fn ambient_occlusion(
    p: vec3<f32>,
    n: vec3<f32>,
    t: vec3<f32>,
    b: vec3<f32>,
    rotation: vec2<f32>,
    size: vec2<i32>,
    texel: vec2<f32>,
) -> f32 {
    let eye = frame.camera_pos.xyz;
    var occlusion = 0.0;
    for (var i = 0u; i < AO_SAMPLES; i++) {
        let k = kernel(i, rotation);
        let q = p + (t * k.x + b * k.y + n * k.z) * frame.ao.x;
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
        occlusion += clamp(frame.ao.x / max(away, 1e-4), 0.0, 1.0);
    }
    return pow(clamp(1.0 - occlusion / f32(AO_SAMPLES), 0.0, 1.0), frame.ao.y);
}

/// 1 lit, `1 − opacity` where a march of `frame.contact.y` metres toward
/// the sun passes behind a drawn surface no thicker than
/// `frame.contact.z`; faded out over the last tenth of
/// `frame.contact.w`.
fn contact_shadow(
    p: vec3<f32>,
    n: vec3<f32>,
    uv: vec2<f32>,
    depth: f32,
    jitter: f32,
    size: vec2<i32>,
    texel: vec2<f32>,
) -> f32 {
    // No test of n·L: the normal from depth is least sure where contact
    // matters most, at a foot, and the mesh pass gives a face turned
    // from the sun no sunlight to darken anyway.
    let l = frame.sun_dir.xyz;
    let eye = frame.camera_pos.xyz;
    let range = distance(p, eye);
    let far = frame.contact.w;
    let fade = clamp((far - range) / (0.1 * far), 0.0, 1.0);
    if fade <= 0.0 {
        return 1.0;
    }
    // A pixel's size at this depth: the march starts that far off the
    // surface, and a hit must be further in than the bias.
    let footprint = distance(world_at(uv + vec2<f32>(texel.x, 0.0), depth), p);
    let bias = CONTACT_BIAS * footprint;
    let start = p + n * footprint;
    for (var i = 0u; i < CONTACT_STEPS; i++) {
        let q = start + l * (frame.contact.y * (f32(i) + jitter) / f32(CONTACT_STEPS));
        let clip = frame.view_proj * vec4<f32>(q, 1.0);
        if clip.w <= 0.0 {
            break;
        }
        let ndc = clip.xyz / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0)) {
            break;
        }
        let sc = clamp(vec2<i32>(suv * vec2<f32>(size)), vec2<i32>(0), size - 1);
        let sd = depth_at(sc);
        if sd >= 1.0 {
            continue;
        }
        // The drawn surface on the step's own ray: how far in front of
        // the step it is.
        let gap = distance(q, eye) - distance(world_at(suv, sd), eye);
        if gap > bias && gap < frame.contact.z + bias {
            return 1.0 - frame.contact.x * fade;
        }
    }
    return 1.0;
}

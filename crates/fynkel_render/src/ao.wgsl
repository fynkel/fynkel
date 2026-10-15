// Light ambient occlusion (ao.rs): darkening where surfaces meet, worked
// out at half the view's size from depth alone.
//
// Its depth is ao_depth.wgsl's. fs_ao: for each pixel, 8 points nearby on screen, within `radius` of it
// in the world: the more of them above its surface, the darker (as
// McGuire et al.'s Scalable Ambient Obscurance). fs_blur: 4 × 4 of those
// averaged, across pixels at about the same depth only, so edges stay.

struct Params {
    // Clip space (reversed-Z) back to the view's space.
    inverse_projection: mat4x4<f32>,
    // x: pixels of this image a metre spans 1 m in front of a perspective
    // camera (anywhere, for an orthographic one). y: the radius, in
    // metres. z: the intensity. w: 1 perspective, 0 orthographic.
    shape: vec4<f32>,
};

@group(0) @binding(0) var depth: texture_depth_2d;
// fs_blur's input: fs_ao's output.
@group(0) @binding(1) var raw: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;

const SAMPLES: u32 = 8u;
// Turns the spiral of samples makes.
const TURNS: f32 = 7.0;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

// Where pixel `px` is in the view's space; `w` 0 where nothing was drawn.
fn position(px: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth));
    let at = clamp(px, vec2<i32>(0), size - 1);
    let d = textureLoad(depth, at, 0);
    let uv = (vec2<f32>(at) + 0.5) / vec2<f32>(size);
    let p = params.inverse_projection * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return vec4<f32>(p.xyz / p.w, select(1.0, 0.0, d <= 0.0));
}

// Noise from the pixel's place (Jimenez's interleaved gradient noise),
// turning each pixel's spiral differently: the blur then averages it out.
fn noise(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fs_ao(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(at.xy);
    let c4 = position(px);
    if c4.w == 0.0 {
        return vec4<f32>(1.0);
    }
    let c = c4.xyz;
    // The surface's normal from its neighbours: on each axis the nearer
    // in depth, so it isn't bent across an edge.
    let l = position(px - vec2<i32>(1, 0)).xyz;
    let r = position(px + vec2<i32>(1, 0)).xyz;
    let u = position(px - vec2<i32>(0, 1)).xyz;
    let d = position(px + vec2<i32>(0, 1)).xyz;
    // At the image's edges, the one side there is.
    let last = vec2<i32>(textureDimensions(depth)) - 1;
    let left = px.x > 0 && (px.x == last.x || abs(l.z - c.z) < abs(r.z - c.z));
    let up = px.y > 0 && (px.y == last.y || abs(u.z - c.z) < abs(d.z - c.z));
    let dx = select(r - c, c - l, left);
    let dy = select(d - c, c - u, up);
    var n = normalize(cross(dx, dy));
    let to_eye = select(vec3<f32>(0.0, 0.0, 1.0), -c, params.shape.w > 0.5);
    if dot(n, to_eye) < 0.0 {
        n = -n;
    }

    let radius = params.shape.y;
    let pixels = radius * params.shape.x / select(1.0, max(-c.z, 1e-3), params.shape.w > 0.5);
    if pixels < 1.0 {
        return vec4<f32>(1.0);
    }
    let r2 = radius * radius;
    // Less than this above the surface is the surface itself.
    let bias = 0.05 * radius;
    let spin = noise(at.xy) * 6.2831853;
    var sum = 0.0;
    for (var i = 0u; i < SAMPLES; i++) {
        let along = (f32(i) + 0.5) / f32(SAMPLES);
        let angle = along * TURNS * 6.2831853 + spin;
        let q = position(px + vec2<i32>(vec2<f32>(cos(angle), sin(angle)) * along * pixels));
        let v = q.xyz - c;
        let vv = dot(v, v);
        let f = max(r2 - vv, 0.0) * q.w;
        sum += f * f * f * max((dot(v, n) - bias) / (vv + 0.01 * r2), 0.0);
    }
    let occlusion = sum * 5.0 * params.shape.z / (r2 * r2 * r2 * f32(SAMPLES));
    return vec4<f32>(max(1.0 - occlusion, 0.0));
}

@fragment
fn fs_blur(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(at.xy);
    let size = vec2<i32>(textureDimensions(raw));
    let centre = position(px);
    if centre.w == 0.0 {
        return vec4<f32>(1.0);
    }
    // Pixels further than a tenth of this one's distance off count less,
    // and not at all beyond twice that.
    let near = 0.1 * max(abs(centre.z), 1e-3);
    var sum = 0.0;
    var weights = 0.0;
    for (var y = -2; y < 2; y++) {
        for (var x = -2; x < 2; x++) {
            let q = clamp(px + vec2<i32>(x, y), vec2<i32>(0), size - 1);
            let w = max(1.0 - abs(position(q).z - centre.z) / (2.0 * near), 0.0) + 1e-4;
            sum += textureLoad(raw, q, 0).r * w;
            weights += w;
        }
    }
    return vec4<f32>(sum / weights);
}

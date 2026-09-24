// The selection outline's composite (docs/renderer.md § The overlay):
// a fullscreen pass over the finished frame, at the target's resolution.
// The mask holds the outlined entities' silhouettes — 1 where a selected
// one covers the texel, 0.5 where only a child does, 0 elsewhere — drawn
// by the mesh vertex stages at this same size with no depth test, so the
// parts behind other geometry are in it too. A pixel outside it finds the
// nearest covered texel within the search radius and takes that texel's
// colour at an alpha that falls off over the outline's last pixel; a
// pixel inside it is left alone, so the object itself is never tinted.

struct Outline {
    selected: vec4<f32>,
    children: vec4<f32>,
    // x: the width (target pixels); y: the search radius (texels,
    // ceil(width) + 1); z: 1 when the target is Unorm (this pass encodes
    // sRGB itself); w: unused.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> ol: Outline;
@group(0) @binding(1) var mask: texture_2d<f32>;

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(id & 1u) * 4 - 1);
    let y = f32(i32(id >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(mask));
    let p = vec2<i32>(position.xy);
    if textureLoad(mask, p, 0).r > 0.0 {
        discard;
    }
    let r = i32(ol.params.y);
    var best = 1e9;
    var value = 0.0;
    for (var dy = -r; dy <= r; dy++) {
        for (var dx = -r; dx <= r; dx++) {
            let q = p + vec2<i32>(dx, dy);
            if any(q < vec2<i32>(0)) || any(q >= size) {
                continue;
            }
            let m = textureLoad(mask, q, 0).r;
            if m <= 0.0 {
                continue;
            }
            // Centre to centre; of two texels as near, a selected one
            // wins, so a selection's ring is never broken by its child's.
            let d = length(vec2<f32>(f32(dx), f32(dy)));
            if d < best - 1e-4 || (d < best + 1e-4 && m > value) {
                best = d;
                value = m;
            }
        }
    }
    let a = clamp(ol.params.x + 0.5 - best, 0.0, 1.0);
    if a <= 0.0 {
        discard;
    }
    let c = select(ol.children, ol.selected, value > 0.75);
    var rgb = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if ol.params.z > 0.5 {
        rgb = srgb_encode(rgb);
    }
    return vec4<f32>(rgb, clamp(c.a * a, 0.0, 1.0));
}

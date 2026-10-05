// Depth-aware upsampling blur. The estimate is made at half resolution;
// each output pixel averages the TAPS x TAPS half-resolution estimates
// around it (the rotation each was made with averages away) that lie on
// the same surface, judged by the distance from the eye of the full-size
// depth texel each one stands for (its top-left). An 8x8 output tile
// shares a TILE x TILE input tile, each depth reconstructed once per tile
// instead of once per tap. A pixel none of whose taps is on its surface
// takes the one nearest in depth.
const TAPS: i32 = 2;
const TILE: u32 = 6u;
@group(1) @binding(0) var depth_tex: DEPTH_TEXTURE;
@group(1) @binding(1) var ao_source: texture_2d<f32>;
@group(1) @binding(2) var ao_output: texture_storage_2d<rgba8unorm, write>;
var<workgroup> samples: array<vec3<f32>, 36>;

fn depth_at(c: vec2<i32>) -> f32 { return DEPTH_LOAD; }
fn radial_distance(c: vec2<i32>, size: vec2<i32>) -> f32 {
    let uv = (vec2<f32>(c) + 0.5) / vec2<f32>(size);
    let ndc = vec3<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth_at(c));
    let p = frame.inv_view_proj * vec4<f32>(ndc, 1.0);
    return distance(p.xyz / p.w, frame.camera_pos.xyz);
}

@compute @workgroup_size(8, 8)
fn cs_main(@builtin(workgroup_id) group: vec3<u32>,
           @builtin(local_invocation_id) local: vec3<u32>,
           @builtin(local_invocation_index) index: u32) {
    let size = vec2<i32>(textureDimensions(depth_tex));
    let half_size = vec2<i32>(textureDimensions(ao_source));
    // The half-resolution texels from 4 * tile - TAPS / 2, TILE across.
    let origin = vec2<i32>(group.xy * 4u) - vec2<i32>(TAPS / 2);
    for (var i = index; i < TILE * TILE; i += 64u) {
        let h = clamp(origin + vec2<i32>(i32(i % TILE), i32(i / TILE)), vec2<i32>(0), half_size - 1);
        samples[i] = vec3<f32>(
            textureLoad(ao_source, h, 0).rg,
            radial_distance(min(h * 2, size - 1), size),
        );
    }
    workgroupBarrier();
    let c = vec2<i32>(group.xy * 8u + local.xy);
    // Every invocation participates in the barrier, including partial edge tiles.
    if any(c >= size) { return; }
    let center = radial_distance(c, size);
    // The window of TAPS half-resolution texels around the pixel, the
    // first of them ((c - 1) >> 1) - (TAPS / 2 - 1), and its place in the tile.
    let first_texel = ((c - vec2<i32>(1)) >> vec2<u32>(1u)) - vec2<i32>(TAPS / 2 - 1);
    let first = max(vec2<i32>(0), -first_texel);
    let end = min(vec2<i32>(TAPS), half_size - first_texel);
    let base = vec2<i32>(first_texel - origin);
    let threshold = 0.02 * max(center, 1.0);
    var sum = vec2<f32>(0.0);
    var weight = 0.0;
    var nearest = vec2<f32>(1.0);
    var nearest_gap = 3.4e38;
    for (var y = first.y; y < end.y; y++) {
        for (var x = first.x; x < end.x; x++) {
            let tap = samples[u32(base.y + y) * TILE + u32(base.x + x)];
            let gap = abs(tap.z - center);
            let include = !(gap > threshold);
            sum += select(vec2<f32>(0.0), tap.xy, include);
            weight += select(0.0, 1.0, include);
            if gap < nearest_gap {
                nearest_gap = gap;
                nearest = tap.xy;
            }
        }
    }
    var value = nearest;
    if weight > 0.0 { value = sum / weight; }
    textureStore(ao_output, c, vec4<f32>(value, 0.0, 1.0));
}

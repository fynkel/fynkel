// Full-resolution bilateral blur. An 8x8 output tile shares an 11x11 input
// tile, including the original asymmetric [-2, 1] halo. Each input depth
// is reconstructed once per tile instead of once for each neighbouring tap.
@group(1) @binding(0) var depth_tex: DEPTH_TEXTURE;
@group(1) @binding(1) var ao_source: texture_2d<f32>;
@group(1) @binding(2) var ao_output: texture_storage_2d<rgba8unorm, write>;
var<workgroup> samples: array<vec3<f32>, 121>;

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
    let origin = vec2<i32>(group.xy * 8u) - vec2<i32>(2);
    for (var i = index; i < 121u; i += 64u) {
        let c = clamp(origin + vec2<i32>(i32(i % 11u), i32(i / 11u)), vec2<i32>(0), size - 1);
        samples[i] = vec3<f32>(textureLoad(ao_source, c, 0).rg, radial_distance(c, size));
    }
    workgroupBarrier();
    let c = vec2<i32>(group.xy * 8u + local.xy);
    // Every invocation participates in the barrier, including partial edge tiles.
    if any(c >= size) { return; }
    let center = samples[(local.y + 2u) * 11u + local.x + 2u];
    // Clip the filter rectangle once. Its row-major order and asymmetric
    // [-2, 1] footprint are unchanged, including partial screen-edge tiles.
    let first = max(vec2<i32>(0), vec2<i32>(2) - c);
    let end = min(vec2<i32>(4), size - c + vec2<i32>(2));
    let base = local.y * 11u + local.x;
    let threshold = 0.02 * max(center.z, 1.0);
    var sum = vec2<f32>(0.0);
    var weight = 0.0;
    for (var y = first.y; y < end.y; y++) {
        for (var x = first.x; x < end.x; x++) {
            let tap = samples[base + u32(y) * 11u + u32(x)];
            let include = !(abs(tap.z - center.z) > threshold);
            sum += select(vec2<f32>(0.0), tap.xy, include);
            weight += select(0.0, 1.0, include);
        }
    }
    var value = center.xy;
    if weight > 0.0 { value = sum / weight; }
    textureStore(ao_output, c, vec4<f32>(value, 0.0, 1.0));
}

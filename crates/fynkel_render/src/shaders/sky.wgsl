
// Sky pass: an equirectangular texture (`Environment.sky`) behind the
// meshes — a fullscreen triangle at depth 1 passing `LessEqual`, so only
// background texels are written. Radiance = texel × sky_intensity (nits).

@group(1) @binding(0) var sky_sampler: sampler;
@group(1) @binding(1) var sky_tex: texture_2d<f32>;

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> SkyOut {
    var out: SkyOut;
    let xy = fullscreen(id);
    out.clip = vec4<f32>(xy, 1.0, 1.0);
    out.ndc = xy;
    return out;
}

@fragment
fn fs_main(in: SkyOut) -> @location(0) vec4<f32> {
    // The view ray through this texel: the far point minus the near point
    // in world space (works for orthographic projections too).
    let near = frame.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far = frame.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    var d = normalize(far.xyz / far.w - near.xyz / near.w);
    // Rotate the sky about +Y.
    let c = cos(frame.sky.y);
    let s = sin(frame.sky.y);
    d = vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
    // Equirectangular: u = 0.5 looks down -Z, u grows toward +X; v = 0 is up.
    let u = 0.5 + atan2(d.x, -d.z) / (2.0 * PI);
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    let texel = textureSample(sky_tex, sky_sampler, vec2<f32>(u, v)).rgb;
    return vec4<f32>(texel * frame.sky.x, 1.0);
}

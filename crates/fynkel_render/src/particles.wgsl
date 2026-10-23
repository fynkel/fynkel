// Particles (stage 12): each a square facing the camera, as Unity's
// billboards are, unlit, as Unity's default particle material is. Its
// colour is the particle's times the material's, times the base colour
// texture, plus the material's emission. Appended to forward.wgsl, for
// its frame, its textures, its fog and its tone mapping.

struct ParticleIn {
    // xyz: its centre in the world. w: its size, across, in metres.
    @location(0) center_size: vec4<f32>,
    // Linear, times the material's base colour; not premultiplied.
    @location(1) color: vec4<f32>,
    // x: its turn round the way to the camera, in radians. yzw: the
    // material's emission.
    @location(2) turn_emissive: vec4<f32>,
};

struct ParticleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) emissive: vec3<f32>,
};

// The camera's right and up in the world: two points across the middle of
// the view, back from clip space.
fn view_axis(towards: vec2<f32>) -> vec3<f32> {
    let m = frame.sky.inverse_view_proj;
    let a = m * vec4<f32>(0.0, 0.0, 0.5, 1.0);
    let b = m * vec4<f32>(towards, 0.5, 1.0);
    return normalize(b.xyz / b.w - a.xyz / a.w);
}

@vertex
fn vs_particle(@builtin(vertex_index) corner: u32, in: ParticleIn) -> ParticleOut {
    // Two triangles: 0 1 2, 2 1 3, over the corners (-1, -1) to (1, 1).
    let k = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u)[corner];
    let c = vec2<f32>(f32(k & 1u), f32(k >> 1u)) * 2.0 - 1.0;
    let turn = in.turn_emissive.x;
    let s = sin(turn);
    let co = cos(turn);
    let turned = vec2<f32>(c.x * co - c.y * s, c.x * s + c.y * co);
    let half = in.center_size.w * 0.5;
    let right = view_axis(vec2<f32>(1.0, 0.0));
    let up = view_axis(vec2<f32>(0.0, 1.0));
    let p = in.center_size.xyz + (right * turned.x + up * turned.y) * half;
    var out: ParticleOut;
    out.clip = frame.view_proj * vec4<f32>(p, 1.0);
    out.world_position = p;
    out.uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    out.color = in.color;
    out.emissive = in.turn_emissive.yzw;
    return out;
}

fn particle_color(in: ParticleOut) -> vec4<f32> {
    var color = in.color;
    if textured {
        color *= textureSample(base_color_texture, material_sampler, in.uv);
    }
    return vec4<f32>(color.rgb + in.emissive, color.a);
}

// Laid over what's behind by their alpha, premultiplied, into the fog.
@fragment
fn fs_particle_transparent(in: ParticleOut) -> @location(0) vec4<f32> {
    let color = particle_color(in);
    let alpha = saturate(color.a);
    return vec4<f32>(fogged(tonemap_neutral(color.rgb), in.world_position) * alpha, alpha);
}

// Their light, times their alpha, added to what's behind, fading to
// nothing in the fog.
@fragment
fn fs_particle_additive(in: ParticleOut) -> @location(0) vec4<f32> {
    let color = particle_color(in);
    let alpha = saturate(color.a);
    return vec4<f32>(tonemap_neutral(color.rgb * alpha) * (1.0 - fog_amount(in.world_position)), alpha);
}

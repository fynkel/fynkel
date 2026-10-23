// Particles (stage 12): each a square facing the camera, as Unity's
// billboards are, or stretched the way it moves, lying flat, standing up,
// or a mesh. Unlit, as Unity's default particle material is, its colour
// the particle's times the material's, times the base colour texture,
// plus the material's emission; or lit as a surface facing the camera; or
// shaded by the material's own shader. Appended to forward.wgsl and
// library.wgsl (and a material's shader), for its frame, its textures,
// its lighting, its fog and its tone mapping.

struct FyParticleIn {
    // xyz: its centre in the world. w: its size, across, in metres.
    @location(4) center_size: vec4<f32>,
    // Linear, times the material's base colour; not premultiplied.
    @location(5) color: vec4<f32>,
    // xyz: how it moves, in the world, metres a second. w: its turn, in
    // radians.
    @location(6) velocity_turn: vec4<f32>,
    // xy: UV scale, zw: UV offset: the material's, and its texture sheet
    // frame's.
    @location(7) uv_transform: vec4<f32>,
    // x: metallic, y: roughness, z: softness in metres, w: how it's drawn
    // (0 facing the camera, 1 stretched, 2 flat, 3 upright, 4 a mesh).
    @location(8) params: vec4<f32>,
    // rgb: the material's emission. w: 1, shadows fall on it.
    @location(9) emissive: vec4<f32>,
    // x: how far through its life, y: its own random number, z: a
    // stretched one's length scale, w: its speed scale.
    @location(10) particle: vec4<f32>,
    // Where the material's values start in `joints` (a shader's).
    @location(11) values: u32,
};

// A mesh particle's vertex, as `fynkel_mesh::Vertex`.
struct FyParticleMeshIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

// As forward.wgsl's `VertexOut`, which a particle's surface is shaded as,
// and its softness besides.
struct FyParticleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) base_color: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) tangent: vec4<f32>,
    @location(5) uv: vec2<f32>,
    @location(6) @interpolate(flat) emissive: vec4<f32>,
    @location(7) @interpolate(flat) own: vec4<f32>,
    @location(8) @interpolate(flat) values: u32,
    @location(9) @interpolate(flat) softness: f32,
};

// Lit as a surface (the built-in material's, or a material's own shader),
// rather than shown as it is.
override particle_lit: bool = false;

// The camera's right and up in the world: two points across the middle of
// the view, back from clip space.
fn fy_view_axis(towards: vec2<f32>) -> vec3<f32> {
    let m = frame.sky.inverse_view_proj;
    let a = m * vec4<f32>(0.0, 0.0, 0.5, 1.0);
    let b = m * vec4<f32>(towards, 0.5, 1.0);
    return normalize(b.xyz / b.w - a.xyz / a.w);
}

fn fy_particle_out(in: FyParticleIn, p: vec3<f32>, normal: vec3<f32>, tangent: vec4<f32>, uv: vec2<f32>) -> FyParticleOut {
    var out: FyParticleOut;
    out.clip = frame.view_proj * vec4<f32>(p, 1.0);
    out.world_position = p;
    out.normal = normal;
    out.tangent = tangent;
    out.uv = uv * in.uv_transform.xy + in.uv_transform.zw;
    out.base_color = in.color;
    out.params = vec4<f32>(in.params.x, in.params.y, 0.0, 1.0);
    out.emissive = in.emissive;
    // A shader's `particle_age()` and `particle_random()`.
    out.own = vec4<f32>(in.particle.x, in.particle.y, 0.0, 0.0);
    out.values = in.values;
    out.softness = in.params.z;
    return out;
}

@vertex
fn vs_fy_particle(@builtin(vertex_index) corner: u32, in: FyParticleIn) -> FyParticleOut {
    // Two triangles: 0 1 2, 2 1 3, over the corners (-1, -1) to (1, 1).
    let k = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u)[corner];
    let c = vec2<f32>(f32(k & 1u), f32(k >> 1u)) * 2.0 - 1.0;
    let turn = in.velocity_turn.w;
    let s = sin(turn);
    let co = cos(turn);
    var at = vec2<f32>(c.x * co - c.y * s, c.x * s + c.y * co);
    let center = in.center_size.xyz;
    let size = in.center_size.w;
    var half = vec2<f32>(size * 0.5);
    var right = fy_view_axis(vec2<f32>(1.0, 0.0));
    var up = fy_view_axis(vec2<f32>(0.0, 1.0));
    let to_camera = to_viewer(center);
    switch u32(in.params.w) {
        // Stretched: long the way it moves, as seen, and as wide as it is
        // across that; facing the camera as it turns round it.
        case 1u: {
            let v = in.velocity_turn.xyz;
            let along = v - to_camera * dot(v, to_camera);
            if dot(along, along) > 1e-10 {
                up = normalize(along);
                right = normalize(cross(up, to_camera));
                let length = size * in.particle.z + length(v) * in.particle.w;
                half = vec2<f32>(size * 0.5, max(length, size) * 0.5);
                at = c;
            }
        }
        // Flat, facing up, its top away along -z.
        case 2u: {
            right = vec3<f32>(1.0, 0.0, 0.0);
            up = vec3<f32>(0.0, 0.0, -1.0);
        }
        // Upright, turned round up to face the camera.
        case 3u: {
            up = vec3<f32>(0.0, 1.0, 0.0);
            let level = vec3<f32>(to_camera.x, 0.0, to_camera.z);
            right = select(vec3<f32>(1.0, 0.0, 0.0), normalize(cross(up, level)), dot(level, level) > 1e-10);
        }
        default: {}
    }
    let p = center + right * at.x * half.x + up * at.y * half.y;
    let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    return fy_particle_out(in, p, cross(right, up), vec4<f32>(right, 1.0), uv);
}

// `v` turned round up by the angle whose sine and cosine are `s` and `c`.
fn fy_turn_round_up(v: vec3<f32>, s: f32, c: f32) -> vec3<f32> {
    return vec3<f32>(v.x * c + v.z * s, v.y, v.z * c - v.x * s);
}

// A mesh particle: the mesh, its size across a metre of it, turned round
// up by its rotation.
@vertex
fn vs_fy_particle_mesh(mesh: FyParticleMeshIn, in: FyParticleIn) -> FyParticleOut {
    let s = sin(in.velocity_turn.w);
    let c = cos(in.velocity_turn.w);
    let p = in.center_size.xyz + fy_turn_round_up(mesh.position, s, c) * in.center_size.w;
    let tangent = vec4<f32>(fy_turn_round_up(mesh.tangent.xyz, s, c), mesh.tangent.w);
    return fy_particle_out(in, p, fy_turn_round_up(mesh.normal, s, c), tangent, mesh.uv);
}

fn fy_particle_vertex(in: FyParticleOut) -> VertexOut {
    return VertexOut(in.clip, in.world_position, in.normal, in.base_color, in.params, in.tangent, in.uv,
        in.emissive, in.own, in.values);
}

// Linear radiance and alpha, not premultiplied.
fn fy_particle_shaded(in: FyParticleOut, front_facing: bool) -> vec4<f32> {
    if particle_lit {
        return shade(fy_particle_vertex(in), front_facing);
    }
    var color = in.base_color;
    if textured {
        color *= textureSample(base_color_texture, material_sampler, in.uv);
    }
    return vec4<f32>(color.rgb + in.emissive.rgb, color.a);
}

// Soft particles: faded out as they come within their softness of what's
// behind them, so they don't cut a hard line where they meet it.
fn fy_soft(in: FyParticleOut) -> f32 {
    let behind = scene_depth(in.clip.xy * frame.view_size.xy) - view_depth(in.world_position);
    return saturate(behind / max(in.softness, 1e-4));
}

// Laid over what's behind by their alpha, premultiplied, into the fog.
fn fy_particle_transparent(in: FyParticleOut, color: vec4<f32>, fade: f32) -> vec4<f32> {
    let alpha = saturate(color.a) * fade;
    return vec4<f32>(fogged(tonemap_neutral(color.rgb), in.world_position) * alpha, alpha);
}

// Their light, times their alpha, added to what's behind, fading to
// nothing in the fog.
fn fy_particle_additive(in: FyParticleOut, color: vec4<f32>, fade: f32) -> vec4<f32> {
    let alpha = saturate(color.a) * fade;
    return vec4<f32>(tonemap_neutral(color.rgb * alpha) * (1.0 - fog_amount(in.world_position)), alpha);
}

@fragment
fn fs_fy_particle_transparent(in: FyParticleOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return fy_particle_transparent(in, fy_particle_shaded(in, front), 1.0);
}

@fragment
fn fs_fy_particle_additive(in: FyParticleOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return fy_particle_additive(in, fy_particle_shaded(in, front), 1.0);
}

@fragment
fn fs_fy_particle_soft_transparent(in: FyParticleOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let color = fy_particle_shaded(in, front);
    return fy_particle_transparent(in, color, fy_soft(in));
}

@fragment
fn fs_fy_particle_soft_additive(in: FyParticleOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let color = fy_particle_shaded(in, front);
    return fy_particle_additive(in, color, fy_soft(in));
}

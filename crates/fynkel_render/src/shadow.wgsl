// Shadow maps: casters' depth as a light sees them (shadows.rs). Opaque
// casters write depth only, with no fragment shader; cut-outs drop what
// their alpha cuts away, as the forward pass does.

struct View {
    // World to map: x and y from -1 to 1 across, depth from 0 to 1.
    matrix: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;

// A material's textures (forward.wgsl's group 1): only the base colour's
// alpha is read.
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(5) var material_sampler: sampler;

struct In {
    @location(0) position: vec3<f32>,
    // The UV's bits, then the vertex's colour (unread).
    @location(3) uv_color: vec3<u32>,
    // Per caster (instance step):
    @location(4) model_0: vec4<f32>,
    @location(5) model_1: vec4<f32>,
    @location(6) model_2: vec4<f32>,
    @location(7) model_3: vec4<f32>,
    // xy: UV scale, zw: UV offset.
    @location(8) uv_transform: vec4<f32>,
    // x: the base colour's alpha, y: the alpha cutoff, z: where a skinned
    // caster's joints start.
    @location(9) alpha: vec4<f32>,
};

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) alpha: vec2<f32>,
};

// A caster's vertex in the map, at `world`.
fn caster_vertex(in: In, world: vec4<f32>) -> Out {
    var out: Out;
    out.clip = view.matrix * world;
    out.uv = bitcast<vec2<f32>>(in.uv_color.xy) * in.uv_transform.xy + in.uv_transform.zw;
    out.alpha = in.alpha.xy;
    return out;
}

// Where the caster's model matrix puts the vertex.
fn placed(in: In) -> vec4<f32> {
    return mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3) * vec4<f32>(in.position, 1.0);
}

// Where a skinned caster's joints put it (skin.wgsl).
fn bent(in: In, skin: SkinIn) -> vec4<f32> {
    let m = skin_rows(skin, u32(in.alpha.z));
    let p = vec4<f32>(in.position, 1.0);
    return vec4<f32>(dot(m[0], p), dot(m[1], p), dot(m[2], p), 1.0);
}

// A cascade's casters nearer the sun than the map's depth range reaches
// are flattened onto its near end, so they still cast (pancaking; the map
// is orthographic, so w is 1).
fn pancake(out: Out) -> Out {
    var flat = out;
    flat.clip.z = max(out.clip.z, 0.0);
    return flat;
}

@vertex
fn vs_main(in: In) -> Out {
    return caster_vertex(in, placed(in));
}

@vertex
fn vs_cascade(in: In) -> Out {
    return pancake(caster_vertex(in, placed(in)));
}

@vertex
fn vs_main_skinned(in: In, skin: SkinIn) -> Out {
    return caster_vertex(in, bent(in, skin));
}

@vertex
fn vs_cascade_skinned(in: In, skin: SkinIn) -> Out {
    return pancake(caster_vertex(in, bent(in, skin)));
}

@fragment
fn fs_cutout(in: Out) {
    let alpha = in.alpha.x * textureSample(base_color_texture, material_sampler, in.uv).a;
    if alpha < in.alpha.y {
        discard;
    }
}

// A shader asset's shadow stage (ADR-0031): depth only from the sun, the
// vertex moved by the shader's `vertex()` as in the scene, a `mask`
// material's cutoff from its `surface()`. `surface.rs` appends this and
// `mesh_input.wgsl` after the shader's code, with `fynkel_hook` written
// for the shader.

// One cascade's slot of the shadow pass's uniform (`ShadowUniforms`).
struct FynkelShadow {
    view_proj: mat4x4<f32>,
    // x: the world's time; y: seconds since the previous frame; zw: the
    // internal target's size in pixels (as `Frame.time`).
    time: vec4<f32>,
    // xyz: the camera's position.
    camera_pos: vec4<f32>,
};
@group(0) @binding(0) var<uniform> fynkel_shadow: FynkelShadow;

fn fynkel_begin() {
    globals = Globals(
        fynkel_shadow.time.x,
        fynkel_shadow.time.y,
        fynkel_shadow.camera_pos.xyz,
        fynkel_shadow.time.zw,
    );
    material = fynkel_load_material();
}

struct FynkelShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    @location(3) uv1: vec2<f32>,
    @location(4) color: vec4<f32>,
    // The alpha cutoff, or -1 (opaque).
    @location(5) cutoff: f32,
};

fn fynkel_cast(v: VsIn, model: mat4x4<f32>) -> FynkelShadowOut {
    var out: FynkelShadowOut;
    let world = model * vec4<f32>(v.position, 1.0);
    out.clip = fynkel_shadow.view_proj * world;
    out.world_pos = world.xyz;
    out.world_normal = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    out.uv0 = v.uv0;
    out.uv1 = v.uv1;
    out.color = v.tint * v.color;
    out.cutoff = v.flags.x;
    return out;
}

@vertex
fn fynkel_shadow_vs(in: VsIn) -> FynkelShadowOut {
    fynkel_begin();
    let v = fynkel_hook(in);
    return fynkel_cast(v, mat4x4<f32>(v.m0, v.m1, v.m2, v.m3));
}

@vertex
fn fynkel_shadow_vs_skinned(in: VsIn, skin: VsSkin) -> FynkelShadowOut {
    fynkel_begin();
    let v = fynkel_hook(in);
    return fynkel_cast(v, mat4x4<f32>(v.m0, v.m1, v.m2, v.m3) * fynkel_pose(v, skin));
}

@fragment
fn fynkel_shadow_fs(in: FynkelShadowOut, @builtin(front_facing) front_facing: bool) {
    if in.cutoff < 0.0 {
        return;
    }
    fynkel_begin();
    var s: SurfaceInput;
    s.world_pos = in.world_pos;
    var n = normalize(in.world_normal);
    if !front_facing {
        n = -n;
    }
    s.world_normal = n;
    s.view_dir = normalize(fynkel_shadow.camera_pos.xyz - in.world_pos);
    s.uv0 = in.uv0;
    s.uv1 = in.uv1;
    s.color = in.color;
    s.screen_uv = vec2<f32>(0.0);
    s.front_facing = front_facing;
    if surface(s).alpha < in.cutoff {
        discard;
    }
}

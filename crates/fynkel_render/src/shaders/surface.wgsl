// A shader asset's entry points (ADR-0031): the engine's vertex stage
// around the shader's `vertex()`, and the engine's light around its
// `surface()`. `surface.rs` appends this after the shader's code, with
// `fynkel_hook` (the shader's `vertex()`, or nothing) and `FYNKEL_LIT`
// (`lighting lit`) written for the shader.

// Fills what the shader's code reads: `globals` from the frame and
// `material` from the material's block.
fn fynkel_begin() {
    globals = Globals(frame.time.x, frame.time.y, frame.camera_pos.xyz, frame.time.zw);
    material = fynkel_load_material();
}

@vertex
fn fynkel_vs(in: VsIn) -> VsOut {
    fynkel_begin();
    let v = fynkel_hook(in);
    return fynkel_place(v, mat4x4<f32>(v.m0, v.m1, v.m2, v.m3));
}

@vertex
fn fynkel_vs_skinned(in: VsIn, skin: VsSkin) -> VsOut {
    fynkel_begin();
    let v = fynkel_hook(in);
    return fynkel_place(v, mat4x4<f32>(v.m0, v.m1, v.m2, v.m3) * fynkel_pose(v, skin));
}

// What the surface sees of the pixel (`SurfaceInput`).
fn fynkel_input(in: VsOut, front_facing: bool) -> SurfaceInput {
    var s: SurfaceInput;
    s.world_pos = in.world_pos;
    var n = normalize(in.world_normal);
    if !front_facing {
        n = -n;
    }
    s.world_normal = n;
    s.view_dir = normalize(frame.camera_pos.xyz - in.world_pos);
    s.uv0 = in.uv;
    s.uv1 = in.uv1;
    s.color = in.color;
    s.screen_uv = in.clip.xy / frame.time.zw;
    s.front_facing = front_facing;
    return s;
}

// The shader's surface, with a `mask` material's cutoff applied.
fn fynkel_surface(in: VsOut, front_facing: bool) -> Surface {
    fynkel_begin();
    let s = surface(fynkel_input(in, front_facing));
    if in.flags.x >= 0.0 && s.alpha < in.flags.x {
        discard;
    }
    return s;
}

@fragment
fn fynkel_fs(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let s = fynkel_surface(in, front_facing);
    let gray = (u32(in.flags.w) & 2u) != 0u;
    if !FYNKEL_LIT {
        // Unlit: display-referred, as the built-in's unlit path.
        if gray {
            return vec4<f32>(grayed(s.base_color), 2.0);
        }
        return vec4<f32>(s.base_color, 2.0);
    }
    // Opaque and mask texels are tagged 1; a blend material writes its
    // alpha (the transparent pipeline blends by it).
    var tag = 1.0;
    if in.flags.x < -1.5 {
        tag = clamp(s.alpha, 0.0, 1.0);
    }
    var l: LitSurface;
    l.albedo = s.base_color;
    l.metallic = s.metallic;
    l.roughness = s.roughness;
    l.normal = s.normal;
    l.occlusion = s.occlusion;
    l.emission = s.emission;
    var radiance = light_surface(in, front_facing, l);
    if gray {
        radiance = grayed(radiance);
    }
    return vec4<f32>(radiance, tag);
}

// The depth prepass and the selection outline's mask: only a `mask`
// material runs its surface, for the cutoff.
@fragment
fn fynkel_fs_depth(in: VsOut, @builtin(front_facing) front_facing: bool) {
    if in.flags.x >= 0.0 {
        _ = fynkel_surface(in, front_facing);
    }
}

@fragment
fn fynkel_fs_mask(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    if in.flags.x >= 0.0 {
        _ = fynkel_surface(in, front_facing);
    }
    return vec4<f32>(in.params.x, 0.0, 0.0, 1.0);
}

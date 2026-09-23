//! Viewport ABI v7: `fynkel_apple_viewport_set_view` — a viewport's own
//! camera over whatever world it renders (the editor's Scene view on a
//! running game, ADR-0021 P7 L5), through the C render entry on a
//! `FynkelWorld`, leaving the world's own camera alone.

use std::ptr;

use fynkel_native::{
    fynkel_runtime_create, fynkel_runtime_destroy, fynkel_world_create, fynkel_world_destroy,
    FynkelWorld,
};
use fynkel_platform_apple::{
    fynkel_apple_viewport_debug_lines, fynkel_apple_viewport_read_frame,
    fynkel_apple_viewport_render, fynkel_apple_viewport_set_view, FynkelAppleViewCamera, SyncMode,
    Viewport,
};

const W: u32 = 48;
const H: u32 = 32;

/// Renders `world` with a line two metres down -Z; answers the pixels that
/// are not the clear colour.
fn drawn(vp: &mut Viewport, world: *const FynkelWorld) -> usize {
    #[rustfmt::skip]
    let line: [f32; 10] = [-1.0, 0.0, -2.0, 1.0, 0.0, -2.0, 1.0, 0.0, 0.0, 1.0];
    unsafe { fynkel_apple_viewport_debug_lines(vp, line.as_ptr(), 1) };
    let slot = unsafe { fynkel_apple_viewport_render(vp, world) };
    assert!(slot >= 0, "rendered: {slot}");
    let mut out = vec![0u8; (W * H * 4) as usize];
    let n =
        unsafe { fynkel_apple_viewport_read_frame(vp, slot, out.as_mut_ptr(), out.len() as u32) };
    assert_eq!(n, out.len() as i32);
    // Presented, as the host's texture would: the slot is free again.
    vp.ring().wait_idle();
    assert!(vp.ring().acquire_presented() >= 0);
    let clear = fynkel_render::clear_rgba8();
    out.chunks_exact(4).filter(|p| *p != clear).count()
}

#[test]
fn a_viewport_looks_through_its_own_camera_and_the_world_keeps_its() {
    let rt = fynkel_runtime_create();
    let world = unsafe { fynkel_world_create(rt, ptr::null()) };
    assert!(!world.is_null());
    let mut vp = Viewport::new(W, H, 3, SyncMode::MetalCompletion).expect("viewport");

    let facing = drawn(&mut vp, world);
    assert!(
        facing > 10,
        "the line is in front of the world's camera: {facing}"
    );

    // Turned around: the same line is behind this viewport's camera.
    let away = glam::Mat4::from_rotation_y(std::f32::consts::PI).to_cols_array();
    let camera = FynkelAppleViewCamera {
        projection: 0,
        fov_y: 60f32.to_radians(),
        ortho_height: 10.0,
        near: 0.1,
        far: 100.0,
        exposure_mode: 0,
        exposure: 12.0,
        culling_mask: u32::MAX,
    };
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view(&mut vp, away.as_ptr(), &camera) },
        0
    );
    assert_eq!(drawn(&mut vp, world), 0, "looking away");
    assert!(
        unsafe { (*world).core().extract().camera.is_none() },
        "the world's camera is untouched"
    );

    // Another viewport on the same world still draws the world's view.
    let mut other = Viewport::new(W, H, 3, SyncMode::MetalCompletion).expect("viewport");
    assert_eq!(drawn(&mut other, world), facing);

    // Cleared: back to the world's camera.
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view(&mut vp, ptr::null(), ptr::null()) },
        0
    );
    assert_eq!(drawn(&mut vp, world), facing);

    // Refusals change nothing.
    let bad = FynkelAppleViewCamera {
        near: 0.0,
        ..camera
    };
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view(&mut vp, away.as_ptr(), &bad) },
        -1
    );
    let bad = FynkelAppleViewCamera {
        projection: 2,
        ..camera
    };
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view(&mut vp, away.as_ptr(), &bad) },
        -1
    );
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view(&mut vp, away.as_ptr(), ptr::null()) },
        -1
    );
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view(ptr::null_mut(), ptr::null(), ptr::null()) },
        -1
    );
    assert_eq!(drawn(&mut vp, world), facing);

    vp.ring().wait_idle();
    other.ring().wait_idle();
    drop(vp);
    drop(other);
    unsafe {
        fynkel_world_destroy(world);
        fynkel_runtime_destroy(rt);
    }
}

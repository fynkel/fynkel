//! Viewport ABI v9: `fynkel_apple_viewport_extra_worlds` — other worlds
//! drawn into the next render's frame beside the world it is given (the
//! editor's Prefab Mode in context and multi-scene editing): each extracted
//! by the call (the world may go before the render), placed by its
//! transform, greyed by its flag, gone from the render after, and the
//! frame pixel for pixel the offscreen renderer's with the same
//! `fynkel_render::ExtraWorld`s.

use std::ptr;

use fynkel_native::{
    fynkel_runtime_create, fynkel_runtime_destroy, fynkel_world_create, fynkel_world_destroy,
    FynkelWorld,
};
use fynkel_platform_apple::{
    fynkel_apple_abi_version, fynkel_apple_viewport_extra_worlds, fynkel_apple_viewport_read_frame,
    fynkel_apple_viewport_render, SyncMode, Viewport, APPLE_ABI_VERSION,
    FYNKEL_APPLE_EXTRA_WORLD_GRAY,
};
use fynkel_scene::Transform;
use glam::{Mat4, Vec3};

const W: u32 = 64;
const H: u32 = 48;

/// A world holding one placeholder cube at `at`.
unsafe fn cube_world(rt: *mut fynkel_native::FynkelRuntime, at: Vec3) -> *mut FynkelWorld {
    let world = fynkel_world_create(rt, ptr::null(), std::ptr::null_mut());
    assert!(!world.is_null());
    let core = (*world).core_mut();
    core.spawn_now("Cube", Transform::from_position(at), None, vec![], None)
        .expect("spawned");
    core.begin_frame(1.0 / 60.0);
    core.step();
    world
}

/// Renders `world` through the C entry and reads the frame back.
fn frame(vp: &mut Viewport, world: *const FynkelWorld) -> Vec<[u8; 4]> {
    let slot = unsafe { fynkel_apple_viewport_render(vp, world) };
    assert!(slot >= 0, "rendered: {slot}");
    let mut out = vec![0u8; (W * H * 4) as usize];
    let n =
        unsafe { fynkel_apple_viewport_read_frame(vp, slot, out.as_mut_ptr(), out.len() as u32) };
    assert_eq!(n, out.len() as i32);
    vp.ring().wait_idle();
    assert!(vp.ring().acquire_presented() >= 0);
    out.chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// The pixel `p` lands on through the renderer's default camera (the
/// worlds here have none).
fn pixel(p: Vec3) -> usize {
    let ndc = fynkel_render::Camera::default()
        .view_proj(W, H)
        .project_point3(p);
    let (x, y) = (
        ((ndc.x * 0.5 + 0.5) * W as f32) as usize,
        ((0.5 - ndc.y * 0.5) * H as f32) as usize,
    );
    y * W as usize + x
}

#[test]
fn extra_worlds_draw_into_the_next_frame_only_and_equal_offscreen() {
    assert_eq!(fynkel_apple_abi_version(), 14);
    assert_eq!(APPLE_ABI_VERSION, 14);
    let rt = fynkel_runtime_create();
    let (main, context) = unsafe { (cube_world(rt, Vec3::ZERO), cube_world(rt, Vec3::ZERO)) };
    let mut vp = Viewport::new(W, H, 3, SyncMode::MetalCompletion).expect("viewport");
    let clear = fynkel_render::clear_rgba8();
    let (left, right) = (Vec3::new(-1.6, 0.0, 0.0), Vec3::new(1.6, 0.0, 0.0));

    // The same world twice: to the left greyed, to the right as it is.
    let transforms: Vec<f32> = [Mat4::from_translation(left), Mat4::from_translation(right)]
        .iter()
        .flat_map(|m| m.to_cols_array())
        .collect();
    let worlds = [context as *const FynkelWorld, context];
    let flags = [FYNKEL_APPLE_EXTRA_WORLD_GRAY, 0];
    let queued = unsafe {
        fynkel_apple_viewport_extra_worlds(
            &mut vp,
            worlds.as_ptr(),
            transforms.as_ptr(),
            flags.as_ptr(),
            2,
        )
    };
    assert_eq!(queued, 2);
    let px = frame(&mut vp, main);
    assert_eq!(vp.frame_stats().extra_worlds, 2);
    assert_eq!(vp.frame_stats().instances, 3);
    for at in [Vec3::ZERO, left, right] {
        assert_ne!(px[pixel(at)], clear, "drawn at {at}");
    }
    let spread = |p: [u8; 4]| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]);
    assert!(spread(px[pixel(right)]) > 20, "{:?}", px[pixel(right)]);
    assert!(spread(px[pixel(left)]) <= 1, "grey: {:?}", px[pixel(left)]);

    // The same through the offscreen path.
    let (device, queue) = fynkel_render::headless_device().expect("gpu");
    let mut renderer =
        fynkel_render::ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, W, H);
    let extract = unsafe { (*context).core().extract() };
    for (at, gray) in [(left, true), (right, false)] {
        renderer.push_extra_world(fynkel_render::ExtraWorld {
            extract: extract.clone(),
            transform: Mat4::from_translation(at),
            gray,
        });
    }
    let main_extract = unsafe { (*main).core().extract() };
    let offscreen = fynkel_render::render_offscreen(&device, &queue, &mut renderer, &main_extract);
    let differing = px.iter().zip(&offscreen).filter(|(a, b)| a != b).count();
    assert_eq!(
        differing, 0,
        "viewport and offscreen differ in {differing} pixels"
    );

    // The next render has none.
    let px = frame(&mut vp, main);
    assert_eq!(vp.frame_stats().extra_worlds, 0);
    assert_ne!(px[pixel(Vec3::ZERO)], clear);
    for at in [left, right] {
        assert_eq!(px[pixel(at)], clear, "gone from {at}");
    }

    // Extracted by the call: a world destroyed before the render is still
    // drawn by it. A NULL world is skipped.
    let gone = unsafe { cube_world(rt, Vec3::ZERO) };
    let worlds = [gone as *const FynkelWorld, ptr::null()];
    let queued = unsafe {
        fynkel_apple_viewport_extra_worlds(
            &mut vp,
            worlds.as_ptr(),
            transforms.as_ptr(),
            flags.as_ptr(),
            2,
        )
    };
    assert_eq!(queued, 1);
    unsafe { fynkel_world_destroy(gone) };
    let px = frame(&mut vp, main);
    assert_eq!(vp.frame_stats().extra_worlds, 1);
    assert_ne!(px[pixel(left)], clear);
    assert_eq!(px[pixel(right)], clear);

    // Refusals queue nothing and leave an earlier call's worlds; a count
    // of 0 clears them.
    let worlds = [context as *const FynkelWorld];
    unsafe {
        assert_eq!(
            fynkel_apple_viewport_extra_worlds(
                &mut vp,
                worlds.as_ptr(),
                transforms.as_ptr(),
                flags.as_ptr(),
                1
            ),
            1
        );
        let mut bad = transforms.clone();
        bad[5] = f32::INFINITY;
        for (w, t, f) in [
            (ptr::null(), transforms.as_ptr(), flags.as_ptr()),
            (worlds.as_ptr(), ptr::null(), flags.as_ptr()),
            (worlds.as_ptr(), transforms.as_ptr(), ptr::null()),
            (worlds.as_ptr(), bad.as_ptr(), flags.as_ptr()),
        ] {
            assert_eq!(fynkel_apple_viewport_extra_worlds(&mut vp, w, t, f, 1), -1);
        }
        assert_eq!(
            fynkel_apple_viewport_extra_worlds(
                ptr::null_mut(),
                worlds.as_ptr(),
                transforms.as_ptr(),
                flags.as_ptr(),
                1
            ),
            -1
        );
    }
    frame(&mut vp, main);
    assert_eq!(vp.frame_stats().extra_worlds, 1, "the earlier call's world");
    unsafe {
        fynkel_apple_viewport_extra_worlds(
            &mut vp,
            worlds.as_ptr(),
            transforms.as_ptr(),
            flags.as_ptr(),
            1,
        );
        assert_eq!(
            fynkel_apple_viewport_extra_worlds(&mut vp, ptr::null(), ptr::null(), ptr::null(), 0),
            0
        );
    }
    frame(&mut vp, main);
    assert_eq!(vp.frame_stats().extra_worlds, 0, "cleared");

    vp.ring().wait_idle();
    drop(vp);
    unsafe {
        fynkel_world_destroy(main);
        fynkel_world_destroy(context);
        fynkel_runtime_destroy(rt);
    }
}

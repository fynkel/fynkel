//! Viewport ABI v16: `fynkel_apple_viewport_set_view_camera` — a viewport
//! that looks through one of the world's own cameras, picked by entity,
//! instead of the one the frame picks by priority (a game's second camera
//! shown in a panel: a minimap, a model preview, ADR-0062 amendment 1). The
//! camera's pose is the frame's interpolated one, as the main picture's;
//! a camera that is gone draws nothing.

use std::ptr;

use fynkel_core::components::ComponentValue;
use fynkel_core::{Camera, Entity, Transform};
use fynkel_native::{
    fynkel_runtime_create, fynkel_runtime_destroy, fynkel_world_create, fynkel_world_destroy,
    FynkelWorld,
};
use fynkel_platform_apple::{
    fynkel_apple_abi_version, fynkel_apple_viewport_debug_lines, fynkel_apple_viewport_read_frame,
    fynkel_apple_viewport_render, fynkel_apple_viewport_set_pipelined,
    fynkel_apple_viewport_set_placeholders, fynkel_apple_viewport_set_view,
    fynkel_apple_viewport_set_view_camera, FynkelAppleViewCamera, SyncMode, Viewport,
    FYNKEL_APPLE_RENDER_NO_CAMERA,
};
use glam::Vec3;

const W: u32 = 48;
const H: u32 = 32;

/// Renders `world` with a short line at (20, 0, 0) across x; answers the
/// slot and how many pixels are not the clear colour (0 when nothing was
/// drawn).
fn drawn(vp: &mut Viewport, world: *const FynkelWorld) -> (i32, usize) {
    #[rustfmt::skip]
    let line: [f32; 10] = [19.0, 0.0, 0.0, 21.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    unsafe { fynkel_apple_viewport_debug_lines(vp, line.as_ptr(), 1) };
    let slot = unsafe { fynkel_apple_viewport_render(vp, world) };
    if slot < 0 {
        return (slot, 0);
    }
    let mut out = vec![0u8; (W * H * 4) as usize];
    let n =
        unsafe { fynkel_apple_viewport_read_frame(vp, slot, out.as_mut_ptr(), out.len() as u32) };
    assert_eq!(n, out.len() as i32);
    vp.ring().wait_idle();
    assert!(vp.ring().acquire_presented() >= 0);
    let clear = fynkel_render::clear_rgba8();
    (slot, out.chunks_exact(4).filter(|p| *p != clear).count())
}

unsafe fn camera(world: *mut FynkelWorld, at: Vec3, priority: i32) -> Entity {
    let core = (*world).core_mut();
    let (e, _) = core
        .spawn_now(
            "Camera",
            Transform::from_position(at),
            None,
            vec![ComponentValue::Camera(Camera {
                priority,
                ..Camera::default()
            })],
            None,
        )
        .expect("spawned");
    e
}

unsafe fn step(world: *mut FynkelWorld) {
    let core = (*world).core_mut();
    core.begin_frame(1.0 / 60.0);
    core.step();
}

fn view_camera(vp: &mut Viewport, e: Entity) -> i32 {
    let bits = e.to_bits();
    unsafe {
        fynkel_apple_viewport_set_view_camera(
            vp,
            bits as u64,
            (bits >> 64) as u32,
            (bits >> 96) as u32,
        )
    }
}

#[test]
fn a_viewport_looks_through_a_camera_of_the_world_by_entity() {
    assert_eq!(fynkel_apple_abi_version(), 16);
    let rt = fynkel_runtime_create();
    let world = unsafe { fynkel_world_create(rt, ptr::null(), std::ptr::null_mut()) };
    assert!(!world.is_null());
    // The main camera (priority 1) looks at the origin, where nothing is;
    // the panel's camera (priority 0) at the line twenty metres along x.
    let (_main, panel) = unsafe {
        (
            camera(world, Vec3::new(0.0, 0.0, 5.0), 1),
            camera(world, Vec3::new(20.0, 0.0, 5.0), 0),
        )
    };
    unsafe { step(world) };
    let mut game = Viewport::new(W, H, 3, SyncMode::MetalCompletion).expect("viewport");
    let mut vp = Viewport::new(W, H, 3, SyncMode::MetalCompletion).expect("viewport");
    for v in [&mut game, &mut vp] {
        unsafe { fynkel_apple_viewport_set_placeholders(v, 0) };
    }
    assert_eq!(drawn(&mut game, world).1, 0, "the main camera sees nothing");

    assert_eq!(view_camera(&mut vp, panel), 0);
    let (_, seen) = drawn(&mut vp, world);
    assert!(seen > 10, "the panel's camera sees the line: {seen}");
    assert_eq!(
        drawn(&mut game, world).1,
        0,
        "the game's picture is its own"
    );

    // It follows its camera (the frame's pose).
    unsafe {
        (*world)
            .core_mut()
            .edit_gameplay(|edit| {
                edit.set_transform(panel, Transform::from_position(Vec3::new(40.0, 0.0, 5.0)))
            })
            .unwrap();
        step(world);
        step(world);
    }
    assert_eq!(drawn(&mut vp, world).1, 0, "moved away from the line");

    // A camera that is gone draws nothing; the last picture stays.
    unsafe {
        (*world)
            .core_mut()
            .edit_gameplay(|edit| edit.destroy(panel))
            .unwrap();
        step(world);
    }
    assert_eq!(drawn(&mut vp, world).0, FYNKEL_APPLE_RENDER_NO_CAMERA);
    // On the render worker too (answered at once, not a frame later).
    let mut worker = Viewport::new(W, H, 3, SyncMode::MetalCompletion).expect("viewport");
    unsafe { fynkel_apple_viewport_set_pipelined(&mut worker, 1) };
    assert_eq!(view_camera(&mut worker, panel), 0);
    assert_eq!(
        unsafe { fynkel_apple_viewport_render(&mut worker, world) },
        FYNKEL_APPLE_RENDER_NO_CAMERA
    );
    drop(worker);

    // Null hands the picture back to the world's camera; a view camera and
    // a view matrix replace each other.
    let other = unsafe { camera(world, Vec3::new(20.0, 0.0, 5.0), 0) };
    unsafe { step(world) };
    assert_eq!(view_camera(&mut vp, other), 0);
    assert!(drawn(&mut vp, world).1 > 10);
    assert_eq!(view_camera(&mut vp, Entity::NULL), 0);
    assert_eq!(drawn(&mut vp, world).1, 0, "the world's camera again");
    assert_eq!(view_camera(&mut vp, other), 0);
    let at_line = glam::Mat4::from_translation(Vec3::new(0.0, 0.0, 5.0)).to_cols_array();
    let fields = FynkelAppleViewCamera {
        projection: 0,
        fov_y: 1.0,
        ortho_height: 10.0,
        near: 0.1,
        far: 100.0,
        exposure_mode: 0,
        exposure: 12.0,
        culling_mask: u32::MAX,
    };
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view(&mut vp, at_line.as_ptr(), &fields) },
        0
    );
    assert_eq!(drawn(&mut vp, world).1, 0, "the matrix replaced the camera");
    assert_eq!(view_camera(&mut vp, other), 0);
    assert!(drawn(&mut vp, world).1 > 10, "and the camera the matrix");
    assert_eq!(
        unsafe { fynkel_apple_viewport_set_view_camera(ptr::null_mut(), 0, 0, 0) },
        -1
    );

    game.ring().wait_idle();
    vp.ring().wait_idle();
    drop(game);
    drop(vp);
    unsafe {
        fynkel_world_destroy(world);
        fynkel_runtime_destroy(rt);
    }
}

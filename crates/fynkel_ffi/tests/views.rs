//! Several views at once: one GPU device for the process, and one set of
//! meshes and textures per world, shared by the world's views.
#![allow(unsafe_code)]

mod common;

use common::*;
use fynkel_ffi::*;
use fynkel_golden::assert_golden;
use fynkel_world::{Transform, Vec3};

fn stats(view: &View) -> FyViewStats {
    let mut stats = FyViewStats::default();
    // SAFETY: a live view and a pointer to a local.
    ok(unsafe { fy_view_stats(view.0, &mut stats) });
    stats
}

#[test]
fn two_views_of_one_world_both_draw_it() {
    let world = World::new();
    stage1(&world);
    let (a, b) = (View::headless(&world, 256, 256), View::headless(&world, 256, 256));
    // Both frames first: the second view draws while the first's uploads
    // may still be in hand.
    let (na, nb) = (a.frame(), b.frame());
    assert_golden("cube", &a.read(na, 256, 256));
    assert_golden("cube", &b.read(nb, 256, 256));
    // And again, with nothing left to upload.
    assert_golden("cube", &b.read(b.frame(), 256, 256));
    assert_golden("cube", &a.read(a.frame(), 256, 256));
}

#[test]
fn a_worlds_views_upload_its_meshes_once() {
    let world = World::new();
    stage1(&world);
    let a = View::headless(&world, 64, 64);
    a.read(a.frame(), 64, 64);
    let alone = stats(&a).mesh_bytes;
    assert!(alone > 0);
    let b = View::headless(&world, 64, 64);
    b.read(b.frame(), 64, 64);
    a.read(a.frame(), 64, 64);
    assert_eq!(stats(&a).mesh_bytes, alone, "the second view uploaded nothing");
    assert_eq!(stats(&b).mesh_bytes, alone, "and sees the same meshes");
}

#[test]
fn a_change_shows_in_every_view() {
    let world = World::new();
    let cube = stage1(&world);
    let (a, b) = (View::headless(&world, 64, 64), View::headless(&world, 64, 64));
    let before = a.read(a.frame(), 64, 64);
    b.read(b.frame(), 64, 64);
    let mut renderer = FyMeshRenderer::default();
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_mesh_renderer_get(world.0, cube, &mut renderer));
        ok(fy_despawn(world.0, cube));
        ok(fy_mesh_remove(world.0, renderer.mesh));
    }
    // One view's frame frees the mesh; the other's, after, draws without it.
    let a_after = a.read(a.frame(), 64, 64);
    let b_after = b.read(b.frame(), 64, 64);
    assert_ne!(before.pixel(32, 32), a_after.pixel(32, 32), "the cube was in the middle");
    assert_eq!(a_after.rgba, b_after.rgba);
    assert_eq!(stats(&b).mesh_bytes, 0, "the mesh is freed for both");
}

#[test]
fn views_of_two_worlds_keep_their_own_meshes() {
    let (one, two) = (World::new(), World::new());
    stage1(&one);
    let a = View::headless(&one, 256, 256);
    let b = View::headless(&two, 64, 64);
    assert_golden("cube", &a.read(a.frame(), 256, 256));
    let empty = b.read(b.frame(), 64, 64);
    assert_eq!(empty.rgba.chunks(4).collect::<std::collections::HashSet<_>>().len(), 1, "nothing in it");
    assert_eq!(stats(&b).mesh_bytes, 0);
    assert!(stats(&a).mesh_bytes > 0);
}

#[test]
fn a_view_outlives_its_sibling() {
    let world = World::new();
    stage1(&world);
    let a = View::headless(&world, 256, 256);
    a.read(a.frame(), 256, 256);
    let b = View::headless(&world, 256, 256);
    b.read(b.frame(), 256, 256);
    drop(a);
    assert_golden("cube", &b.read(b.frame(), 256, 256));
}

#[test]
fn a_view_after_the_last_one_uploads_again() {
    let world = World::new();
    stage1(&world);
    let a = View::headless(&world, 256, 256);
    a.read(a.frame(), 256, 256);
    // The world's meshes go with its last view, and come back with the next.
    drop(a);
    let b = View::headless(&world, 256, 256);
    assert_golden("cube", &b.read(b.frame(), 256, 256));
    assert!(stats(&b).mesh_bytes > 0);
}

/// Adds a camera at `eye` looking at the origin, clearing to `clear`.
fn add_camera(world: &World, eye: Vec3, clear: FyColor) -> u64 {
    let camera = world.spawn();
    let lens = FyCamera {
        fov_y: 60.0,
        near_clip: 0.1,
        far_clip: 1000.0,
        clear_color: clear,
        orthographic: false,
        size: 5.0,
        culling_mask: u32::MAX,
        show_sky: true,
        ..Default::default()
    };
    let looking = Transform::from_position(eye).looking_at(Vec3::ZERO);
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_camera_set(world.0, camera, &lens));
        ok(fy_transform_set(world.0, camera, &transform(looking)));
    }
    camera
}

fn set_camera(view: &View, camera: u64) {
    // SAFETY: a live view.
    ok(unsafe { fy_view_camera_set(view.0, camera) });
}

fn colours(image: &fynkel_golden::Image) -> usize {
    image.rgba.chunks(4).collect::<std::collections::HashSet<_>>().len()
}

#[test]
fn a_view_draws_from_its_own_camera() {
    let world = World::new();
    stage1(&world);
    // Behind the cube, clearing to red.
    let behind = add_camera(&world, Vec3::new(0.0, 0.0, -4.0), srgb(1.0, 0.0, 0.0));
    let (main, other) = (View::headless(&world, 256, 256), View::headless(&world, 256, 256));
    set_camera(&other, behind);
    let seen = other.read(other.frame(), 256, 256);
    assert_golden("cube", &main.read(main.frame(), 256, 256));
    assert_eq!(seen.pixel(0, 0), [255, 0, 0, 255], "its clear colour");
    assert_ne!(seen.pixel(128, 128), [255, 0, 0, 255], "and the cube in the middle");

    // Back to the world's main camera.
    set_camera(&other, 0);
    assert_golden("cube", &other.read(other.frame(), 256, 256));
}

#[test]
fn a_view_whose_camera_is_gone_draws_black() {
    let world = World::new();
    stage1(&world);
    let own = add_camera(&world, Vec3::new(3.0, 3.0, 3.0), srgb(0.0, 0.0, 1.0));
    let view = View::headless(&world, 64, 64);
    set_camera(&view, own);
    assert_eq!(view.read(view.frame(), 64, 64).pixel(0, 0), [0, 0, 255, 255]);
    // SAFETY: a live world.
    ok(unsafe { fy_despawn(world.0, own) });
    let image = view.read(view.frame(), 64, 64);
    assert_eq!((colours(&image), image.pixel(0, 0)), (1, [0, 0, 0, 255]), "not the main camera's picture");
}

fn shake(world: &World, camera: u64, by: Vec3) {
    let offset = FyVec3 { x: by.x, y: by.y, z: by.z };
    // SAFETY: a live world and a pointer to a local.
    ok(unsafe { fy_camera_shake_set(world.0, camera, &offset) });
}

#[test]
fn a_shake_moves_the_shaken_camera_when_drawn_and_only_it() {
    let world = World::new();
    stage1(&world);
    let mut main = 0;
    // SAFETY: a live world and a local out-value.
    ok(unsafe { fy_camera_active(world.0, &mut main) });
    let other = add_camera(&world, Vec3::new(0.0, 1.5, 4.0), srgb(0.1, 0.1, 0.12));
    let (a, b) = (View::headless(&world, 256, 256), View::headless(&world, 256, 256));
    set_camera(&b, other);
    let still = b.read(b.frame(), 256, 256);

    shake(&world, main, Vec3::new(0.5, 0.0, 0.0));
    let shaken = a.read(a.frame(), 256, 256);
    assert_ne!(shaken.rgba, still.rgba, "drawn half a metre to the side");
    assert_eq!(b.read(b.frame(), 256, 256).rgba, still.rgba, "the other camera isn't shaken");
    let mut moved = FyTransform::default();
    // SAFETY: a live world and a pointer to a local.
    ok(unsafe { fy_transform_get(world.0, main, &mut moved) });
    assert_eq!(moved.position, FyVec3 { x: 0.0, y: 1.5, z: 4.0 }, "the object stays where it was");

    // SAFETY: a live world; a null offset stops it.
    ok(unsafe { fy_camera_shake_set(world.0, 0, std::ptr::null()) });
    assert_golden("cube", &a.read(a.frame(), 256, 256));
}

#[test]
fn a_camera_whose_near_is_not_below_its_far_is_refused_saying_so() {
    let world = World::new();
    stage1(&world);
    let mut main = 0;
    let mut lens = FyCamera::default();
    // SAFETY: a live world and local out-values.
    unsafe {
        ok(fy_camera_active(world.0, &mut main));
        ok(fy_camera_get(world.0, main, &mut lens));
        ok(fy_camera_set(world.0, main, &FyCamera { near_clip: 50.0, far_clip: 10.0, ..lens }));
    }
    let view = View::headless(&world, 256, 256);
    let mut number = 0;
    // SAFETY: a live view and a local out-value.
    assert_eq!(unsafe { fy_view_frame(view.0, &mut number) }, FyStatus::InvalidValue);
    let message = last_error();
    assert!(message.contains("near 50") && message.contains("far 10"), "{message}");
    // Setting far after near, as a game may: the next frame draws.
    // SAFETY: a live world.
    ok(unsafe { fy_camera_set(world.0, main, &lens) });
    assert_golden("cube", &view.read(view.frame(), 256, 256));
}

#[test]
fn a_world_draws_the_same_at_every_quality_level_and_refuses_others() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 256, 256);
    for level in [FY_QUALITY_LOW, FY_QUALITY_HIGH, FY_QUALITY_MEDIUM, FY_QUALITY_AUTO] {
        set_level(world.0, level);
        assert_golden("cube", &view.read(view.frame(), 256, 256));
    }
    let mut quality = FyQuality::default();
    // SAFETY: a local out-value.
    assert_eq!(unsafe { fy_quality_preset(4, &mut quality) }, FyStatus::InvalidValue);
    assert!(last_error().contains("4 isn't a level"), "{}", last_error());

    let mut auto = 0;
    // SAFETY: a local out-value.
    ok(unsafe { fy_quality_auto(&mut auto) });
    assert!([FY_QUALITY_LOW, FY_QUALITY_MEDIUM, FY_QUALITY_HIGH].contains(&auto), "{auto}");
    #[cfg(target_os = "macos")]
    assert_eq!(auto, FY_QUALITY_HIGH, "an Apple GPU");
}

/// What GPU this is, for a game choosing its quality by device (Unity's
/// `SystemInfo.graphicsDeviceName` and the rest): its name, its maker's
/// PCI id and the API drawing with it, a line each; its length told first.
#[test]
fn the_gpu_says_what_it_is() {
    let mut len = 0;
    // SAFETY: no buffer, a local out-value.
    ok(unsafe { fy_gpu_describe(std::ptr::null_mut(), 0, &mut len) });
    let mut buf = vec![0u8; len];
    // SAFETY: a buffer of `len` bytes.
    ok(unsafe { fy_gpu_describe(buf.as_mut_ptr(), len, &mut len) });
    let text = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text:?}");
    assert!(!lines[0].is_empty(), "a name: {text:?}");
    lines[1].parse::<u32>().unwrap_or_else(|_| panic!("a maker's id: {text:?}"));
    #[cfg(target_os = "macos")]
    {
        assert!(lines[0].starts_with("Apple"), "{text:?}");
        assert_eq!(lines[2], "metal", "{text:?}");
    }
}

/// A game's own quality: a level's, changed, and what's out of range
/// refused, the world keeping the one it had.
#[test]
fn a_world_takes_a_quality_of_its_own() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 256, 256);
    let mut low = FyQuality::default();
    // SAFETY: a local out-value.
    ok(unsafe { fy_quality_preset(FY_QUALITY_LOW, &mut low) });
    assert_eq!((low.hdr, low.bloom, low.ambient_occlusion, low.shadows), (false, false, false, true));
    assert_eq!((low.shadow_cascades, low.shadow_resolution, low.shadow_softness), (1, 1024, FY_SHADOWS_HARD));
    let mine = FyQuality { bloom: true, ambient_occlusion: true, shadows: false, ..low };
    // SAFETY: a live world, a local value.
    ok(unsafe { fy_world_quality_set(world.0, &mine) });
    assert_golden("cube", &view.read(view.frame(), 256, 256));

    for (bad, says) in [
        (FyQuality { shadow_cascades: 0, ..low }, "0 cascades"),
        (FyQuality { shadow_resolution: 1000, ..low }, "powers of two"),
        (FyQuality { spot_shadows: 9, ..low }, "9 spot maps"),
        (FyQuality { shadow_distance: -1.0, ..low }, "shadow distance"),
        (FyQuality { shadow_softness: 7, ..low }, "7 isn't a shadow softness"),
    ] {
        // SAFETY: a live world, a local value.
        assert_eq!(unsafe { fy_world_quality_set(world.0, &bad) }, FyStatus::InvalidValue, "{bad:?}");
        assert!(last_error().contains(says), "{}", last_error());
    }
    // Shadows off: the rest isn't looked at.
    let off = FyQuality { shadows: false, shadow_cascades: 0, ..low };
    // SAFETY: a live world, a local value.
    ok(unsafe { fy_world_quality_set(world.0, &off) });
}

/// Asks `view` for a frame now and then for half a second; the first
/// failure, if any.
fn fails_soon(view: &View) -> Option<FyStatus> {
    for _ in 0..50 {
        let mut n = 0;
        // SAFETY: a live view, a local out-value.
        let status = unsafe { fy_view_frame(view.0, &mut n) };
        if status != FyStatus::Ok {
            return Some(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

#[test]
fn a_view_bigger_than_the_device_draws_is_refused_and_the_others_draw_on() {
    // Stage 14, BR1: one past the device's largest texture was made, and
    // the device was lost for every view; resized past it, its worker
    // failed for good.
    let world = World::new();
    stage1(&world);
    let (a, b) = (View::headless(&world, 64, 64), View::headless(&world, 64, 64));
    a.read(a.frame(), 64, 64);
    let mut big = std::ptr::null_mut();
    // SAFETY: a live world and a local out-value.
    assert_eq!(unsafe { fy_view_new_headless(world.0, 9000, 100, &mut big) }, FyStatus::InvalidValue);
    assert!(last_error().contains("8192"), "{}", last_error());
    // SAFETY: a live view.
    assert_eq!(unsafe { fy_view_resize(a.0, 100, 9000) }, FyStatus::InvalidValue);
    assert_eq!((fails_soon(&a), fails_soon(&b)), (None, None));
    let image = b.read(b.frame(), 64, 64);
    assert!(image.rgba.chunks(4).any(|p| p != &image.rgba[..4]), "b still draws the cube");
}

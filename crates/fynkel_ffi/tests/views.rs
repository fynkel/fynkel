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

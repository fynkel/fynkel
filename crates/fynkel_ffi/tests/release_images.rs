//! A view lets go of its images when asked, without a frame to draw, and
//! its next frame makes them again; and its stats say what they hold.
#![allow(unsafe_code)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use fynkel_ffi::*;
use fynkel_world::{Transform, Vec3};

fn stats(view: &View) -> FyViewStats {
    let mut stats = FyViewStats::default();
    // SAFETY: a live view and a pointer to a local.
    ok(unsafe { fy_view_stats(view.0, &mut stats) });
    stats
}

#[test]
fn a_view_lets_go_of_its_images_and_makes_them_again() {
    let world = World::new();
    let sun = world.spawn();
    let light = FyLight {
        kind: FY_LIGHT_DIRECTIONAL,
        color: argb(0xFFFFFFFF),
        intensity: 0.5,
        shadows: true,
        ..Default::default()
    };
    let facing = Transform::default().looking_at(Vec3::new(0.6, -1.0, -0.5));
    // SAFETY: a live world, and pointers to locals.
    unsafe {
        ok(fy_light_set(world.0, sun, &light));
        ok(fy_transform_set(world.0, sun, &transform(facing)));
    }
    let cube = stage1(&world);
    let mut renderer = FyMeshRenderer::default();
    // SAFETY: a live world, and pointers to locals.
    unsafe {
        ok(fy_mesh_renderer_get(world.0, cube, &mut renderer));
        renderer.cast_shadows = true;
        ok(fy_mesh_renderer_set(world.0, cube, &renderer));
    }
    set_level(world.0, FY_QUALITY_HIGH);
    let view = View::headless(&world, 64, 64);
    let before = view.read(view.frame(), 64, 64);
    let held = stats(&view);
    assert!(held.shadow_bytes > 0, "the sun casts shadows: {held:?}");

    // SAFETY: a live view.
    ok(unsafe { fy_view_release_images(view.0) });
    let asked = Instant::now();
    while stats(&view).shadow_bytes != 0 {
        assert!(asked.elapsed() < Duration::from_secs(5), "still held, with no frame to draw");
        std::thread::yield_now();
    }

    let after = view.read(view.frame(), 64, 64);
    assert_eq!(stats(&view).shadow_bytes, held.shadow_bytes);
    assert_eq!(before.rgba, after.rgba, "the same picture");
}

#[test]
fn releasing_a_null_views_images_is_an_error() {
    // SAFETY: null is allowed, and refused.
    assert_ne!(unsafe { fy_view_release_images(std::ptr::null()) }, FyStatus::Ok);
}

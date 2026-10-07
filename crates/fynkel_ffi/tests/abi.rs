//! The C interface, called the way `package:fynkel` calls it. Calling C
//! functions is unsafe, so these tests allow it, like the crate itself.
#![allow(unsafe_code)]

mod common;

use std::ptr;

use common::*;
use fynkel_ffi::*;
use fynkel_golden::assert_golden;

#[test]
fn the_stage_1_scene_made_through_the_c_interface_matches_the_golden() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 256, 256);
    let number = view.frame();
    assert_eq!(number, 1);
    assert_golden("cube", &view.read(number, 256, 256));
}

/// Step 2's "sphere lit from the left" golden, made through the C
/// interface: checks the sphere the worker uploads, and a sun that comes
/// from a transform's rotation.
#[test]
fn a_sphere_lit_from_the_left_matches_its_golden() {
    use fynkel_world::{Transform, Vec3};
    let world = World::new();
    let w = world.0;
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let camera = world.spawn();
        let black = srgb(0.0, 0.0, 0.0);
        ok(fy_camera_set(w, camera, &FyCamera { fov_y: 40.0, near_clip: 0.1, far_clip: 1000.0, clear_color: black }));
        let looking = Transform::from_position(Vec3::new(0.0, 0.0, 2.2)).looking_at(Vec3::ZERO);
        ok(fy_transform_set(w, camera, &transform(looking)));

        let sun = world.spawn();
        let white = srgb(1.0, 1.0, 1.0);
        ok(fy_light_set(w, sun, &FyLight { kind: FY_LIGHT_DIRECTIONAL, color: white, intensity: 1.0 }));
        ok(fy_transform_set(w, sun, &transform(Transform::default().looking_at(Vec3::new(1.0, 0.0, -0.5)))));
        let ambient = world.spawn();
        ok(fy_light_set(w, ambient, &FyLight { kind: FY_LIGHT_AMBIENT, color: white, intensity: 0.05 }));

        let (mut mesh, mut material) = (0, 0);
        ok(fy_mesh_add(w, FY_MESH_SPHERE, &mut mesh));
        // Linear 0.8, as the golden scene has it, written in sRGB.
        let grey = 1.055 * 0.8f32.powf(1.0 / 2.4) - 0.055;
        let matte = FyMaterial { base_color: srgb(grey, grey, grey), metallic: 0.0, roughness: 0.6 };
        ok(fy_material_add(w, &matte, &mut material));
        let ball = world.spawn();
        ok(fy_transform_set(w, ball, &transform(Transform::default())));
        ok(fy_mesh_renderer_set(w, ball, &FyMeshRenderer { mesh, material }));
    }
    let view = View::headless(&world, 128, 128);
    assert_golden("sphere_left", &view.read(view.frame(), 128, 128));
}

#[test]
fn changes_after_a_frame_show_in_the_next_one() {
    let world = World::new();
    let cube = stage1(&world);
    let view = View::headless(&world, 64, 64);
    let before = view.read(view.frame(), 64, 64);
    // SAFETY: a live world.
    ok(unsafe { fy_despawn(world.0, cube) });
    let after = view.read(view.frame(), 64, 64);
    let centre = |image: &fynkel_golden::Image| image.pixel(32, 32);
    assert_ne!(centre(&before), centre(&after), "the cube was in the middle");
    assert_eq!(
        after.rgba.chunks(4).collect::<std::collections::HashSet<_>>().len(),
        1,
        "only the clear colour is left"
    );
}

#[test]
fn a_despawned_id_is_a_clear_error_everywhere() {
    let world = World::new();
    let id = world.spawn();
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_transform_set(
            world.0,
            id,
            &FyTransform { rotation: FyQuat { w: 1.0, ..Default::default() }, ..Default::default() },
        ));
        ok(fy_despawn(world.0, id));
        assert!(!fy_exists(world.0, id));
        let message = format!("object#{id} does not exist (despawned, or never spawned)");

        let mut out = FyTransform::default();
        assert_eq!(fy_transform_get(world.0, id, &mut out), FyStatus::NoObject);
        assert_eq!(last_error(), message);
        assert_eq!(fy_despawn(world.0, id), FyStatus::NoObject);
        assert_eq!(
            fy_camera_set(world.0, id, &FyCamera { fov_y: 60.0, near_clip: 0.1, far_clip: 10.0, ..Default::default() }),
            FyStatus::NoObject
        );
        assert_eq!(last_error(), message);

        let newer = world.spawn();
        assert_ne!(newer, id, "ids are never reused");
        assert_eq!(fy_transform_get(world.0, 0, &mut out), FyStatus::NoObject);
        assert_eq!(last_error(), "0 is not an object id");
    }
}

#[test]
fn a_missing_component_is_its_own_error() {
    let world = World::new();
    let id = world.spawn();
    let mut out = FyCamera::default();
    // SAFETY: a live world and a local out-value.
    assert_eq!(unsafe { fy_camera_get(world.0, id, &mut out) }, FyStatus::NoComponent);
    assert_eq!(last_error(), format!("object#{id} has no Camera"));
}

#[test]
fn values_are_validated_with_a_message_that_names_the_field() {
    let world = World::new();
    let id = world.spawn();
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let camera = FyCamera { fov_y: 0.0, near_clip: 0.1, far_clip: 10.0, clear_color: srgb(0.0, 0.0, 0.0) };
        assert_eq!(fy_camera_set(world.0, id, &camera), FyStatus::InvalidValue);
        assert!(last_error().contains("Camera.fov_y"), "{}", last_error());

        let squashed = FyTransform { rotation: FyQuat { x: 1.0, w: 1.0, ..Default::default() }, ..Default::default() };
        assert_eq!(fy_transform_set(world.0, id, &squashed), FyStatus::InvalidValue);
        assert!(last_error().contains("Transform.rotation"), "{}", last_error());

        let light = FyLight { kind: 7, color: srgb(1.0, 1.0, 1.0), intensity: 1.0 };
        assert_eq!(fy_light_set(world.0, id, &light), FyStatus::InvalidValue);
        assert_eq!(last_error(), "Light.kind 7 doesn't exist (0 = directional, 1 = ambient)");

        let mut mesh = 0;
        assert_eq!(fy_mesh_add(world.0, 3, &mut mesh), FyStatus::InvalidValue);
        assert_eq!(mesh, 0, "nothing written on failure");

        let rough = FyMaterial { base_color: srgb(1.0, 1.0, 1.0), metallic: 0.0, roughness: 2.0 };
        assert_eq!(fy_material_add(world.0, &rough, &mut mesh), FyStatus::InvalidValue);
        assert!(last_error().contains("Material.roughness"), "{}", last_error());

        let mut out = FyCamera::default();
        assert_eq!(fy_camera_get(world.0, id, &mut out), FyStatus::NoComponent, "a refused write changes nothing");
    }
}

#[test]
fn resources_in_use_cannot_be_removed() {
    let world = World::new();
    let cube = stage1(&world);
    let mut renderer = FyMeshRenderer::default();
    // SAFETY: a live world and a local out-value.
    unsafe {
        ok(fy_mesh_renderer_get(world.0, cube, &mut renderer));
        assert_eq!(fy_mesh_remove(world.0, renderer.mesh), FyStatus::InUse);
        assert_eq!(fy_material_remove(world.0, renderer.material), FyStatus::InUse);
        ok(fy_mesh_renderer_remove(world.0, cube));
        ok(fy_mesh_remove(world.0, renderer.mesh));
        ok(fy_material_remove(world.0, renderer.material));
        assert_eq!(fy_mesh_remove(world.0, renderer.mesh), FyStatus::NoMesh);
    }
}

#[test]
fn materials_change_for_everyone_using_them() {
    let world = World::new();
    let cube = stage1(&world);
    let mut renderer = FyMeshRenderer::default();
    let mut material = FyMaterial::default();
    let blue = FyMaterial { base_color: srgb(0.0, 0.0, 1.0), metallic: 1.0, roughness: 0.25 };
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_mesh_renderer_get(world.0, cube, &mut renderer));
        ok(fy_material_set(world.0, renderer.material, &blue));
        ok(fy_material_get(world.0, renderer.material, &mut material));
    }
    assert_eq!(material, blue);
}

#[test]
fn every_component_reads_back_what_was_written() {
    let world = World::new();
    let id = world.spawn();
    let transform = FyTransform {
        position: FyVec3 { x: 1.0, y: 2.0, z: 3.0 },
        rotation: FyQuat { x: 0.0, y: 0.6, z: 0.0, w: 0.8 },
        scale: FyVec3 { x: 2.0, y: 2.0, z: 2.0 },
    };
    let camera = FyCamera { fov_y: 45.0, near_clip: 0.5, far_clip: 50.0, clear_color: srgb(0.2, 0.3, 0.4) };
    let light = FyLight { kind: FY_LIGHT_AMBIENT, color: srgb(0.5, 0.5, 0.5), intensity: 2.0 };
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_transform_set(world.0, id, &transform));
        ok(fy_camera_set(world.0, id, &camera));
        ok(fy_light_set(world.0, id, &light));
        let (mut t, mut c, mut l) = Default::default();
        ok(fy_transform_get(world.0, id, &mut t));
        ok(fy_camera_get(world.0, id, &mut c));
        ok(fy_light_get(world.0, id, &mut l));
        assert_eq!((t, c, l), (transform, camera, light));
        ok(fy_light_remove(world.0, id));
        assert_eq!(fy_light_get(world.0, id, &mut l), FyStatus::NoComponent);
    }
}

#[test]
fn null_pointers_are_errors_not_crashes() {
    let world = World::new();
    let id = world.spawn();
    // SAFETY: null is allowed for every pointer argument.
    unsafe {
        assert_eq!(fy_spawn(ptr::null(), &mut 0), FyStatus::NullPointer);
        assert_eq!(last_error(), "world is null");
        assert_eq!(fy_spawn(world.0, ptr::null_mut()), FyStatus::NullPointer);
        assert_eq!(last_error(), "out is null");
        assert_eq!(fy_transform_set(world.0, id, ptr::null()), FyStatus::NullPointer);
        assert_eq!(fy_view_frame(ptr::null(), &mut 0), FyStatus::NullPointer);
        assert_eq!(fy_view_new_headless(ptr::null(), 8, 8, &mut ptr::null_mut()), FyStatus::NullPointer);
        assert!(!fy_exists(ptr::null(), id));
        fy_world_free(ptr::null_mut());
        fy_view_free(ptr::null_mut());
    }
}

#[test]
fn a_view_outlives_its_world_handle() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 256, 256);
    drop(world);
    assert_golden("cube", &view.read(view.frame(), 256, 256));
}

#[test]
fn reading_checks_the_buffer_and_times_out() {
    let world = World::new();
    let view = View::headless(&world, 4, 4);
    let (mut rgba, mut read) = (vec![0u8; 4 * 4 * 4 - 1], 0);
    // SAFETY: a live view and a buffer of the length passed.
    unsafe {
        assert_eq!(fy_view_read(view.0, 0, 0, rgba.as_mut_ptr(), rgba.len(), &mut read), FyStatus::WrongSize);
        rgba.push(0);
        assert_eq!(fy_view_read(view.0, 1, 10, rgba.as_mut_ptr(), rgba.len(), &mut read), FyStatus::Timeout);
        assert_eq!(last_error(), "frame 1 didn't finish in time (the latest finished is 0)");
    }
}

#[test]
fn a_view_of_zero_size_is_refused() {
    let world = World::new();
    let mut view = ptr::null_mut();
    // SAFETY: a live world and a local out-value.
    assert_eq!(unsafe { fy_view_new_headless(world.0, 0, 8, &mut view) }, FyStatus::InvalidValue);
    assert!(view.is_null());
}

#[test]
fn the_abi_version_is_2() {
    assert_eq!(fy_abi_version(), 2);
}

#[test]
fn a_resized_headless_view_reads_back_frames_of_the_new_size() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 16, 16);
    view.read(view.frame(), 16, 16);
    // SAFETY: a live view.
    ok(unsafe { fy_view_resize(view.0, 24, 8) });
    let image = view.read(view.frame(), 24, 8);
    assert_eq!(image.rgba.len(), 24 * 8 * 4);
    // SAFETY: a live view.
    assert_eq!(unsafe { fy_view_resize(view.0, 0, 8) }, FyStatus::InvalidValue);
    assert_eq!(last_error(), "a view can't be 0 × 8");
}

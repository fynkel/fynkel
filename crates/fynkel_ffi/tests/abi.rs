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
fn the_abi_version_is_4() {
    assert_eq!(fy_abi_version(), 4);
}

#[cfg(not(target_os = "android"))]
#[test]
fn an_android_view_is_refused_off_android() {
    let world = World::new();
    let mut view = ptr::null_mut();
    // SAFETY: a live world and a local out-value.
    assert_eq!(unsafe { fy_view_new_android(world.0, 8, 8, &mut view) }, FyStatus::Unsupported);
    assert!(view.is_null());
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

/// The children through `fy_children_get`, asking again when the first
/// buffer is too small, as `package:fynkel` does.
fn children(world: &World, id: u64) -> Vec<u64> {
    let mut buf = [0u64; 2];
    let mut count = 0;
    // SAFETY: a live world and a buffer of the length passed.
    ok(unsafe { fy_children_get(world.0, id, buf.as_mut_ptr(), buf.len(), &mut count) });
    if count <= buf.len() {
        return buf[..count].to_vec();
    }
    let mut all = vec![0; count];
    // SAFETY: the same, with a bigger buffer.
    ok(unsafe { fy_children_get(world.0, id, all.as_mut_ptr(), all.len(), &mut count) });
    all
}

#[test]
fn parents_and_children_through_the_c_interface() {
    let world = World::new();
    let [root, a, b, c] = std::array::from_fn(|_| world.spawn());
    // SAFETY: a live world and pointers to locals.
    unsafe {
        for child in [a, b, c] {
            ok(fy_parent_set(world.0, child, root));
        }
        let mut parent = 0;
        ok(fy_parent_get(world.0, b, &mut parent));
        assert_eq!(parent, root);
        ok(fy_parent_get(world.0, root, &mut parent));
        assert_eq!(parent, 0, "a root's parent is 0");
        assert_eq!(children(&world, root), [a, b, c], "three don't fit the first buffer of two");
        assert_eq!(children(&world, a), []);

        let mut count = 99;
        ok(fy_children_get(world.0, root, ptr::null_mut(), 0, &mut count));
        assert_eq!(count, 3, "a null buffer of 0 asks only how many");

        ok(fy_parent_set(world.0, c, a));
        let mut part = false;
        ok(fy_is_part_of(world.0, c, root, &mut part));
        assert!(part);
        ok(fy_is_part_of(world.0, root, c, &mut part));
        assert!(!part);

        assert_eq!(fy_parent_set(world.0, root, c), FyStatus::InvalidValue);
        assert_eq!(
            last_error(),
            format!("object#{c} can't be the parent of object#{root}: it is object#{root} or below it")
        );
        ok(fy_parent_set(world.0, a, 0));
        assert_eq!(children(&world, root), [b]);

        ok(fy_despawn(world.0, a));
        assert!(!fy_exists(world.0, c), "despawning takes the children along");
        assert!(fy_exists(world.0, root));
    }
}

/// Reads a name through `fy_name_get`.
fn name(world: &World, id: u64) -> String {
    let mut buf = [0u8; 64];
    let mut len = 0;
    // SAFETY: a live world and a buffer of the length passed.
    ok(unsafe { fy_name_get(world.0, id, buf.as_mut_ptr(), buf.len(), &mut len) });
    String::from_utf8(buf[..len].to_vec()).unwrap()
}

#[test]
fn names_and_find_through_the_c_interface() {
    let world = World::new();
    let [root, arm, hand] = std::array::from_fn(|_| world.spawn());
    let set = |id, name: &str| ok(unsafe { fy_name_set(world.0, id, name.as_ptr(), name.len()) });
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_parent_set(world.0, arm, root));
        ok(fy_parent_set(world.0, hand, arm));
        set(arm, "Arm");
        set(hand, "Hand · Größe");
        assert_eq!(name(&world, hand), "Hand · Größe");
        assert_eq!(name(&world, root), "");

        let mut len = 0;
        ok(fy_name_get(world.0, hand, ptr::null_mut(), 0, &mut len));
        assert_eq!(len, "Hand · Größe".len(), "the length in bytes, without copying");

        let mut found = 0;
        let wanted = "Hand · Größe";
        ok(fy_find(world.0, root, wanted.as_ptr(), wanted.len(), &mut found));
        assert_eq!(found, hand);
        ok(fy_find(world.0, root, "Leg".as_ptr(), 3, &mut found));
        assert_eq!(found, 0);

        let broken = [b'A', 0xff];
        assert_eq!(fy_name_set(world.0, arm, broken.as_ptr(), broken.len()), FyStatus::InvalidValue);
        assert!(last_error().starts_with("name is not UTF-8"), "{}", last_error());
        assert_eq!(name(&world, arm), "Arm", "a refused name changes nothing");

        ok(fy_name_set(world.0, arm, ptr::null(), 0));
        assert_eq!(name(&world, arm), "", "length 0 clears the name");
    }
}

#[test]
fn the_world_transform_composes_the_parents() {
    use fynkel_world::{Quat, Transform, Vec3};
    let world = World::new();
    let [parent, child] = std::array::from_fn(|_| world.spawn());
    let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let mut out = FyTransform::default();
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let p = Transform { position: Vec3::new(1.0, 0.0, 0.0), rotation: turn, scale: Vec3::splat(2.0) };
        ok(fy_transform_set(world.0, parent, &transform(p)));
        ok(fy_transform_set(world.0, child, &transform(Transform::from_position(Vec3::new(0.0, 0.0, -1.0)))));
        ok(fy_parent_set(world.0, child, parent));
        ok(fy_world_transform_get(world.0, child, &mut out));
    }
    // One unit ahead (-Z), doubled, turned a quarter left: two along -X.
    let position = Vec3::from(out.position);
    assert!(position.abs_diff_eq(Vec3::new(-1.0, 0.0, 0.0), 1e-5), "{position}");
    assert!(Quat::from(out.rotation).abs_diff_eq(turn, 1e-5));
    assert!(Vec3::from(out.scale).abs_diff_eq(Vec3::splat(2.0), 1e-5));
}

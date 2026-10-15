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
        ok(fy_camera_set(
            w,
            camera,
            &FyCamera {
                fov_y: 40.0,
                near_clip: 0.1,
                far_clip: 1000.0,
                clear_color: black,
                orthographic: false,
                size: 5.0,
                culling_mask: u32::MAX,
                show_sky: true,
                ..Default::default()
            },
        ));
        let looking = Transform::from_position(Vec3::new(0.0, 0.0, 2.2)).looking_at(Vec3::ZERO);
        ok(fy_transform_set(w, camera, &transform(looking)));

        let sun = world.spawn();
        let white = srgb(1.0, 1.0, 1.0);
        ok(fy_light_set(
            w,
            sun,
            &FyLight { kind: FY_LIGHT_DIRECTIONAL, color: white, intensity: 1.0, ..Default::default() },
        ));
        ok(fy_transform_set(w, sun, &transform(Transform::default().looking_at(Vec3::new(1.0, 0.0, -0.5)))));
        let ambient = world.spawn();
        ok(fy_light_set(
            w,
            ambient,
            &FyLight { kind: FY_LIGHT_AMBIENT, color: white, intensity: 0.05, ..Default::default() },
        ));

        let (mesh, mut material) = (add_shape(&world, FY_MESH_SPHERE), 0);
        // Linear 0.8, as the golden scene has it, written in sRGB.
        let grey = 1.055 * 0.8f32.powf(1.0 / 2.4) - 0.055;
        let matte =
            FyMaterial { base_color: srgb(grey, grey, grey), metallic: 0.0, roughness: 0.6, ..FyMaterial::default() };
        ok(fy_material_add(w, &matte, &mut material));
        let ball = world.spawn();
        ok(fy_transform_set(w, ball, &transform(Transform::default())));
        ok(fy_mesh_renderer_set(w, ball, &FyMeshRenderer { mesh, material, ..Default::default() }));
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
        let message = format!("object#{id} does not exist (destroyed, or never added)");

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
        let camera = FyCamera {
            fov_y: 0.0,
            near_clip: 0.1,
            far_clip: 10.0,
            clear_color: srgb(0.0, 0.0, 0.0),
            orthographic: false,
            size: 5.0,
            culling_mask: u32::MAX,
            show_sky: true,
            ..Default::default()
        };
        assert_eq!(fy_camera_set(world.0, id, &camera), FyStatus::InvalidValue);
        assert!(last_error().contains("Camera.fov_y"), "{}", last_error());
        let flat = FyCamera { orthographic: true, size: 0.0, ..camera };
        assert_eq!(fy_camera_set(world.0, id, &FyCamera { fov_y: 60.0, ..flat }), FyStatus::InvalidValue);
        assert!(last_error().contains("Camera.size"), "{}", last_error());

        let squashed = FyTransform { rotation: FyQuat { x: 1.0, w: 1.0, ..Default::default() }, ..Default::default() };
        assert_eq!(fy_transform_set(world.0, id, &squashed), FyStatus::InvalidValue);
        assert!(last_error().contains("Transform.rotation"), "{}", last_error());

        let light = FyLight { kind: 7, color: srgb(1.0, 1.0, 1.0), intensity: 1.0, ..Default::default() };
        assert_eq!(fy_light_set(world.0, id, &light), FyStatus::InvalidValue);
        assert_eq!(last_error(), "Light.kind 7 doesn't exist (0 = directional, 1 = ambient, 2 = point, 3 = spot)");

        let mut shape = ptr::null_mut();
        assert_eq!(fy_mesh_new_shape(6, &mut shape), FyStatus::InvalidValue);
        assert!(shape.is_null(), "nothing written on failure");
        let mut mesh = 0;

        let rough =
            FyMaterial { base_color: srgb(1.0, 1.0, 1.0), metallic: 0.0, roughness: 2.0, ..FyMaterial::default() };
        assert_eq!(fy_material_add(world.0, &rough, &mut mesh), FyStatus::InvalidValue);
        assert!(last_error().contains("Material.roughness"), "{}", last_error());
        let unknown = FyMaterial { blend: 3, ..FyMaterial::default() };
        assert_eq!(fy_material_add(world.0, &unknown, &mut mesh), FyStatus::InvalidValue);
        assert!(last_error().contains("3 isn't a kind of blend"), "{}", last_error());

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
    let blue = FyMaterial { base_color: srgb(0.0, 0.0, 1.0), metallic: 1.0, roughness: 0.25, ..FyMaterial::default() };
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
    let camera = FyCamera {
        fov_y: 45.0,
        near_clip: 0.5,
        far_clip: 50.0,
        clear_color: srgb(0.2, 0.3, 0.4),
        orthographic: true,
        size: 3.0,
        culling_mask: u32::MAX,
        show_sky: true,
        ..Default::default()
    };
    let light = FyLight { kind: FY_LIGHT_AMBIENT, color: srgb(0.5, 0.5, 0.5), intensity: 2.0, ..Default::default() };
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
fn the_abi_version_is_the_constant() {
    assert_eq!(fy_abi_version(), FY_ABI_VERSION);
}

#[test]
fn a_transform_is_laid_out_as_the_physics_one() {
    use fynkel_cabi::transform;
    use std::mem::{offset_of, size_of};
    assert_eq!(size_of::<FyTransform>(), transform::SIZE);
    assert_eq!(offset_of!(FyTransform, position), transform::POSITION);
    assert_eq!(offset_of!(FyTransform, rotation), transform::ROTATION);
    assert_eq!(offset_of!(FyTransform, scale), transform::SCALE);
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
        assert!(children(&world, a).is_empty());

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

/// An inactive rig spawned before the stage 1 scene, so its camera and
/// sun would win on their lower ids, and its own cube right in front of
/// the camera. Returns the rig.
fn hidden_rig(world: &World) -> u64 {
    use fynkel_world::{Transform, Vec3};
    let w = world.0;
    let [rig, camera, sun, ambient, cube] = std::array::from_fn(|_| world.spawn());
    // SAFETY: a live world, and pointers to locals.
    unsafe {
        for part in [camera, sun, ambient, cube] {
            ok(fy_parent_set(w, part, rig));
        }
        let wide = FyCamera {
            fov_y: 100.0,
            near_clip: 0.1,
            far_clip: 10.0,
            clear_color: srgb(1.0, 0.0, 0.0),
            orthographic: false,
            size: 5.0,
            culling_mask: u32::MAX,
            show_sky: true,
            ..Default::default()
        };
        ok(fy_camera_set(w, camera, &wide));
        ok(fy_transform_set(w, camera, &transform(Transform::from_position(Vec3::new(0.0, 0.0, 6.0)))));
        let bright =
            FyLight { kind: FY_LIGHT_DIRECTIONAL, color: srgb(0.0, 1.0, 0.0), intensity: 5.0, ..Default::default() };
        ok(fy_light_set(w, sun, &bright));
        ok(fy_transform_set(w, sun, &transform(Transform::default())));
        let flood =
            FyLight { kind: FY_LIGHT_AMBIENT, color: srgb(0.0, 0.0, 1.0), intensity: 5.0, ..Default::default() };
        ok(fy_light_set(w, ambient, &flood));
        let (mesh, mut material) = (add_shape(world, FY_MESH_CUBE), 0);
        ok(fy_material_add(w, &FyMaterial::default(), &mut material));
        ok(fy_transform_set(w, cube, &transform(Transform::from_position(Vec3::new(0.0, 1.5, 3.0)))));
        ok(fy_mesh_renderer_set(w, cube, &FyMeshRenderer { mesh, material, ..Default::default() }));
        ok(fy_active_set(w, rig, false));
    }
    rig
}

#[test]
fn an_inactive_object_and_everything_below_it_are_missing_from_the_golden() {
    let world = World::new();
    let rig = hidden_rig(&world);
    stage1(&world);
    let view = View::headless(&world, 256, 256);
    assert_golden("cube", &view.read(view.frame(), 256, 256));
    let (mut camera, rig_camera) = (0, children(&world, rig)[0]);
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_camera_active(world.0, &mut camera));
        assert_eq!(fy_camera_active(world.0, ptr::null_mut()), FyStatus::NullPointer);
    }
    assert!(camera > rig_camera, "the stage 1 camera, the one drawn from, not the hidden rig's");

    // Shown, the rig's camera wins and clears to red.
    // SAFETY: a live world.
    ok(unsafe { fy_active_set(world.0, rig, true) });
    let [r, g, b, _] = view.read(view.frame(), 256, 256).pixel(0, 0);
    assert!(r > 200 && g < 30 && b < 30, "{:?}", [r, g, b]);
    // SAFETY: a live world and a pointer to a local.
    ok(unsafe { fy_camera_active(world.0, &mut camera) });
    assert_eq!(camera, rig_camera);
}

#[test]
fn the_active_flag_through_the_c_interface() {
    let world = World::new();
    let [root, child] = std::array::from_fn(|_| world.spawn());
    let flags = |id| {
        let (mut own, mut shown) = (false, false);
        // SAFETY: a live world and pointers to locals.
        unsafe {
            ok(fy_active_get(world.0, id, &mut own));
            ok(fy_active_in_hierarchy_get(world.0, id, &mut shown));
        }
        (own, shown)
    };
    // SAFETY: a live world and pointers to locals; null is allowed for
    // every pointer argument.
    unsafe {
        ok(fy_parent_set(world.0, child, root));
        assert_eq!(flags(child), (true, true), "on by default");
        ok(fy_active_set(world.0, root, false));
        assert_eq!(flags(root), (false, false));
        assert_eq!(flags(child), (true, false), "hidden by its parent, its own flag kept");

        assert_eq!(fy_active_get(world.0, root, ptr::null_mut()), FyStatus::NullPointer);
        assert_eq!(fy_active_in_hierarchy_get(world.0, root, ptr::null_mut()), FyStatus::NullPointer);
        assert_eq!(fy_active_set(ptr::null(), root, true), FyStatus::NullPointer);
        ok(fy_despawn(world.0, root));
        assert_eq!(fy_active_set(world.0, child, true), FyStatus::NoObject);
        assert_eq!(fy_active_get(world.0, child, &mut false), FyStatus::NoObject);
        assert_eq!(fy_active_in_hierarchy_get(world.0, child, &mut false), FyStatus::NoObject);
    }
}

#[test]
fn many_transforms_are_set_in_one_call_or_none_are() {
    use fynkel_world::{Quat, Transform, Vec3};
    let world = World::new();
    let ids: Vec<u64> = (0..3).map(|_| world.spawn()).collect();
    let at = |x: f32| transform(Transform::from_position(Vec3::new(x, 0.0, 0.0)));
    let position = |id| {
        let mut out = FyTransform::default();
        // SAFETY: a live world and a pointer to a local.
        ok(unsafe { fy_transform_get(world.0, id, &mut out) });
        out.position.x
    };
    // SAFETY: a live world, and slices of the length passed.
    unsafe {
        ok(fy_transform_set_many(world.0, ids.as_ptr(), [at(1.0), at(2.0), at(3.0)].as_ptr(), 3));
        assert_eq!(ids.iter().map(|&id| position(id)).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);

        let bad_rotation =
            transform(Transform { rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 2.0), ..Transform::default() });
        let refused = [
            ([ids[0], ids[1], ids[2]], [at(9.0), at(9.0), at(f32::NAN)], FyStatus::InvalidValue, "NaN or infinity"),
            ([ids[0], ids[1], ids[2]], [at(9.0), bad_rotation, at(9.0)], FyStatus::InvalidValue, "unit quaternion"),
            ([ids[0], 0, ids[2]], [at(9.0), at(9.0), at(9.0)], FyStatus::NoObject, "0 is not an object id"),
            ([ids[0], ids[1], 999], [at(9.0), at(9.0), at(9.0)], FyStatus::NoObject, "object#999 does not exist"),
        ];
        for (which, values, status, message) in refused {
            assert_eq!(fy_transform_set_many(world.0, which.as_ptr(), values.as_ptr(), 3), status, "{message}");
            assert!(last_error().contains(message), "{}", last_error());
            assert_eq!(ids.iter().map(|&id| position(id)).collect::<Vec<_>>(), [1.0, 2.0, 3.0], "none written");
        }

        ok(fy_transform_set_many(world.0, ptr::null(), ptr::null(), 0));
        assert_eq!(fy_transform_set_many(world.0, ptr::null(), [at(0.0)].as_ptr(), 1), FyStatus::NullPointer);
        assert_eq!(fy_transform_set_many(world.0, ids.as_ptr(), ptr::null(), 1), FyStatus::NullPointer);
        assert_eq!(fy_transform_set_many(ptr::null(), ids.as_ptr(), [at(0.0)].as_ptr(), 1), FyStatus::NullPointer);
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

#[test]
fn many_world_transforms_are_read_in_one_call_as_one_by_one() {
    use fynkel_world::{Quat, Transform, Vec3};
    let world = World::new();
    let [parent, child, other] = std::array::from_fn(|_| world.spawn());
    let turn = Quat::from_rotation_y(0.7);
    let mut one = FyTransform::default();
    let mut many = [FyTransform::default(); 3];
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let p = Transform { position: Vec3::new(1.0, 2.0, 3.0), rotation: turn, scale: Vec3::new(1.0, 2.0, 3.0) };
        ok(fy_transform_set(world.0, parent, &transform(p)));
        ok(fy_transform_set(world.0, child, &transform(Transform::from_position(Vec3::new(0.0, 1.0, -1.0)))));
        ok(fy_parent_set(world.0, child, parent));
        ok(fy_world_transform_get_many(world.0, [parent, child, other].as_ptr(), 3, many.as_mut_ptr()));
        for (i, id) in [parent, child, other].into_iter().enumerate() {
            ok(fy_world_transform_get(world.0, id, &mut one));
            assert_eq!(many[i], one, "object {i}");
        }
        // Nothing to read: no pointers needed.
        ok(fy_world_transform_get_many(world.0, ptr::null(), 0, ptr::null_mut()));
        assert_eq!(fy_world_transform_get_many(world.0, ptr::null(), 1, many.as_mut_ptr()), FyStatus::NullPointer);
        assert_eq!(
            fy_world_transform_get_many(world.0, [parent, 0].as_ptr(), 2, many.as_mut_ptr()),
            FyStatus::NoObject
        );
    }
}

/// `mesh` as `fy_mesh_new` takes it, positions and indices only.
fn custom(positions: &[[f32; 3]], indices: &[u32]) -> FyMeshData {
    FyMeshData {
        positions: positions.as_ptr().cast(),
        normals: ptr::null(),
        tangents: ptr::null(),
        uvs: ptr::null(),
        vertex_count: positions.len(),
        indices: indices.as_ptr(),
        index_count: indices.len(),
    }
}

/// A mesh handle that frees itself.
struct Mesh(*mut FyMesh);

impl Mesh {
    fn new(data: &FyMeshData) -> Result<Self, FyStatus> {
        let mut mesh = ptr::null_mut();
        // SAFETY: a pointer to a local, whose arrays outlive the call.
        match unsafe { fy_mesh_new(data, &mut mesh) } {
            FyStatus::Ok => Ok(Self(mesh)),
            status => {
                assert!(mesh.is_null(), "nothing written on failure");
                Err(status)
            }
        }
    }

    fn counts(&self) -> (usize, usize) {
        let (mut vertices, mut indices) = (0, 0);
        // SAFETY: a live mesh and pointers to locals.
        ok(unsafe { fy_mesh_counts(self.0, &mut vertices, &mut indices) });
        (vertices, indices)
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        // SAFETY: made by fy_mesh_new, not used again.
        unsafe { fy_mesh_free(self.0) };
    }
}

/// A mesh's positions and indices read back give the triangles it was
/// made of; wrong counts are refused.
#[test]
fn a_mesh_reads_back_its_positions_and_indices() {
    let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let indices = [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
    let mesh = Mesh::new(&custom(&positions, &indices)).unwrap();
    let (vertex_count, index_count) = mesh.counts();
    let (mut read, mut read_indices) = (vec![0.0f32; vertex_count * 3], vec![0u32; index_count]);
    // SAFETY: a live mesh and buffers of the given sizes.
    unsafe {
        ok(fy_mesh_read(mesh.0, read.as_mut_ptr(), vertex_count, read_indices.as_mut_ptr(), index_count));
        assert_eq!(fy_mesh_read(mesh.0, read.as_mut_ptr(), vertex_count + 1, ptr::null_mut(), 0), FyStatus::WrongSize);
        assert_eq!(fy_mesh_read(mesh.0, ptr::null_mut(), 0, read_indices.as_mut_ptr(), 1), FyStatus::WrongSize);
        ok(fy_mesh_read(mesh.0, ptr::null_mut(), 0, ptr::null_mut(), 0));
    }
    let at = |i: u32| [read[i as usize * 3], read[i as usize * 3 + 1], read[i as usize * 3 + 2]];
    for (given, read) in indices.iter().zip(&read_indices) {
        assert_eq!(positions[*given as usize], at(*read));
    }
}

/// Two meshes made from code with only positions and indices, so the
/// engine computes their normals: a pyramid with a vertex per face corner
/// (flat faces) and an octahedron whose six vertices are shared (smooth).
/// The handles are freed before drawing; the world keeps its share.
#[test]
fn custom_meshes_render_with_computed_normals() {
    use fynkel_world::{Transform, Vec3};
    let world = World::new();
    let cube = stage1(&world);
    let (w, mut renderer) = (world.0, FyMeshRenderer::default());
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_mesh_renderer_get(w, cube, &mut renderer));
        ok(fy_despawn(w, cube));
    }

    let apex = [0.0, 0.6, 0.0];
    let base = [[-0.5, -0.4, 0.5], [0.5, -0.4, 0.5], [0.5, -0.4, -0.5], [-0.5, -0.4, -0.5]];
    let mut pyramid: Vec<[f32; 3]> = vec![];
    for i in 0..4 {
        pyramid.extend([base[i], base[(i + 1) % 4], apex]);
    }
    pyramid.extend([base[0], base[3], base[2], base[0], base[2], base[1]]);
    let pyramid_indices: Vec<u32> = (0..pyramid.len() as u32).collect();

    let octahedron =
        [[0.6, 0.0, 0.0], [-0.6, 0.0, 0.0], [0.0, 0.6, 0.0], [0.0, -0.6, 0.0], [0.0, 0.0, 0.6], [0.0, 0.0, -0.6]];
    let mut octahedron_indices = vec![];
    for (x, y, z) in [(0, 2, 4), (4, 2, 1), (1, 2, 5), (5, 2, 0), (4, 3, 0), (1, 3, 4), (5, 3, 1), (0, 3, 5)] {
        octahedron_indices.extend([x, y, z]);
    }

    for (positions, indices, x) in
        [(&pyramid[..], &pyramid_indices[..], -0.8), (&octahedron[..], &octahedron_indices[..], 0.8)]
    {
        let handle = Mesh::new(&custom(positions, indices)).unwrap();
        assert_eq!(handle.counts(), (positions.len(), indices.len()));
        let mut mesh = 0;
        // SAFETY: a live world and mesh, and pointers to locals.
        unsafe {
            ok(fy_mesh_add(w, handle.0, &mut mesh));
            let id = world.spawn();
            let placed = Transform::from_position(Vec3::new(x, 0.0, 0.0));
            ok(fy_transform_set(w, id, &transform(placed.looking_at(Vec3::new(x + 1.0, 0.0, 1.0)))));
            ok(fy_mesh_renderer_set(
                w,
                id,
                &FyMeshRenderer { mesh, material: renderer.material, ..Default::default() },
            ));
        }
    }
    let view = View::headless(&world, 256, 160);
    assert_golden("custom_meshes", &view.read(view.frame(), 256, 160));
}

#[test]
fn a_broken_mesh_is_refused_with_a_clear_message() {
    let triangle = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    assert_eq!(Mesh::new(&custom(&triangle, &[0, 1, 9])).err(), Some(FyStatus::InvalidValue));
    assert_eq!(last_error(), "index 2 is 9, but there are only 3 vertices");
    assert_eq!(Mesh::new(&custom(&triangle, &[0, 1])).err(), Some(FyStatus::InvalidValue));
    let no_positions = FyMeshData { positions: ptr::null(), ..custom(&triangle, &[0, 1, 2]) };
    assert_eq!(Mesh::new(&no_positions).err(), Some(FyStatus::NullPointer));
    assert_eq!(last_error(), "positions is null");

    let world = World::new();
    let mut id = 0;
    // SAFETY: a live world; null for the mesh.
    assert_eq!(unsafe { fy_mesh_add(world.0, ptr::null(), &mut id) }, FyStatus::NullPointer);
    assert_eq!(last_error(), "mesh is null");
}

/// A texture mirrored down the middle: the two vertices on the mirror line
/// need opposite tangents, so they're split, and the counts say so.
#[test]
fn generated_tangents_can_add_vertices() {
    let positions =
        [[-1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let uvs = [[0.0f32, 0.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = [3, 4, 1, 3, 1, 0, 4, 5, 2, 4, 2, 1];
    let mesh = Mesh::new(&FyMeshData { uvs: uvs.as_ptr().cast(), ..custom(&positions, &indices) }).unwrap();
    assert_eq!(mesh.counts(), (8, 12));
}

// --- Textures (stage 2 plan §3.2) ---------------------------------------------

/// A 128 × 128 test card (tools/make_test_images.py), compiled in so the
/// tests also run on devices.
const GRID_PNG: &[u8] = include_bytes!("../../../tests/assets/grid.png");
const GRID_JPG: &[u8] = include_bytes!("../../../tests/assets/grid.jpg");

struct Texture(*mut FyTexture);

impl Texture {
    fn decode(bytes: &[u8]) -> Result<Self, FyStatus> {
        let mut texture = ptr::null_mut();
        // SAFETY: a slice and a pointer to a local.
        match unsafe { fy_texture_decode(bytes.as_ptr(), bytes.len(), &mut texture) } {
            FyStatus::Ok => Ok(Self(texture)),
            status => {
                assert!(texture.is_null(), "nothing written on failure");
                Err(status)
            }
        }
    }

    fn rgba(width: u32, height: u32, pixels: &[u8]) -> Result<Self, FyStatus> {
        let mut texture = ptr::null_mut();
        // SAFETY: a slice and a pointer to a local.
        match unsafe { fy_texture_new_rgba(width, height, pixels.as_ptr(), pixels.len(), &mut texture) } {
            FyStatus::Ok => Ok(Self(texture)),
            status => Err(status),
        }
    }

    fn size(&self) -> (u32, u32) {
        let (mut width, mut height) = (0, 0);
        // SAFETY: a live texture and pointers to locals.
        ok(unsafe { fy_texture_size(self.0, &mut width, &mut height) });
        (width, height)
    }

    fn add(&self, world: &World) -> u64 {
        let mut id = 0;
        // SAFETY: a live world and texture, and a pointer to a local.
        ok(unsafe { fy_texture_add(world.0, self.0, &mut id) });
        id
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        // SAFETY: made by fy_texture_decode or fy_texture_new_rgba, not used
        // again.
        unsafe { fy_texture_free(self.0) };
    }
}

#[test]
fn png_and_jpeg_files_become_textures() {
    assert_eq!(Texture::decode(GRID_PNG).unwrap().size(), (128, 128));
    assert_eq!(Texture::decode(GRID_JPG).unwrap().size(), (128, 128));
    assert_eq!(Texture::rgba(2, 1, &[0; 8]).unwrap().size(), (2, 1));
}

#[test]
fn an_image_that_cant_be_used_is_refused_with_a_clear_message() {
    assert_eq!(Texture::decode(b"not an image").err(), Some(FyStatus::InvalidValue));
    assert_eq!(last_error(), "the image isn't a PNG or JPEG");
    assert_eq!(Texture::decode(&GRID_PNG[..100]).err(), Some(FyStatus::InvalidValue));
    assert!(last_error().starts_with("the PNG couldn't be decoded"), "{}", last_error());
    assert_eq!(Texture::decode(&[]).err(), Some(FyStatus::InvalidValue));
    assert_eq!(last_error(), "the image is empty (0 bytes)");
    assert_eq!(Texture::rgba(2, 2, &[0; 15]).err(), Some(FyStatus::InvalidValue));
    assert_eq!(last_error(), "a 2 × 2 image has 16 bytes of RGBA, but there are 15");

    let mut texture = ptr::null_mut();
    // SAFETY: null bytes with a length, and a pointer to a local.
    assert_eq!(unsafe { fy_texture_decode(ptr::null(), 4, &mut texture) }, FyStatus::NullPointer);
    assert_eq!(last_error(), "bytes is null");
    // SAFETY: a slice, and null for the out.
    assert_eq!(unsafe { fy_texture_decode(GRID_PNG.as_ptr(), GRID_PNG.len(), ptr::null_mut()) }, FyStatus::NullPointer);
}

#[test]
fn a_texture_cant_be_removed_while_a_material_uses_it() {
    let world = World::new();
    let id = Texture::decode(GRID_PNG).unwrap().add(&world);
    let (w, mut material) = (world.0, 0);
    let ghost = FyMaterial { normal_texture: 99, ..FyMaterial::default() };
    // SAFETY: a live world and pointers to locals.
    unsafe {
        assert_eq!(fy_material_add(w, &ghost, &mut material), FyStatus::NoTexture);
        assert_eq!(last_error(), "texture#99 does not exist");
        ok(fy_material_add(w, &FyMaterial { base_color_texture: id, ..FyMaterial::default() }, &mut material));
        assert_eq!(fy_texture_remove(w, id), FyStatus::InUse);
        assert_eq!(last_error(), format!("texture#{id} is still used by material#{material}"));
        let mut read = FyMaterial::default();
        ok(fy_material_get(w, material, &mut read));
        assert_eq!(read.base_color_texture, id);

        ok(fy_material_set(w, material, &FyMaterial::default()));
        ok(fy_texture_remove(w, id));
        assert_eq!(fy_texture_remove(w, id), FyStatus::NoTexture);
        assert_eq!(fy_texture_remove(w, 0), FyStatus::NoTexture);
        assert_eq!(last_error(), "0 is not a texture id");
    }
}

/// The test card on the stage 1 cube and on a ground that repeats it
/// four times. `packages/fynkel`'s tests draw the same scene from Dart and
/// compare with the same golden.
#[test]
fn a_textured_scene_made_through_the_c_interface_matches_its_golden() {
    let world = World::new();
    let cube = stage1(&world);
    let w = world.0;
    let grid = Texture::decode(GRID_PNG).unwrap();
    let texture = grid.add(&world);
    drop(grid); // the world keeps its share
    let (mut card, mut ground, plane) = (0, 0, add_shape(&world, FY_MESH_PLANE));
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_material_add(w, &FyMaterial { base_color_texture: texture, ..FyMaterial::default() }, &mut card));
        let tiled = FyMaterial {
            base_color_texture: texture,
            uv_scale: FyVec2 { x: 4.0, y: 4.0 },
            roughness: 0.9,
            ..FyMaterial::default()
        };
        ok(fy_material_add(w, &tiled, &mut ground));
        let mut renderer = FyMeshRenderer::default();
        ok(fy_mesh_renderer_get(w, cube, &mut renderer));
        ok(fy_mesh_renderer_set(w, cube, &FyMeshRenderer { material: card, ..renderer }));
        let floor = world.spawn();
        let placed = fynkel_world::Transform {
            position: fynkel_world::Vec3::new(0.0, -0.5, 0.0),
            scale: fynkel_world::Vec3::new(8.0, 1.0, 8.0),
            ..Default::default()
        };
        ok(fy_transform_set(w, floor, &transform(placed)));
        ok(fy_mesh_renderer_set(w, floor, &FyMeshRenderer { mesh: plane, material: ground, ..Default::default() }));
    }
    let view = View::headless(&world, 256, 160);
    assert_golden("textured_scene", &view.read(view.frame(), 256, 160));

    let mut stats = FyViewStats::default();
    // SAFETY: a live view and a pointer to a local.
    ok(unsafe { fy_view_stats(view.0, &mut stats) });
    let mips: u64 = (0..8).map(|i| (128u64 >> i).pow(2) * 4).sum();
    assert_eq!(stats.texture_bytes, mips, "one 128² texture, sRGB only, with its mipmaps");
    assert!(stats.mesh_bytes > 0);
}

// --- Raycasts --------------------------------------------------------------

fn cast(world: &World, origin: [f32; 3], direction: [f32; 3], max_distance: f32) -> Result<FyRayHit, FyStatus> {
    let ray = FyRay {
        origin: FyVec3 { x: origin[0], y: origin[1], z: origin[2] },
        direction: FyVec3 { x: direction[0], y: direction[1], z: direction[2] },
    };
    let mut hit = FyRayHit { object: 99, ..FyRayHit::default() };
    // SAFETY: a live world and pointers to locals.
    let status = unsafe { fy_raycast(world.0, &ray, max_distance, u32::MAX, &mut hit) };
    if status == FyStatus::Ok { Ok(hit) } else { Err(status) }
}

#[test]
fn a_raycast_reports_the_object_the_point_the_normal_and_the_distance() {
    let world = World::new();
    let cube = stage1(&world);
    // The stage 1 cube is turned 30° about Y; straight down it shows its top.
    let hit = cast(&world, [0.1, 3.0, 0.2], [0.0, -2.0, 0.0], f32::INFINITY).unwrap();
    assert_eq!(hit.object, cube);
    let close =
        |v: FyVec3, x: f32, y: f32, z: f32| (v.x - x).abs() < 1e-5 && (v.y - y).abs() < 1e-5 && (v.z - z).abs() < 1e-5;
    assert!(close(hit.point, 0.1, 0.5, 0.2), "{:?}", hit.point);
    assert!(close(hit.normal, 0.0, 1.0, 0.0), "{:?}", hit.normal);
    assert!((hit.distance - 2.5).abs() < 1e-5);

    assert_eq!(cast(&world, [0.1, 3.0, 0.2], [0.0, 1.0, 0.0], f32::INFINITY).unwrap(), FyRayHit::default(), "a miss");
    assert_eq!(cast(&world, [0.1, 3.0, 0.2], [0.0, -1.0, 0.0], 2.4).unwrap().object, 0, "out of reach");
    assert_eq!(cast(&world, [0.1, 3.0, 0.2], [0.0, -1.0, 0.0], 2.6).unwrap().object, cube);
}

#[test]
fn a_raycast_sees_the_world_as_it_is_now_not_the_last_frame() {
    let world = World::new();
    let cube = stage1(&world);
    let view = View::headless(&world, 32, 32);
    view.read(view.frame(), 32, 32);
    let moved = fynkel_world::Transform::from_position(fynkel_world::Vec3::new(10.0, 0.0, 0.0));
    // SAFETY: a live world and a pointer to a local.
    ok(unsafe { fy_transform_set(world.0, cube, &transform(moved)) });
    assert_eq!(cast(&world, [0.0, 3.0, 0.0], [0.0, -1.0, 0.0], f32::INFINITY).unwrap().object, 0);
    assert_eq!(cast(&world, [10.0, 3.0, 0.0], [0.0, -1.0, 0.0], f32::INFINITY).unwrap().object, cube);
}

#[test]
fn a_raycast_refuses_a_ray_it_cant_follow() {
    let world = World::new();
    stage1(&world);
    for (origin, direction, max, what) in [
        ([0.0; 3], [0.0; 3], 1.0, "a direction that isn't zero"),
        ([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], 1.0, "a finite origin"),
        ([0.0; 3], [1.0, 0.0, 0.0], -1.0, "max_distance can't be -1"),
        ([0.0; 3], [1.0, 0.0, 0.0], f32::NAN, "max_distance can't be NaN"),
    ] {
        assert_eq!(cast(&world, origin, direction, max), Err(FyStatus::InvalidValue));
        assert!(last_error().contains(what), "{}", last_error());
    }
    let mut hit = FyRayHit::default();
    // SAFETY: null pointers are refused before anything is read.
    unsafe {
        assert_eq!(fy_raycast(world.0, ptr::null(), 1.0, u32::MAX, &mut hit), FyStatus::NullPointer);
        assert_eq!(fy_raycast(ptr::null(), &FyRay::default(), 1.0, u32::MAX, &mut hit), FyStatus::NullPointer);
        let ray = FyRay { direction: FyVec3 { x: 1.0, y: 0.0, z: 0.0 }, ..FyRay::default() };
        assert_eq!(fy_raycast(world.0, &ray, 1.0, u32::MAX, ptr::null_mut()), FyStatus::NullPointer);
    }
}

// --- Layers and debug lines ----------------------------------------------------

/// The stage 1 scene with a wall on layer 3 between the camera and the
/// cube. Returns the camera, the cube and the wall.
fn stage1_behind_a_wall(world: &World) -> (u64, u64, u64) {
    use fynkel_world::{Transform, Vec3};
    let cube = stage1(world);
    let (wall, mesh, mut material, mut camera) = (world.spawn(), add_shape(world, FY_MESH_CUBE), 0, 0);
    // SAFETY: a live world, and pointers to locals.
    unsafe {
        ok(fy_material_add(
            world.0,
            &FyMaterial { base_color: argb(0xFF3A8EE0), ..FyMaterial::default() },
            &mut material,
        ));
        let wide = Transform { scale: Vec3::new(1.5, 1.5, 0.2), ..Transform::from_position(Vec3::new(0.0, 0.5, 2.0)) };
        ok(fy_transform_set(world.0, wall, &transform(wide)));
        ok(fy_mesh_renderer_set(world.0, wall, &FyMeshRenderer { mesh, material, ..Default::default() }));
        ok(fy_layer_set(world.0, wall, 3));
        ok(fy_camera_active(world.0, &mut camera));
    }
    (camera, cube, wall)
}

fn set_culling_mask(world: &World, camera: u64, mask: u32) {
    let mut lens = FyCamera::default();
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_camera_get(world.0, camera, &mut lens));
        ok(fy_camera_set(world.0, camera, &FyCamera { culling_mask: mask, ..lens }));
    }
}

#[test]
fn a_layer_left_out_of_the_cameras_mask_is_missing_from_the_golden() {
    let world = World::new();
    let (camera, ..) = stage1_behind_a_wall(&world);
    let view = View::headless(&world, 256, 256);
    let [r, g, b, _] = view.read(view.frame(), 256, 256).pixel(128, 128);
    assert!(b > r && b > g, "the wall hides the cube: {:?}", [r, g, b]);

    set_culling_mask(&world, camera, !(1 << 3));
    assert_golden("cube", &view.read(view.frame(), 256, 256));
    set_culling_mask(&world, camera, 1 << 3);
    let image = view.read(view.frame(), 256, 256);
    let [r, g, b, _] = image.pixel(128, 128);
    assert!(b > r && b > g, "only the wall: {:?}", [r, g, b]);
    let [r, g, b, _] = image.pixel(0, 0);
    assert!(r < 40 && g < 40 && b < 40, "and the clear colour: {:?}", [r, g, b]);
}

#[test]
fn a_raycast_goes_through_the_layers_left_out_of_its_mask() {
    let world = World::new();
    let (_, cube, wall) = stage1_behind_a_wall(&world);
    let hit = |mask: u32| {
        let ray = FyRay { origin: FyVec3 { x: 0.1, y: 0.2, z: 5.0 }, direction: FyVec3 { x: 0.0, y: 0.0, z: -1.0 } };
        let mut hit = FyRayHit::default();
        // SAFETY: a live world and pointers to locals.
        ok(unsafe { fy_raycast(world.0, &ray, f32::INFINITY, mask, &mut hit) });
        hit.object
    };
    assert_eq!(hit(u32::MAX), wall);
    assert_eq!(hit(!(1 << 3)), cube);
    assert_eq!(hit(1 << 3), wall);
    assert_eq!(hit(1), cube, "layer 0 only");
    assert_eq!(hit(0), 0, "no layers: nothing");
}

#[test]
fn layers_through_the_c_interface() {
    let world = World::new();
    let [parent, child] = std::array::from_fn(|_| world.spawn());
    let layer = |id| {
        let mut layer = 99;
        // SAFETY: a live world and a pointer to a local.
        ok(unsafe { fy_layer_get(world.0, id, &mut layer) });
        layer
    };
    // SAFETY: a live world and pointers to locals; null is allowed for
    // every pointer argument.
    unsafe {
        ok(fy_parent_set(world.0, child, parent));
        assert_eq!(layer(parent), 0, "0 by default");
        ok(fy_layer_set(world.0, parent, 31));
        assert_eq!((layer(parent), layer(child)), (31, 0), "a child keeps its own");

        assert_eq!(fy_layer_set(world.0, parent, 32), FyStatus::InvalidValue);
        assert!(last_error().contains("layer 32 doesn't exist: layers are 0 to 31"), "{}", last_error());
        assert_eq!(layer(parent), 31, "unchanged");

        assert_eq!(fy_layer_get(world.0, parent, ptr::null_mut()), FyStatus::NullPointer);
        assert_eq!(fy_layer_set(ptr::null(), parent, 1), FyStatus::NullPointer);
        ok(fy_despawn(world.0, child));
        assert_eq!(fy_layer_set(world.0, child, 1), FyStatus::NoObject);
        assert_eq!(fy_layer_get(world.0, child, &mut 0), FyStatus::NoObject);
    }
}

fn line(a: [f32; 3], b: [f32; 3], color: FyColor) -> FyDebugLine {
    let v = |p: [f32; 3]| FyVec3 { x: p[0], y: p[1], z: p[2] };
    FyDebugLine { a: v(a), b: v(b), color }
}

#[test]
fn debug_lines_are_drawn_over_the_scene_until_replaced() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 256, 256);
    let (red, green) = (srgb(1.0, 0.0, 0.0), srgb(0.0, 1.0, 0.0));
    let lines = [
        line([-2.0, 0.0, 0.0], [2.0, 0.0, 0.0], red),
        line([0.0, -2.0, -3.0], [0.0, 2.0, -3.0], green),
        // Left out: a point that isn't finite.
        line([f32::NAN, 0.0, 0.0], [1.0, 1.0, 1.0], red),
    ];
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_debug_lines_set(world.0, lines.as_ptr(), lines.len()));
        // The same lines as fynkel_render's test, over the same scene.
        assert_golden("lines", &view.read(view.frame(), 256, 256));
        assert_golden("lines", &view.read(view.frame(), 256, 256));

        ok(fy_debug_lines_set(world.0, ptr::null(), 0));
        assert_golden("cube", &view.read(view.frame(), 256, 256));

        assert_eq!(fy_debug_lines_set(world.0, ptr::null(), 1), FyStatus::NullPointer);
        assert_eq!(fy_debug_lines_set(ptr::null(), lines.as_ptr(), 1), FyStatus::NullPointer);
    }
}

//! The C interface, called the way `package:fynkel_physics` calls it.
//! Calling C functions is unsafe, so these tests allow it, like the crate.
#![allow(unsafe_code)]

use std::ffi::CStr;
use std::ptr;

use fynkel_physics_ffi::*;

const STEP: f32 = 1.0 / 50.0;

fn at(x: f32, y: f32, z: f32) -> FpPose {
    FpPose { position: FpVec3 { x, y, z }, rotation: FpQuat { w: 1.0, ..FpQuat::default() } }
}

fn last_error() -> String {
    // SAFETY: valid until the next failed call on this thread.
    unsafe { CStr::from_ptr(fp_last_error()) }.to_str().unwrap().to_owned()
}

struct World(*mut FpWorld);

impl World {
    fn new() -> Self {
        Self(fp_world_new())
    }

    fn body(&self, kind: FpBodyKind, key: u64, pose: FpPose) -> u64 {
        let mut id = 0;
        // SAFETY: a live world and a writable out.
        assert_eq!(unsafe { fp_body_add(self.0, kind as u32, key, pose, &mut id) }, FpStatus::Ok, "{}", last_error());
        id
    }

    fn collider(&self, body: u64, shape: FpShape, pose: FpPose) -> u64 {
        let mut id = 0;
        // SAFETY: a live world and a writable out.
        assert_eq!(unsafe { fp_collider_add(self.0, body, shape, pose, &mut id) }, FpStatus::Ok, "{}", last_error());
        id
    }

    fn step(&self) -> Vec<FpMoved> {
        let mut count = 0;
        // SAFETY: a live world and a writable out.
        assert_eq!(unsafe { fp_step(self.0, STEP, &mut count) }, FpStatus::Ok);
        let mut moved = vec![FpMoved::default(); count as usize];
        // SAFETY: room for exactly count.
        assert_eq!(unsafe { fp_moved_read(self.0, moved.as_mut_ptr(), moved.len()) }, FpStatus::Ok);
        moved
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // SAFETY: made by fp_world_new, not used after.
        unsafe { fp_world_free(self.0) };
    }
}

fn sphere(radius: f32) -> FpShape {
    FpShape { kind: FpShapeKind::Sphere as u32, a: radius, b: 0.0, c: 0.0 }
}

fn cube(half: f32) -> FpShape {
    FpShape { kind: FpShapeKind::Box as u32, a: half, b: half, c: half }
}

#[test]
fn a_ball_dropped_through_the_interface_lands_and_its_moves_come_back_with_its_key() {
    let world = World::new();
    world.collider(0, FpShape { kind: FpShapeKind::Box as u32, a: 50.0, b: 0.5, c: 50.0 }, at(0.0, -0.5, 0.0));
    let ball = world.body(FpBodyKind::Dynamic, 1234, at(0.0, 10.5, 0.0));
    world.collider(ball, sphere(0.5), at(0.0, 0.0, 0.0));

    let moved = world.step();
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].key, 1234);
    assert!(moved[0].pose.position.y < 10.5);

    for _ in 0..200 {
        world.step();
    }
    let mut pose = FpPose::default();
    // SAFETY: a live world and a writable out.
    assert_eq!(unsafe { fp_body_pose_get(world.0, ball, &mut pose) }, FpStatus::Ok);
    assert!((pose.position.y - 0.5).abs() < 0.02, "rests at {pose:?}");
}

#[test]
fn a_teleport_moves_a_body_at_once() {
    let world = World::new();
    let body = world.body(FpBodyKind::Dynamic, 1, at(0.0, 0.0, 0.0));
    world.collider(body, cube(0.5), at(0.0, 0.0, 0.0));
    // SAFETY: a live world.
    assert_eq!(unsafe { fp_body_pose_set(world.0, body, at(5.0, 6.0, 7.0)) }, FpStatus::Ok);
    let mut pose = FpPose::default();
    // SAFETY: a live world and a writable out.
    unsafe { fp_body_pose_get(world.0, body, &mut pose) };
    assert_eq!(pose.position, FpVec3 { x: 5.0, y: 6.0, z: 7.0 });
}

#[test]
fn removed_and_unknown_ids_are_refused_with_a_message() {
    let world = World::new();
    let body = world.body(FpBodyKind::Dynamic, 1, at(0.0, 0.0, 0.0));
    let collider = world.collider(body, sphere(1.0), at(0.0, 0.0, 0.0));
    // SAFETY: a live world throughout.
    unsafe {
        assert_eq!(fp_body_remove(world.0, body), FpStatus::Ok);
        assert_eq!(fp_body_remove(world.0, body), FpStatus::NoBody);
        assert_eq!(last_error(), format!("no body {body} (removed, or never added)"));
        assert_eq!(fp_collider_remove(world.0, collider), FpStatus::NoCollider);
        let mut id = 0;
        assert_eq!(fp_collider_add(world.0, body, sphere(1.0), at(0.0, 0.0, 0.0), &mut id), FpStatus::NoBody);
        assert_eq!(id, 0);
    }
}

#[test]
fn bad_values_are_refused_and_nothing_is_added() {
    let world = World::new();
    let mut id = 0;
    // SAFETY: a live world and writable outs throughout.
    unsafe {
        assert_eq!(fp_body_add(world.0, 3, 1, at(0.0, 0.0, 0.0), &mut id), FpStatus::InvalidValue);
        assert_eq!(last_error(), "3 is no body kind");
        let bad = FpShape { kind: 9, a: 1.0, b: 1.0, c: 1.0 };
        assert_eq!(fp_collider_add(world.0, 0, bad, at(0.0, 0.0, 0.0), &mut id), FpStatus::InvalidValue);
        assert_eq!(fp_collider_add(world.0, 0, sphere(-1.0), at(0.0, 0.0, 0.0), &mut id), FpStatus::InvalidValue);
        assert_eq!(last_error(), "a sphere's radius must be finite and more than 0, not -1");
        let mut count = 0;
        assert_eq!(fp_step(world.0, 0.0, &mut count), FpStatus::InvalidValue);
        assert_eq!(fp_gravity_set(world.0, FpVec3 { x: f32::NAN, y: 0.0, z: 0.0 }), FpStatus::InvalidValue);
    }
    assert_eq!(id, 0);
    assert!(world.step().is_empty());
}

#[test]
fn null_pointers_are_refused() {
    let world = World::new();
    let mut id = 0;
    let mut count = 0;
    // SAFETY: nulls are what's under test; the rest is live.
    unsafe {
        assert_eq!(fp_body_add(ptr::null_mut(), 0, 1, at(0.0, 0.0, 0.0), &mut id), FpStatus::NullPointer);
        assert_eq!(last_error(), "world is null");
        assert_eq!(fp_body_add(world.0, 0, 1, at(0.0, 0.0, 0.0), ptr::null_mut()), FpStatus::NullPointer);
        assert_eq!(fp_step(world.0, STEP, ptr::null_mut()), FpStatus::NullPointer);
        assert_eq!(fp_step(ptr::null_mut(), STEP, &mut count), FpStatus::NullPointer);
        fp_world_free(ptr::null_mut());
    }
    // Nothing was added by the call with a null out.
    assert!(world.step().is_empty());
}

#[test]
fn reading_the_moves_takes_exactly_as_many_as_the_step_said() {
    let world = World::new();
    for key in 0..3 {
        let body = world.body(FpBodyKind::Dynamic, key, at(key as f32 * 3.0, 5.0, 0.0));
        world.collider(body, sphere(0.5), at(0.0, 0.0, 0.0));
    }
    let mut count = 0;
    let mut moved = [FpMoved::default(); 4];
    // SAFETY: a live world, and room for 4.
    unsafe {
        assert_eq!(fp_step(world.0, STEP, &mut count), FpStatus::Ok);
        assert_eq!(count, 3);
        assert_eq!(fp_moved_read(world.0, moved.as_mut_ptr(), 4), FpStatus::WrongSize);
        assert_eq!(last_error(), "the last step moved 3 bodies, not 4");
        assert_eq!(fp_moved_read(world.0, moved.as_mut_ptr(), 3), FpStatus::Ok);
    }
    let mut keys: Vec<_> = moved[..3].iter().map(|m| m.key).collect();
    keys.sort_unstable();
    assert_eq!(keys, [0, 1, 2]);
}

#[test]
fn the_abi_version_is_the_constant() {
    assert_eq!(fp_abi_version(), FP_ABI_VERSION);
}

// --- Colliders that follow objects, and queries --------------------------------

const ALL: (u32, u32) = (u32::MAX, 1);

fn object(x: f32, y: f32, z: f32, scale: f32) -> FpTransform {
    FpTransform {
        position: FpVec3 { x, y, z },
        rotation: FpQuat { w: 1.0, ..FpQuat::default() },
        scale: FpVec3 { x: scale, y: scale, z: scale },
    }
}

const FORWARD: FpVec3 = FpVec3 { x: 0.0, y: 0.0, z: 1.0 };

fn forward_from(origin: FpVec3, max_distance: f32) -> FpRay {
    FpRay { origin, direction: FORWARD, max_distance }
}

fn ahead(max_distance: f32) -> FpSweep {
    FpSweep { direction: FORWARD, max_distance }
}

fn ball_spec(radius: f32, layer: u32, is_trigger: u32) -> FpColliderSpec {
    FpColliderSpec {
        kind: FpColliderKind::Sphere as u32,
        size: FpVec3 { x: radius, y: 0.0, z: 0.0 },
        is_trigger,
        layer,
        ..FpColliderSpec::default()
    }
}

impl World {
    fn follower(&self, key: u64, spec: FpColliderSpec, at: FpTransform) -> u64 {
        let mut id = 0;
        // SAFETY: a live world and a writable out.
        let status = unsafe { fp_object_collider_add(self.0, key, spec, at, 0, &mut id) };
        assert_eq!(status, FpStatus::Ok, "{}", last_error());
        id
    }

    fn ray(&self, x: f32, (layers, triggers): (u32, u32)) -> FpHit {
        let mut hit = FpHit::default();
        let ray = forward_from(FpVec3 { x, y: 0.0, z: 0.0 }, 100.0);
        // SAFETY: a live world, a readable ray and a writable out.
        let status = unsafe { fp_raycast(self.0, &ray, layers, triggers, &mut hit) };
        assert_eq!(status, FpStatus::Ok, "{}", last_error());
        hit
    }
}

#[test]
fn a_ray_finds_a_scaled_collider_by_key_and_misses_with_collider_0() {
    let world = World::new();
    let id = world.follower(7, ball_spec(1.0, 0, 0), object(0.0, 0.0, 10.0, 2.0));
    let hit = world.ray(0.0, ALL);
    assert_eq!((hit.collider, hit.key), (id, 7));
    assert!((hit.distance - 8.0).abs() < 1e-4, "{hit:?}");
    assert!((hit.normal.z + 1.0).abs() < 1e-4);
    assert_eq!(world.ray(5.0, ALL), FpHit::default());
    // Not on a layer the ray sees, and not a trigger it sees.
    assert_eq!(world.ray(0.0, (!1, 1)).collider, 0);
    // SAFETY: a live world.
    assert_eq!(unsafe { fp_collider_spec_set(world.0, id, ball_spec(1.0, 0, 1)) }, FpStatus::Ok);
    assert_eq!(world.ray(0.0, (u32::MAX, 0)).collider, 0);
    assert_eq!(world.ray(0.0, ALL).collider, id);
}

#[test]
fn following_moves_colliders_and_the_drawn_shape_is_scaled() {
    let world = World::new();
    let a = world.follower(1, ball_spec(1.0, 0, 0), object(0.0, 0.0, 10.0, 1.0));
    let b = world.follower(2, ball_spec(1.0, 0, 0), object(9.0, 0.0, 10.0, 1.0));
    let ids = [a, b];
    let objects = [object(4.0, 0.0, 10.0, 3.0), object(9.0, 0.0, 10.0, 1.0)];
    // SAFETY: a live world, and two of each.
    assert_eq!(unsafe { fp_colliders_follow(world.0, ids.as_ptr(), objects.as_ptr(), 2) }, FpStatus::Ok);
    assert_eq!(world.ray(0.0, ALL).collider, 0, "a moved away from the ray");
    assert_eq!(world.ray(4.0, ALL).collider, a);

    let (mut shape, mut pose) = (FpShape::default(), FpPose::default());
    // SAFETY: a live world and writable outs.
    assert_eq!(unsafe { fp_collider_shape_get(world.0, a, &mut shape, &mut pose) }, FpStatus::Ok);
    assert_eq!(shape, FpShape { kind: FpShapeKind::Sphere as u32, a: 3.0, b: 0.0, c: 0.0 });
    assert_eq!(pose.position, FpVec3 { x: 4.0, y: 0.0, z: 10.0 });

    // SAFETY: a live world; nulls with no colliders are fine, with some not.
    unsafe {
        assert_eq!(fp_colliders_follow(world.0, ptr::null(), ptr::null(), 0), FpStatus::Ok);
        assert_eq!(fp_colliders_follow(world.0, ptr::null(), objects.as_ptr(), 2), FpStatus::NullPointer);
    }
}

#[test]
fn all_hits_and_overlaps_are_read_in_exactly_as_many_as_were_found() {
    let world = World::new();
    let near = world.follower(1, ball_spec(1.0, 0, 0), object(0.0, 0.0, 5.0, 1.0));
    let far = world.follower(2, ball_spec(1.0, 0, 0), object(0.0, 0.0, 10.0, 1.0));
    let origin = FpVec3::default();
    let mut count = 0;
    let mut hits = [FpHit::default(); 2];
    let probe = FpQueryShape {
        kind: FpQueryShapeKind::Sphere as u32,
        a: FpVec3 { x: 0.0, y: 0.0, z: 7.5 },
        radius: 3.0,
        ..FpQueryShape::default()
    };
    let mut found = [FpFound::default(); 2];
    let mut any = 9;
    // SAFETY: a live world, and room for what's read.
    unsafe {
        assert_eq!(fp_raycast_all(world.0, &forward_from(origin, 100.0), ALL.0, ALL.1, &mut count), FpStatus::Ok);
        assert_eq!(count, 2);
        assert_eq!(fp_hits_read(world.0, hits.as_mut_ptr(), 1), FpStatus::WrongSize);
        assert_eq!(last_error(), "the last query hit 2, not 1");
        assert_eq!(fp_hits_read(world.0, hits.as_mut_ptr(), 2), FpStatus::Ok);
        assert_eq!([hits[0].collider, hits[1].collider], [near, far]);

        let ball = FpQueryShape { radius: 0.5, a: FpVec3::default(), ..probe };
        assert_eq!(fp_cast_all(world.0, &ball, &ahead(100.0), ALL.0, ALL.1, &mut count), FpStatus::Ok);
        assert_eq!(count, 2);
        assert_eq!(fp_hits_read(world.0, hits.as_mut_ptr(), 2), FpStatus::Ok);
        assert!((hits[0].distance - 3.5).abs() < 1e-3, "{:?}", hits[0]);
        let mut first = FpHit::default();
        assert_eq!(fp_cast(world.0, &ball, &ahead(100.0), ALL.0, ALL.1, &mut first), FpStatus::Ok);
        assert_eq!(first.key, 1);

        assert_eq!(fp_overlap(world.0, &probe, ALL.0, ALL.1, &mut count), FpStatus::Ok);
        assert_eq!(count, 2);
        assert_eq!(fp_found_read(world.0, found.as_mut_ptr(), 2), FpStatus::Ok);
        let mut keys = [found[0].key, found[1].key];
        keys.sort_unstable();
        assert_eq!(keys, [1, 2]);
        assert_eq!(fp_check(world.0, &probe, 0, 1, &mut any), FpStatus::Ok);
        assert_eq!(any, 0);
        assert_eq!(fp_check(world.0, &probe, ALL.0, ALL.1, &mut any), FpStatus::Ok);
        assert_eq!(any, 1);

        // A miss reads nothing, with no buffer at all.
        let up = FpRay { direction: FpVec3 { x: 0.0, y: 1.0, z: 0.0 }, max_distance: 100.0, ..FpRay::default() };
        assert_eq!(fp_raycast_all(world.0, &up, ALL.0, ALL.1, &mut count), FpStatus::Ok);
        assert_eq!(count, 0);
        assert_eq!(fp_hits_read(world.0, ptr::null_mut(), 0), FpStatus::Ok);
    }
}

/// A square, 2 × 2, facing -Z, as a mesh through C.
fn square(world: &World) -> u64 {
    let positions: [f32; 12] = [-1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0, 0.0];
    let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];
    let mut mesh = 0;
    // SAFETY: a live world, arrays of the given lengths and a writable out.
    let status = unsafe { fp_mesh_add(world.0, positions.as_ptr(), 4, indices.as_ptr(), 6, &mut mesh) };
    assert_eq!(status, FpStatus::Ok, "{}", last_error());
    mesh
}

fn mesh_spec(mesh: u64, convex: u32) -> FpColliderSpec {
    FpColliderSpec { kind: FpColliderKind::Mesh as u32, mesh, convex, ..FpColliderSpec::default() }
}

#[test]
fn a_mesh_collider_comes_through_c_and_bad_meshes_are_refused() {
    let world = World::new();
    let mesh = square(&world);
    let id = world.follower(3, mesh_spec(mesh, 0), object(0.0, 0.0, 10.0, 2.0));
    let hit = world.ray(1.5, ALL);
    assert_eq!((hit.collider, hit.key, hit.distance), (id, 3, 10.0));
    assert_eq!(world.ray(2.5, ALL).collider, 0, "past its scaled edge");
    let positions = [0.0f32; 9];
    let mut out = 0;
    // SAFETY: a live world, arrays at least as long as said and a writable out.
    unsafe {
        assert_eq!(
            fp_mesh_add(world.0, positions.as_ptr(), 3, [0u32, 1].as_ptr(), 2, &mut out),
            FpStatus::InvalidValue
        );
        assert_eq!(last_error(), "2 indices: three per triangle");
        assert_eq!(
            fp_mesh_add(world.0, positions.as_ptr(), 3, [0u32, 1, 3].as_ptr(), 3, &mut out),
            FpStatus::InvalidValue
        );
        assert_eq!(fp_mesh_add(world.0, ptr::null(), 3, [0u32, 1, 2].as_ptr(), 3, &mut out), FpStatus::NullPointer);
        let mut other = 0;
        for bad in [mesh_spec(mesh, 2), mesh_spec(mesh + 100, 0)] {
            let status = fp_object_collider_add(world.0, 4, bad, object(0.0, 0.0, 0.0, 1.0), 0, &mut other);
            assert_eq!(status, FpStatus::InvalidValue);
        }
        let mut edges = 0;
        assert_eq!(fp_collider_edges(world.0, id, &mut edges), FpStatus::Ok);
        assert_eq!(edges, 5, "the square's four sides and its diagonal");
        let mut points = [FpVec3::default(); 10];
        assert_eq!(fp_edges_read(world.0, points.as_mut_ptr(), 4), FpStatus::WrongSize);
        assert_eq!(fp_edges_read(world.0, points.as_mut_ptr(), 5), FpStatus::Ok);
        // Each end on the square's edge, at z = 10.
        assert!(points.iter().all(|p| p.z == 10.0 && (p.x.abs() == 2.0 || p.y.abs() == 2.0)));
        assert_eq!(fp_mesh_remove(world.0, mesh), FpStatus::InvalidValue);
        assert_eq!(last_error(), format!("mesh {mesh} is used by a collider"));
        assert_eq!(fp_collider_remove(world.0, id), FpStatus::Ok);
        assert_eq!(fp_mesh_remove(world.0, mesh), FpStatus::Ok);
    }
    assert_eq!(out, 0, "nothing written on failure");
}

#[test]
fn a_character_moves_through_c_and_bad_moves_are_refused() {
    let world = World::new();
    // A ball 1 m round at z = 3, and a character, a ball too, at the origin.
    let wall = world.follower(2, ball_spec(1.0, 0, 0), object(0.0, 0.0, 3.0, 1.0));
    let character = world.follower(1, ball_spec(0.5, 0, 0), object(0.0, 0.0, 0.0, 1.0));
    let settings = FpCharacter { slope_limit: 45.0, step_offset: 0.0, skin_width: 0.05, mass: 0.0 };
    let mut moved = FpCharacterMove::default();
    let mut hits = [FpCharacterHit::default(); 2];
    // SAFETY: a live world, readable settings and motions, and writable outs.
    unsafe {
        let status = fp_character_move(world.0, character, &settings, &FORWARD, 1.0 / 60.0, &mut moved);
        assert_eq!(status, FpStatus::Ok, "{}", last_error());
        assert_eq!(moved.translation.z, 1.0, "nothing in the way");
        assert_eq!(moved.hit_count, 0);
        let far = FpVec3 { z: 5.0, ..FpVec3::default() };
        assert_eq!(fp_character_move(world.0, character, &settings, &far, 1.0 / 60.0, &mut moved), FpStatus::Ok);
        // To 3 - 1 - 0.5, less the skin.
        assert!((moved.translation.z - 1.45).abs() < 0.01, "{moved:?}");
        assert_eq!((moved.flags, moved.hit_count), (1, 1), "on its side");
        assert_eq!(fp_character_hits_read(world.0, hits.as_mut_ptr(), 2), FpStatus::WrongSize);
        assert_eq!(fp_character_hits_read(world.0, hits.as_mut_ptr(), 1), FpStatus::Ok);
        assert_eq!((hits[0].collider, hits[0].key), (wall, 2));
        assert!(hits[0].normal.z < -0.99 && (hits[0].point.z - 2.0).abs() < 0.01, "{:?}", hits[0]);
        let bad = FpCharacter { skin_width: 0.0, ..settings };
        assert_eq!(fp_character_move(world.0, character, &bad, &far, 1.0 / 60.0, &mut moved), FpStatus::InvalidValue);
        assert_eq!(fp_character_move(world.0, character, &settings, &far, 0.0, &mut moved), FpStatus::InvalidValue);
        let status = fp_character_move(world.0, character, ptr::null(), &far, 1.0 / 60.0, &mut moved);
        assert_eq!(status, FpStatus::NullPointer);
        assert_eq!(fp_character_move(world.0, 99, &settings, &far, 1.0 / 60.0, &mut moved), FpStatus::NoCollider);
    }
}

#[test]
fn a_colliders_own_point_and_ray_come_through() {
    let world = World::new();
    let id = world.follower(1, ball_spec(1.0, 0, 0), object(0.0, 0.0, 10.0, 1.0));
    let mut point = FpVec3::default();
    let mut hit = FpHit::default();
    // SAFETY: a live world and writable outs.
    unsafe {
        assert_eq!(fp_closest_point(world.0, id, &FpVec3::default(), &mut point), FpStatus::Ok);
        assert!((point.z - 9.0).abs() < 1e-4, "{point:?}");
        assert_eq!(fp_collider_raycast(world.0, id, &forward_from(FpVec3::default(), 100.0), &mut hit), FpStatus::Ok);
        assert_eq!(hit.key, 1);
        assert_eq!(fp_collider_raycast(world.0, id, &forward_from(FpVec3::default(), 5.0), &mut hit), FpStatus::Ok);
        assert_eq!(hit.collider, 0, "too short");
    }
}

#[test]
fn bad_specs_and_queries_are_refused_and_add_nothing() {
    let world = World::new();
    let mut id = 0;
    let mut hit = FpHit::default();
    let at = object(0.0, 0.0, 10.0, 1.0);
    // SAFETY: a live world and writable outs throughout.
    unsafe {
        let capsule = FpColliderSpec {
            kind: FpColliderKind::Capsule as u32,
            size: FpVec3 { x: 0.5, y: 2.0, z: 0.0 },
            direction: 3,
            ..FpColliderSpec::default()
        };
        assert_eq!(fp_object_collider_add(world.0, 1, capsule, at, 0, &mut id), FpStatus::InvalidValue);
        assert_eq!(last_error(), "3 is no direction: 0, 1 or 2 for X, Y or Z");
        let kind = FpColliderSpec { kind: 5, ..ball_spec(1.0, 0, 0) };
        assert_eq!(fp_object_collider_add(world.0, 1, kind, at, 0, &mut id), FpStatus::InvalidValue);
        assert_eq!(fp_object_collider_add(world.0, 1, ball_spec(1.0, 0, 2), at, 0, &mut id), FpStatus::InvalidValue);
        assert_eq!(fp_object_collider_add(world.0, 1, ball_spec(1.0, 32, 0), at, 0, &mut id), FpStatus::InvalidValue);
        assert_eq!(fp_object_collider_add(world.0, 1, ball_spec(-1.0, 0, 0), at, 0, &mut id), FpStatus::InvalidValue);
        assert_eq!(id, 0);

        let zero = FpVec3::default();
        let ray = forward_from(zero, 1.0);
        assert_eq!(fp_raycast(world.0, &ray, 1, 2, &mut hit), FpStatus::InvalidValue);
        assert_eq!(last_error(), "hit_triggers is 0 or 1, not 2");
        assert_eq!(fp_raycast(world.0, &forward_from(zero, -1.0), ALL.0, ALL.1, &mut hit), FpStatus::InvalidValue);
        let shape = FpQueryShape { kind: 7, ..FpQueryShape::default() };
        assert_eq!(fp_cast(world.0, &shape, &ahead(1.0), ALL.0, ALL.1, &mut hit), FpStatus::InvalidValue);
        assert_eq!(fp_raycast(world.0, &ray, ALL.0, ALL.1, ptr::null_mut()), FpStatus::NullPointer);
        assert_eq!(fp_raycast(world.0, ptr::null(), ALL.0, ALL.1, &mut hit), FpStatus::NullPointer);
        assert_eq!(last_error(), "ray is null");
        assert_eq!(fp_check(world.0, ptr::null(), ALL.0, ALL.1, &mut 0), FpStatus::NullPointer);
        assert_eq!(fp_closest_point(world.0, 99, &zero, &mut FpVec3::default()), FpStatus::NoCollider);
    }
    assert_eq!(world.ray(0.0, ALL).collider, 0);
}

#[test]
fn a_step_on_the_worlds_own_thread_moves_as_a_step_here_does_and_the_world_waits_for_it() {
    let make = || {
        let world = World::new();
        world.collider(0, cube(5.0), at(0.0, -5.0, 0.0));
        for i in 0..20 {
            let body = world.body(FpBodyKind::Dynamic, i, at(0.0, 1.0 + i as f32, 0.0));
            world.collider(body, cube(0.4), at(0.0, 0.0, 0.0));
        }
        world
    };
    let here = make();
    let there = make();
    for _ in 0..60 {
        let expected = here.step();
        // SAFETY: a live world.
        assert_eq!(unsafe { fp_step_begin(there.0, STEP) }, FpStatus::Ok);
        let mut count = 0;
        // Nothing else is asked of the world while it steps.
        // SAFETY: a live world and a writable out.
        assert_eq!(unsafe { fp_step(there.0, STEP, &mut count) }, FpStatus::InvalidValue);
        assert!(last_error().contains("fp_step_end"), "{}", last_error());
        // SAFETY: a live world and a writable out.
        assert_eq!(unsafe { fp_step_end(there.0, &mut count) }, FpStatus::Ok, "{}", last_error());
        let mut moved = vec![FpMoved::default(); count as usize];
        // SAFETY: room for exactly count.
        assert_eq!(unsafe { fp_moved_read(there.0, moved.as_mut_ptr(), moved.len()) }, FpStatus::Ok);
        assert_eq!(moved.len(), expected.len());
        for (a, b) in moved.iter().zip(&expected) {
            assert_eq!((a.key, a.pose.position.y.to_bits()), (b.key, b.pose.position.y.to_bits()));
        }
    }
    let mut count = 0;
    // SAFETY: a live world and a writable out.
    assert_eq!(unsafe { fp_step_end(there.0, &mut count) }, FpStatus::InvalidValue);
    assert_eq!(last_error(), "no step is running");
    // Freed while a step runs: the step ends first.
    // SAFETY: a live world.
    assert_eq!(unsafe { fp_step_begin(there.0, STEP) }, FpStatus::Ok);
}

// --- Rigidbodies -----------------------------------------------------------------

fn body_spec() -> FpBodySpec {
    FpBodySpec { mass: 2.0, use_gravity: 1, max_angular_velocity: 50.0, ..FpBodySpec::default() }
}

impl World {
    fn rigidbody(&self, key: u64, spec: FpBodySpec, position: FpPose) -> u64 {
        let mut id = 0;
        // SAFETY: a live world, readable spec and pose, a writable out.
        let status = unsafe { fp_rigidbody_add(self.0, key, &spec, &position, &mut id) };
        assert_eq!(status, FpStatus::Ok, "{}", last_error());
        id
    }

    fn state(&self, body: u64) -> FpBodyState {
        let mut state = FpBodyState::default();
        // SAFETY: a live world and a writable out.
        assert_eq!(unsafe { fp_rigidbody_state_get(self.0, body, &mut state) }, FpStatus::Ok, "{}", last_error());
        state
    }
}

#[test]
fn a_rigidbody_with_its_collider_falls_and_its_moves_come_back_with_their_velocities() {
    let world = World::new();
    let body = world.rigidbody(9, body_spec(), at(0.0, 10.0, 0.0));
    let mut collider = 0;
    // SAFETY: a live world and a writable out.
    let status = unsafe {
        fp_object_collider_add(world.0, 9, ball_spec(0.5, 0, 0), object(0.0, 10.0, 0.0, 1.0), body, &mut collider)
    };
    assert_eq!(status, FpStatus::Ok, "{}", last_error());
    let moved = world.step();
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].key, 9);
    assert!((moved[0].linear_velocity.y + 9.81 * STEP).abs() < 1e-4, "{:?}", moved[0]);
    let state = world.state(body);
    assert_eq!(state.pose, moved[0].pose);
    assert_eq!(state.linear_velocity, moved[0].linear_velocity);
    assert_eq!(state.sleeping, 0);
    let mut spec = FpBodySpec::default();
    // SAFETY: a live world and a writable out.
    assert_eq!(unsafe { fp_rigidbody_spec_get(world.0, body, &mut spec) }, FpStatus::Ok);
    assert_eq!(spec, body_spec());
}

#[test]
fn pushes_velocities_moves_sleep_and_settings_come_through() {
    let world = World::new();
    let floating = FpBodySpec { use_gravity: 0, ..body_spec() };
    let body = world.rigidbody(1, floating, at(0.0, 0.0, 0.0));
    // SAFETY: a live world and readable or writable pointers throughout.
    unsafe {
        let push = FpPush {
            kind: FpPushKind::Force as u32,
            mode: FpForceMode::VelocityChange as u32,
            vector: FpVec3 { x: 3.0, y: 0.0, z: 0.0 },
            ..FpPush::default()
        };
        assert_eq!(fp_rigidbody_push(world.0, body, &push), FpStatus::Ok, "{}", last_error());
        world.step();
        assert!((world.state(body).linear_velocity.x - 3.0).abs() < 1e-5);

        let (linear, angular) = (FpVec3 { x: 0.0, y: 1.0, z: 0.0 }, FpVec3 { x: 0.0, y: 0.0, z: 2.0 });
        assert_eq!(fp_rigidbody_velocities_set(world.0, body, &linear, &angular), FpStatus::Ok);
        let state = world.state(body);
        assert_eq!((state.linear_velocity, state.angular_velocity), (linear, angular));

        let kinematic = FpBodySpec { kinematic: 1, ..floating };
        assert_eq!(fp_rigidbody_spec_set(world.0, body, &kinematic), FpStatus::Ok);
        assert_eq!(fp_rigidbody_move(world.0, body, &at(5.0, 0.0, 0.0)), FpStatus::Ok);
        world.step();
        assert_eq!(world.state(body).pose.position.x, 5.0);

        assert_eq!(fp_rigidbody_sleep_set(world.0, body, 1), FpStatus::Ok);
        assert_eq!(world.state(body).sleeping, 1);
        assert_eq!(fp_rigidbody_sleep_set(world.0, body, 0), FpStatus::Ok);
        assert_eq!(world.state(body).sleeping, 0);

        let mut settings = FpSettings::default();
        assert_eq!(fp_settings_get(world.0, &mut settings), FpStatus::Ok);
        assert_eq!(settings, FpSettings { solver_iterations: 6, sleep_threshold: 0.005 });
        let changed = FpSettings { solver_iterations: 10, sleep_threshold: 0.1 };
        assert_eq!(fp_settings_set(world.0, &changed), FpStatus::Ok);
        assert_eq!(fp_settings_get(world.0, &mut settings), FpStatus::Ok);
        assert_eq!(settings, changed);
    }
}

#[test]
fn bad_rigidbody_values_are_refused() {
    let world = World::new();
    let body = world.rigidbody(1, body_spec(), at(0.0, 0.0, 0.0));
    let mut id = 0;
    // SAFETY: a live world and readable or writable pointers throughout.
    unsafe {
        for (spec, message) in [
            (FpBodySpec { kinematic: 2, ..body_spec() }, "kinematic is 0 or 1, not 2"),
            (FpBodySpec { constraints: 1, ..body_spec() }, "1 has bits that are no constraint"),
            (FpBodySpec { mass: 0.0, ..body_spec() }, "a rigidbody's mass"),
        ] {
            assert_eq!(fp_rigidbody_add(world.0, 1, &spec, &at(0.0, 0.0, 0.0), &mut id), FpStatus::InvalidValue);
            assert!(last_error().contains(message), "{}", last_error());
        }
        assert_eq!(fp_rigidbody_add(world.0, 1, ptr::null(), &at(0.0, 0.0, 0.0), &mut id), FpStatus::NullPointer);
        assert_eq!(id, 0);
        let push = FpPush { kind: 4, ..FpPush::default() };
        assert_eq!(fp_rigidbody_push(world.0, body, &push), FpStatus::InvalidValue);
        assert_eq!(last_error(), "4 is no push kind");
        let push = FpPush { mode: 4, ..FpPush::default() };
        assert_eq!(fp_rigidbody_push(world.0, body, &push), FpStatus::InvalidValue);
        let bouncy = FpColliderSpec {
            material: FpMaterial { bounciness: 2.0, ..FpMaterial::default() },
            ..ball_spec(1.0, 0, 0)
        };
        assert_eq!(
            fp_object_collider_add(world.0, 1, bouncy, object(0.0, 0.0, 0.0, 1.0), body, &mut id),
            FpStatus::InvalidValue
        );
        let combine = FpColliderSpec {
            material: FpMaterial { friction_combine: 4, ..FpMaterial::default() },
            ..ball_spec(1.0, 0, 0)
        };
        assert_eq!(
            fp_object_collider_add(world.0, 1, combine, object(0.0, 0.0, 0.0, 1.0), body, &mut id),
            FpStatus::InvalidValue
        );
        assert_eq!(last_error(), "4 is no combine");
        assert_eq!(
            fp_settings_set(world.0, &FpSettings { solver_iterations: 0, sleep_threshold: 0.0 }),
            FpStatus::InvalidValue
        );
        // A plain body isn't a rigidbody.
        let plain = world.body(FpBodyKind::Dynamic, 2, at(0.0, 0.0, 0.0));
        assert_eq!(fp_rigidbody_sleep_set(world.0, plain, 1), FpStatus::InvalidValue);
    }
}

impl World {
    fn contacts(&self) -> (Vec<FpContact>, Vec<FpContactPoint>) {
        let (mut count, mut point_count) = (0, 0);
        // SAFETY: a live world; null arrays with capacity 0 and writable counts.
        let status =
            unsafe { fp_contacts_read(self.0, ptr::null_mut(), 0, ptr::null_mut(), 0, &mut count, &mut point_count) };
        assert_eq!(status, FpStatus::Ok, "{}", last_error());
        let mut contacts = vec![FpContact::default(); count as usize];
        let mut points = vec![FpContactPoint::default(); point_count as usize];
        // SAFETY: room for exactly the counts.
        let status = unsafe {
            fp_contacts_read(
                self.0,
                contacts.as_mut_ptr(),
                contacts.len(),
                points.as_mut_ptr(),
                points.len(),
                &mut count,
                &mut point_count,
            )
        };
        assert_eq!(status, FpStatus::Ok, "{}", last_error());
        (contacts, points)
    }
}

#[test]
fn a_listening_ball_reads_its_enter_its_points_and_its_exit_through_c() {
    let world = World::new();
    let body = world.rigidbody(1, body_spec(), at(0.0, 0.95, 0.0));
    let listening = FpColliderSpec { listens: 2, ..ball_spec(0.5, 0, 0) };
    let mut ball = 0;
    // SAFETY: a live world and a writable out.
    let status = unsafe { fp_object_collider_add(world.0, 1, listening, object(0.0, 0.95, 0.0, 1.0), body, &mut ball) };
    assert_eq!(status, FpStatus::Ok, "{}", last_error());
    world.follower(2, ball_spec(0.5, 0, 0), object(0.0, 0.0, 0.0, 1.0));
    world.step();
    let (contacts, points) = world.contacts();
    assert_eq!(contacts.len(), 1, "{contacts:?}");
    let enter = contacts[0];
    assert_eq!((enter.kind, enter.trigger, enter.a, enter.b), (FpContactKind::Enter as u32, 0, 1, 2));
    assert_eq!(enter.first_point, 0);
    assert_eq!(enter.point_count as usize, points.len());
    assert!(!points.is_empty());
    // From b (the floor ball) toward a (the falling ball): up.
    assert!(points[0].normal.y > 0.9, "{:?}", points[0]);
    // Too small an array writes nothing, but the counts still come.
    let (mut count, mut point_count) = (0, 0);
    let mut one = [FpContact::default(); 1];
    // SAFETY: room for one contact and no points.
    let status =
        unsafe { fp_contacts_read(world.0, one.as_mut_ptr(), 1, ptr::null_mut(), 0, &mut count, &mut point_count) };
    assert_eq!(status, FpStatus::Ok);
    assert_eq!((count, point_count as usize, one[0].kind, one[0].a), (1, points.len(), 0, 0));
    // SAFETY: a live world.
    assert_eq!(unsafe { fp_collider_remove(world.0, ball) }, FpStatus::Ok, "{}", last_error());
    world.step();
    let (contacts, points) = world.contacts();
    assert_eq!(contacts.len(), 1, "{contacts:?}");
    assert_eq!((contacts[0].kind, contacts[0].a, contacts[0].b), (FpContactKind::Exit as u32, 1, 2));
    assert!(points.is_empty());
}

#[test]
fn the_layer_matrix_and_ignored_pairs_come_through_c() {
    let world = World::new();
    let one = world.follower(1, ball_spec(0.5, 0, 0), object(0.0, 0.0, 0.0, 1.0));
    let two = world.follower(2, ball_spec(0.5, 0, 0), object(3.0, 0.0, 0.0, 1.0));
    let mut meet = 7;
    // SAFETY: a live world and writable outs throughout.
    unsafe {
        assert_eq!(fp_layer_collision_get(world.0, 3, 5, &mut meet), FpStatus::Ok);
        assert_eq!(meet, 1);
        assert_eq!(fp_layer_collision_set(world.0, 5, 3, 0), FpStatus::Ok);
        assert_eq!(fp_layer_collision_get(world.0, 3, 5, &mut meet), FpStatus::Ok);
        assert_eq!(meet, 0);
        assert_eq!(fp_layer_collision_set(world.0, 5, 3, 2), FpStatus::InvalidValue);
        assert_eq!(fp_layer_collision_get(world.0, 32, 0, &mut meet), FpStatus::InvalidValue);
        let mut ignore = 7;
        assert_eq!(fp_collision_ignore_get(world.0, one, two, &mut ignore), FpStatus::Ok);
        assert_eq!(ignore, 0);
        assert_eq!(fp_collision_ignore_set(world.0, two, one, 1), FpStatus::Ok);
        assert_eq!(fp_collision_ignore_get(world.0, one, two, &mut ignore), FpStatus::Ok);
        assert_eq!(ignore, 1);
        // Keys are the caller's: forgetting one forgets its pairs.
        assert_eq!(fp_collider_key_forget(world.0, 1), FpStatus::Ok);
        assert_eq!(fp_collision_ignore_get(world.0, one, two, &mut ignore), FpStatus::Ok);
        assert_eq!(ignore, 0);
        assert_eq!(fp_collision_ignore_set(world.0, one, one, 1), FpStatus::InvalidValue);
        assert_eq!(fp_collision_ignore_get(world.0, one, two, ptr::null_mut()), FpStatus::NullPointer);
        let listens = FpColliderSpec { listens: 4, ..ball_spec(0.5, 0, 0) };
        let mut id = 0;
        let status = fp_object_collider_add(world.0, 1, listens, object(0.0, 0.0, 0.0, 1.0), 0, &mut id);
        assert_eq!(status, FpStatus::InvalidValue);
        assert!(last_error().contains("listens"), "{}", last_error());
    }
}

// --- Joints ----------------------------------------------------------------------

#[test]
fn a_joint_holds_through_c_says_how_hard_and_breaks_and_bad_joints_are_refused() {
    let world = World::new();
    // 2 kg hanging from the world, 1 m below where it's held.
    let body = world.rigidbody(3, body_spec(), at(0.0, 5.0, 0.0));
    let spec = FpJointSpec {
        body,
        kind: FpJointKind::Fixed as u32,
        anchor: FpVec3 { y: 1.0, ..FpVec3::default() },
        break_force: f32::INFINITY,
        break_torque: f32::INFINITY,
        ..FpJointSpec::default()
    };
    let (mut joint, mut count) = (0, 0);
    let mut state = FpJointState::default();
    let mut broken = [FpBrokenJoint::default(); 2];
    // SAFETY: a live world, readable specs, and writable outs.
    unsafe {
        assert_eq!(fp_joint_add(world.0, 42, &spec, &mut joint), FpStatus::Ok, "{}", last_error());
        for _ in 0..10 {
            world.step();
            assert_eq!(fp_joints_broken(world.0, &mut count), FpStatus::Ok);
            assert_eq!(count, 0);
        }
        assert_eq!(fp_joint_state_get(world.0, joint, &mut state), FpStatus::Ok);
        assert!((state.force.y - 19.62).abs() < 0.1, "{state:?}");
        assert_eq!(state.connected_anchor, FpVec3 { x: 0.0, y: 6.0, z: 0.0 });
        assert_eq!(world.state(body).pose, at(0.0, 5.0, 0.0));
        // Weaker than its load, it breaks in the next step.
        let weak = FpJointSpec { break_force: 10.0, ..spec };
        assert_eq!(fp_joint_spec_set(world.0, joint, &weak), FpStatus::Ok, "{}", last_error());
        world.step();
        assert_eq!(fp_joints_broken(world.0, &mut count), FpStatus::Ok);
        assert_eq!(count, 1);
        assert_eq!(fp_joints_broken_read(world.0, broken.as_mut_ptr(), 2), FpStatus::WrongSize);
        assert_eq!(fp_joints_broken_read(world.0, broken.as_mut_ptr(), 1), FpStatus::Ok);
        assert_eq!(broken[0].key, 42);
        assert!((broken[0].force - 19.62).abs() < 0.5, "{:?}", broken[0]);
        assert_eq!(fp_joint_state_get(world.0, joint, &mut state), FpStatus::NoJoint);
        assert_eq!(fp_joint_remove(world.0, joint), FpStatus::NoJoint);

        for bad in [
            FpJointSpec { kind: 3, ..spec },
            FpJointSpec { use_limits: 2, ..spec },
            FpJointSpec { break_force: 0.0, ..spec },
            FpJointSpec { kind: FpJointKind::Hinge as u32, axis: FpVec3::default(), ..spec },
        ] {
            assert_eq!(fp_joint_add(world.0, 1, &bad, &mut joint), FpStatus::InvalidValue, "{bad:?}");
        }
        assert_eq!(fp_joint_add(world.0, 1, &FpJointSpec { body: 99, ..spec }, &mut joint), FpStatus::NoBody);
        assert_eq!(fp_joint_add(world.0, 1, ptr::null(), &mut joint), FpStatus::NullPointer);
        assert_eq!(fp_joint_add(world.0, 1, &spec, ptr::null_mut()), FpStatus::NullPointer);
        let hinge = FpJointSpec { kind: FpJointKind::Hinge as u32, axis: FpVec3 { x: 1.0, y: 0.0, z: 0.0 }, ..spec };
        assert_eq!(fp_joint_add(world.0, 1, &hinge, &mut joint), FpStatus::Ok);
        assert_eq!(fp_joint_remove(world.0, joint), FpStatus::Ok);
    }
}

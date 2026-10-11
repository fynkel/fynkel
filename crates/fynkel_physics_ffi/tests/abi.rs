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
        let status = unsafe { fp_static_collider_add(self.0, key, spec, at, &mut id) };
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
        assert_eq!(fp_static_collider_add(world.0, 1, capsule, at, &mut id), FpStatus::InvalidValue);
        assert_eq!(last_error(), "3 is no direction: 0, 1 or 2 for X, Y or Z");
        let kind = FpColliderSpec { kind: 5, ..ball_spec(1.0, 0, 0) };
        assert_eq!(fp_static_collider_add(world.0, 1, kind, at, &mut id), FpStatus::InvalidValue);
        assert_eq!(fp_static_collider_add(world.0, 1, ball_spec(1.0, 0, 2), at, &mut id), FpStatus::InvalidValue);
        assert_eq!(fp_static_collider_add(world.0, 1, ball_spec(1.0, 32, 0), at, &mut id), FpStatus::InvalidValue);
        assert_eq!(fp_static_collider_add(world.0, 1, ball_spec(-1.0, 0, 0), at, &mut id), FpStatus::InvalidValue);
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

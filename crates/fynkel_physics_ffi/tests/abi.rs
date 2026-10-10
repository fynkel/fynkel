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

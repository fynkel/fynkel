//! What the loader sees of `Physics` is the same with the Rapier adapter
//! and with the stand-in a stripped runtime carries (ADR-0029
//! `modules.strip`; foundation 6.5): the same registrations succeed, the
//! same ones are refused, and a failed one leaves the same state. Run both
//! ways: `cargo test -p fynkel_physics` and `… --no-default-features`.

use fynkel_contracts::AssetRef;
use fynkel_physics::{CombineRule, Physics, PhysicsMaterial, Vec3, MAX_MESH_COLLIDER_TRIANGLES};

fn key(n: u8) -> AssetRef {
    AssetRef::new(fynkel_contracts::Uuid::from_bytes([n; 16]))
}

const QUAD: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 0.0, 1.0],
    [0.0, 0.0, 1.0],
];

#[test]
fn gravity_starts_at_earths_and_ignores_a_value_that_is_not_finite() {
    let mut p = Physics::new();
    assert_eq!(p.gravity(), Vec3::new(0.0, -9.81, 0.0));
    p.set_gravity(Vec3::new(0.0, -1.62, 0.0));
    assert_eq!(p.gravity(), Vec3::new(0.0, -1.62, 0.0));
    p.set_gravity(Vec3::new(f32::NAN, 0.0, 0.0));
    assert_eq!(p.gravity(), Vec3::new(0.0, -1.62, 0.0));
}

#[test]
fn a_collision_mesh_is_refused_empty_out_of_range_or_too_big() {
    let mut p = Physics::new();
    assert_eq!(p.register_mesh(key(1), &QUAD, &[0, 1, 2, 0, 2, 3]), Ok(2));
    assert!(p.has_mesh(&key(1)));
    assert!(p.register_mesh(key(2), &QUAD, &[]).is_err());
    assert!(p.register_mesh(key(2), &QUAD, &[0, 1, 4]).is_err());
    let too_many = vec![0u32; (MAX_MESH_COLLIDER_TRIANGLES + 1) * 3];
    assert!(p.register_mesh(key(2), &QUAD, &too_many).is_err());
    assert!(!p.has_mesh(&key(2)));
    assert_eq!(p.mesh_count(), 1);
}

#[test]
fn a_replaced_mesh_that_is_refused_is_forgotten_and_none_removes_it() {
    let mut p = Physics::new();
    p.register_mesh(key(1), &QUAD, &[0, 1, 2]).unwrap();
    p.replace_mesh(&key(1), Some((&QUAD, &[0, 2, 3]))).unwrap();
    assert!(p.has_mesh(&key(1)));
    assert!(p.replace_mesh(&key(1), Some((&QUAD, &[0, 9, 3]))).is_err());
    assert!(
        !p.has_mesh(&key(1)),
        "a refused replacement forgets the old triangles"
    );
    p.register_mesh(key(1), &QUAD, &[0, 1, 2]).unwrap();
    p.replace_mesh(&key(1), None).unwrap();
    assert!(!p.has_mesh(&key(1)));
    assert_eq!(p.mesh_count(), 0);
}

#[test]
fn a_material_out_of_range_is_refused_and_a_good_one_is_kept() {
    let mut p = Physics::new();
    let ice = PhysicsMaterial {
        friction: 0.02,
        bounciness: 0.1,
        friction_combine: CombineRule::Minimum,
        bounce_combine: CombineRule::Average,
    };
    p.register_material(key(1), ice).unwrap();
    assert_eq!(p.material(&key(1)), Some(ice));
    for bad in [
        PhysicsMaterial {
            friction: -1.0,
            ..ice
        },
        PhysicsMaterial {
            bounciness: 1.5,
            ..ice
        },
        PhysicsMaterial {
            friction: f32::NAN,
            ..ice
        },
    ] {
        assert!(p.register_material(key(2), bad).is_err(), "{bad:?}");
    }
    assert!(!p.has_material(&key(2)));
    assert_eq!(p.material_count(), 1);
}

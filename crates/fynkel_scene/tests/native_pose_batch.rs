use fynkel_ecs::World;
use fynkel_scene::{self as scene, Transform, WorldTransform};
use glam::{Quat, Vec3};

#[test]
fn world_pose_batch_uses_updated_ancestors_independent_of_request_order() {
    let mut w = World::new();
    let parent = scene::spawn(&mut w, Transform::IDENTITY, None).unwrap();
    let child = scene::spawn(&mut w, Transform::from_position(Vec3::X), Some(parent)).unwrap();
    scene::propagate(&mut w);
    let poses = scene::prepare_world_poses(
        &w,
        &[
            (child, Vec3::new(5.0, 2.0, 0.0), Quat::IDENTITY),
            (
                parent,
                Vec3::new(5.0, 0.0, 0.0),
                Quat::from_rotation_z(core::f32::consts::FRAC_PI_2),
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        w.copy::<Transform>(parent).unwrap(),
        Transform::IDENTITY,
        "preparation is pure"
    );
    scene::set_local_batch(&w, &poses).unwrap();
    scene::propagate(&mut w);
    assert!(
        scene::current_world(&w, child)
            .unwrap()
            .transform_point3(Vec3::ZERO)
            .distance(Vec3::new(5.0, 2.0, 0.0))
            < 1e-5
    );
    assert!(scene::world_rotation(&w, child)
        .unwrap()
        .abs_diff_eq(Quat::IDENTITY, 1e-5));
}

#[test]
fn invalid_local_output_rejects_the_whole_batch_without_dirtying_valid_targets() {
    let mut w = World::new();
    let a = scene::spawn(&mut w, Transform::IDENTITY, None).unwrap();
    let b = scene::spawn(&mut w, Transform::IDENTITY, None).unwrap();
    scene::propagate(&mut w);
    let bad = Transform {
        rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 2.0),
        ..Transform::IDENTITY
    };
    assert!(!bad.is_valid());
    assert!(
        scene::set_local_batch(&w, &[(a, Transform::from_position(Vec3::X)), (b, bad)]).is_err()
    );
    assert_eq!(w.copy::<Transform>(a).unwrap(), Transform::IDENTITY);
    assert!(!w.copy::<WorldTransform>(a).unwrap().dirty);
    assert!(scene::set_local(&w, b, bad).is_err());
    assert!(scene::spawn(&mut w, bad, None).is_err());
}

#[test]
fn world_pose_preparation_rejects_singular_parents_and_raw_hierarchy_cycles() {
    let mut w = World::new();
    let parent = scene::spawn(
        &mut w,
        Transform {
            scale: Vec3::ZERO,
            ..Transform::IDENTITY
        },
        None,
    )
    .unwrap();
    let child = scene::spawn(&mut w, Transform::IDENTITY, Some(parent)).unwrap();
    assert!(scene::prepare_world_poses(&w, &[(child, Vec3::X, Quat::IDENTITY)]).is_err());
    assert_eq!(w.copy::<Transform>(child).unwrap(), Transform::IDENTITY);
    w.insert(parent, scene::Parent(child)).unwrap();
    assert!(matches!(
        scene::prepare_world_poses(&w, &[(child, Vec3::X, Quat::IDENTITY)]),
        Err(scene::SceneError::Cycle { .. })
    ));
}

#[test]
fn unchanged_native_pose_batches_preserve_clean_transform_state() {
    let mut world = World::new();
    let entity = scene::spawn(&mut world, Transform::from_position(Vec3::X), None).unwrap();
    scene::propagate(&mut world);
    scene::set_local_batch(&world, &[(entity, Transform::from_position(Vec3::X))]).unwrap();
    assert!(!world.copy::<WorldTransform>(entity).unwrap().dirty);
    scene::set_world_pose(&world, entity, Vec3::X, Quat::IDENTITY).unwrap();
    assert!(!world.copy::<WorldTransform>(entity).unwrap().dirty);
}

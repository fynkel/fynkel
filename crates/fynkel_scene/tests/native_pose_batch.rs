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

#[test]
fn root_pose_batch_preserves_scale_last_request_and_atomic_validation() {
    let mut world = World::new();
    let initial = Transform {
        scale: Vec3::new(-2.0, 3.0, 0.0),
        ..Transform::IDENTITY
    };
    let root = scene::spawn(&mut world, initial, None).unwrap();
    let other = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
    scene::propagate(&mut world);
    let rotation = Quat::from_rotation_y(0.7);
    let poses = scene::prepare_world_poses(
        &world,
        &[
            (root, Vec3::splat(f32::NAN), Quat::IDENTITY),
            (other, Vec3::Y, Quat::IDENTITY),
            (root, Vec3::X, rotation),
        ],
    )
    .unwrap();
    assert_eq!(poses.len(), 2);
    assert_eq!(
        poses.iter().find(|(e, _)| *e == root).unwrap().1,
        Transform {
            position: Vec3::X,
            rotation,
            scale: initial.scale
        }
    );
    for (position, rotation) in [
        (Vec3::splat(f32::NAN), Quat::IDENTITY),
        (Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 2.0)),
    ] {
        assert!(scene::prepare_world_poses(
            &world,
            &[(other, Vec3::Y, Quat::IDENTITY), (root, position, rotation),]
        )
        .is_err());
        assert_eq!(world.copy::<Transform>(root).unwrap(), initial);
        assert_eq!(world.copy::<Transform>(other).unwrap(), Transform::IDENTITY);
        assert!(!world.copy::<WorldTransform>(root).unwrap().dirty);
        assert!(!world.copy::<WorldTransform>(other).unwrap().dirty);
    }
}

#[test]
#[ignore = "manual root world-pose preparation qualification"]
fn root_pose_preparation_qualification() {
    let mut world = World::new();
    let requests: Vec<_> = (0..5000)
        .map(|i| {
            let entity = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
            (
                entity,
                Vec3::new(i as f32, 2.0, 3.0),
                Quat::from_rotation_y(0.7),
            )
        })
        .collect();
    for _ in 0..40 {
        std::hint::black_box(scene::prepare_world_poses(&world, &requests).unwrap());
    }
    let mut samples = Vec::new();
    for _ in 0..300 {
        let start = std::time::Instant::now();
        let result = scene::prepare_world_poses(&world, &requests).unwrap();
        samples.push(start.elapsed().as_secs_f64() * 1e6);
        assert_eq!(result.len(), requests.len());
        std::hint::black_box(result);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "5000 root poses: median_us={} p95_us={}",
        samples[150], samples[285]
    );
}

#[test]
fn shared_pose_reads_preserve_missing_stale_foreign_and_recycled_handle_errors() {
    use fynkel_ecs::EcsError;
    let mut world = World::new();
    let good = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
    scene::propagate(&mut world);
    let missing_local = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
    world.remove::<Transform>(missing_local).unwrap();
    let missing_world = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
    world.remove::<WorldTransform>(missing_world).unwrap();
    for bad in [missing_local, missing_world] {
        assert!(matches!(scene::prepare_world_poses(&world,
            &[(good, Vec3::X, Quat::IDENTITY), (bad, Vec3::Y, Quat::IDENTITY)]),
            Err(scene::SceneError::Ecs(EcsError::MissingComponent(e))) if e == bad));
    }
    let stale = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
    world.despawn(stale).unwrap();
    let replacement = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
    let mut foreign_world = World::new();
    let foreign = scene::spawn(&mut foreign_world, Transform::IDENTITY, None).unwrap();
    for bad in [stale, foreign, fynkel_contracts::Entity::NULL] {
        assert!(matches!(scene::prepare_world_poses(&world,
            &[(good, Vec3::X, Quat::IDENTITY), (bad, Vec3::Y, Quat::IDENTITY)]),
            Err(scene::SceneError::Ecs(EcsError::StaleEntity(e))) if e == bad));
    }
    assert_eq!(world.copy::<Transform>(good).unwrap(), Transform::IDENTITY);
    assert!(!world.copy::<WorldTransform>(good).unwrap().dirty);
    assert_eq!(
        world.copy::<Transform>(replacement).unwrap(),
        Transform::IDENTITY
    );
    // Ancestors need local poses, but need not carry a propagated component.
    let child = scene::spawn(&mut world, Transform::IDENTITY, Some(missing_world)).unwrap();
    assert!(scene::prepare_world_poses(&world, &[(child, Vec3::Y, Quat::IDENTITY)]).is_ok());
    world.remove::<Transform>(missing_world).unwrap();
    assert!(
        matches!(scene::prepare_world_poses(&world, &[(child, Vec3::Y, Quat::IDENTITY)]),
        Err(scene::SceneError::Ecs(EcsError::MissingComponent(e))) if e == missing_world)
    );
}

#[test]
fn empty_pose_preparation_does_not_borrow_unrelated_component_columns() {
    let mut world = World::new();
    let entity = scene::spawn(&mut world, Transform::IDENTITY, None).unwrap();
    let _exclusive = world.get_mut::<Transform>(entity).unwrap();
    assert!(scene::prepare_world_poses(&world, &[]).unwrap().is_empty());
}

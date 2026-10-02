use fynkel_ecs::World;
use fynkel_scene::*;
use glam::{Mat4, Quat, Vec3};

fn approx(a: Mat4, b: Mat4) -> bool {
    a.abs_diff_eq(b, 1e-4)
}

#[test]
fn propagation_and_dirty_subtrees() {
    let mut w = World::new();
    let root = spawn(
        &mut w,
        Transform::from_position(Vec3::new(1.0, 0.0, 0.0)),
        None,
    )
    .unwrap();
    let mid = spawn(
        &mut w,
        Transform::from_position(Vec3::new(0.0, 2.0, 0.0)),
        Some(root),
    )
    .unwrap();
    let leaf = spawn(
        &mut w,
        Transform::from_position(Vec3::new(0.0, 0.0, 3.0)),
        Some(mid),
    )
    .unwrap();
    let other = spawn(
        &mut w,
        Transform::from_position(Vec3::new(9.0, 9.0, 9.0)),
        None,
    )
    .unwrap();

    assert_eq!(propagate(&mut w), 4);
    assert!(approx(
        world_matrix(&w, leaf).unwrap(),
        Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
    ));
    assert_eq!(propagate(&mut w), 0, "nothing dirty");

    // Moving `mid` recomputes mid + leaf only.
    set_local(&w, mid, Transform::from_position(Vec3::new(0.0, 5.0, 0.0))).unwrap();
    assert_eq!(propagate(&mut w), 2);
    assert!(approx(
        world_matrix(&w, leaf).unwrap(),
        Mat4::from_translation(Vec3::new(1.0, 5.0, 3.0))
    ));
    assert!(approx(
        world_matrix(&w, other).unwrap(),
        Mat4::from_translation(Vec3::splat(9.0))
    ));

    // Rotation composes: root rotated 90° about Y moves leaf's +Z to +X.
    set_local(
        &w,
        root,
        Transform {
            position: Vec3::ZERO,
            rotation: Quat::from_rotation_y(core::f32::consts::FRAC_PI_2),
            scale: Vec3::ONE,
        },
    )
    .unwrap();
    assert_eq!(propagate(&mut w), 3);
    let p = world_matrix(&w, leaf).unwrap().transform_point3(Vec3::ZERO);
    assert!(p.abs_diff_eq(Vec3::new(3.0, 5.0, 0.0), 1e-4), "{p}");
}

#[test]
fn cycle_rejection() {
    let mut w = World::new();
    let a = spawn(&mut w, Transform::IDENTITY, None).unwrap();
    let b = spawn(&mut w, Transform::IDENTITY, Some(a)).unwrap();
    let c = spawn(&mut w, Transform::IDENTITY, Some(b)).unwrap();
    assert_eq!(
        set_parent_keep_local(&mut w, a, Some(a)),
        Err(SceneError::Cycle {
            child: a,
            parent: a
        })
    );
    assert_eq!(
        set_parent_keep_local(&mut w, a, Some(c)),
        Err(SceneError::Cycle {
            child: a,
            parent: c
        })
    );
    assert_eq!(
        set_parent_keep_local(&mut w, b, Some(c)),
        Err(SceneError::Cycle {
            child: b,
            parent: c
        })
    );
    // Hierarchy untouched after rejections.
    assert_eq!(parent_of(&w, a), None);
    assert_eq!(parent_of(&w, b), Some(a));
    assert_eq!(children_of(&w, b), vec![c]);
    // Spawn with cyclic/stale parent fails and leaves nothing behind.
    let before = w.len();
    assert!(matches!(
        spawn(
            &mut w,
            Transform::IDENTITY,
            Some(fynkel_contracts::Entity::NULL)
        ),
        Err(SceneError::Ecs(_))
    ));
    assert_eq!(w.len(), before);
}

#[test]
fn reparent_keep_world_and_keep_local() {
    let mut w = World::new();
    let a = spawn(
        &mut w,
        Transform::from_position(Vec3::new(10.0, 0.0, 0.0)),
        None,
    )
    .unwrap();
    let b = spawn(
        &mut w,
        Transform {
            position: Vec3::new(0.0, 0.0, 0.0),
            rotation: Quat::from_rotation_z(1.0),
            scale: Vec3::splat(2.0),
        },
        None,
    )
    .unwrap();
    let c = spawn(
        &mut w,
        Transform::from_position(Vec3::new(1.0, 1.0, 1.0)),
        Some(a),
    )
    .unwrap();
    propagate(&mut w);
    let c_world = world_matrix(&w, c).unwrap();

    set_parent_keep_world(&mut w, c, Some(b)).unwrap();
    propagate(&mut w);
    assert!(
        approx(world_matrix(&w, c).unwrap(), c_world),
        "world preserved"
    );
    assert_eq!(parent_of(&w, c), Some(b));
    assert_eq!(children_of(&w, a), vec![]);
    assert_eq!(children_of(&w, b), vec![c]);
    assert!(
        !approx(
            local(&w, c).unwrap().matrix(),
            Mat4::from_translation(Vec3::ONE)
        ),
        "local changed"
    );

    set_parent_keep_local(&mut w, c, None).unwrap();
    propagate(&mut w);
    assert!(approx(
        world_matrix(&w, c).unwrap(),
        local(&w, c).unwrap().matrix()
    ));
    assert_eq!(children_of(&w, b), vec![]);
}

#[test]
fn deterministic_child_order_and_sibling_index() {
    let mut w = World::new();
    let p = spawn(&mut w, Transform::IDENTITY, None).unwrap();
    let kids: Vec<_> = (0..5)
        .map(|_| spawn(&mut w, Transform::IDENTITY, Some(p)).unwrap())
        .collect();
    assert_eq!(children_of(&w, p), kids);
    set_sibling_index(&mut w, kids[4], 0).unwrap();
    assert_eq!(children_of(&w, p)[0], kids[4]);
    set_sibling_index(&mut w, kids[4], 99).unwrap();
    assert_eq!(*children_of(&w, p).last().unwrap(), kids[4]);
    // subtree() follows child order.
    let g = spawn(&mut w, Transform::IDENTITY, Some(kids[0])).unwrap();
    let order = subtree(&w, p);
    assert_eq!(order[0], p);
    assert_eq!(order[1], kids[0]);
    assert_eq!(order[2], g, "depth-first: grandchild before next sibling");
    assert_eq!(order[3], kids[1]);
    assert_eq!(*order.last().unwrap(), kids[4]);
}

#[test]
fn destroy_subtree_and_unlink() {
    let mut w = World::new();
    let root = spawn(&mut w, Transform::IDENTITY, None).unwrap();
    let a = spawn(&mut w, Transform::IDENTITY, Some(root)).unwrap();
    let b = spawn(&mut w, Transform::IDENTITY, Some(root)).unwrap();
    let a1 = spawn(&mut w, Transform::IDENTITY, Some(a)).unwrap();
    let a2 = spawn(&mut w, Transform::IDENTITY, Some(a1)).unwrap();

    let gone = despawn_subtree(&mut w, a).unwrap();
    assert_eq!(gone, vec![a, a1, a2]);
    assert!(!w.contains(a) && !w.contains(a1) && !w.contains(a2));
    assert_eq!(children_of(&w, root), vec![b]);
    assert_eq!(w.len(), 2);
    assert!(matches!(
        despawn_subtree(&mut w, a),
        Err(SceneError::Ecs(_))
    ));
    // Propagation copes with the survivors.
    assert_eq!(propagate(&mut w), 2);
}

#[test]
fn keep_world_reparent_uses_the_current_poses() {
    // Foundation 3.3: a keep-world reparent reads the poses from the
    // local transforms, so a batch of them needs no whole-world pass.
    let mut w = World::new();
    let a = spawn(
        &mut w,
        Transform::from_position(Vec3::new(10.0, 0.0, 0.0)),
        None,
    )
    .unwrap();
    let b = spawn(
        &mut w,
        Transform::from_position(Vec3::new(0.0, 4.0, 0.0)),
        None,
    )
    .unwrap();
    let c = spawn(&mut w, Transform::from_position(Vec3::ONE), Some(a)).unwrap();
    propagate(&mut w);
    // Both moved since the last propagate: the cached matrices are stale.
    set_local(&w, a, Transform::from_position(Vec3::new(20.0, 0.0, 0.0))).unwrap();
    set_local(&w, b, Transform::from_position(Vec3::new(0.0, 8.0, 0.0))).unwrap();
    set_parent_keep_world(&mut w, c, Some(b)).unwrap();
    propagate(&mut w);
    let p = world_matrix(&w, c).unwrap().transform_point3(Vec3::ZERO);
    assert!(p.abs_diff_eq(Vec3::new(21.0, 1.0, 1.0), 1e-4), "{p}");
}

#[test]
fn a_dirty_node_under_a_dirty_ancestor_is_recomputed_once() {
    let mut w = World::new();
    let root = spawn(&mut w, Transform::default(), None).unwrap();
    let mid = spawn(&mut w, Transform::default(), Some(root)).unwrap();
    let leaf = spawn(&mut w, Transform::default(), Some(mid)).unwrap();
    let lone = spawn(&mut w, Transform::default(), None).unwrap();
    let under_lone = spawn(&mut w, Transform::default(), Some(lone)).unwrap();
    propagate(&mut w);
    set_local(&w, leaf, Transform::from_position(Vec3::Z)).unwrap();
    set_local(&w, root, Transform::from_position(Vec3::X)).unwrap();
    set_local(&w, under_lone, Transform::from_position(Vec3::Y)).unwrap();
    assert_eq!(propagate(&mut w), 4, "root, mid, leaf once, and under_lone");
    let p = world_matrix(&w, leaf).unwrap().transform_point3(Vec3::ZERO);
    assert!(p.abs_diff_eq(Vec3::new(1.0, 0.0, 1.0), 1e-4), "{p}");
    assert_eq!(propagate(&mut w), 0);
}

#[test]
fn keep_world_rejects_shear_and_singular_parent_without_changing_links() {
    let mut w = World::new();
    let scaled = spawn(
        &mut w,
        Transform {
            scale: Vec3::new(2.0, 1.0, 1.0),
            ..Transform::IDENTITY
        },
        None,
    )
    .unwrap();
    let child = spawn(
        &mut w,
        Transform {
            rotation: glam::Quat::from_rotation_z(0.7),
            ..Transform::IDENTITY
        },
        Some(scaled),
    )
    .unwrap();
    let before = local(&w, child).unwrap();
    assert!(matches!(
        set_parent_keep_world(&mut w, child, None),
        Err(SceneError::UnsupportedTransform(_))
    ));
    assert_eq!(parent_of(&w, child), Some(scaled));
    assert_eq!(local(&w, child).unwrap(), before);
    let singular = spawn(
        &mut w,
        Transform {
            scale: Vec3::new(0.0, 1.0, 1.0),
            ..Transform::IDENTITY
        },
        None,
    )
    .unwrap();
    assert!(matches!(
        set_parent_keep_world(&mut w, child, Some(singular)),
        Err(SceneError::UnsupportedTransform(_))
    ));
    assert_eq!(parent_of(&w, child), Some(scaled));
}

#[test]
fn world_pose_uses_current_affine_ancestors_and_authored_orientation() {
    let mut w = World::new();
    let root = spawn(
        &mut w,
        Transform {
            scale: Vec3::new(2.0, 1.0, 3.0),
            ..Transform::IDENTITY
        },
        None,
    )
    .unwrap();
    let branch = spawn(
        &mut w,
        Transform {
            rotation: Quat::from_rotation_z(0.7),
            ..Transform::IDENTITY
        },
        Some(root),
    )
    .unwrap();
    let leaf = spawn(&mut w, Transform::IDENTITY, Some(branch)).unwrap();
    let target = Vec3::new(7.0, 4.0, 2.0);
    set_world_pose(&w, leaf, target, Quat::IDENTITY).unwrap();
    assert!(current_world(&w, leaf)
        .unwrap()
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(target, 1e-5));
    assert!(world_rotation(&w, leaf)
        .unwrap()
        .abs_diff_eq(Quat::IDENTITY, 1e-5));
    let before = local(&w, leaf).unwrap();
    set_local(
        &w,
        root,
        Transform {
            scale: Vec3::new(0.0, 1.0, 1.0),
            ..Transform::IDENTITY
        },
    )
    .unwrap();
    assert!(matches!(
        set_world_pose(&w, leaf, target, Quat::IDENTITY),
        Err(SceneError::UnsupportedTransform(_))
    ));
    assert_eq!(local(&w, leaf).unwrap(), before);
}

#[test]
fn derived_scene_writes_participate_in_ecs_change_tracking() {
    let mut world = World::new();
    let root = spawn(&mut world, Transform::default(), None).unwrap();
    let child = spawn(&mut world, Transform::default(), Some(root)).unwrap();
    let before = world.change_stamp().unwrap();
    assert_eq!(propagate(&mut world), 2);
    let propagated = world.change_stamp().unwrap();
    assert_ne!(before, propagated);
    assert!(before.same_structure(propagated));
    assert_eq!(propagate(&mut world), 0);
    assert_eq!(world.change_stamp(), Some(propagated));

    world.insert(root, InactiveSelf).unwrap();
    let flagged = world.change_stamp().unwrap();
    assert_eq!(refresh_active(&mut world), 2);
    let inactive = world.change_stamp().unwrap();
    assert!(flagged.same_entities(inactive));
    assert!(!flagged.same_structure(inactive));
    assert!(world.has::<Inactive>(child));
    assert_eq!(refresh_active(&mut world), 0);
    assert_eq!(world.change_stamp(), Some(inactive));

    world.remove::<InactiveSelf>(root).unwrap();
    let unflagged = world.change_stamp().unwrap();
    assert_eq!(refresh_active_subtree(&mut world, root), 2);
    assert!(!unflagged.same_structure(world.change_stamp().unwrap()));
    set_local(&world, root, Transform::from_position(Vec3::X)).unwrap();
    let moved = world.change_stamp().unwrap();
    assert_eq!(propagate_subtree(&mut world, root), Some(2));
    let settled = world.change_stamp().unwrap();
    assert_ne!(moved, settled);
    assert!(moved.same_structure(settled));
    assert!(approx(
        world_matrix(&world, child).unwrap(),
        Mat4::from_translation(Vec3::X)
    ));
}

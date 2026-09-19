use fynkel_contracts::WorldId;
use fynkel_ecs::World;
use fynkel_scene::*;
use glam::{Mat4, Quat, Vec3};

fn approx(a: Mat4, b: Mat4) -> bool {
    a.abs_diff_eq(b, 1e-4)
}

#[test]
fn propagation_and_dirty_subtrees() {
    let mut w = World::new(WorldId(1));
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
    let mut w = World::new(WorldId(1));
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
    let mut w = World::new(WorldId(1));
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
    let mut w = World::new(WorldId(1));
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
    let mut w = World::new(WorldId(1));
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

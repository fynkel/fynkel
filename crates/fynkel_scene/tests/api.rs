//! The rest of the hierarchy API (foundation 6.5): world poses, the active
//! flags, subtree-only refreshes, layers and tags, and the refusals.

use fynkel_contracts::WorldId;
use fynkel_ecs::World;
use fynkel_scene::*;
use glam::{Mat4, Quat, Vec3};

fn approx(a: Mat4, b: Mat4) -> bool {
    a.abs_diff_eq(b, 1e-4)
}

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_position(Vec3::new(x, y, z))
}

#[test]
fn set_world_pose_under_a_moved_turned_scaled_parent_lands_where_asked() {
    let mut w = World::new(WorldId(1));
    let parent = spawn(
        &mut w,
        Transform {
            position: Vec3::new(4.0, 1.0, -2.0),
            rotation: Quat::from_rotation_y(0.7),
            scale: Vec3::splat(2.0),
        },
        None,
    )
    .unwrap();
    let child = spawn(
        &mut w,
        Transform {
            scale: Vec3::new(1.0, 3.0, 1.0),
            ..at(1.0, 0.0, 0.0)
        },
        Some(parent),
    )
    .unwrap();
    propagate(&mut w);

    let pos = Vec3::new(-3.0, 5.0, 8.0);
    let rot = Quat::from_rotation_x(0.4);
    set_world_pose(&w, child, pos, rot).unwrap();
    propagate(&mut w);
    let (_, r, p) = world_matrix(&w, child)
        .unwrap()
        .to_scale_rotation_translation();
    assert!(p.abs_diff_eq(pos, 1e-4), "{p} != {pos}");
    assert!(r.abs_diff_eq(rot, 1e-4) || r.abs_diff_eq(-rot, 1e-4));
    assert_eq!(
        local(&w, child).unwrap().scale,
        Vec3::new(1.0, 3.0, 1.0),
        "the local scale is kept"
    );

    // The same pose again leaves the subtree clean: nothing recomputes
    // (a body at rest costs no propagate).
    set_world_pose(&w, child, pos, rot).unwrap();
    assert_eq!(propagate(&mut w), 0);

    // A root takes the pose as its local transform.
    set_world_pose(&w, parent, Vec3::ZERO, Quat::IDENTITY).unwrap();
    assert_eq!(local(&w, parent).unwrap().position, Vec3::ZERO);
    assert_eq!(local(&w, parent).unwrap().scale, Vec3::splat(2.0));
}

#[test]
fn a_parent_switched_off_switches_off_its_subtree_and_back() {
    let mut w = World::new(WorldId(1));
    let root = spawn(&mut w, at(0.0, 0.0, 0.0), None).unwrap();
    let mid = spawn(&mut w, at(0.0, 0.0, 0.0), Some(root)).unwrap();
    let leaf = spawn(&mut w, at(0.0, 0.0, 0.0), Some(mid)).unwrap();
    assert_eq!(refresh_active(&mut w), 0, "nothing to do costs nothing");

    assert!(set_active(&mut w, mid, false).unwrap());
    assert!(!set_active(&mut w, mid, false).unwrap(), "no change");
    assert_eq!(refresh_active(&mut w), 2);
    assert!(is_active_in_hierarchy(&w, root));
    assert!(!is_active_in_hierarchy(&w, mid));
    assert!(!is_active_in_hierarchy(&w, leaf));
    assert!(!is_active_self(&w, mid));
    assert!(is_active_self(&w, leaf), "the leaf's own flag is untouched");

    // The leaf off as well, then the middle on: the leaf stays off.
    set_active(&mut w, leaf, false).unwrap();
    set_active(&mut w, mid, true).unwrap();
    refresh_active(&mut w);
    assert!(is_active_in_hierarchy(&w, mid));
    assert!(!is_active_in_hierarchy(&w, leaf));

    // Moved under an active root, a switched-off parent's child comes back.
    set_active(&mut w, leaf, true).unwrap();
    set_active(&mut w, mid, false).unwrap();
    refresh_active(&mut w);
    assert!(!is_active_in_hierarchy(&w, leaf));
    set_parent_keep_local(&mut w, leaf, Some(root)).unwrap();
    refresh_active(&mut w);
    assert!(is_active_in_hierarchy(&w, leaf));
}

#[test]
fn a_subtree_spawned_under_an_inactive_parent_is_inactive_from_the_subtree_refresh() {
    let mut w = World::new(WorldId(1));
    let root = spawn(&mut w, at(0.0, 0.0, 0.0), None).unwrap();
    set_active(&mut w, root, false).unwrap();
    refresh_active(&mut w);
    let child = spawn(&mut w, at(0.0, 0.0, 0.0), Some(root)).unwrap();
    let grandchild = spawn(&mut w, at(0.0, 0.0, 0.0), Some(child)).unwrap();
    assert_eq!(refresh_active_subtree(&mut w, child), 2);
    assert!(!is_active_in_hierarchy(&w, child));
    assert!(!is_active_in_hierarchy(&w, grandchild));
    assert_eq!(refresh_active_subtree(&mut w, child), 0, "already current");
}

#[test]
fn propagate_subtree_computes_a_new_subtree_alone_and_refuses_under_a_dirty_ancestor() {
    let mut w = World::new(WorldId(1));
    let root = spawn(&mut w, at(10.0, 0.0, 0.0), None).unwrap();
    let other = spawn(&mut w, at(0.0, 0.0, 0.0), None).unwrap();
    propagate(&mut w);
    let child = spawn(&mut w, at(0.0, 1.0, 0.0), Some(root)).unwrap();
    let grandchild = spawn(&mut w, at(0.0, 0.0, 1.0), Some(child)).unwrap();
    set_local(&w, other, at(5.0, 5.0, 5.0)).unwrap();

    assert_eq!(propagate_subtree(&mut w, child), Some(2));
    assert!(approx(
        world_matrix(&w, grandchild).unwrap(),
        Mat4::from_translation(Vec3::new(10.0, 1.0, 1.0))
    ));
    assert!(
        approx(world_matrix(&w, other).unwrap(), Mat4::IDENTITY),
        "the rest of the world is not walked"
    );

    set_local(&w, root, at(20.0, 0.0, 0.0)).unwrap();
    assert_eq!(
        propagate_subtree(&mut w, child),
        None,
        "a dirty ancestor's matrix is stale: only a whole propagate is right"
    );
}

#[test]
fn current_world_reads_through_dirty_ancestors_without_a_propagate() {
    let mut w = World::new(WorldId(1));
    let root = spawn(&mut w, at(1.0, 0.0, 0.0), None).unwrap();
    let child = spawn(&mut w, at(0.0, 2.0, 0.0), Some(root)).unwrap();
    propagate(&mut w);
    assert!(approx(
        current_world(&w, child).unwrap(),
        Mat4::from_translation(Vec3::new(1.0, 2.0, 0.0))
    ));
    set_local(&w, root, at(7.0, 0.0, 0.0)).unwrap();
    assert!(approx(
        current_world(&w, child).unwrap(),
        Mat4::from_translation(Vec3::new(7.0, 2.0, 0.0))
    ));
    assert!(
        approx(
            world_matrix(&w, child).unwrap(),
            Mat4::from_translation(Vec3::new(1.0, 2.0, 0.0))
        ),
        "the cache is untouched until propagate"
    );
}

#[test]
fn transforms_round_trip_through_a_matrix_and_refuse_non_finite_values() {
    let t = Transform {
        position: Vec3::new(1.0, -2.0, 3.0),
        rotation: Quat::from_euler(glam::EulerRot::YXZ, 0.3, -0.2, 1.1),
        scale: Vec3::new(2.0, 0.5, 1.5),
    };
    let back = Transform::from_matrix(t.matrix());
    assert!(back.position.abs_diff_eq(t.position, 1e-5));
    assert!(back.rotation.abs_diff_eq(t.rotation, 1e-5));
    assert!(back.scale.abs_diff_eq(t.scale, 1e-5));
    assert_eq!(Transform::default(), Transform::IDENTITY);

    let mut w = World::new(WorldId(1));
    let bad = at(f32::NAN, 0.0, 0.0);
    assert!(!bad.is_finite());
    assert!(matches!(
        spawn(&mut w, bad, None),
        Err(SceneError::NotFinite(_))
    ));
    let e = spawn(&mut w, at(0.0, 0.0, 0.0), None).unwrap();
    assert_eq!(
        set_local(&w, e, at(0.0, f32::INFINITY, 0.0)),
        Err(SceneError::NotFinite(e))
    );
    assert_eq!(local(&w, e).unwrap(), at(0.0, 0.0, 0.0), "left as it was");
}

#[test]
fn spawn_under_a_gone_parent_fails_and_leaves_nothing_behind() {
    let mut w = World::new(WorldId(1));
    let gone = spawn(&mut w, at(0.0, 0.0, 0.0), None).unwrap();
    despawn_subtree(&mut w, gone).unwrap();
    let before = w.len();
    assert!(spawn(&mut w, at(0.0, 0.0, 0.0), Some(gone)).is_err());
    assert_eq!(w.len(), before, "the half-made entity is gone");
    assert!(despawn_subtree(&mut w, gone).is_err());
    assert!(set_active(&mut w, gone, false).is_err());
    assert!(subtree(&w, gone).is_empty());
}

#[test]
fn a_root_has_no_sibling_order_and_an_index_past_the_end_is_the_end() {
    let mut w = World::new(WorldId(1));
    let root = spawn(&mut w, at(0.0, 0.0, 0.0), None).unwrap();
    let a = spawn(&mut w, at(0.0, 0.0, 0.0), Some(root)).unwrap();
    let b = spawn(&mut w, at(0.0, 0.0, 0.0), Some(root)).unwrap();
    set_sibling_index(&mut w, a, 99).unwrap();
    assert_eq!(children_of(&w, root), vec![b, a]);
    set_sibling_index(&mut w, root, 0).unwrap();
    assert!(parent_of(&w, root).is_none());
    assert!(is_ancestor(&w, root, a));
    assert!(!is_ancestor(&w, a, root));
    assert!(!is_ancestor(&w, a, b));
}

#[test]
fn layers_are_bits_below_32_and_tags_answer_only_in_range() {
    assert_eq!(Layer { layer: 0 }.bit(), 1);
    assert_eq!(Layer { layer: 31 }.bit(), 1 << 31);
    assert_eq!(Layer { layer: 32 }.bit(), 0);
    assert!(Layer { layer: 31 }.validate().is_ok());
    assert!(Layer { layer: 32 }.validate().is_err());
    assert_eq!(Layer::default().layer, 0, "default is layer 0");

    let t = Tags {
        bits: (1 << 3) | (1 << 31),
    };
    assert!(t.has(3));
    assert!(t.has(31));
    assert!(!t.has(4));
    assert!(!t.has(32), "out of range is never set");
    assert!(!Tags::default().has(0));
}

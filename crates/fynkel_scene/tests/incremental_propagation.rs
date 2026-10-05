use fynkel_ecs::World;
use fynkel_scene::*;
use glam::{Mat4, Quat, Vec3};
use std::any::TypeId;

#[test]
fn propagation_preserves_sparse_notifications_and_matches_hierarchy_products() {
    let mut world = World::new();
    let roots: Vec<_> = (0..128)
        .map(|i| {
            spawn(
                &mut world,
                Transform::from_position(Vec3::X * i as f32),
                None,
            )
            .unwrap()
        })
        .collect();
    let child = spawn(
        &mut world,
        Transform::from_position(Vec3::Y),
        Some(roots[0]),
    )
    .unwrap();
    let leaf = spawn(&mut world, Transform::from_position(Vec3::Z), Some(child)).unwrap();
    let mut sync = SceneSync::default();
    assert_eq!(sync.propagate(&mut world), 130);
    for (dense, preflagged) in [(false, false), (true, true), (false, true), (true, false)] {
        let targets = if dense { roots.as_slice() } else { &roots[..1] };
        for &root in targets {
            if preflagged {
                let mut local = world.copy::<Transform>(root).unwrap();
                local.rotation = Quat::from_rotation_y(0.3);
                set_local(&world, root, local).unwrap();
            } else {
                // Raw local writes must also invalidate derived matrices.
                world.get_mut::<Transform>(root).unwrap().rotation = Quat::from_rotation_y(0.3);
            }
        }
        world.get_mut::<Transform>(child).unwrap().position += Vec3::X;
        let cursor = world.change_cursor();
        assert_eq!(sync.propagate(&mut world), targets.len() + 2);
        let changes = world
            .changes_since(cursor, &[TypeId::of::<WorldTransform>()], false)
            .unwrap();
        assert!(
            !changes.reset,
            "precise writes must not force a global rescan"
        );
        let mut expected = targets.to_vec();
        expected.extend([child, leaf]);
        expected.sort_unstable();
        assert_eq!(changes.entities, expected);
        for entity in roots.iter().copied().chain([child, leaf]) {
            let mut chain = vec![entity];
            let mut parent = parent_of(&world, entity);
            while let Some(p) = parent {
                chain.push(p);
                parent = parent_of(&world, p);
            }
            let expected = chain.iter().rev().fold(Mat4::IDENTITY, |m, &e| {
                m * world.copy::<Transform>(e).unwrap().matrix()
            });
            assert!(world
                .copy::<WorldTransform>(entity)
                .unwrap()
                .matrix
                .abs_diff_eq(expected, 1e-5));
        }
        let stamp = world.change_stamp();
        assert_eq!(sync.propagate(&mut world), 0);
        assert_eq!(
            world.change_stamp(),
            stamp,
            "clean propagation must not publish writes"
        );
    }
}

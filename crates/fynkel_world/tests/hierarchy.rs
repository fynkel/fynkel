//! The hierarchy's promises (stage 2 plan §3.1): parents and children agree
//! both ways, a cycle is refused and leaves the world unchanged, despawning
//! takes the children along, names find descendants, world transforms
//! compose parent before child, whichever way they are asked for, and an
//! inactive object hides everything below it (stage 3 plan §3.3).

use fynkel_world::*;

fn spawn_at(world: &mut World, position: Vec3) -> ObjectId {
    let id = world.spawn();
    world.insert(id, Transform::from_position(position)).unwrap();
    id
}

fn close(a: Mat4, b: Mat4) -> bool {
    a.abs_diff_eq(b, 1e-4)
}

// --- Parents and children --------------------------------------------------

#[test]
fn a_parent_lists_its_children_in_the_order_they_were_added() {
    let mut world = World::new();
    let [root, a, b, c] = std::array::from_fn(|_| world.spawn());
    for child in [b, a, c] {
        world.set_parent(child, Some(root)).unwrap();
    }
    assert_eq!(world.children(root).unwrap(), [b, a, c]);
    assert_eq!(world.parent(a).unwrap(), Some(root));
    assert_eq!(world.parent(root).unwrap(), None);
    assert!(world.children(a).unwrap().is_empty());
}

#[test]
fn moving_a_child_takes_it_from_its_old_parent() {
    let mut world = World::new();
    let [first, second, child] = std::array::from_fn(|_| world.spawn());
    world.set_parent(child, Some(first)).unwrap();
    world.set_parent(child, Some(second)).unwrap();
    assert!(world.children(first).unwrap().is_empty());
    assert_eq!(world.children(second).unwrap(), [child]);

    world.set_parent(child, None).unwrap();
    assert!(world.children(second).unwrap().is_empty());
    assert_eq!(world.parent(child).unwrap(), None);
}

#[test]
fn setting_the_same_parent_again_keeps_the_order() {
    let mut world = World::new();
    let [root, a, b] = std::array::from_fn(|_| world.spawn());
    world.set_parent(a, Some(root)).unwrap();
    world.set_parent(b, Some(root)).unwrap();
    world.set_parent(a, Some(root)).unwrap();
    assert_eq!(world.children(root).unwrap(), [a, b]);
}

#[test]
fn a_cycle_is_refused_and_changes_nothing() {
    let mut world = World::new();
    let [a, b, c] = std::array::from_fn(|_| world.spawn());
    world.set_parent(b, Some(a)).unwrap();
    world.set_parent(c, Some(b)).unwrap();

    for (child, parent) in [(a, a), (a, b), (a, c), (b, c)] {
        assert_eq!(world.set_parent(child, Some(parent)), Err(WorldError::Cycle { child, parent }));
    }
    assert_eq!(world.parent(a).unwrap(), None);
    assert_eq!(world.parent(b).unwrap(), Some(a));
    assert_eq!(world.parent(c).unwrap(), Some(b));
    assert_eq!(world.children(a).unwrap(), [b]);
}

#[test]
fn unknown_objects_are_errors() {
    let mut world = World::new();
    let live = world.spawn();
    let gone = world.spawn();
    world.despawn(gone).unwrap();

    assert_eq!(world.set_parent(gone, Some(live)), Err(WorldError::NoObject(gone)));
    assert_eq!(world.set_parent(live, Some(gone)), Err(WorldError::NoObject(gone)));
    assert_eq!(world.parent(gone), Err(WorldError::NoObject(gone)));
    assert_eq!(world.children(gone).map(<[_]>::len), Err(WorldError::NoObject(gone)));
    assert_eq!(world.world_matrix(gone), Err(WorldError::NoObject(gone)));
    assert_eq!(world.is_part_of(live, gone), Err(WorldError::NoObject(gone)));
    assert_eq!(world.parent(live).unwrap(), None);
}

#[test]
fn despawning_takes_the_children_along() {
    let mut world = World::new();
    let [keep, root, child, grandchild, sibling] = std::array::from_fn(|_| world.spawn());
    world.set_parent(root, Some(keep)).unwrap();
    world.set_parent(child, Some(root)).unwrap();
    world.set_parent(grandchild, Some(child)).unwrap();
    world.set_parent(sibling, Some(keep)).unwrap();

    world.despawn(root).unwrap();
    for gone in [root, child, grandchild] {
        assert!(!world.contains(gone), "{gone} outlived its parent");
    }
    assert_eq!(world.children(keep).unwrap(), [sibling]);
    assert_eq!(world.len(), 2);
}

#[test]
fn is_part_of_is_true_for_the_object_and_everything_below_it() {
    let mut world = World::new();
    let [root, child, grandchild, other] = std::array::from_fn(|_| world.spawn());
    world.set_parent(child, Some(root)).unwrap();
    world.set_parent(grandchild, Some(child)).unwrap();

    assert!(world.is_part_of(root, root).unwrap());
    assert!(world.is_part_of(grandchild, root).unwrap());
    assert!(world.is_part_of(grandchild, child).unwrap());
    assert!(!world.is_part_of(root, child).unwrap());
    assert!(!world.is_part_of(other, root).unwrap());
}

// --- Names -----------------------------------------------------------------

#[test]
fn names_are_set_read_and_cleared() {
    let mut world = World::new();
    let id = world.spawn();
    assert_eq!(world.name(id).unwrap(), None);
    world.set_name(id, Some("Head")).unwrap();
    assert_eq!(world.name(id).unwrap(), Some("Head"));
    world.set_name(id, None).unwrap();
    assert_eq!(world.name(id).unwrap(), None);
    world.set_name(id, Some("Head")).unwrap();
    world.set_name(id, Some("")).unwrap();
    assert_eq!(world.name(id).unwrap(), None, "an empty name is no name");
}

#[test]
fn find_searches_the_descendants_depth_first() {
    let mut world = World::new();
    let [root, arm, hand, leg, toe] = std::array::from_fn(|_| world.spawn());
    world.set_parent(arm, Some(root)).unwrap();
    world.set_parent(hand, Some(arm)).unwrap();
    world.set_parent(leg, Some(root)).unwrap();
    world.set_parent(toe, Some(leg)).unwrap();
    for (id, name) in [(root, "Bone"), (arm, "Arm"), (hand, "Bone"), (leg, "Leg"), (toe, "Bone")] {
        world.set_name(id, Some(name)).unwrap();
    }

    assert_eq!(world.find(root, "Bone").unwrap(), Some(hand), "the root itself isn't searched; the arm comes first");
    assert_eq!(world.find(root, "Leg").unwrap(), Some(leg));
    assert_eq!(world.find(leg, "Bone").unwrap(), Some(toe));
    assert_eq!(world.find(root, "Tail").unwrap(), None);
    assert_eq!(world.find(toe, "Bone").unwrap(), None);
}

#[test]
fn a_name_goes_with_its_object() {
    let mut world = World::new();
    let [root, child] = std::array::from_fn(|_| world.spawn());
    world.set_parent(child, Some(root)).unwrap();
    world.set_name(child, Some("Child")).unwrap();
    world.despawn(child).unwrap();
    assert_eq!(world.find(root, "Child").unwrap(), None);
    assert_eq!(world.set_name(child, Some("x")), Err(WorldError::NoObject(child)));
}

// --- The active flag -------------------------------------------------------

#[test]
fn an_inactive_object_hides_everything_below_it_and_each_keeps_its_own_flag() {
    let mut world = World::new();
    let [root, child, grandchild, loose] = std::array::from_fn(|_| world.spawn());
    world.set_parent(child, Some(root)).unwrap();
    world.set_parent(grandchild, Some(child)).unwrap();
    let shown = |w: &World| [root, child, grandchild, loose].map(|id| w.active_in_hierarchy(id).unwrap());
    assert_eq!(shown(&world), [true; 4], "on by default");

    world.set_active(grandchild, false).unwrap();
    world.set_active(root, false).unwrap();
    assert_eq!(shown(&world), [false, false, false, true]);
    assert_eq!([root, child, grandchild].map(|id| world.active(id).unwrap()), [false, true, false]);

    // On again: the child shows, the grandchild keeps its own flag.
    world.set_active(root, true).unwrap();
    assert_eq!(shown(&world), [true, true, false, true]);
    world.set_active(root, true).unwrap();
    assert!(world.active(root).unwrap(), "setting it twice is fine");

    // Moved out from under an inactive parent: shown again.
    world.set_active(child, false).unwrap();
    world.set_active(grandchild, true).unwrap();
    assert!(!world.active_in_hierarchy(grandchild).unwrap());
    world.set_parent(grandchild, None).unwrap();
    assert!(world.active_in_hierarchy(grandchild).unwrap());
}

#[test]
fn the_flag_goes_with_its_object() {
    let mut world = World::new();
    let [root, child] = std::array::from_fn(|_| world.spawn());
    world.set_parent(child, Some(root)).unwrap();
    world.set_active(child, false).unwrap();
    world.despawn(root).unwrap();
    for gone in [root, child] {
        assert_eq!(world.set_active(gone, true), Err(WorldError::NoObject(gone)));
        assert_eq!(world.active(gone), Err(WorldError::NoObject(gone)));
        assert_eq!(world.active_in_hierarchy(gone), Err(WorldError::NoObject(gone)));
    }
}

// --- World transforms ------------------------------------------------------

#[test]
fn a_child_moves_turns_and_scales_with_its_parent() {
    let mut world = World::new();
    let parent = world.spawn();
    world
        .insert(
            parent,
            Transform {
                position: Vec3::new(1.0, 0.0, 0.0),
                rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                scale: Vec3::splat(2.0),
            },
        )
        .unwrap();
    let child = spawn_at(&mut world, Vec3::new(0.0, 0.0, -1.0));
    world.set_parent(child, Some(parent)).unwrap();

    // One unit ahead of the parent (-Z), doubled, then turned a quarter to
    // the left: two units along -X from the parent.
    let point = world.world_matrix(child).unwrap().transform_point3(Vec3::ZERO);
    assert!(point.abs_diff_eq(Vec3::new(-1.0, 0.0, 0.0), 1e-5), "{point}");
    // The local transform is kept as it is.
    assert_eq!(world.get::<Transform>(child).unwrap().position, Vec3::new(0.0, 0.0, -1.0));
}

#[test]
fn an_object_without_a_transform_passes_its_parent_through() {
    let mut world = World::new();
    let root = spawn_at(&mut world, Vec3::new(0.0, 5.0, 0.0));
    let group = world.spawn();
    let leaf = spawn_at(&mut world, Vec3::new(1.0, 0.0, 0.0));
    world.set_parent(group, Some(root)).unwrap();
    world.set_parent(leaf, Some(group)).unwrap();
    let point = world.world_matrix(leaf).unwrap().transform_point3(Vec3::ZERO);
    assert!(point.abs_diff_eq(Vec3::new(1.0, 5.0, 0.0), 1e-6), "{point}");
}

/// A small deterministic generator, so a failure repeats.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % n
    }

    fn unit(&mut self) -> f32 {
        self.below(10_000) as f32 / 10_000.0
    }
}

/// The matrices the extract uses agree with `world_matrix`, and what they
/// show with `active_in_hierarchy`, after every kind of change: new
/// parents, detaching, moves, despawns, new objects and turning objects on
/// and off.
#[test]
fn cached_world_matrices_always_match_the_walk_up() {
    let mut world = World::new();
    let mut matrices = WorldMatrices::default();
    let mut rng = Lcg(7);
    let mut live: Vec<ObjectId> = Vec::new();

    for step in 0..2_000 {
        match rng.below(12) {
            0..=2 => {
                let id = world.spawn();
                if rng.below(4) != 0 {
                    let t = Transform {
                        position: Vec3::new(rng.unit(), rng.unit(), rng.unit()) * 4.0 - 2.0,
                        rotation: Quat::from_rotation_y(rng.unit() * 6.0) * Quat::from_rotation_x(rng.unit()),
                        scale: Vec3::splat(0.5 + rng.unit()),
                    };
                    world.insert(id, t).unwrap();
                }
                live.push(id);
            }
            3..=5 if live.len() > 1 => {
                let child = live[rng.below(live.len())];
                let parent = live[rng.below(live.len())];
                // Cycles are refused; that's fine here, the rest goes on.
                let _ = world.set_parent(child, Some(parent));
            }
            6 if !live.is_empty() => {
                let id = live[rng.below(live.len())];
                world.set_parent(id, None).unwrap();
            }
            7 if !live.is_empty() => {
                let id = live[rng.below(live.len())];
                world.insert(id, Transform::from_position(Vec3::splat(rng.unit()))).unwrap();
            }
            8 if live.len() > 10 => {
                let id = live[rng.below(live.len())];
                world.despawn(id).unwrap();
                live.retain(|&id| world.contains(id));
            }
            9 | 10 if !live.is_empty() => {
                let id = live[rng.below(live.len())];
                world.set_active(id, rng.below(3) == 0).unwrap();
            }
            _ => {}
        }

        if step % 7 == 0 {
            matrices.update(&world);
            for &id in &live {
                let local = world.get::<Transform>(id).unwrap_or_default();
                let shown = world.active_in_hierarchy(id).unwrap();
                assert_eq!(matrices.shown(&world, id), shown, "step {step}, {id}");
                let Some(cached) = matrices.world(&world, id, &local) else {
                    assert!(!shown, "step {step}, {id}: hidden but active");
                    continue;
                };
                assert!(shown, "step {step}, {id}: placed but inactive");
                let walked = world.world_matrix(id).unwrap();
                assert!(close(cached, walked), "step {step}, {id}: {cached} vs {walked}");
            }
        }
    }
}

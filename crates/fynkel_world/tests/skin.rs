//! Skinned models (stage 9 phase 2): a copy's skinned parts are bent by
//! its own joint objects, which a clip moves; at rest every vertex is
//! where it was bound; and a sword made a child of a hand stays in it as
//! the hand swings. The vertices are bent here on the CPU, as skin.wgsl
//! bends them, to check the joints and inverse binds the renderer gets.

use std::path::PathBuf;

use fynkel_world::*;

fn load(path: &str) -> ModelData {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
    ModelData::from_glb(&std::fs::read(&path).unwrap()).unwrap()
}

fn knight() -> ModelData {
    load("tests/assets/gltf/Knight.glb")
}

/// Every object from `root` down.
fn tree(world: &World, root: ObjectId) -> Vec<ObjectId> {
    let mut all = vec![root];
    for &child in world.children(root).unwrap() {
        all.extend(tree(world, child));
    }
    all
}

/// The parts of the copy at `root` a skin bends.
fn skinned(world: &World, root: ObjectId) -> Vec<ObjectId> {
    tree(world, root).into_iter().filter(|&id| world.skin(id).is_some()).collect()
}

/// Where `part`'s vertex `v` is, bent by its skin's joints as they are.
fn bent(world: &World, part: ObjectId, v: usize) -> Vec3 {
    let skin = world.skin(part).unwrap();
    let mesh = world.mesh(world.get::<MeshRenderer>(part).unwrap().mesh.unwrap()).unwrap();
    let p = Vec3::from_array(mesh.vertices()[v].position);
    let s = mesh.skin().unwrap()[v];
    let mut at = Vec3::ZERO;
    for (&joint, &weight) in s.joints.iter().zip(&s.weights) {
        let j = usize::from(joint);
        let matrix = world.world_matrix(skin.joints()[j]).unwrap() * skin.inverse_binds()[j];
        at += matrix.transform_point3(p) * (f32::from(weight) / 65535.0);
    }
    at
}

fn clip(world: &World, root: ObjectId, name: &str) -> u32 {
    world.clips(root).unwrap().iter().position(|&(n, _)| n == name).unwrap() as u32
}

#[test]
fn a_copys_skinned_parts_are_bent_by_its_own_joints() {
    let model = knight();
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let one = world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
    let two = world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
    let (a, b) = (skinned(&world, one), skinned(&world, two));
    // Arms, legs, body and head.
    assert_eq!(a.len(), 6);
    // One skeleton for every part of a copy; each copy its own.
    let skin = world.skin(a[0]).unwrap();
    assert!(a.iter().all(|&p| world.skin(p).unwrap().key() == skin.key()));
    assert_ne!(world.skin(b[0]).unwrap().key(), skin.key());
    assert_eq!(skin.joints().len(), 41);
    let mine = tree(&world, one);
    assert!(skin.joints().iter().all(|j| mine.contains(j)));
    assert_eq!(world.name(skin.joints()[0]).unwrap(), Some("root"));

    world.despawn(one).unwrap();
    assert!(a.iter().all(|&p| world.skin(p).is_none()));
    world.despawn(two).unwrap();
    assert!(!world.has_skins());
}

/// The KayKit characters are bound in the pose they rest in.
#[test]
fn at_rest_every_vertex_is_where_it_was_bound() {
    let model = knight();
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let at = Transform { position: Vec3::new(3.0, 0.0, -2.0), rotation: Quat::from_rotation_y(1.0), scale: Vec3::ONE };
    let root = world.spawn_model(&model, &ids.meshes, &ids.materials, at).unwrap();
    for part in skinned(&world, root) {
        let matrix = world.world_matrix(part).unwrap();
        let mesh = world.mesh(world.get::<MeshRenderer>(part).unwrap().mesh.unwrap()).unwrap().clone();
        for (v, vertex) in mesh.vertices().iter().enumerate().step_by(7) {
            let bound = matrix.transform_point3(Vec3::from_array(vertex.position));
            let off = bent(&world, part, v).distance(bound);
            assert!(off < 1e-3, "{:?} vertex {v} is {off} m from where it was bound", world.name(part));
        }
    }
}

/// A loose sword made a child of the right hand's slot: as a chop swings
/// the arm, the hand's vertices keep their place on the hand joint (the
/// clip turns the slot a little in the hand, a flick of the wrist), and
/// the sword keeps its place on the slot.
#[test]
fn a_sword_in_the_hand_stays_in_it() {
    let model = knight();
    let sword = load("examples/lab/assets/kaykit/sword.glb");
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let sword_ids = world.add_model(&sword).unwrap();
    let root = world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
    let held = world.spawn_model(&sword, &sword_ids.meshes, &sword_ids.materials, Transform::default()).unwrap();
    let slot = world.find(root, "handslot.r").unwrap().unwrap();
    world.set_parent(held, Some(slot)).unwrap();

    // The right arm's vertices the right hand alone moves.
    let arm = skinned(&world, root).into_iter().find(|&p| world.name(p).unwrap() == Some("Knight_ArmRight")).unwrap();
    let hand = world.find(root, "hand.r").unwrap().unwrap();
    let skin = world.skin(arm).unwrap().clone();
    let hand_joint = skin.joints().iter().position(|&j| j == hand).unwrap() as u16;
    let mesh = world.mesh(world.get::<MeshRenderer>(arm).unwrap().mesh.unwrap()).unwrap().clone();
    let grip: Vec<usize> = (0..mesh.vertices().len())
        .filter(|&v| {
            let s = mesh.skin().unwrap()[v];
            s.joints[0] == hand_joint && s.weights[0] == 65535
        })
        .collect();
    assert!(grip.len() > 10, "the hand has {} vertices", grip.len());

    // Each grip vertex where the hand joint sees it, and the sword where
    // its slot does.
    let in_hand = |world: &World| -> Vec<Vec3> {
        let into_hand = world.world_matrix(hand).unwrap().inverse();
        grip.iter().map(|&v| into_hand.transform_point3(bent(world, arm, v))).collect()
    };
    let on_slot = |world: &World| world.world_matrix(slot).unwrap().inverse() * world.world_matrix(held).unwrap();
    let (grip_at_rest, sword_at_rest) = (in_hand(&world), on_slot(&world));
    let hand_at_rest = world.world_matrix(hand).unwrap().w_axis;
    world.play(root, clip(&world, root, "1H_Melee_Attack_Chop"), 0.0, false).unwrap();
    for _ in 0..3 {
        world.animate(0.2, &mut Vec::new());
        // The arm really moved.
        assert!(world.world_matrix(hand).unwrap().w_axis.distance(hand_at_rest) > 0.05);
        for (p, q) in in_hand(&world).iter().zip(&grip_at_rest) {
            assert!(p.distance(*q) < 1e-3, "a grip vertex moved {} m in the hand", p.distance(*q));
        }
        assert!(on_slot(&world).abs_diff_eq(sword_at_rest, 1e-5));
    }
}

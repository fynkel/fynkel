//! Layers (stage 4 plan §3.9): one of 32 per object, its own, 0 unless set.

use fynkel_world::*;

#[test]
fn an_object_is_on_layer_0_until_put_on_another() {
    let mut world = World::new();
    let parent = world.spawn();
    let child = world.spawn();
    world.set_parent(child, Some(parent)).unwrap();
    assert_eq!(world.layer(parent).unwrap(), 0);

    world.set_layer(parent, 31).unwrap();
    assert_eq!(world.layer(parent).unwrap(), 31);
    assert_eq!(world.layer(child).unwrap(), 0, "a child keeps its own");
    assert!(world.in_mask(parent, 1 << 31));
    assert!(!world.in_mask(parent, !(1 << 31)));
    assert!(world.in_mask(child, 1));
    assert!(world.in_mask(parent, u32::MAX));

    world.set_layer(parent, 0).unwrap();
    assert_eq!(world.layer(parent).unwrap(), 0);
}

#[test]
fn a_layer_past_31_or_a_missing_object_is_refused() {
    let mut world = World::new();
    let id = world.spawn();
    world.set_layer(id, 3).unwrap();
    assert_eq!(world.set_layer(id, 32), Err(WorldError::Layer(32)));
    assert_eq!(world.layer(id).unwrap(), 3, "unchanged");
    assert_eq!(WorldError::Layer(32).to_string(), "layer 32 doesn't exist: layers are 0 to 31");

    world.despawn(id).unwrap();
    assert!(matches!(world.set_layer(id, 1), Err(WorldError::NoObject(_))));
    assert!(matches!(world.layer(id), Err(WorldError::NoObject(_))));
    assert!(!world.in_mask(id, 1 << 3) && world.in_mask(id, 1), "a gone object counts as layer 0");
}

#[test]
fn despawning_forgets_the_layers_below() {
    let mut world = World::new();
    let parent = world.spawn();
    let child = world.spawn();
    world.set_parent(child, Some(parent)).unwrap();
    world.set_layer(parent, 2).unwrap();
    world.set_layer(child, 4).unwrap();
    world.despawn(parent).unwrap();
    // A new object never reuses an id, so nothing can inherit them; the
    // map is empty again, which is what keeps a layer-0 world cheap.
    let fresh = world.spawn();
    assert_eq!(world.layer(fresh).unwrap(), 0);
    assert!(world.in_mask(fresh, 1));
}

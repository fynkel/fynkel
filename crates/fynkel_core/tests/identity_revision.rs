use fynkel_core::{Name, Transform, World, WorldConfig};

#[test]
fn identity_cursor_covers_native_name_mutation_and_membership_only() {
    let mut world = World::new(WorldConfig::default());
    let empty = world.identity_revision().unwrap();
    assert_ne!(empty, 0);
    let committed = world
        .edit_gameplay(|edit| edit.spawn("first", Transform::IDENTITY, None))
        .unwrap();
    let e = committed.resolve(committed.value).unwrap();
    let spawned = world.identity_revision().unwrap();
    assert_ne!(empty, spawned);
    world.ecs().get_mut::<Transform>(e).unwrap().position.x = 3.;
    assert_eq!(world.identity_revision(), Some(spawned));
    world.ecs().get_mut::<Name>(e).unwrap().value = "direct".into();
    let direct = world.identity_revision().unwrap();
    assert_ne!(spawned, direct);
    for name in world.ecs_mut().query_mut::<Option<&mut Name>>() {
        name.unwrap().value = "bulk".into();
    }
    let bulk = world.identity_revision().unwrap();
    assert_ne!(direct, bulk);
    world.ecs_mut().remove::<Name>(e).unwrap();
    let removed = world.identity_revision().unwrap();
    assert_ne!(bulk, removed);
    world.ecs_mut().insert(e, Name::new("restored")).unwrap();
    assert_ne!(world.identity_revision(), Some(removed));
    let before_abort = world.identity_revision();
    let failed = world.edit_gameplay(|edit| {
        edit.set_name(e, "discard")?;
        edit.destroy(fynkel_core::Entity::NULL)
    });
    assert!(failed.is_err());
    assert_eq!(world.identity_revision(), before_abort);
    world.ecs_mut().despawn(e).unwrap();
    assert_ne!(world.identity_revision(), before_abort);
}

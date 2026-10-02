use fynkel_ecs::{EntityBuilder, World};

#[test]
fn cache_stamps_distinguish_values_structure_and_entity_membership() {
    let mut world = World::new();
    let empty = world.change_stamp().unwrap();
    let entity = world.spawn(EntityBuilder::new().with(1_i32));
    let spawned = world.change_stamp().unwrap();
    assert!(!empty.same_entities(spawned));
    assert_eq!(*world.get::<i32>(entity).unwrap(), 1);
    assert_eq!(world.query::<&i32>().iter().copied().sum::<i32>(), 1);
    assert_eq!(world.change_stamp(), Some(spawned));

    world.insert(entity, 2_i32).unwrap();
    let replaced = world.change_stamp().unwrap();
    assert_ne!(spawned, replaced);
    assert!(spawned.same_structure(replaced));

    world.insert(entity, true).unwrap();
    let inserted = world.change_stamp().unwrap();
    assert!(replaced.same_entities(inserted));
    assert!(!replaced.same_structure(inserted));
    world.remove::<bool>(entity).unwrap();
    let removed = world.change_stamp().unwrap();
    assert!(!removed.same_structure(inserted));
    assert!(removed.same_entities(inserted));

    world.despawn(entity).unwrap();
    let destroyed = world.change_stamp().unwrap();
    assert!(!removed.same_entities(destroyed));
    let reused = world.spawn_empty();
    assert_eq!(reused.slot(), entity.slot());
    assert_ne!(reused.generation(), entity.generation());
    assert!(!destroyed.same_entities(world.change_stamp().unwrap()));
    world.clear();
    assert!(!empty.same_entities(world.change_stamp().unwrap()));
    let cleared = world.change_stamp();
    world.clear();
    assert_eq!(cleared, world.change_stamp());
}

#[test]
fn outstanding_write_guards_prevent_premature_cache_validation() {
    let mut world = World::new();
    let entity = world.spawn(EntityBuilder::new().with(1_i32).with(false));
    let before = world.change_stamp().unwrap();
    let mut number = world.get_mut::<i32>(entity).unwrap();
    let mut flag = world.get_mut::<bool>(entity).unwrap();
    assert_eq!(world.change_stamp(), None);
    *number = 4;
    drop(number);
    assert_eq!(world.change_stamp(), None);
    *flag = true;
    drop(flag);
    let after = world.change_stamp().unwrap();
    assert_ne!(before, after);
    assert!(before.same_structure(after));
    assert_eq!(world.copy::<i32>(entity).unwrap(), 4);
    assert!(world.copy::<bool>(entity).unwrap());
}

#[test]
fn bulk_queries_invalidate_values_and_keep_membership() {
    let mut world = World::new();
    let e = world.spawn(EntityBuilder::new().with(1_i32));
    let before = world.change_stamp().unwrap();
    for number in world.query_mut::<&mut i32>() {
        *number += 2;
    }
    let after = world.change_stamp().unwrap();
    assert_ne!(before, after);
    assert!(before.same_structure(after));
    assert_eq!(world.copy::<i32>(e).unwrap(), 3);
    // Empty/no-op write leases may invalidate conservatively.
    {
        let _empty = world.query_mut::<&mut bool>();
    }
    assert_ne!(Some(after), world.change_stamp());
}

#[test]
fn failed_access_and_queued_destruction_do_not_report_committed_changes() {
    let mut world = World::new();
    let entity = world.spawn_empty();
    let before = world.change_stamp();
    assert!(world.get_mut::<i32>(entity).is_err());
    assert!(world.remove::<i32>(entity).is_err());
    world.despawn_deferred(entity);
    world.despawn_deferred(entity);
    assert_eq!(before, world.change_stamp());
    assert_eq!(world.flush_deferred(), vec![entity]);
    let after = world.change_stamp();
    assert_ne!(before, after);
    assert!(world.insert(entity, 1_i32).is_err());
    assert!(world.despawn(entity).is_err());
    assert!(world.get_mut::<i32>(entity).is_err());
    assert_eq!(after, world.change_stamp());
}

#[test]
fn stamps_cannot_alias_across_worlds() {
    let a = World::new().change_stamp().unwrap();
    let b = World::new().change_stamp().unwrap();
    assert_ne!(a, b);
    assert!(!a.same_entities(b));
    assert!(!a.same_structure(b));
}

#[test]
fn unwinding_releases_write_guard_and_invalidates_old_stamp() {
    let mut world = World::new();
    let entity = world.spawn(EntityBuilder::new().with(1_i32));
    let before = world.change_stamp().unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut number = world.get_mut::<i32>(entity).unwrap();
        *number = 9;
        panic!("system failed after writing");
    }));
    assert!(panic.is_err());
    assert_ne!(before, world.change_stamp().unwrap());
    assert_eq!(world.copy::<i32>(entity).unwrap(), 9);
}

#[test]
fn conflicting_borrow_does_not_leave_an_unsettled_stamp() {
    let mut world = World::new();
    let entity = world.spawn(EntityBuilder::new().with(1_i32));
    let before = world.change_stamp();
    let read = world.get::<i32>(entity).unwrap();
    let conflict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _write = world.get_mut::<i32>(entity);
    }));
    assert!(conflict.is_err());
    assert_eq!(*read, 1);
    assert_eq!(before, world.change_stamp());
}

#[test]
fn writes_on_another_thread_cannot_publish_a_settled_stamp_early() {
    let mut world = World::new();
    let entity = world.spawn(EntityBuilder::new().with(1_i32));
    let before = world.change_stamp().unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let world = &world;
        let barrier = &barrier;
        scope.spawn(move || {
            let mut value = world.get_mut::<i32>(entity).unwrap();
            barrier.wait();
            barrier.wait();
            *value = 7;
        });
        barrier.wait();
        assert_eq!(world.change_stamp(), None);
        barrier.wait();
    });
    assert_ne!(before, world.change_stamp().unwrap());
    assert_eq!(world.copy::<i32>(entity).unwrap(), 7);
}

#[test]
fn component_stamps_cover_typed_writes_without_unrelated_invalidation() {
    let mut world = World::new();
    let entity = world.spawn(EntityBuilder::new().with(1_i32).with(false));
    let number = world.component_stamp::<i32>().unwrap();
    let flag = world.component_stamp::<bool>().unwrap();
    assert_ne!(number, flag);
    *world.get_mut::<bool>(entity).unwrap() = true;
    assert_eq!(world.component_stamp::<i32>(), Some(number));
    assert_ne!(world.component_stamp::<bool>(), Some(flag));
    world.insert(entity, 2_i32).unwrap();
    let replaced = world.component_stamp::<i32>().unwrap();
    assert_ne!(number, replaced);
    world.remove::<i32>(entity).unwrap();
    let removed = world.component_stamp::<i32>().unwrap();
    assert_ne!(replaced, removed);
    world.insert(entity, 3_i32).unwrap();
    let inserted = world.component_stamp::<i32>().unwrap();
    assert_ne!(removed, inserted);
    let guard = world.get_mut::<i32>(entity).unwrap();
    assert_eq!(world.component_stamp::<i32>(), None);
    drop(guard);
    assert_ne!(world.component_stamp::<i32>(), Some(inserted));
    let settled = world.component_stamp::<i32>().unwrap();
    let new = world.spawn_empty();
    assert_ne!(world.component_stamp::<i32>(), Some(settled));
    let spawned = world.component_stamp::<i32>().unwrap();
    world.despawn(new).unwrap();
    assert_ne!(world.component_stamp::<i32>(), Some(spawned));
}

#[test]
fn bulk_component_stamps_follow_write_borrows_including_optional_queries() {
    use fynkel_ecs::internal::hecs;
    let mut world = World::new();
    world.spawn(EntityBuilder::new().with(1_i32).with(false));
    let number = world.component_stamp::<i32>().unwrap();
    let flag = world.component_stamp::<bool>().unwrap();
    for (_, value) in world.query_mut::<(&i32, Option<&mut bool>)>() {
        *value.unwrap() = true;
    }
    assert_eq!(world.component_stamp::<i32>(), Some(number));
    assert_ne!(world.component_stamp::<bool>(), Some(flag));
    let flag = world.component_stamp::<bool>();
    // A mutable type mentioned only as a membership filter is not borrowed.
    for _ in world.query_mut::<hecs::With<&i32, &mut bool>>() {}
    assert_eq!(world.component_stamp::<i32>(), Some(number));
    assert_eq!(world.component_stamp::<bool>(), flag);
    for value in world.query_mut::<hecs::Without<&mut i32, &String>>() {
        *value = 9;
    }
    assert_ne!(world.component_stamp::<i32>(), Some(number));
    assert_eq!(world.component_stamp::<bool>(), flag);
}

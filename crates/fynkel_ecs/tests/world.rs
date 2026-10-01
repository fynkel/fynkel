//! §4.6 required tests: stale handles, slot reuse, world destruction,
//! add/remove churn, destruction during iteration, mapping integrity.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fynkel_contracts::{Entity, WorldId};
use fynkel_ecs::{EcsError, EntityBuilder, World};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos(f32, f32);
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vel(f32, f32);

#[test]
fn spawn_get_update_despawn() {
    let mut w = World::new();
    let e = w.spawn(EntityBuilder::new().with(Pos(1.0, 2.0)));
    assert!(w.contains(e));
    assert_eq!(*w.get::<Pos>(e).unwrap(), Pos(1.0, 2.0));
    w.get_mut::<Pos>(e).unwrap().0 = 5.0;
    assert_eq!(w.copy::<Pos>(e).unwrap(), Pos(5.0, 2.0));
    assert!(!w.has::<Vel>(e));
    w.insert(e, Vel(0.0, 1.0)).unwrap();
    assert!(w.has::<Vel>(e));
    assert_eq!(w.remove::<Vel>(e).unwrap(), Vel(0.0, 1.0));
    assert_eq!(w.remove::<Vel>(e), Err(EcsError::MissingComponent(e)));
    w.despawn(e).unwrap();
    assert!(!w.contains(e));
    assert!(w.is_empty());
}

#[test]
fn stale_handles_fail_and_slot_reuse_does_not_alias() {
    let mut w = World::new();
    let a = w.spawn(EntityBuilder::new().with(Pos(0.0, 0.0)));
    w.despawn(a).unwrap();
    let b = w.spawn(EntityBuilder::new().with(Pos(9.0, 9.0)));
    assert_eq!(a.slot(), b.slot(), "slot is recycled");
    assert_ne!(a.generation(), b.generation());
    assert!(!w.contains(a));
    assert_eq!(w.get::<Pos>(a).err(), Some(EcsError::StaleEntity(a)));
    assert_eq!(w.despawn(a), Err(EcsError::StaleEntity(a)));
    assert_eq!(*w.get::<Pos>(b).unwrap(), Pos(9.0, 9.0));
    assert!(!w.contains(Entity::NULL));
    // Wrong world.
    let other = Entity::new(WorldId(w.id().0 + 1), b.slot(), b.generation());
    assert!(!w.contains(other));
}

#[test]
fn deferred_despawn_during_iteration() {
    let mut w = World::new();
    let es: Vec<Entity> = (0..10)
        .map(|i| w.spawn(EntityBuilder::new().with(Pos(i as f32, 0.0))))
        .collect();
    for e in w.entities() {
        if w.get::<Pos>(e).unwrap().0 as i32 % 2 == 0 {
            w.despawn_deferred(e);
        }
    }
    w.despawn_deferred(es[0]); // duplicate: ignored at flush
    let gone = w.flush_deferred();
    assert_eq!(gone.len(), 5);
    assert_eq!(w.len(), 5);
    assert!(!w.contains(es[0]));
    assert!(w.contains(es[1]));
}

#[test]
fn churn_and_mapping_integrity() {
    let mut w = World::new();
    let mut live = Vec::new();
    for round in 0..50u32 {
        for i in 0..100 {
            live.push(w.spawn(EntityBuilder::new().with(Pos(round as f32, i as f32))));
        }
        for e in live.drain(..).step_by(2).collect::<Vec<_>>() {
            w.despawn(e).unwrap();
        }
        // remaining entities of this round are still valid and map back correctly
    }
    for e in w.entities() {
        let raw = w.raw(e).unwrap();
        assert_eq!(w.handle_of(raw), Some(e));
        assert_eq!(e.world(), w.id());
    }
    assert_eq!(w.len(), 50 * 50);
}

#[test]
fn teardown_drops_every_component() {
    struct Counted(Arc<AtomicUsize>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    {
        let mut w = World::new();
        for _ in 0..1000 {
            w.spawn(EntityBuilder::new().with(Counted(drops.clone())));
        }
        let some: Vec<Entity> = w.entities().take(10).collect();
        for e in some {
            w.despawn(e).unwrap();
        }
        assert_eq!(drops.load(Ordering::SeqCst), 10);
        w.clear();
        assert_eq!(drops.load(Ordering::SeqCst), 1000);
        assert!(w.is_empty());
    }
    assert_eq!(Arc::strong_count(&drops), 1, "nothing leaked");
}

#[test]
fn large_spawn_despawn_batches() {
    let mut w = World::new();
    let es: Vec<Entity> = (0..100_000).map(|_| w.spawn_empty()).collect();
    assert_eq!(w.len(), 100_000);
    for e in &es {
        w.despawn(*e).unwrap();
    }
    assert!(w.is_empty());
    let again = w.spawn_empty();
    assert!(es.iter().all(|e| !w.contains(*e)));
    assert!(w.contains(again));
}

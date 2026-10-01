//! The live Inspector crosses the same gameplay validation boundary as game code.
use fynkel_authoring::live;
use fynkel_contracts::ObjectRef;
use fynkel_core::*;
use fynkel_document::generated::encode_component;
use serde_json::json;

fn spawn(w: &mut World, name: &str) -> Entity {
    let commit = w
        .edit_gameplay(|edit| edit.spawn(name, Transform::default(), None))
        .unwrap();
    commit.resolve(commit.value).unwrap()
}

fn id(e: Entity) -> String {
    format!("runtime:{}", e.to_hex())
}

#[test]
fn inspector_write_observes_committed_gameplay_and_succeeds_paused() {
    let mut w = World::new(Default::default());
    let e = spawn(&mut w, "initial");
    w.edit_gameplay(|edit| edit.set_name(e, "gameplay"))
        .unwrap();
    let tick = w.tick();
    let result = live::call(&mut w, "rename", &json!({"id":id(e),"name":"inspector"})).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(w.ecs().copy::<Name>(e).unwrap().value, "inspector");
    assert_eq!(w.tick(), tick);
    w.step();
    assert_eq!(w.ecs().copy::<Name>(e).unwrap().value, "inspector");
}

#[test]
fn inspector_object_reference_uses_gameplay_liveness_validation() {
    let mut w = World::new(WorldConfig::default());
    let owner = spawn(&mut w, "owner");
    let stale = spawn(&mut w, "target");
    w.edit_gameplay(|edit| edit.destroy(stale)).unwrap();
    let value = ComponentValue::Follow(Follow {
        target: ObjectRef::to(stale),
        ..Follow::default()
    });
    let (kind, data) = encode_component(&value);
    let result = live::call(
        &mut w,
        "set",
        &json!({ "id": id(owner), "type": kind, "data": data.to_serde() }),
    )
    .unwrap();
    assert_eq!(result["ok"], false, "{result}");
    assert!(!w.ecs().has::<Follow>(owner));
    assert!(w
        .diagnostics()
        .last()
        .unwrap()
        .message
        .contains("StaleEntity"));
}

#[test]
fn inspector_spawn_rejects_stale_component_reference_without_partial_object() {
    let mut w = World::new(WorldConfig::default());
    let parent = spawn(&mut w, "parent");
    let stale = spawn(&mut w, "target");
    w.edit_gameplay(|edit| edit.destroy(stale)).unwrap();
    w.drain_events();
    let (kind, data) = encode_component(&ComponentValue::Follow(Follow {
        target: ObjectRef::to(stale),
        ..Follow::default()
    }));
    let tick = w.tick();
    let result = live::call(
        &mut w,
        "spawn",
        &json!({
            "name": "partial", "parent": id(parent), "index": 0,
            "components": [{ "type": kind, "data": data.to_serde() }]
        }),
    );
    assert!(result.is_err(), "{result:?}");
    assert_eq!(w.live_entities(), 1);
    assert!(fynkel_scene::children_of(w.ecs(), parent).is_empty());
    assert!(w.drain_events().is_empty());
    assert_eq!(w.tick(), tick);
    let valid = live::call(
        &mut w,
        "spawn",
        &json!({"name": "valid", "parent": id(parent)}),
    )
    .unwrap();
    assert_eq!(valid["ok"], true);
    assert_eq!(w.live_entities(), 2);
}

#[test]
fn inspector_spawn_preserves_preceding_gameplay_commit_without_a_tick() {
    let mut w = World::new(Default::default());
    let parent = spawn(&mut w, "original");
    w.edit_gameplay(|edit| edit.set_name(parent, "latest"))
        .unwrap();
    let tick = w.tick();
    assert_eq!(
        live::call(&mut w, "spawn", &json!({"name":"editor"})).unwrap()["ok"],
        true
    );
    assert_eq!(w.live_entities(), 2);
    assert_eq!(w.ecs().copy::<Name>(parent).unwrap().value, "latest");
    assert_eq!(w.tick(), tick);
}

//! Live Play mode's calls (ADR-0021 P7 L1): a running world read and
//! changed in the scene document's terms — authoring ids for what a scene
//! loaded, `runtime:` ids for what the game made, components as document
//! JSON, writes applied at once without a tick.

use fynkel_authoring::live::{self, RUNTIME_PREFIX};
use fynkel_core::components::{Camera, ComponentValue, PointLight};
use fynkel_core::*;
use glam::Vec3;
use serde_json::{json, Value};

fn id(n: u8) -> Uuid {
    let mut b = [0u8; 16];
    b[15] = n;
    b[6] = 0x40;
    b[8] = 0x80;
    Uuid(b)
}

fn entity(
    n: u8,
    parent: Option<u8>,
    children: &[u8],
    components: Vec<ComponentValue>,
) -> EntityDescription {
    EntityDescription {
        registered: Vec::new(),
        id: id(n),
        name: format!("e{n}"),
        active: true,
        parent: parent.map(id),
        children: children.iter().map(|c| id(*c)).collect(),
        components,
    }
}

fn at(x: f32) -> ComponentValue {
    ComponentValue::Transform(Transform::from_position(Vec3::new(x, 0.0, 0.0)))
}

fn scene() -> SceneDescription {
    SceneDescription {
        id: id(99),
        name: "Test".into(),
        entities: vec![
            entity(1, None, &[3, 2], vec![at(10.0)]),
            entity(
                2,
                Some(1),
                &[],
                vec![at(1.0), ComponentValue::Camera(Camera::default())],
            ),
            entity(
                3,
                Some(1),
                &[],
                vec![at(0.0), ComponentValue::PointLight(PointLight::default())],
            ),
            entity(4, None, &[], vec![at(-1.0)]),
        ],
    }
}

fn running() -> World {
    let mut w = World::new(WorldConfig::default());
    w.load_scene(&scene()).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w
}

fn call(w: &mut World, kind: &str, args: Value) -> Value {
    live::call(w, kind, &args).unwrap_or_else(|d| panic!("{kind}: {d:?}"))
}

fn uid(n: u8) -> String {
    uuid::Uuid::from_bytes(id(n).0).to_string()
}

#[test]
fn entities_carry_authoring_ids_and_the_games_own_are_runtime() {
    let mut w = running();
    let prefab = SceneDescription {
        id: id(50),
        name: "Coin".into(),
        entities: vec![
            entity(51, None, &[52], vec![at(0.0)]),
            entity(52, Some(51), &[], vec![at(1.0)]),
        ],
    };
    w.instantiate(&prefab, "Coin", Transform::default(), None)
        .unwrap();
    w.edit_gameplay(|edit| edit.spawn("Bullet", Transform::default(), None))
        .unwrap();
    assert_eq!(w.step().status, Status::Ok);

    let table = call(&mut w, "entities", json!({}));
    let rows = table["entities"].as_array().unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(
        &ids[..4],
        [uid(1), uid(3), uid(2), uid(4)],
        "document order, parents first"
    );
    assert_eq!(rows[1]["parent"], json!(uid(1)));
    assert_eq!(rows[0]["children"], json!(2));
    assert_eq!(rows[0]["spawned"], json!(false));
    assert_eq!(
        rows[2]["types"],
        json!(["fynkel.Transform", "fynkel.Camera"])
    );
    let e2 = rows[2]["entity"].as_str().unwrap();
    assert_eq!(
        live::id_of(&w, Entity::from_hex(e2).unwrap()),
        uid(2),
        "the handle is the entity's"
    );
    let names: Vec<&str> = rows[4..]
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Coin", "e52", "Bullet"],
        "an instance (the prefab's root, renamed), its child, a spawn"
    );
    for r in &rows[4..] {
        assert!(r["id"].as_str().unwrap().starts_with(RUNTIME_PREFIX), "{r}");
        assert_eq!(r["spawned"], json!(true));
        // A runtime id resolves back to its entity.
        let one = call(&mut w, "entity", json!({ "id": r["id"] }));
        assert_eq!(one["name"], r["name"]);
    }
}

#[test]
fn an_unchanged_table_is_not_sent_again() {
    let mut w = running();
    let first = call(&mut w, "entities", json!({}));
    let revision = first["revision"].as_str().unwrap().to_string();
    assert_eq!(first["entities"].as_array().unwrap().len(), 4);

    // Values moving (a transform) is not a change of the table.
    assert!(call(
        &mut w,
        "set",
        json!({ "id": uid(4), "type": "fynkel.Transform",
        "data": { "position": [5, 0, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } })
    )["ok"]
        .as_bool()
        .unwrap());
    assert_eq!(w.step().status, Status::Ok);
    let same = call(&mut w, "entities", json!({ "since": revision }));
    assert_eq!(same["unchanged"], true);
    assert_eq!(same["revision"], revision.as_str());
    assert!(same.get("entities").is_none());

    // A rename, a new component, a spawn: each is.
    let light = call(&mut w, "entity", json!({ "id": uid(3) }))["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "fynkel.PointLight")
        .unwrap()["data"]
        .clone();
    let mut last = revision;
    for change in [
        json!(["rename", { "id": uid(4), "name": "Floor" }]),
        json!(["set", { "id": uid(4), "type": "fynkel.PointLight", "data": light }]),
        json!(["setActive", { "id": uid(4), "active": false }]),
    ] {
        assert!(
            call(&mut w, change[0].as_str().unwrap(), change[1].clone())["ok"]
                .as_bool()
                .unwrap()
        );
        let next = call(&mut w, "entities", json!({ "since": last }));
        assert!(next.get("unchanged").is_none(), "{change}");
        assert_eq!(next["entities"].as_array().unwrap().len(), 4);
        last = next["revision"].as_str().unwrap().to_string();
    }
    w.edit_gameplay(|edit| edit.spawn("Bullet", Transform::default(), None))
        .unwrap();
    assert_eq!(w.step().status, Status::Ok);
    let grown = call(&mut w, "entities", json!({ "since": last }));
    assert_eq!(grown["entities"].as_array().unwrap().len(), 5);
}

#[test]
fn components_read_as_the_document_and_write_back_unchanged() {
    let mut w = running();
    for n in 1..=4 {
        let before = call(&mut w, "entity", json!({ "id": uid(n) }));
        for c in before["components"].as_array().unwrap() {
            let r = call(
                &mut w,
                "set",
                json!({ "id": uid(n), "type": c["type"], "data": c["data"] }),
            );
            assert_eq!(r["ok"], json!(true), "{r}");
        }
        let after = call(&mut w, "entity", json!({ "id": uid(n) }));
        assert_eq!(before, after, "a round trip of e{n} changes nothing");
    }
    let camera = call(&mut w, "entity", json!({ "id": uid(2) }));
    let types: Vec<&str> = camera["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["fynkel.Transform", "fynkel.Camera"]);
}

#[test]
fn a_write_shows_at_once_without_a_tick() {
    let mut w = running();
    let tick = w.tick();
    let light = call(&mut w, "entity", json!({ "id": uid(3) }));
    let mut data = light["components"][1]["data"].clone();
    data["intensity"] = json!(42.0);
    assert_eq!(
        call(
            &mut w,
            "set",
            json!({ "id": uid(3), "type": "fynkel.PointLight", "data": data })
        )["ok"],
        json!(true)
    );
    let mut t = light["components"][0]["data"].clone();
    t["position"] = json!([0.0, 5.0, 0.0]);
    assert_eq!(
        call(
            &mut w,
            "set",
            json!({ "id": uid(3), "type": "fynkel.Transform", "data": t })
        )["ok"],
        json!(true)
    );
    assert_eq!(
        call(&mut w, "rename", json!({ "id": uid(3), "name": "Lamp" }))["ok"],
        json!(true)
    );

    let now = call(&mut w, "entity", json!({ "id": uid(3) }));
    assert_eq!(now["name"], json!("Lamp"));
    assert_eq!(now["activeSelf"], json!(true));
    // Deactivating the parent: the child's own flag stays, the hierarchy's
    // goes; a non-boolean is refused.
    let parent = now["parent"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &mut w,
            "setActive",
            json!({ "id": parent, "active": false })
        )["ok"],
        json!(true)
    );
    let off = call(&mut w, "entity", json!({ "id": uid(3) }));
    assert_eq!(off["activeSelf"], json!(true));
    assert_eq!(off["activeInHierarchy"], json!(false));
    assert!(live::call(&mut w, "setActive", &json!({ "id": parent, "active": 1 })).is_err());
    assert_eq!(
        call(&mut w, "setActive", json!({ "id": parent, "active": true }))["ok"],
        json!(true)
    );
    assert_eq!(now["components"][1]["data"]["intensity"], json!(42.0));
    assert_eq!(
        now["world"]["position"],
        json!([10.0, 5.0, 0.0]),
        "propagated under its parent"
    );
    assert_eq!(w.tick(), tick);

    assert_eq!(
        call(
            &mut w,
            "remove",
            json!({ "id": uid(3), "type": "fynkel.PointLight" })
        )["ok"],
        json!(true)
    );
    let now = call(&mut w, "entity", json!({ "id": uid(3) }));
    assert_eq!(now["components"].as_array().unwrap().len(), 1);
    assert_eq!(w.tick(), tick);
}

#[test]
fn entities_move_between_parents_and_go_at_once() {
    let mut w = running();
    let tick = w.tick();
    // e4 (a root at −1) under e1 (at 10), first among its children: its
    // local transform is kept, so it lands at 9.
    let r = call(
        &mut w,
        "parent",
        json!({ "id": uid(4), "parent": uid(1), "index": 0 }),
    );
    assert_eq!(r["ok"], json!(true), "{r}");
    let rows = call(&mut w, "entities", json!({}))["entities"].clone();
    let ids: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [uid(1), uid(4), uid(3), uid(2)]);
    let e4 = call(&mut w, "entity", json!({ "id": uid(4) }));
    assert_eq!(e4["parent"], json!(uid(1)));
    assert_eq!(e4["world"]["position"], json!([9.0, 0.0, 0.0]));

    // Back to the root; then a subtree destroyed.
    let r = call(&mut w, "parent", json!({ "id": uid(4), "parent": null }));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(
        call(&mut w, "entity", json!({ "id": uid(4) }))["parent"],
        Value::Null
    );
    let r = call(&mut w, "destroy", json!({ "id": uid(1) }));
    assert_eq!(r["ok"], json!(true), "{r}");
    let rows = call(&mut w, "entities", json!({}))["entities"].clone();
    assert_eq!(
        rows.as_array().unwrap().len(),
        1,
        "e1 went with its children: {rows}"
    );
    assert_eq!(w.tick(), tick, "none of it steps the world");

    // A cycle is the world's to refuse.
    let mut w = running();
    let r = call(&mut w, "parent", json!({ "id": uid(1), "parent": uid(2) }));
    assert_eq!(r["ok"], json!(false), "{r}");
}

#[test]
fn an_entity_made_live_is_there_at_once_under_its_authoring_id() {
    let mut w = running();
    let tick = w.tick();
    let r = call(
        &mut w,
        "spawn",
        json!({
            "id": uid(9),
            "name": "Lamp",
            "parent": uid(1),
            "index": 0,
            "components": [
                { "type": "fynkel.Transform", "data": { "position": [0.0, 2.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0] } },
                { "type": "fynkel.PointLight", "data": { "color": [1.0, 1.0, 1.0], "intensity": 4.0, "range": 5.0, "culling_mask": 1 } },
            ],
        }),
    );
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["id"], json!(uid(9)), "named by the id it was given");
    let lamp = call(&mut w, "entity", json!({ "id": uid(9) }));
    assert_eq!(lamp["name"], json!("Lamp"));
    assert_eq!(lamp["parent"], json!(uid(1)));
    assert_eq!(lamp["world"]["position"], json!([10.0, 2.0, 0.0]));
    let light = lamp["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "fynkel.PointLight")
        .unwrap();
    assert_eq!(light["data"]["intensity"], json!(4.0));
    let rows = call(&mut w, "entities", json!({}))["entities"].clone();
    let first_child = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["parent"] == json!(uid(1)))
        .unwrap();
    assert_eq!(first_child["id"], json!(uid(9)), "first among its siblings");
    assert_eq!(w.tick(), tick, "nothing steps");

    // Without an id it is the game's kind of entity; an id in use and a
    // component that is not native are refused.
    let r = call(&mut w, "spawn", json!({ "name": "Loose" }));
    assert!(r["id"].as_str().unwrap().starts_with(RUNTIME_PREFIX), "{r}");
    assert!(live::call(&mut w, "spawn", &json!({ "id": uid(9) })).is_err());
    assert!(live::call(
        &mut w,
        "spawn",
        &json!({ "components": [{ "type": "game.Spinner", "data": {} }] })
    )
    .is_err());
}

/// A Scene view camera at `x` on the X axis, 5 m back, looking down −Z.
fn view_at(x: f32) -> Value {
    let m = glam::Mat4::from_translation(Vec3::new(x, 0.0, 5.0));
    json!({ "world": m.to_cols_array(), "aspect": 1.0, "projection": "perspective" })
}

#[test]
fn the_scene_view_picks_and_outlines_the_running_world() {
    let mut w = running();
    w.edit_gameplay(|edit| {
        edit.spawn(
            "Bullet",
            Transform::from_position(Vec3::new(-5.0, 0.0, 0.0)),
            None,
        )
    })
    .unwrap();
    assert_eq!(w.step().status, Status::Ok);

    // No assets: every entity is an icon sphere at its origin.
    let hit = call(
        &mut w,
        "pick",
        json!({ "ray": { "origin": [-1, 0, 5], "dir": [0, 0, -1] } }),
    );
    let hits = hit["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "{hit}");
    assert_eq!(hits[0]["id"], json!(uid(4)));
    assert_eq!(hits[0]["mesh"], json!(false));
    let through = call(
        &mut w,
        "pick",
        json!({ "view": view_at(-1.0), "ndc": [0, 0] }),
    );
    // The same ray, from the near plane instead.
    assert_eq!(through["hits"][0]["id"], hits[0]["id"]);
    assert_eq!(through["hits"][0]["point"], hits[0]["point"]);
    let spawned = call(
        &mut w,
        "pick",
        json!({ "view": view_at(-5.0), "ndc": [0, 0] }),
    );
    let bullet = spawned["hits"][0]["id"].as_str().unwrap().to_string();
    assert!(bullet.starts_with(RUNTIME_PREFIX), "{spawned}");
    // Under its parent at 10: e3 sits at 10, e2 at 11.
    let nested = call(
        &mut w,
        "pick",
        json!({ "view": view_at(11.0), "ndc": [0, 0] }),
    );
    assert_eq!(nested["hits"][0]["id"], json!(uid(2)));

    let rect = call(
        &mut w,
        "pickRect",
        json!({ "view": view_at(-1.0), "min": [0.1, 0.1], "max": [-0.1, -0.1] }),
    );
    assert_eq!(rect["ids"], json!([uid(4)]));

    let matrices = call(
        &mut w,
        "worldMatrix",
        json!({ "ids": [uid(2), bullet, uid(77)] }),
    );
    let m = matrices["matrices"].as_object().unwrap();
    assert_eq!(m.len(), 2, "an id no longer alive is left out");
    assert_eq!(m[&uid(2)][12], json!(11.0));
    assert_eq!(m[&bullet][12], json!(-5.0));

    let outline = |w: &mut World, ids: Value| {
        call(w, "lines", json!({ "outline": { "ids": ids } }))["count"]
            .as_u64()
            .unwrap()
    };
    let one = outline(&mut w, json!([uid(4)]));
    assert!(one > 0);
    assert_eq!(outline(&mut w, json!([uid(4), bullet])), one * 2);
    assert_eq!(outline(&mut w, json!([uid(77)])), 0);
    // Without the boxes an entity that draws no mesh keeps its cross.
    let crosses = call(
        &mut w,
        "lines",
        json!({ "outline": { "ids": [uid(4)], "boxes": false } }),
    );
    assert_eq!(crosses["count"].as_u64().unwrap(), one);
    // The handles the renderer's outline takes: the runtime one is its
    // own bits, a gone one is left out.
    let handles = call(
        &mut w,
        "handles",
        json!({ "ids": [uid(4), bullet, uid(77)] }),
    );
    let bits: Vec<String> = handles["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b.as_str().unwrap().to_string())
        .collect();
    assert_eq!(bits.len(), 2, "{handles}");
    assert_eq!(
        bits[1].to_string(),
        bullet.strip_prefix(RUNTIME_PREFIX).unwrap()
    );
    assert_eq!(
        bits[0],
        live::resolve(&w, &uid(4)).unwrap().to_hex(),
        "{handles}"
    );
    // Icons: the camera's frustum and the light's range, for everyone.
    let icons = call(&mut w, "lines", json!({ "icons": { "aspect": 1.5 } }));
    assert!(icons["count"].as_u64().unwrap() > 0, "{icons}");
    assert_eq!(w.tick(), 2, "none of it steps the world");
}

#[test]
fn bad_calls_are_refused() {
    let mut w = running();
    let err = |w: &mut World, kind: &str, args: Value| {
        live::call(w, kind, &args).unwrap_err()[0].message.clone()
    };
    assert!(err(&mut w, "entity", json!({ "id": uid(77) })).contains("no live entity"));
    assert!(err(&mut w, "entity", json!({ "id": "runtime:12345" })).contains("no live entity"));
    assert!(err(
        &mut w,
        "set",
        json!({ "id": uid(1), "type": "game.Health", "data": {} })
    )
    .contains("not a native component"));
    assert!(err(
        &mut w,
        "set",
        json!({ "id": uid(3), "type": "fynkel.PointLight", "data": { "intensity": "bright" } })
    )
    .contains("fynkel.PointLight"));
    assert!(err(
        &mut w,
        "remove",
        json!({ "id": uid(1), "type": "fynkel.Transform" })
    )
    .contains("keeps its Transform"));
    assert!(err(&mut w, "fly", json!({})).contains("unknown live call"));
    assert!(err(&mut w, "pick", json!({})).contains("`ray` or `view`"));
    assert!(err(
        &mut w,
        "pick",
        json!({ "view": { "world": [1, 0], "aspect": 1 }, "ndc": [0, 0] })
    )
    .contains("view.world"));
    let mut near = view_at(0.0);
    near["near"] = json!(0.0);
    assert!(err(
        &mut w,
        "pickRect",
        json!({ "view": near, "min": [0, 0], "max": [1, 1] })
    )
    .contains("near"));
    assert!(err(&mut w, "worldMatrix", json!({ "ids": 3 })).contains("`ids`"));
    assert!(
        err(&mut w, "parent", json!({ "id": uid(4), "parent": uid(77) }))
            .contains("no live entity")
    );
    assert!(err(&mut w, "parent", json!({ "id": uid(4), "index": -1 })).contains("`index`"));
    // The world refuses what it would refuse from a game.
    let r = call(
        &mut w,
        "remove",
        json!({ "id": uid(1), "type": "fynkel.Camera" }),
    );
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(
        r["rejected"][0]["message"]
            .as_str()
            .unwrap()
            .contains("Camera"),
        "{r}"
    );
}

#[test]
fn registered_live_inspector_reads_writes_versions_and_atomic_creation() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../examples/registered_gameplay/schemas/registered/game.Health.schema.json"
    ))
    .unwrap();
    let mut w = World::new(Default::default());
    w.register_dynamic_component(
        fynkel_document::runtime::registered_schema(&schema["x-fynkel"]["registration"]).unwrap(),
    )
    .unwrap();
    let created=live::call(&mut w,"spawn",&json!({"name":"health","components":[{"type":"game.Health","version":1,"data":{"hp":40}}]})).unwrap();
    let owner = created["id"].as_str().unwrap();
    let data = live::call(&mut w, "entity", &json!({"id":owner})).unwrap();
    let health = data["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "game.Health")
        .unwrap();
    assert_eq!(health["version"], 1);
    assert_eq!(health["data"]["hp"], 40.0);
    live::call(
        &mut w,
        "set",
        &json!({"id":owner,"type":"game.Health","data":{"hp":20,"target":{"entity":owner}}}),
    )
    .unwrap();
    let e = live::resolve(&w, owner).unwrap();
    assert_eq!(
        w.registered_values(e, "game.Health").unwrap().unwrap()[0],
        ScalarValue::Float(20.)
    );
    assert!(live::call(
        &mut w,
        "set",
        &json!({"id":owner,"type":"game.Health","version":2,"data":{"hp":10}})
    )
    .is_err());
    let count = w.live_entities();
    assert!(live::call(
        &mut w,
        "spawn",
        &json!({"name":"discard","components":[{"type":"game.Health","data":{"hp":"bad"}}]})
    )
    .is_err());
    assert_eq!(w.live_entities(), count);
    // Both decoding failures and final gameplay validation roll back the batch.
    for bad in [
        json!({"id":owner,"type":"game.Health","data":{"hp":"bad"}}),
        json!({"id":owner,"type":"fynkel.PointLight","data":{"color":[1,1,1],"intensity":1,"range":-1,"culling_mask":4294967295u32}}),
    ] {
        let result = live::call(
            &mut w,
            "setComponents",
            &json!({"components":[
                {"id":owner,"type":"game.Health","data":{"hp":7}}, bad
            ]}),
        );
        assert!(result.is_err() || result.unwrap()["ok"] == false);
        assert_eq!(
            w.registered_values(e, "game.Health").unwrap().unwrap()[0],
            ScalarValue::Float(20.)
        );
        assert!(!w.ecs().has::<PointLight>(e));
    }
    live::call(
        &mut w,
        "setComponents",
        &json!({"components":[
            {"id":owner,"type":"game.Health","data":{"hp":9}},
            {"id":owner,"type":"fynkel.PointLight","data":{"color":[1,1,1],"intensity":1,"range":4,"culling_mask":4294967295u32}}
        ]}),
    )
    .unwrap();
    assert_eq!(
        w.registered_values(e, "game.Health").unwrap().unwrap()[0],
        ScalarValue::Float(9.)
    );
    assert_eq!(w.ecs().get::<PointLight>(e).unwrap().range, 4.);
    live::call(&mut w, "remove", &json!({"id":owner,"type":"game.Health"})).unwrap();
    assert!(w.registered_values(e, "game.Health").unwrap().is_none());
}

#[test]
fn delta_consumers_track_sparse_changes_order_and_reset_independently() {
    let mut w = running();
    let first = call(&mut w, "entities", json!({"deltas":true}));
    let cursor = first["revision"].clone();
    call(&mut w, "rename", json!({"id":uid(4),"name":"Changed"}));
    let renamed = call(&mut w, "entities", json!({"deltas":true,"since":cursor}));
    assert_eq!(renamed["changes"].as_array().unwrap().len(), 1);
    assert_eq!(renamed["changes"][0]["name"], "Changed");
    assert!(renamed.get("order").is_none());
    let second = call(&mut w, "entities", json!({"deltas":true,"since":cursor}));
    assert_eq!(
        renamed, second,
        "reading one consumer must not drain another"
    );
    call(
        &mut w,
        "parent",
        json!({"id":uid(4),"parent":uid(1),"index":0}),
    );
    let moved = call(
        &mut w,
        "entities",
        json!({"deltas":true,"since":renamed["revision"]}),
    );
    assert_eq!(moved["order"].as_array().unwrap().len(), 4);
    let handle = moved["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == uid(4))
        .unwrap()["entity"]
        .clone();
    call(&mut w, "setActive", json!({"id":uid(1),"active":false}));
    let inactive = call(
        &mut w,
        "entities",
        json!({"deltas":true,"since":moved["revision"]}),
    );
    assert!(inactive["changes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["activeInHierarchy"] == false));
    call(&mut w, "destroy", json!({"id":uid(4)}));
    let removed = call(
        &mut w,
        "entities",
        json!({"deltas":true,"since":inactive["revision"]}),
    );
    assert!(removed["removed"].as_array().unwrap().contains(&handle));
    assert_eq!(removed["order"].as_array().unwrap().len(), 3);
    let mut other = running();
    let reset = call(
        &mut other,
        "entities",
        json!({"deltas":true,"since":removed["revision"]}),
    );
    assert_eq!(reset["entities"].as_array().unwrap().len(), 4);
}

//! Document scenes in the headless host (blueprint §19.2c item 5): load a
//! `.scene.json` into the world and dump the runtime state with authoring
//! ids. `fynkel test --headless --scene <path>` and `fynkel state dump`.

use fynkel_contracts::Diagnostic;
use fynkel_core::{ComponentValue, LoadedScene, Name, World};
use fynkel_document::generated::encode_component;
use fynkel_document::{diagnostic_json, DocumentCodec};
use fynkel_scene as scene;
use fynkel_scene::Transform;
use serde_json::{json, Value};

pub use fynkel_document::read_scene;

pub fn diagnostics_json(diags: &[Diagnostic]) -> Vec<Value> {
    diags.iter().map(diagnostic_json).collect()
}

/// Runtime state keyed by authoring id (`fynkel state dump --json`). Loaded
/// entities come first in spawn order; entities spawned at runtime follow
/// with `"id": null` and their handle text.
pub fn dump_json(world: &World, loaded: &LoadedScene) -> Value {
    let mut entities = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (id, e) in loaded.entities() {
        seen.insert(*e);
        entities.push(entity_json(world, loaded, *e, Some(*id)));
    }
    let mut rest: Vec<_> = world
        .ecs()
        .entities()
        .filter(|e| !seen.contains(e))
        .collect();
    rest.sort_by_key(|e| e.to_bits());
    for e in rest {
        entities.push(entity_json(world, loaded, e, None));
    }
    json!({
        "scene": { "id": loaded.id.to_string(), "name": loaded.name },
        "tick": world.tick(),
        "entityCount": entities.len(),
        "entities": entities,
    })
}

fn entity_json(
    world: &World,
    loaded: &LoadedScene,
    e: fynkel_contracts::Entity,
    id: Option<fynkel_contracts::Uuid>,
) -> Value {
    let ecs = world.ecs();
    let name = ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default();
    let parent = scene::parent_of(ecs, e).map(|p| id_or_handle(loaded, p));
    let children: Vec<Value> = scene::children_of(ecs, e)
        .into_iter()
        .map(|c| id_or_handle(loaded, c))
        .collect();
    let local = ecs.copy::<Transform>(e).unwrap_or_default();
    let world_t = scene::world_matrix(ecs, e)
        .map(Transform::from_matrix)
        .unwrap_or_default();
    let components: Vec<Value> = ComponentValue::all_of(world, e)
        .iter()
        .map(|c| {
            let (type_id, data) = encode_component(c);
            json!({ "type": type_id, "data": data.to_serde() })
        })
        .collect();
    json!({
        "id": id.map(|u| u.to_string()),
        "handle": if id.is_some() { Value::Null } else { Value::String(handle_text(e)) },
        "name": name,
        "parent": parent,
        "children": children,
        "local": local.to_node().to_serde(),
        "world": world_t.to_node().to_serde(),
        "components": components,
    })
}

fn id_or_handle(loaded: &LoadedScene, e: fynkel_contracts::Entity) -> Value {
    match loaded.id_of(e) {
        Some(id) => Value::String(id.to_string()),
        None => Value::String(handle_text(e)),
    }
}

fn handle_text(e: fynkel_contracts::Entity) -> String {
    format!("handle:{:#x}", e.to_bits())
}

/// Human form of [`dump_json`]: the tree with world positions.
pub fn dump_text(world: &World, loaded: &LoadedScene) -> String {
    let mut out = format!(
        "scene \"{}\" ({}) tick {}: {} entities\n",
        loaded.name,
        loaded.id,
        world.tick(),
        world.live_entities()
    );
    let roots: Vec<_> = loaded
        .entities()
        .iter()
        .filter(|(_, e)| scene::parent_of(world.ecs(), *e).is_none())
        .map(|(_, e)| *e)
        .collect();
    for r in roots {
        dump_tree(world, loaded, r, 0, &mut out);
    }
    out
}

fn dump_tree(
    world: &World,
    loaded: &LoadedScene,
    e: fynkel_contracts::Entity,
    depth: usize,
    out: &mut String,
) {
    let ecs = world.ecs();
    let name = ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default();
    let p = scene::world_matrix(ecs, e)
        .map(|m| m.transform_point3(glam::Vec3::ZERO))
        .unwrap_or_default();
    let types: Vec<&str> = ComponentValue::all_of(world, e)
        .iter()
        .map(|c| c.type_name())
        .collect();
    let id = match loaded.id_of(e) {
        Some(id) => id.to_string(),
        None => handle_text(e),
    };
    out.push_str(&format!(
        "{}{} ({})  @ [{}, {}, {}]  [{}]\n",
        "  ".repeat(depth),
        name,
        id,
        p.x,
        p.y,
        p.z,
        types.join(", ")
    ));
    for c in scene::children_of(ecs, e) {
        dump_tree(world, loaded, c, depth + 1, out);
    }
}

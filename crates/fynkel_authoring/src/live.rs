//! Live Play mode (ADR-0021 P7, `docs/design/live-play-mode.md`): the
//! editor's Hierarchy and Inspector on the world a game is running, in the
//! scene document's own terms.
//!
//! - An entity is named by its **authoring id** when a loaded scene spawned
//!   it (a cooked prefab instance's children carry their derived ids), and
//!   by `runtime:<handle bits>` when the game spawned or instantiated it at
//!   run time.
//! - Components read and write as the document's JSON
//!   (`encode_component`/`decode_component`), so the Inspector shows a
//!   running entity exactly as it shows a scene's.
//! - A write applies at once, at the current tick, without stepping
//!   ([`World::apply_now`]): it shows while the game is paused, runs no
//!   system and is gone on Stop with the world. Nothing here touches a
//!   document.
//!
//! Every call is `(kind, args JSON) -> JSON`, like the session's `query`:
//! `entities`, `entity`, `set`, `remove`, `rename`.

use fynkel_contracts::{Code, ComponentId, Diagnostic, Entity};
use fynkel_core::components::ComponentValue;
use fynkel_core::{Command, Name, World};
use fynkel_document::generated::{decode_component, encode_component};
use fynkel_document::{DocumentCodec, Node};
use fynkel_scene::{self as scene, Layer, Tags, Transform};
use serde_json::{json, Value};
use uuid::Uuid;

/// The prefix of the id of an entity no loaded scene spawned.
pub const RUNTIME_PREFIX: &str = "runtime:";

pub const KINDS: [&str; 5] = ["entities", "entity", "set", "remove", "rename"];

/// Runs one live call on `world`. A refused write is `{ "ok": false,
/// "rejected": [...] }` with the world's own diagnostics; a malformed call
/// (unknown kind or entity, bad JSON) is an `Err`.
pub fn call(world: &mut World, kind: &str, args: &Value) -> Result<Value, Vec<Diagnostic>> {
    match kind {
        "entities" => Ok(entities(world)),
        "entity" => {
            let e = entity_arg(world, args)?;
            Ok(entity(world, e))
        }
        "set" => {
            let e = entity_arg(world, args)?;
            let type_id = str_arg(args, "type")?;
            let data = args
                .get("data")
                .ok_or_else(|| vec![invalid("set: `data` is required")])?;
            let value = match decode_component(type_id, &Node::from_serde(data)) {
                None => {
                    return Err(vec![invalid(format!(
                        "set: {type_id} is not a native component"
                    ))])
                }
                Some(Err(e)) => {
                    return Err(vec![invalid(format!(
                        "set: {type_id}{}: {}",
                        e.pointer, e.message
                    ))])
                }
                Some(Ok(v)) => v,
            };
            Ok(write(
                world,
                vec![Command::SetComponent { entity: e, value }],
            ))
        }
        "remove" => {
            let e = entity_arg(world, args)?;
            let type_id = str_arg(args, "type")?;
            let component = component_id(type_id).ok_or_else(|| {
                vec![invalid(format!(
                    "remove: {type_id} is not a native component"
                ))]
            })?;
            if component == ComponentId::TRANSFORM {
                return Err(vec![invalid("remove: an entity keeps its Transform")]);
            }
            Ok(write(
                world,
                vec![Command::RemoveComponent {
                    entity: e,
                    component,
                }],
            ))
        }
        "rename" => {
            let e = entity_arg(world, args)?;
            let name = str_arg(args, "name")?.to_string();
            Ok(write(world, vec![Command::SetName { entity: e, name }]))
        }
        _ => Err(vec![invalid(format!(
            "unknown live call `{kind}`; one of {}",
            KINDS.join(", ")
        ))]),
    }
}

/// The id the editor knows `entity` by.
pub fn id_of(world: &World, entity: Entity) -> String {
    match world.scene_of(entity).and_then(|s| s.id_of(entity)) {
        Some(id) => id.to_string(),
        None => format!("{RUNTIME_PREFIX}{}", entity.to_bits()),
    }
}

/// The live entity `id` names, if it is alive.
pub fn resolve(world: &World, id: &str) -> Option<Entity> {
    let e = if let Some(bits) = id.strip_prefix(RUNTIME_PREFIX) {
        Entity::from_bits(bits.parse().ok()?)
    } else {
        let id = fynkel_contracts::Uuid(*Uuid::parse_str(id).ok()?.as_bytes());
        world.loaded_scenes().iter().find_map(|s| s.handle_of(id))?
    };
    world.ecs().contains(e).then_some(e)
}

/// Every entity, parents before children: the loaded scenes' roots in
/// document order, then the rest by handle; children in sibling order.
fn entities(world: &World) -> Value {
    let ecs = world.ecs();
    let mut roots: Vec<Entity> = Vec::new();
    for s in world.loaded_scenes() {
        for &(_, e) in s.entities() {
            if ecs.contains(e) && scene::parent_of(ecs, e).is_none() && !roots.contains(&e) {
                roots.push(e);
            }
        }
    }
    let mut rest: Vec<Entity> = ecs
        .entities()
        .filter(|e| scene::parent_of(ecs, *e).is_none() && !roots.contains(e))
        .collect();
    rest.sort();
    roots.extend(rest);

    let mut out = Vec::new();
    let mut stack: Vec<(Entity, Option<String>)> =
        roots.into_iter().rev().map(|e| (e, None)).collect();
    while let Some((e, parent)) = stack.pop() {
        let id = id_of(world, e);
        let children = scene::children_of(ecs, e);
        out.push(json!({
            "id": id,
            "name": ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default(),
            "parent": parent,
            "spawned": id.starts_with(RUNTIME_PREFIX),
            "layer": ecs.copy::<Layer>(e).map(|l| l.layer).unwrap_or(0),
            "tags": ecs.get::<Tags>(e).map(|t| t.to_node().to_serde()).unwrap_or(Value::Null),
            "children": children.len(),
        }));
        for c in children.into_iter().rev() {
            stack.push((c, Some(id.clone())));
        }
    }
    json!({ "tick": world.tick(), "entities": out })
}

/// One entity as the Inspector reads it: name, parent, local and world
/// transforms, every native component as the document would store it.
fn entity(world: &World, e: Entity) -> Value {
    let ecs = world.ecs();
    let components: Vec<Value> = ComponentValue::all_of(world, e)
        .iter()
        .map(|c| {
            let (type_id, data) = encode_component(c);
            json!({ "type": type_id, "data": data.to_serde() })
        })
        .collect();
    let world_t = scene::world_matrix(ecs, e)
        .map(Transform::from_matrix)
        .unwrap_or_default();
    json!({
        "id": id_of(world, e),
        "name": ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default(),
        "parent": scene::parent_of(ecs, e).map(|p| id_of(world, p)),
        "world": world_t.to_node().to_serde(),
        "components": components,
    })
}

fn write(world: &mut World, commands: Vec<Command>) -> Value {
    let rejected = world.apply_now(commands);
    if rejected.is_empty() {
        return json!({ "ok": true });
    }
    let rejected: Vec<Value> = rejected
        .iter()
        .map(|d| json!({ "code": d.code.0, "message": d.message }))
        .collect();
    json!({ "ok": false, "rejected": rejected })
}

fn component_id(type_id: &str) -> Option<ComponentId> {
    (1..=u8::MAX as u32)
        .map(ComponentId)
        .find(|c| c.name() == Some(type_id))
}

fn entity_arg(world: &World, args: &Value) -> Result<Entity, Vec<Diagnostic>> {
    let id = str_arg(args, "id")?;
    resolve(world, id).ok_or_else(|| {
        vec![invalid(format!("no live entity {id}"))
            .with_id(id.to_string())
            .with_suggestion("it was destroyed, or the world was reloaded; read `entities` again")]
    })
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, Vec<Diagnostic>> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| vec![invalid(format!("`{key}` must be a string"))])
}

fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::SESSION_INVALID_REQUEST, message)
}

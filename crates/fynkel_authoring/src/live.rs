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
//!   ([`World::apply_gameplay_now`]): it shows while the game is paused, runs no
//!   system and is gone on Stop with the world. Nothing here touches a
//!   document.
//!
//! Every call is `(kind, args JSON) -> JSON`, like the session's `query`:
//! `entities`, `entity`, `set`, `remove`, `rename`, `setActive`, `parent`,
//! `destroy`, `spawn`, and for the Scene view
//! on the running world (L5) `pick`, `pickRect`, `lines`, `worldMatrix`
//! and `handles` — the edit preview's own picking and overlays
//! ([`crate::preview`]), over every live entity, and the handle bits the
//! renderer's selection outline takes (the preview's `entities`; the name
//! is this module's entity table's).

use fynkel_assets::Assets;
use fynkel_contracts::{Code, ComponentId, Diagnostic, Entity};
use fynkel_core::components::{Camera, ComponentValue, Projection};
use fynkel_core::{Command, Name, World};
use fynkel_document::generated::{decode_component, encode_component};
use fynkel_document::{DocumentCodec, Node};
use fynkel_scene::{self as scene, Layer, Tags, Transform};
use glam::{Mat4, Vec2, Vec3, Vec4};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::preview::{self, Line, Ray, View};

/// The prefix of the id of an entity no loaded scene spawned.
pub const RUNTIME_PREFIX: &str = "runtime:";

pub const KINDS: [&str; 17] = [
    "entities",
    "entity",
    "set",
    "setComponents",
    "remove",
    "rename",
    "setActive",
    "parent",
    "destroy",
    "spawn",
    "pick",
    "pickRect",
    "lines",
    "worldMatrix",
    "handles",
    "setMaterial",
    "setVolume",
];

/// The selection outline's colour when a `lines` call gives none (the
/// edit preview's).
const OUTLINE_COLOR: Vec4 = Vec4::new(1.0, 0.6, 0.0, 1.0);

/// Runs one live call on `world`. A refused write is `{ "ok": false,
/// "rejected": [...] }` with the world's own diagnostics; a malformed call
/// (unknown kind or entity, bad JSON) is an `Err`.
pub fn call(world: &mut World, kind: &str, args: &Value) -> Result<Value, Vec<Diagnostic>> {
    call_with(world, None, kind, args)
}

/// [`call`], with the assets the world draws from: picking and outlines
/// test meshes by their triangles and bounds (without, every entity is an
/// icon sphere).
pub fn call_with(
    world: &mut World,
    assets: Option<&Assets>,
    kind: &str,
    args: &Value,
) -> Result<Value, Vec<Diagnostic>> {
    match kind {
        "pick" => {
            let ray = if args.get("ndc").is_some() {
                view_arg(args)?.ray(vec2_arg(args, "ndc")?)
            } else {
                let ray = args
                    .get("ray")
                    .ok_or_else(|| vec![invalid("pick: needs `ray` or `view` + `ndc`")])?;
                Ray {
                    origin: vec3_arg(ray, "origin")?,
                    dir: vec3_arg(ray, "dir")?.normalize_or(Vec3::NEG_Z),
                }
            };
            Ok(pick(world, assets, &ray))
        }
        "pickRect" => {
            let view = view_arg(args)?;
            let (a, b) = (vec2_arg(args, "min")?, vec2_arg(args, "max")?);
            let (min, max) = (a.min(b), a.max(b));
            let ids: Vec<String> = pickable(world)
                .filter(|e| preview::rect_hits(world, assets, &view, *e, min, max))
                .map(|e| id_of(world, e))
                .collect();
            Ok(json!({ "ids": ids }))
        }
        "lines" => {
            let mut out: Vec<Line> = Vec::new();
            let mut selected: Vec<Entity> = Vec::new();
            if let Some(o) = args.get("outline").filter(|o| !o.is_null()) {
                let color = match o.get("color").filter(|c| !c.is_null()) {
                    Some(c) => {
                        let f = floats(c, "outline.color", 4)?;
                        Vec4::new(f[0], f[1], f[2], f[3])
                    }
                    None => OUTLINE_COLOR,
                };
                // `boxes: false`: the meshes are ringed by the renderer's
                // silhouette outline, only the crosses are lines.
                let boxes = o.get("boxes").and_then(Value::as_bool).unwrap_or(true);
                // An entity gone since the selection was made draws nothing.
                selected = ids_arg(o)?
                    .iter()
                    .filter_map(|id| resolve(world, id))
                    .collect();
                for e in &selected {
                    preview::outline_entity(world, assets, *e, color, boxes, &mut out);
                }
            }
            if let Some(i) = args.get("icons").filter(|i| !i.is_null()) {
                let aspect = f32_arg(i, "aspect")?;
                for e in pickable(world) {
                    let is_selected = selected.contains(&e);
                    preview::icon_entity(world, assets, aspect, e, is_selected, &mut out);
                }
            }
            Ok(json!({
                "count": out.len(),
                "lines": out.iter().map(Line::json).collect::<Vec<_>>(),
            }))
        }
        "handles" => {
            // What the renderer's selection outline takes: the live
            // handles' bits, the gone and the hidden left out.
            let hidden: Vec<Entity> = world.edit_hidden().collect();
            let mut out: Vec<String> = Vec::new();
            for id in ids_arg(args)? {
                if let Some(e) = resolve(world, &id).filter(|e| !hidden.contains(e)) {
                    if !out.contains(&e.to_hex()) {
                        out.push(e.to_hex());
                    }
                }
            }
            Ok(json!({ "entities": out }))
        }
        "worldMatrix" => {
            let mut matrices = serde_json::Map::new();
            for id in ids_arg(args)? {
                let m = resolve(world, &id).and_then(|e| scene::world_matrix(world.ecs(), e).ok());
                if let Some(m) = m {
                    matrices.insert(id, json!(m.to_cols_array()));
                }
            }
            Ok(json!({ "matrices": matrices }))
        }
        "entities" => Ok(
            if args.get("deltas").and_then(Value::as_bool) == Some(true) {
                entities_delta(world, args.get("since").and_then(Value::as_str))
            } else {
                entities(world, args.get("since").and_then(Value::as_str))
            },
        ),
        "entity" => {
            let e = entity_arg(world, args)?;
            Ok(entity(world, e))
        }
        "set" => set_components(world, std::slice::from_ref(args)),
        "setComponents" => {
            let values = args
                .get("components")
                .and_then(Value::as_array)
                .ok_or_else(|| vec![invalid("setComponents: components must be an array")])?;
            set_components(world, values)
        }
        "remove" => {
            let e = entity_arg(world, args)?;
            let type_id = str_arg(args, "type")?;
            if world.registered_schemas().any(|s| s.id == type_id) {
                world
                    .edit_gameplay(|edit| edit.remove_registered(e, type_id))
                    .map_err(|e| vec![invalid(e.to_string())])?;
                return Ok(json!({"ok":true}));
            }
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
        "setActive" => {
            let e = entity_arg(world, args)?;
            let active = args
                .get("active")
                .and_then(Value::as_bool)
                .ok_or_else(|| vec![invalid("setActive: `active` must be a boolean")])?;
            Ok(write(world, vec![Command::SetActive { entity: e, active }]))
        }
        "parent" => {
            // The local transform is kept (the editor rewrites it when the
            // world pose should stay); `index` places it among its new
            // siblings, last when absent. A root keeps the world's order.
            let e = entity_arg(world, args)?;
            let parent = parent_arg(world, args)?;
            let index = index_arg(args)?;
            let r = write(
                world,
                vec![Command::SetParent {
                    child: e,
                    parent,
                    keep_world: false,
                }],
            );
            if let (Some(i), Some(true)) = (index, r["ok"].as_bool()) {
                let _ = scene::set_sibling_index(world.ecs_mut(), e, i);
            }
            Ok(r)
        }
        "destroy" => {
            let e = entity_arg(world, args)?;
            Ok(write(world, vec![Command::Destroy(e)]))
        }
        "spawn" => {
            // An atomic new entity through the gameplay journal (`World::spawn_now`): `name`,
            // `parent` (null: a root) at `index` (last when absent), the
            // native `components` as `[{type, data}]` (a Transform among
            // them is its local transform). With an `id` it joins a loaded
            // scene under that authoring id — `scene`'s, else its
            // parent's, else the first loaded one — and is named by it.
            let name = args.get("name").and_then(Value::as_str).unwrap_or("");
            let parent = parent_arg(world, args)?;
            let index = index_arg(args)?;
            let mut transform = Transform::IDENTITY;
            let mut components = Vec::new();
            let mut registered = Vec::new();
            for c in args
                .get("components")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let type_id = str_arg(c, "type")?;
                let data = c.get("data").cloned().unwrap_or_else(|| json!({}));
                if let Some(value) = registered_from_live(
                    world,
                    type_id,
                    &data,
                    c.get("version").and_then(Value::as_u64),
                )? {
                    registered.push(value);
                    continue;
                }
                match decode_component(type_id, &Node::from_serde(&data)) {
                    None => {
                        return Err(vec![invalid(format!(
                            "spawn: {type_id} is not a native component"
                        ))])
                    }
                    Some(Err(e)) => {
                        return Err(vec![invalid(format!(
                            "spawn: {type_id}{}: {}",
                            e.pointer, e.message
                        ))])
                    }
                    Some(Ok(ComponentValue::Transform(t))) => transform = t,
                    Some(Ok(v)) => components.push(v),
                }
            }
            let authoring = match args.get("id") {
                None | Some(Value::Null) => None,
                Some(Value::String(id)) => {
                    let id = uuid_of(id).ok_or_else(|| vec![invalid("`id` must be a UUID")])?;
                    let scene = match args.get("scene").and_then(Value::as_str) {
                        Some(s) => Some(
                            uuid_of(s).ok_or_else(|| vec![invalid("`scene` must be a UUID")])?,
                        ),
                        None => parent
                            .and_then(|p| world.scene_of(p))
                            .or_else(|| world.loaded_scenes().first())
                            .map(|s| s.id),
                    };
                    scene.map(|scene| (scene, id))
                }
                Some(_) => return Err(vec![invalid("`id` must be a UUID string")]),
            };
            let (e, rejected) = world
                .spawn_now_registered(name, transform, parent, components, registered, authoring)
                .map_err(|e| vec![invalid(format!("spawn: {e}"))])?;
            if let Some(i) = index {
                let _ = scene::set_sibling_index(world.ecs_mut(), e, i);
            }
            let mut r = rejections(&rejected);
            r["id"] = json!(id_of(world, e));
            Ok(r)
        }
        "setMaterial" | "setVolume" => {
            // An asset document edited in the Inspector during Play (saved
            // at once, as in Unity): `data` (its fields) drawn for the
            // asset `id` by the running game from its next frame, as the
            // Scene view's edit preview draws it
            // ([`preview::Preview::set_material`] / `set_volume`). It lasts
            // the run; the next Play cooks the saved file.
            let assets =
                assets.ok_or_else(|| vec![invalid(format!("{kind}: this world has no assets"))])?;
            let id = str_arg(args, "id")?;
            let id = Uuid::parse_str(id)
                .map_err(|_| vec![invalid(format!("{kind}: `id` must be a UUID"))])?;
            let data = args
                .get("data")
                .ok_or_else(|| vec![invalid(format!("{kind}: `data` is required"))])?;
            let r = if kind == "setMaterial" {
                preview::Preview::set_material(assets, id, data)
            } else {
                preview::Preview::set_volume(assets, id, data)
            };
            r.map_err(|d| vec![d])?;
            Ok(json!({ "ok": true }))
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
        None => format!("{RUNTIME_PREFIX}{}", entity.to_hex()),
    }
}

/// The live entity `id` names, if it is alive.
pub fn resolve(world: &World, id: &str) -> Option<Entity> {
    let e = if let Some(bits) = id.strip_prefix(RUNTIME_PREFIX) {
        Entity::from_hex(bits)?
    } else {
        let id = fynkel_contracts::Uuid(*Uuid::parse_str(id).ok()?.as_bytes());
        world.loaded_scenes().iter().find_map(|s| s.handle_of(id))?
    };
    world.ecs().contains(e).then_some(e)
}

/// Every entity, parents before children: the loaded scenes' roots in
/// document order, then the rest by handle, then the persistent roots
/// (ADR-0037, `persistent: true`); children in sibling order.
///
/// Stable revision check precedes traversal/encoding; ordinary value changes
/// leave this table unchanged.
fn entities(world: &World, since: Option<&str>) -> Value {
    let revision = format!("{}:{}", world.id().0, world.entity_table_revision());
    if since == Some(revision.as_str()) {
        return json!({ "tick": world.tick(), "revision": revision, "unchanged": true });
    }
    let persistent = world.persistent_roots();
    let rows: Vec<_> = ordered_entities(world)
        .into_iter()
        .map(|e| entity_row(world, e, &persistent))
        .collect();
    json!({ "tick": world.tick(), "revision": revision, "entities": rows })
}

fn ordered_entities(world: &World) -> Vec<Entity> {
    let ecs = world.ecs();
    let persistent = world.persistent_roots();
    let mut seen: std::collections::HashSet<Entity> = persistent.iter().copied().collect();
    let mut roots = Vec::new();
    for s in world.loaded_scenes() {
        for &(_, e) in s.entities() {
            if ecs.contains(e) && scene::parent_of(ecs, e).is_none() && seen.insert(e) {
                roots.push(e);
            }
        }
    }
    let mut rest: Vec<_> = ecs
        .entities()
        .filter(|e| scene::parent_of(ecs, *e).is_none() && seen.insert(*e))
        .collect();
    rest.sort_unstable();
    roots.extend(rest);
    roots.extend(persistent);
    let mut stack: Vec<_> = roots.into_iter().rev().collect();
    let mut out = Vec::new();
    while let Some(e) = stack.pop() {
        out.push(e);
        stack.extend(scene::children_of(ecs, e).into_iter().rev());
    }
    out
}

fn entity_row(world: &World, e: Entity, persistent: &[Entity]) -> Value {
    let ecs = world.ecs();
    let id = id_of(world, e);
    let types: Vec<_> = ComponentValue::all_of(world, e)
        .iter()
        .filter_map(|c| c.id().name().map(str::to_string))
        .chain(
            world
                .registered_schemas()
                .filter(|s| world.registered_values(e, &s.id).ok().flatten().is_some())
                .map(|s| s.id.clone()),
        )
        .collect();
    json!({
        "id": id, "entity": e.to_hex(),
        "name": ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default(),
        "parent": scene::parent_of(ecs, e).map(|p| id_of(world, p)),
        "activeSelf": scene::is_active_self(ecs, e), "activeInHierarchy": scene::is_active_in_hierarchy(ecs, e),
        "spawned": id.starts_with(RUNTIME_PREFIX), "layer": ecs.copy::<Layer>(e).map_or(0, |l| l.layer),
        "tags": ecs.copy::<Tags>(e).ok().map(|t| t.to_node().to_serde()),
        "children": scene::children_of(ecs, e).len(), "types": types, "persistent": persistent.contains(&e),
    })
}

fn entities_delta(world: &World, since: Option<&str>) -> Value {
    let metadata = format!(
        "{}:{}:{}",
        world.id().0,
        world.scene_metadata_revision(),
        world.registered_schemas().count()
    );
    let token = since
        .and_then(|s| s.rsplit_once('/'))
        .filter(|(m, _)| *m == metadata)
        .and_then(|(_, token)| token.parse::<u64>().ok())
        .unwrap_or(0);
    let changes = world
        .entity_table_changes_since(token)
        .expect("settled live table");
    let revision = format!("{metadata}/{}", changes.cursor.token());
    if changes.reset {
        let mut full = entities(world, None);
        full["revision"] = json!(revision);
        return full;
    }
    if changes.entities.is_empty() {
        return json!({ "tick": world.tick(), "revision": revision, "unchanged": true });
    }
    let persistent = world.persistent_roots();
    let mut rows = Vec::new();
    let mut removed = Vec::new();
    for e in changes.entities {
        if world.ecs().contains(e) {
            rows.push(entity_row(world, e, &persistent));
        } else {
            removed.push(e.to_hex());
        }
    }
    let order_changes = world
        .hierarchy_changes_since(token)
        .expect("settled live table");
    let mut result =
        json!({ "tick": world.tick(), "revision": revision, "changes": rows, "removed": removed });
    if order_changes.reset || !order_changes.entities.is_empty() {
        result["order"] = json!(ordered_entities(world)
            .into_iter()
            .map(|e| e.to_hex())
            .collect::<Vec<_>>());
    }
    result
}

/// One entity as the Inspector reads it: name, parent, active flags, local and world
/// transforms, every native component as the document would store it.
fn entity(world: &World, e: Entity) -> Value {
    let ecs = world.ecs();
    let mut components: Vec<Value> = ComponentValue::all_of(world, e)
        .iter()
        .map(|c| {
            let (type_id, data) = encode_component(c);
            json!({ "type": type_id, "data": data.to_serde() })
        })
        .collect();
    for schema in world.registered_schemas() {
        if let Ok(Some(fields)) = world.registered_values(e, &schema.id) {
            let data: serde_json::Map<String, Value> = schema
                .fields
                .iter()
                .zip(fields)
                .map(|(field, value)| {
                    use fynkel_core::ScalarValue as V;
                    let value = match value {
                        V::Float(v) => json!(v),
                        V::Int(v) => json!(v),
                        V::Bool(v) => json!(v),
                        V::Text(v) => json!(v),
                        V::Asset(v) => {
                            if v.sub.is_empty() {
                                json!({"asset":v.asset.to_string()})
                            } else {
                                json!({"asset":v.asset.to_string(),"sub":v.sub})
                            }
                        }
                        V::Object(v) => v
                            .and_then(|t| t.live())
                            .map(|target| json!({"entity":id_of(world,target)}))
                            .unwrap_or(Value::Null),
                    };
                    (field.name.clone(), value)
                })
                .collect();
            components.push(json!({"type":schema.id,"version":schema.version,"data":data}));
        }
    }
    let world_t = scene::world_matrix(ecs, e)
        .map(Transform::from_matrix)
        .unwrap_or_default();
    json!({
        "id": id_of(world, e),
        "entity": e.to_hex(),
        "name": ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default(),
        "parent": scene::parent_of(ecs, e).map(|p| id_of(world, p)),
        "activeSelf": scene::is_active_self(ecs, e),
        "activeInHierarchy": scene::is_active_in_hierarchy(ecs, e),
        "world": world_t.to_node().to_serde(),
        "components": components,
    })
}

/// The entities a click or a marquee can meet: every live one active in
/// the hierarchy (a game's world hides nothing else from the editor; only
/// an edit world can).
fn pickable(world: &World) -> impl Iterator<Item = Entity> + '_ {
    world
        .ecs()
        .entities()
        .filter(|e| scene::is_active_in_hierarchy(world.ecs(), *e))
}

/// Every entity `ray` meets, nearest first, as the edit preview picks
/// (a mesh by its triangles, anything else by an icon sphere) — each
/// entity itself: a running prefab instance has no anchor to stand for it.
fn pick(world: &World, assets: Option<&Assets>, ray: &Ray) -> Value {
    let mut hits: Vec<(Entity, f32, Vec3, bool)> = pickable(world)
        .filter_map(|e| {
            preview::pick_entity(world, assets, ray, e).map(|(d, p, mesh)| (e, d, p, mesh))
        })
        .collect();
    hits.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let hits: Vec<Value> = hits
        .into_iter()
        .map(|(e, distance, point, mesh)| {
            json!({
                "id": id_of(world, e),
                "entity": e.to_hex(),
                "distance": distance,
                "point": point.to_array(),
                "mesh": mesh,
            })
        })
        .collect();
    json!({
        "ray": { "origin": ray.origin.to_array(), "dir": ray.dir.to_array() },
        "hits": hits,
    })
}

fn set_components(world: &mut World, replacements: &[Value]) -> Result<Value, Vec<Diagnostic>> {
    let mut values = Vec::new();
    let mut registered = Vec::new();
    for args in replacements {
        let entity = entity_arg(world, args)?;
        let type_id = str_arg(args, "type")?;
        let data = args
            .get("data")
            .ok_or_else(|| vec![invalid("set: data is required")])?;
        if let Some(value) = registered_from_live(
            world,
            type_id,
            data,
            args.get("version").and_then(Value::as_u64),
        )? {
            registered.push((entity, value));
        } else {
            let value = decode_component(type_id, &Node::from_serde(data))
                .ok_or_else(|| vec![invalid(format!("set: {type_id} is not a native component"))])?
                .map_err(|e| {
                    vec![invalid(format!(
                        "set: {type_id}{}: {}",
                        e.pointer, e.message
                    ))]
                })?;
            values.push((entity, value));
        }
    }
    Ok(rejections(&world.set_live_components(values, registered)))
}

fn registered_from_live(
    world: &World,
    type_id: &str,
    data: &Value,
    version: Option<u64>,
) -> Result<Option<fynkel_core::RegisteredDescription>, Vec<Diagnostic>> {
    let Some(schema) = world.registered_schemas().find(|s| s.id == type_id) else {
        return Ok(None);
    };
    if version.is_some_and(|v| v != u64::from(schema.version)) {
        return Err(vec![invalid("registered component version mismatch")]);
    }
    let mut data = data.clone();
    let mut refs = Vec::new();
    for field in &schema.fields {
        if matches!(field.default, fynkel_core::ScalarValue::Object(_)) {
            if let Some(id) = data
                .get(&field.name)
                .and_then(|v| v.get("entity"))
                .and_then(Value::as_str)
            {
                let target = resolve(world, id)
                    .ok_or_else(|| vec![invalid(format!("unresolved component reference {id}"))])?;
                refs.push((field.name.clone(), target));
                data[&field.name] = Value::Null;
            }
        }
    }
    let mut value =
        fynkel_document::runtime::registered_description(type_id, schema.version, &data)
            .map_err(|e| vec![invalid(e)])?;
    for (name, target) in refs {
        value.fields.insert(
            name,
            fynkel_core::AuthoredRegisteredValue::Object(fynkel_contracts::ObjectRef {
                id: fynkel_contracts::Uuid::NIL,
                entity: target,
            }),
        );
    }
    Ok(Some(value))
}

fn write(world: &mut World, commands: Vec<Command>) -> Value {
    rejections(&world.apply_gameplay_now(commands))
}

fn rejections(rejected: &[fynkel_core::Diagnostic]) -> Value {
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

/// `parent`: a live entity id, or null/absent for a root.
fn parent_arg(world: &World, args: &Value) -> Result<Option<Entity>, Vec<Diagnostic>> {
    match args.get("parent") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) => resolve(world, id).map(Some).ok_or_else(|| {
            vec![invalid(format!("parent: no live entity {id}")).with_id(id.clone())]
        }),
        Some(_) => Err(vec![invalid("`parent` must be an entity id or null")]),
    }
}

fn index_arg(args: &Value) -> Result<Option<usize>, Vec<Diagnostic>> {
    match args.get("index") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(|i| Some(i as usize))
            .ok_or_else(|| vec![invalid("`index` must be a non-negative integer")]),
    }
}

fn uuid_of(s: &str) -> Option<fynkel_contracts::Uuid> {
    Some(fynkel_contracts::Uuid(*Uuid::parse_str(s).ok()?.as_bytes()))
}

fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::SESSION_INVALID_REQUEST, message)
}

fn ids_arg(args: &Value) -> Result<Vec<String>, Vec<Diagnostic>> {
    args.get("ids")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_string)).collect())
        .ok_or_else(|| vec![invalid("`ids` must be an array of entity ids")])
}

fn floats(v: &Value, what: &str, n: usize) -> Result<Vec<f32>, Vec<Diagnostic>> {
    v.as_array()
        .filter(|a| a.len() == n)
        .and_then(|a| {
            a.iter()
                .map(|f| f.as_f64().map(|f| f as f32).filter(|f| f.is_finite()))
                .collect::<Option<Vec<f32>>>()
        })
        .ok_or_else(|| vec![invalid(format!("`{what}` must be {n} finite numbers"))])
}

fn field<'a>(args: &'a Value, key: &str) -> Result<&'a Value, Vec<Diagnostic>> {
    args.get(key)
        .filter(|v| !v.is_null())
        .ok_or_else(|| vec![invalid(format!("`{key}` is required"))])
}

fn f32_arg(args: &Value, key: &str) -> Result<f32, Vec<Diagnostic>> {
    Ok(floats(&json!([field(args, key)?]), key, 1)?[0])
}

fn vec2_arg(args: &Value, key: &str) -> Result<Vec2, Vec<Diagnostic>> {
    let f = floats(field(args, key)?, key, 2)?;
    Ok(Vec2::new(f[0], f[1]))
}

fn vec3_arg(args: &Value, key: &str) -> Result<Vec3, Vec<Diagnostic>> {
    let f = floats(field(args, key)?, key, 3)?;
    Ok(Vec3::new(f[0], f[1], f[2]))
}

/// `view`: the Scene view's camera — `world` (16 floats, column-major),
/// `aspect`, and `projection` ("perspective" | "orthographic"), `fovY`
/// (radians), `orthoHeight`, `near`, `far`, each defaulting to
/// `Camera::default()`'s. The same shape as the edit preview's.
fn view_arg(args: &Value) -> Result<View, Vec<Diagnostic>> {
    let view = field(args, "view")?;
    let world = floats(field(view, "world")?, "view.world", 16)?;
    let mut camera = Camera::default();
    match view.get("projection").and_then(Value::as_str) {
        None => {}
        Some("perspective") => camera.projection = Projection::Perspective,
        Some("orthographic") => camera.projection = Projection::Orthographic,
        Some(_) => {
            return Err(vec![invalid(
                "`view.projection` must be 'perspective' or 'orthographic'",
            )])
        }
    }
    for (key, slot) in [
        ("fovY", &mut camera.fov_y),
        ("orthoHeight", &mut camera.ortho_height),
        ("near", &mut camera.near),
        ("far", &mut camera.far),
    ] {
        if view.get(key).is_some_and(|v| !v.is_null()) {
            *slot = f32_arg(view, key)?;
        }
    }
    if camera.near <= 0.0
        || camera.far <= camera.near
        || camera.fov_y <= 0.0
        || camera.ortho_height <= 0.0
    {
        return Err(vec![invalid(
            "`view` needs 0 < near < far, fovY > 0 and orthoHeight > 0",
        )]);
    }
    Ok(View {
        world: Mat4::from_cols_array(&world.try_into().expect("16 floats")),
        camera,
        aspect: f32_arg(view, "aspect")?,
    })
}

//! F0 candidate state boundary over the existing core/ECS. Not a shipping API.
use fynkel_contracts::Entity;
use fynkel_core::{world::Name, Command, Transform, World, WorldConfig};
use fynkel_scene::{self as scene, InactiveSelf};
use glam::{Mat4, Quat, Vec3};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Live(Entity),
    Temp(u32),
}

impl Key {
    fn parse(value: &Value) -> Result<Self> {
        if value.as_object().is_none_or(|object| object.len() != 1) {
            return Err("Expected exactly one live or temporary reference tag".into());
        }
        if let Some(bits) = value.get("entity").and_then(Value::as_str) {
            return u128::from_str_radix(bits, 16)
                .map(|v| Self::Live(Entity::from_bits(v)))
                .map_err(|_| "Invalid handle".into());
        }
        let n = value
            .get("temp")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0 && *n <= u32::MAX as u64)
            .ok_or("Invalid reference")?;
        Ok(Self::Temp(n as u32))
    }
    fn value(self) -> Value {
        match self {
            Self::Live(e) => json!({"entity":format!("{:x}",e.to_bits())}),
            Self::Temp(n) => json!({"temp":n}),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Health {
    pub hp: f64,
    pub poison: f64,
}
#[derive(Clone, Copy)]
struct DoorState(bool);
#[derive(Clone)]
pub struct Node {
    name: String,
    transform: Transform,
    parent: Option<Key>,
    active: bool,
    pub health: Option<Health>,
    door: Option<bool>,
}

impl Node {
    fn new(name: String) -> Self {
        Self {
            name,
            transform: Transform::IDENTITY,
            parent: None,
            active: true,
            health: None,
            door: None,
        }
    }
    fn validate(&self) -> Result<()> {
        if !self.transform.is_finite()
            || (self.transform.rotation.length_squared() - 1.0).abs() > 0.001
        {
            return Err("Expected finite TRS and unit quaternion".into());
        }
        if self.health.is_some_and(|h| {
            !h.hp.is_finite() || !h.poison.is_finite() || h.hp < 0.0 || h.poison < 0.0
        }) {
            return Err("Expected finite nonnegative Health".into());
        }
        Ok(())
    }
}

pub struct Transaction {
    changes: BTreeMap<Key, Option<Node>>,
    next_temp: u32,
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    failed: bool,
}

impl Transaction {
    pub fn all() -> Self {
        let fields = [
            "Name",
            "Transform",
            "Hierarchy",
            "Activity",
            "Health",
            "DoorState",
            "Structure",
        ];
        Self {
            changes: BTreeMap::new(),
            next_temp: 1,
            reads: fields.iter().map(|v| v.to_string()).collect(),
            writes: fields.iter().map(|v| v.to_string()).collect(),
            failed: false,
        }
    }
    fn allow(&self, field: &str, write: bool) -> Result<()> {
        if self.failed {
            return Err("Transaction is failed; abort required".into());
        }
        if self.writes.contains(field) || (!write && self.reads.contains(field)) {
            Ok(())
        } else {
            Err(format!(
                "Undeclared {} {field}",
                if write { "write" } else { "read" }
            ))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Adapter,
    Shared,
}

pub struct Kernel {
    pub core: World,
    live: BTreeSet<Entity>,
    pub transaction: Option<Transaction>,
    pub strategy: Strategy,
    pub revision: u64,
    pub preflight_entities: u64,
}

impl Kernel {
    pub fn new(strategy: Strategy) -> Self {
        Self {
            core: World::new(WorldConfig {
                ..WorldConfig::default()
            }),
            live: BTreeSet::new(),
            transaction: None,
            strategy,
            revision: 0,
            preflight_entities: 0,
        }
    }
    fn read(&self, tx: &Transaction, key: Key) -> Result<Node> {
        if let Some(value) = tx.changes.get(&key) {
            return value.clone().ok_or("Destroyed reference".into());
        }
        let Key::Live(e) = key else {
            return Err("Unknown temporary reference".into());
        };
        if !self.live.contains(&e) || !self.core.ecs().contains(e) {
            return Err("Foreign or stale entity".into());
        }
        let ecs = self.core.ecs();
        Ok(Node {
            name: ecs
                .get::<Name>(e)
                .map(|n| n.value.clone())
                .unwrap_or_default(),
            transform: ecs.copy::<Transform>(e).map_err(|_| "Missing Transform")?,
            parent: scene::parent_of(ecs, e).map(Key::Live),
            active: !ecs.has::<InactiveSelf>(e),
            health: ecs.copy::<Health>(e).ok(),
            door: ecs.copy::<DoorState>(e).ok().map(|v| v.0),
        })
    }
    fn keys(&self, tx: &Transaction) -> Vec<Key> {
        self.live
            .iter()
            .copied()
            .map(Key::Live)
            .chain(
                tx.changes
                    .keys()
                    .filter(|k| matches!(k, Key::Temp(_)))
                    .copied(),
            )
            .filter(|k| !matches!(tx.changes.get(k), Some(None)))
            .collect()
    }
    fn ancestry(&self, tx: &Transaction, key: Key) -> Result<Vec<Node>> {
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(key);
        while let Some(k) = current {
            if !seen.insert(k) {
                return Err("Hierarchy cycle".into());
            }
            let node = self.read(tx, k)?;
            current = node.parent;
            result.push(node);
        }
        Ok(result)
    }
    fn matrix(&self, tx: &Transaction, key: Key) -> Result<Mat4> {
        Ok(self
            .ancestry(tx, key)?
            .iter()
            .rev()
            .fold(Mat4::IDENTITY, |m, n| m * n.transform.matrix()))
    }
    fn active(&self, tx: &Transaction, key: Key) -> Result<bool> {
        Ok(self.ancestry(tx, key)?.iter().all(|n| n.active))
    }
    fn validate(&self, tx: &Transaction) -> Result<()> {
        if tx.failed {
            return Err("Transaction failed".into());
        }
        for (key, node) in &tx.changes {
            if let Some(node) = node {
                node.validate()?;
                self.ancestry(tx, *key)?;
            }
        }
        Ok(())
    }
    fn require_keys(&self, tx: &Transaction) -> Result<()> {
        // Deleting a parent requires deleting or moving every surviving descendant.
        if tx.changes.values().any(Option::is_none) {
            for key in self.keys(tx) {
                self.ancestry(tx, key)?;
            }
        }
        self.validate(tx)
    }
    pub fn spawn(&self, tx: &mut Transaction, name: &str, parent: Option<Key>) -> Result<Key> {
        tx.allow("Structure", true)?;
        if let Some(p) = parent {
            self.read(tx, p)?;
        }
        let key = Key::Temp(tx.next_temp);
        tx.next_temp = tx
            .next_temp
            .checked_add(1)
            .ok_or("Temp identity exhausted")?;
        let mut node = Node::new(name.into());
        node.parent = parent;
        tx.changes.insert(key, Some(node));
        Ok(key)
    }
    pub fn set_health(&self, tx: &mut Transaction, key: Key, h: Health) -> Result<()> {
        tx.allow("Health", true)?;
        let mut node = self.read(tx, key)?;
        node.health = Some(h);
        node.validate()?;
        tx.changes.insert(key, Some(node));
        Ok(())
    }
    fn set(&self, tx: &mut Transaction, key: Key, field: &str, value: &Value) -> Result<()> {
        tx.allow(field, true)?;
        let mut node = self.read(tx, key)?;
        match field {
            "Name" => node.name = value.as_str().ok_or("Expected name")?.into(),
            "Activity" => node.active = value.as_bool().ok_or("Expected active flag")?,
            "Health" => {
                if value.is_null() {
                    node.health = None;
                } else {
                    fields(value, &["hp", "poisonPerSecond"])?;
                    return self.set_health(
                        tx,
                        key,
                        Health {
                            hp: number(value, "hp")?,
                            poison: number(value, "poisonPerSecond")?,
                        },
                    );
                }
            }
            "DoorState" => {
                node.door = if value.is_null() {
                    None
                } else {
                    fields(value, &["open"])?;
                    Some(
                        value
                            .get("open")
                            .and_then(Value::as_bool)
                            .ok_or("Expected open flag")?,
                    )
                }
            }
            "Transform" => node.transform = parse_transform(value)?,
            "Hierarchy" => {
                node.parent = if value.is_null() {
                    None
                } else {
                    Some(Key::parse(value)?)
                };
                if let Some(p) = node.parent {
                    self.read(tx, p)?;
                }
            }
            _ => return Err("Unsupported field".into()),
        }
        node.validate()?;
        let before = tx.changes.insert(key, Some(node));
        if let Err(error) = self.ancestry(tx, key) {
            if let Some(old) = before {
                tx.changes.insert(key, old);
            } else {
                tx.changes.remove(&key);
            }
            return Err(error);
        }
        Ok(())
    }
    fn destroy(&self, tx: &mut Transaction, key: Key) -> Result<()> {
        tx.allow("Structure", true)?;
        self.read(tx, key)?;
        let mut removed = vec![key];
        // Snapshot candidates, including children spawned in this transaction.
        for candidate in self.keys(tx) {
            let mut cursor = Some(candidate);
            while let Some(k) = cursor {
                if k == key {
                    removed.push(candidate);
                    break;
                }
                cursor = self.read(tx, k)?.parent;
            }
        }
        for k in removed {
            tx.changes.insert(k, None);
        }
        Ok(())
    }
    fn get(&self, tx: &Transaction, key: Key, field: &str) -> Result<Value> {
        tx.allow(field, false)?;
        let node = self.read(tx, key)?;
        Ok(match field {
            "Name" => json!(node.name),
            "Activity" => json!({"self":node.active,"hierarchy":self.active(tx,key)?}),
            "Hierarchy" => node.parent.map(Key::value).unwrap_or(Value::Null),
            "Transform" => {
                json!({"local":transform_value(node.transform),"worldMatrix":self.matrix(tx,key)?.to_cols_array()})
            }
            "Health" => node
                .health
                .map(|h| json!({"hp":h.hp,"poisonPerSecond":h.poison}))
                .unwrap_or(Value::Null),
            "DoorState" => node
                .door
                .map(|open| json!({"open":open}))
                .unwrap_or(Value::Null),
            _ => return Err("Unsupported field".into()),
        })
    }
    // Full preflight reconstruction is restricted to this experiment's supported
    // components. It is intentionally measured, never described as a general clone.
    fn preflight(&mut self, tx: &Transaction) -> Result<()> {
        let empty = Transaction::all();
        let mut preview = World::new(WorldConfig {
            ..WorldConfig::default()
        });
        let mut mapping = BTreeMap::new();
        for entity in &self.live {
            let node = self.read(&empty, Key::Live(*entity))?;
            let e = scene::spawn(preview.ecs_mut(), node.transform, None)
                .map_err(|e| format!("{e:?}"))?;
            preview.ecs_mut().insert(e, Name::new(&node.name)).unwrap();
            mapping.insert(Key::Live(*entity), e);
        }
        let mut commands = Vec::new();
        for entity in &self.live {
            let node = self.read(&empty, Key::Live(*entity))?;
            let e = mapping[&Key::Live(*entity)];
            commands.push(Command::SetParent {
                child: e,
                parent: node.parent.map(|p| mapping[&p]),
                keep_world: false,
            });
            commands.push(Command::SetActive {
                entity: e,
                active: node.active,
            });
            write_custom(&mut preview, e, &node);
        }
        if !preview.apply_now(commands).is_empty() {
            return Err("Preflight reconstruction rejected".into());
        }
        apply(&mut preview, &mut mapping, tx)?;
        self.preflight_entities += self.live.len() as u64;
        Ok(())
    }
    pub fn commit(&mut self, tx: Transaction) -> Result<Value> {
        self.require_keys(&tx)?;
        if tx.changes.is_empty() {
            return Ok(json!({"created":{},"revision":self.revision}));
        }
        if self.strategy == Strategy::Adapter {
            self.preflight(&tx)?;
        }
        let mut mapping: BTreeMap<_, _> = self.live.iter().map(|e| (Key::Live(*e), *e)).collect();
        let result = apply(&mut self.core, &mut mapping, &tx);
        let created = match result {
            Ok(value) => value,
            Err(error) => {
                self.core.poison();
                return Err(format!(
                    "Unexpected apply failure; kernel poisoned: {error}"
                ));
            }
        };
        self.live = mapping.values().copied().collect();
        self.revision += 1;
        Ok(json!({"created":created,"revision":self.revision}))
    }
    pub fn call(&mut self, request: Value) -> Result<Value> {
        let op = request
            .get("op")
            .and_then(Value::as_str)
            .ok_or("Missing operation")?;
        if op == "abort" {
            self.transaction = None;
            return Ok(Value::Null);
        }
        if self.core.is_poisoned() {
            return Err("Kernel poisoned".into());
        }
        if op == "begin" {
            if self.transaction.is_some() {
                return Err("Nested transaction".into());
            }
            let mut tx = Transaction::all();
            for (name, target) in [("reads", &mut tx.reads), ("writes", &mut tx.writes)] {
                if let Some(values) = request.get(name) {
                    *target = values
                        .as_array()
                        .ok_or("Expected access declarations")?
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(String::from)
                                .ok_or("Expected field name".to_string())
                        })
                        .collect::<Result<_>>()?;
                }
            }
            self.transaction = Some(tx);
            return Ok(Value::Null);
        }
        if op == "publish" {
            if self.transaction.is_some() {
                return Err("Publish requires a callback boundary".into());
            }
            self.core.begin_frame(self.core.config().fixed_dt);
            self.core.step();
            let tx = Transaction::all();
            let rows=self.keys(&tx).into_iter().map(|k|Ok(json!({"ref":k.value(),"transform":self.get(&tx,k,"Transform")?,"activity":self.get(&tx,k,"Activity")?}))).collect::<Result<Vec<_>>>()?;
            let extract = self.core.extract();
            let render:Vec<_>=extract.records.iter().map(|r|json!({"ref":Key::Live(r.entity).value(),"worldMatrix":r.world.to_cols_array()})).collect();
            return Ok(
                json!({"tick":self.core.tick(),"revision":self.revision,"rows":rows,"render":render}),
            );
        }
        let mut tx = self.transaction.take().ok_or("Begin a transaction")?;
        if op == "commit" {
            return self.commit(tx);
        }
        let result = (|| match op {
            "spawn" => self
                .spawn(
                    &mut tx,
                    request
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or("Expected name")?,
                    request
                        .get("parent")
                        .filter(|v| !v.is_null())
                        .map(Key::parse)
                        .transpose()?,
                )
                .map(Key::value),
            "set" => self
                .set(
                    &mut tx,
                    Key::parse(&request["ref"])?,
                    request["field"].as_str().ok_or("Expected field")?,
                    &request["value"],
                )
                .map(|_| Value::Null),
            "get" => self.get(
                &tx,
                Key::parse(&request["ref"])?,
                request["field"].as_str().ok_or("Expected field")?,
            ),
            "exists" => Ok(json!(self.read(&tx, Key::parse(&request["ref"])?).is_ok())),
            "destroy" => self
                .destroy(&mut tx, Key::parse(&request["ref"])?)
                .map(|_| Value::Null),
            "query" => {
                let field = request["field"].as_str().ok_or("Expected field")?;
                tx.allow(field, false)?;
                let mut rows = Vec::new();
                for key in self.keys(&tx) {
                    let value = self.get(&tx, key, field)?;
                    if !value.is_null() {
                        rows.push(json!({"ref":key.value(),"value":value}));
                    }
                }
                Ok(json!(rows))
            }
            "find" => {
                tx.allow("Name", false)?;
                let name = request["name"].as_str().ok_or("Expected name")?;
                let found = self
                    .keys(&tx)
                    .into_iter()
                    .find(|k| self.read(&tx, *k).is_ok_and(|n| n.name == name));
                Ok(found.map(Key::value).unwrap_or(Value::Null))
            }
            _ => Err("Unknown operation".into()),
        })();
        if result.is_err() {
            tx.failed = true;
        }
        self.transaction = Some(tx);
        result
    }
}

fn number(v: &Value, name: &str) -> Result<f64> {
    v.get(name)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or(format!("Expected finite {name}"))
}
fn array<const N: usize>(v: &Value) -> Result<[f32; N]> {
    let values = v
        .as_array()
        .filter(|v| v.len() == N)
        .ok_or("Wrong vector length")?;
    let mut result = [0.0; N];
    for (i, value) in values.iter().enumerate() {
        result[i] = value.as_f64().ok_or("Expected number")? as f32;
    }
    Ok(result)
}
fn parse_transform(v: &Value) -> Result<Transform> {
    fields(v, &["position", "rotation", "scale"])?;
    Ok(Transform {
        position: Vec3::from_array(array(&v["position"])?),
        rotation: Quat::from_array(array(&v["rotation"])?),
        scale: Vec3::from_array(array(&v["scale"])?),
    })
}

fn fields(value: &Value, names: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or("Expected component object")?;
    if object.len() != names.len() || names.iter().any(|name| !object.contains_key(*name)) {
        return Err("Unknown or missing component field".into());
    }
    Ok(())
}
fn transform_value(t: Transform) -> Value {
    json!({"position":t.position.to_array(),"rotation":t.rotation.to_array(),"scale":t.scale.to_array()})
}
fn write_custom(world: &mut World, e: Entity, node: &Node) {
    let ecs = world.ecs_mut();
    if let Some(h) = node.health {
        ecs.insert(e, h).unwrap();
    } else if ecs.has::<Health>(e) {
        ecs.remove::<Health>(e).unwrap();
    }
    if let Some(open) = node.door {
        ecs.insert(e, DoorState(open)).unwrap();
    } else if ecs.has::<DoorState>(e) {
        ecs.remove::<DoorState>(e).unwrap();
    }
}
fn apply(
    world: &mut World,
    mapping: &mut BTreeMap<Key, Entity>,
    tx: &Transaction,
) -> Result<Value> {
    let mut commands = Vec::new();
    // Detach touched survivors first to avoid transient cycles/subtree deletion.
    for (key, node) in &tx.changes {
        if let Some(node) = node {
            if let Some(e) = mapping.get(key) {
                if scene::parent_of(world.ecs(), *e)
                    != node.parent.and_then(|p| mapping.get(&p).copied())
                    || node.parent.is_some_and(|p| matches!(p, Key::Temp(_)))
                {
                    commands.push(Command::SetParent {
                        child: *e,
                        parent: None,
                        keep_world: false,
                    });
                }
            }
        }
    }
    if !world.apply_now(commands).is_empty() {
        return Err("Detach rejected".into());
    }
    for (key, node) in &tx.changes {
        if node.is_none() {
            if let Some(e) = mapping.remove(key) {
                if world.ecs().contains(e) {
                    let diagnostics = world.apply_now(vec![Command::Destroy(e)]);
                    if !diagnostics.is_empty() {
                        return Err("Destroy rejected".into());
                    }
                }
            }
        }
    }
    // Remove mappings for children destroyed by a core subtree command.
    mapping.retain(|_, e| world.ecs().contains(*e));
    let mut born = serde_json::Map::new();
    for (key, node) in &tx.changes {
        if let (Key::Temp(temp), Some(node)) = (key, node) {
            let (e, d) = world
                .spawn_now(&node.name, node.transform, None, vec![], None)
                .map_err(|e| format!("{e:?}"))?;
            if !d.is_empty() {
                return Err("Spawn rejected".into());
            }
            mapping.insert(*key, e);
            born.insert(temp.to_string(), json!(format!("{:x}", e.to_bits())));
        }
    }
    let mut commands = Vec::new();
    for (key, node) in &tx.changes {
        if let Some(node) = node {
            let e = *mapping.get(key).ok_or("Missing surviving entity")?;
            if world.ecs().copy::<Transform>(e).ok() != Some(node.transform) {
                commands.push(Command::SetTransform {
                    entity: e,
                    transform: node.transform,
                });
            }
            if !world
                .ecs()
                .get::<Name>(e)
                .map(|n| n.value.as_str() == node.name)
                .unwrap_or(node.name.is_empty())
            {
                commands.push(Command::SetName {
                    entity: e,
                    name: node.name.clone(),
                });
            }
            if world.ecs().has::<InactiveSelf>(e) == node.active {
                commands.push(Command::SetActive {
                    entity: e,
                    active: node.active,
                });
            }
            let parent = node.parent.map(|p| mapping[&p]);
            if scene::parent_of(world.ecs(), e) != parent {
                commands.push(Command::SetParent {
                    child: e,
                    parent,
                    keep_world: false,
                });
            }
        }
    }
    if !world.apply_now(commands).is_empty() {
        return Err("Validated built-in edit rejected".into());
    }
    for (key, node) in &tx.changes {
        if let Some(node) = node {
            write_custom(world, mapping[key], node);
        }
    }
    Ok(Value::Object(born))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seeded(strategy: Strategy) -> (Kernel, Value) {
        let mut kernel = Kernel::new(strategy);
        kernel.call(json!({"op":"begin"})).unwrap();
        let temp = kernel.call(json!({"op":"spawn","name":"before"})).unwrap();
        kernel.call(json!({"op":"set","ref":temp,"field":"Health","value":{"hp":100.0,"poisonPerSecond":2.0}})).unwrap();
        let committed = kernel.call(json!({"op":"commit"})).unwrap();
        let reference = json!({"entity":committed["created"]["1"]});
        (kernel, reference)
    }
    #[test]
    fn invalid_native_fields_poison_only_the_transaction_and_preserve_all_data() {
        for strategy in [Strategy::Shared, Strategy::Adapter] {
            let (mut kernel, reference) = seeded(strategy);
            let revision = kernel.revision;
            kernel.call(json!({"op":"begin"})).unwrap();
            kernel
                .call(json!({"op":"set","ref":reference,"field":"Name","value":"must-rollback"}))
                .unwrap();
            assert!(kernel.call(json!({"op":"set","ref":reference,"field":"Health","value":{"hp":1,"poisonPerSecond":2,"unknown":true}})).is_err());
            assert!(kernel.call(json!({"op":"commit"})).is_err());
            assert!(!kernel.core.is_poisoned());
            assert_eq!(kernel.revision, revision);
            kernel.call(json!({"op":"begin"})).unwrap();
            assert_eq!(
                kernel
                    .call(json!({"op":"get","ref":reference,"field":"Name"}))
                    .unwrap(),
                json!("before")
            );
            assert_eq!(
                kernel
                    .call(json!({"op":"get","ref":reference,"field":"Health"}))
                    .unwrap()["hp"],
                json!(100.0)
            );
            kernel.call(json!({"op":"commit"})).unwrap();
            assert_eq!(kernel.revision, revision);
        }
    }
    #[test]
    fn rejected_stale_handle_does_not_leak_an_earlier_valid_edit() {
        for strategy in [Strategy::Shared, Strategy::Adapter] {
            let (mut kernel, reference) = seeded(strategy);
            kernel.call(json!({"op":"begin"})).unwrap();
            kernel
                .call(json!({"op":"set","ref":reference,"field":"Name","value":"must-rollback"}))
                .unwrap();
            assert!(kernel
                .call(json!({"op":"destroy","ref":{"entity":"100000199999999"}}))
                .is_err());
            assert!(kernel.call(json!({"op":"commit"})).is_err());
            kernel.call(json!({"op":"begin"})).unwrap();
            assert_eq!(
                kernel.call(json!({"op":"find","name":"before"})).unwrap(),
                reference
            );
        }
    }
    #[test]
    fn query_captures_pending_components_and_destruction_retires_handles() {
        for strategy in [Strategy::Shared, Strategy::Adapter] {
            let (mut kernel, reference) = seeded(strategy);
            kernel.call(json!({"op":"begin"})).unwrap();
            let temp = kernel.call(json!({"op":"spawn","name":"new"})).unwrap();
            kernel.call(json!({"op":"set","ref":temp,"field":"Health","value":{"hp":5,"poisonPerSecond":0}})).unwrap();
            assert_eq!(
                kernel
                    .call(json!({"op":"query","field":"Health"}))
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            kernel
                .call(json!({"op":"destroy","ref":reference}))
                .unwrap();
            assert_eq!(
                kernel
                    .call(json!({"op":"query","field":"Health"}))
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            kernel.call(json!({"op":"commit"})).unwrap();
            kernel.call(json!({"op":"begin"})).unwrap();
            assert_eq!(
                kernel.call(json!({"op":"exists","ref":reference})).unwrap(),
                json!(false)
            );
        }
    }
}

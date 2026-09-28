//! Transform and hierarchy (blueprint §4.5).
//!
//! Coordinate convention: right-handed, Y-up, metres, seconds, radians;
//! quaternions stored `[x, y, z, w]` (glam's layout).
//!
//! The hierarchy is authoring structure and transform propagation only; it
//! does not imply component ownership. Components:
//! * [`Transform`] — local TRS, authoritative;
//! * [`WorldTransform`] — cached world matrix, recomputed by [`propagate`]
//!   for dirty subtrees only;
//! * [`Parent`] / [`Children`] — kept consistent by this module's functions;
//!   never edit them directly.
//!
//! * [`InactiveSelf`] / [`Inactive`] — the entity's own active flag and
//!   the derived "not active in the hierarchy" marker every runtime system
//!   skips; [`set_active`] and [`refresh_active`] keep them right.
//!
//! Every mutation goes through `Hierarchy` functions so the dirty flag and
//! links stay right; a system that writes `Transform` directly must call
//! [`mark_dirty`].

use fynkel_contracts::Entity;
use fynkel_ecs::internal::hecs;
use fynkel_ecs::{EcsError, EntityBuilder, World};
use fynkel_schema_derive::Schema;
use glam::{Mat4, Quat, Vec3};

/// Local transform relative to the parent (or the world for roots).
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 1, name = "fynkel.Transform")]
pub struct Transform {
    /// Metres.
    pub position: Vec3,
    /// Unit quaternion `[x, y, z, w]`.
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_position(p: Vec3) -> Self {
        Self {
            position: p,
            ..Self::IDENTITY
        }
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    pub fn from_matrix(m: Mat4) -> Self {
        let (scale, rotation, position) = m.to_scale_rotation_translation();
        Self {
            position,
            rotation,
            scale,
        }
    }

    pub fn is_finite(&self) -> bool {
        self.position.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// The object's layer (ADR-0020): an index into the project's `layers`
/// (at most 32), used by physics (a collider's membership is
/// `1 << layer`), by cameras' and lights' culling masks and by queries.
/// An entity without one is on layer 0, `default`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Schema)]
#[schema(kind = "component", id = 26, name = "fynkel.Layer")]
pub struct Layer {
    /// `0..32`: the project's `layers[layer]`.
    pub layer: u32,
}

impl Layer {
    /// Layers are bits of a `u32` mask.
    pub const COUNT: u32 = 32;

    /// This layer's bit in a mask (`0` for an index out of range, which
    /// validation rejects before it gets here).
    pub fn bit(self) -> u32 {
        1u32.checked_shl(self.layer).unwrap_or(0)
    }

    pub fn validate(&self) -> core::result::Result<(), String> {
        if self.layer >= Self::COUNT {
            return Err(format!(
                "layer must be below {}, got {}",
                Self::COUNT,
                self.layer
            ));
        }
        Ok(())
    }
}

/// The object's tags (ADR-0020): bit `i` is the project's `tags[i]` (at
/// most 32 — a document number stays exact, and a web client can hold the
/// set). An entity without one has no tags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Schema)]
#[schema(kind = "component", id = 27, name = "fynkel.Tags")]
pub struct Tags {
    /// Bit `i` set: the object carries the project's `tags[i]`.
    pub bits: u32,
}

impl Tags {
    pub const COUNT: u32 = 32;

    pub fn has(self, index: u32) -> bool {
        index < Self::COUNT && self.bits & (1u32 << index) != 0
    }
}

/// Cached world matrix. `dirty` means the local transform or an ancestor
/// changed since the last [`propagate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldTransform {
    pub matrix: Mat4,
    pub dirty: bool,
}

impl Default for WorldTransform {
    fn default() -> Self {
        Self {
            matrix: Mat4::IDENTITY,
            dirty: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent(pub Entity);

/// The entity's own flag is off (Unity's `activeSelf == false`). Set
/// through [`set_active`]; absent means active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InactiveSelf;

/// Derived: the entity or an ancestor is inactive (not active in the
/// hierarchy). Every runtime system skips an entity carrying it — not
/// drawn, simulated, animated or heard. Maintained by [`refresh_active`];
/// never insert it directly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inactive;

/// Deterministic order: insertion order, adjustable with [`set_sibling_index`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Children(pub Vec<Entity>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneError {
    Ecs(EcsError),
    /// Parent is the entity itself or one of its descendants.
    Cycle {
        child: Entity,
        parent: Entity,
    },
    NotFinite(Entity),
}

impl From<EcsError> for SceneError {
    fn from(e: EcsError) -> Self {
        SceneError::Ecs(e)
    }
}

pub type Result<T> = core::result::Result<T, SceneError>;

/// Builder with `Transform` + `WorldTransform` attached.
pub fn spatial(transform: Transform) -> EntityBuilder {
    EntityBuilder::new()
        .with(transform)
        .with(WorldTransform::default())
}

/// Spawns a spatial entity, optionally parented (keep-local semantics).
pub fn spawn(world: &mut World, transform: Transform, parent: Option<Entity>) -> Result<Entity> {
    if !transform.is_finite() {
        return Err(SceneError::NotFinite(Entity::NULL));
    }
    let e = world.spawn(spatial(transform));
    if let Some(p) = parent {
        if let Err(err) = set_parent_keep_local(world, e, Some(p)) {
            let _ = world.despawn(e);
            return Err(err);
        }
    }
    Ok(e)
}

pub fn parent_of(world: &World, entity: Entity) -> Option<Entity> {
    world.get::<Parent>(entity).ok().map(|p| p.0)
}

pub fn children_of(world: &World, entity: Entity) -> Vec<Entity> {
    world
        .get::<Children>(entity)
        .map(|c| c.0.clone())
        .unwrap_or_default()
}

pub fn is_ancestor(world: &World, ancestor: Entity, of: Entity) -> bool {
    let mut cur = parent_of(world, of);
    while let Some(p) = cur {
        if p == ancestor {
            return true;
        }
        cur = parent_of(world, p);
    }
    false
}

pub fn local(world: &World, entity: Entity) -> Result<Transform> {
    Ok(world.copy::<Transform>(entity)?)
}

/// Cached world matrix (call [`propagate`] first for current values).
pub fn world_matrix(world: &World, entity: Entity) -> Result<Mat4> {
    Ok(world.get::<WorldTransform>(entity)?.matrix)
}

/// Sets the local transform and marks the subtree dirty.
pub fn set_local(world: &World, entity: Entity, transform: Transform) -> Result<()> {
    if !transform.is_finite() {
        return Err(SceneError::NotFinite(entity));
    }
    *world.get_mut::<Transform>(entity)? = transform;
    mark_dirty(world, entity)?;
    Ok(())
}

/// Sets the local transform so the entity's world pose is `position` /
/// `rotation` (its local scale kept), relative to the parent's cached world
/// matrix — what physics and native movers write. A pose equal to the
/// current one leaves the subtree clean.
pub fn set_world_pose(world: &World, entity: Entity, position: Vec3, rotation: Quat) -> Result<()> {
    let current = world.copy::<Transform>(entity)?;
    let parent = world.get::<Parent>(entity).ok().map(|p| p.0);
    let local = match parent.and_then(|p| world.copy::<WorldTransform>(p).ok()) {
        Some(pw) => {
            let m = pw.matrix.inverse() * Mat4::from_rotation_translation(rotation, position);
            let t = Transform::from_matrix(m);
            Transform {
                position: t.position,
                rotation: t.rotation.normalize(),
                scale: current.scale,
            }
        }
        None => Transform {
            position,
            rotation,
            scale: current.scale,
        },
    };
    if local != current {
        set_local(world, entity, local)?;
    }
    Ok(())
}

/// Marks an entity (and, at propagation time, its descendants) as needing a
/// world-matrix recompute. Descendants are recomputed by `propagate` because
/// it descends through any dirty node.
pub fn mark_dirty(world: &World, entity: Entity) -> Result<()> {
    world.get_mut::<WorldTransform>(entity)?.dirty = true;
    Ok(())
}

fn unlink(world: &mut World, child: Entity) -> Result<()> {
    if let Some(Parent(p)) = world.get::<Parent>(child).ok().map(|p| *p) {
        if let Ok(mut c) = world.get_mut::<Children>(p) {
            c.0.retain(|e| *e != child);
        }
        world.remove::<Parent>(child)?;
    }
    Ok(())
}

/// Reparents, keeping the local transform (so the world transform changes).
pub fn set_parent_keep_local(
    world: &mut World,
    child: Entity,
    parent: Option<Entity>,
) -> Result<()> {
    if !world.contains(child) {
        return Err(EcsError::StaleEntity(child).into());
    }
    if let Some(p) = parent {
        if !world.contains(p) {
            return Err(EcsError::StaleEntity(p).into());
        }
        if p == child || is_ancestor(world, child, p) {
            return Err(SceneError::Cycle { child, parent: p });
        }
    }
    if parent_of(world, child) == parent {
        return Ok(());
    }
    unlink(world, child)?;
    if let Some(p) = parent {
        if !world.has::<Children>(p) {
            world.insert(p, Children::default())?;
        }
        world.get_mut::<Children>(p)?.0.push(child);
        world.insert(child, Parent(p))?;
    }
    mark_dirty(world, child)?;
    Ok(())
}

/// Reparents, keeping the world transform (so the local transform changes).
/// Reads both world poses from the local transforms ([`current_world`]), so
/// a batch of reparents needs no [`propagate`] between them.
pub fn set_parent_keep_world(
    world: &mut World,
    child: Entity,
    parent: Option<Entity>,
) -> Result<()> {
    let child_world = current_world(world, child)?;
    let parent_world = match parent {
        Some(p) => current_world(world, p)?,
        None => Mat4::IDENTITY,
    };
    set_parent_keep_local(world, child, parent)?;
    let local = Transform::from_matrix(parent_world.inverse() * child_world);
    set_local(world, child, local)
}

/// The entity's world matrix as of now: the cached one when it and every
/// ancestor are clean, else the cached matrix above the topmost dirty
/// ancestor times the local transforms below it. Costs the depth, never
/// the world.
pub fn current_world(world: &World, entity: Entity) -> Result<Mat4> {
    // The chain from the entity up, and how far up the topmost dirty is.
    let mut chain = Vec::new();
    let mut top_dirty = None;
    let mut cur = Some(entity);
    while let Some(e) = cur {
        let wt = world.copy::<WorldTransform>(e)?;
        if wt.dirty {
            top_dirty = Some(chain.len());
        }
        chain.push((e, wt.matrix));
        cur = parent_of(world, e);
    }
    let Some(top) = top_dirty else {
        return Ok(chain[0].1);
    };
    let mut m = chain.get(top + 1).map_or(Mat4::IDENTITY, |c| c.1);
    for &(e, _) in chain[..=top].iter().rev() {
        m *= world.copy::<Transform>(e)?.matrix();
    }
    Ok(m)
}

/// Moves `child` within its parent's child list (clamped).
pub fn set_sibling_index(world: &mut World, child: Entity, index: usize) -> Result<()> {
    let Some(p) = parent_of(world, child) else {
        return Ok(());
    };
    let mut c = world.get_mut::<Children>(p)?;
    c.0.retain(|e| *e != child);
    let i = index.min(c.0.len());
    c.0.insert(i, child);
    Ok(())
}

/// Entity plus all descendants, depth-first, deterministic child order.
pub fn subtree(world: &World, root: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if !world.contains(e) {
            continue;
        }
        out.push(e);
        let kids = children_of(world, e);
        stack.extend(kids.into_iter().rev());
    }
    out
}

/// Destroys an entity and its whole subtree; the root is unlinked from its
/// parent first. Returns the destroyed entities (root first).
pub fn despawn_subtree(world: &mut World, root: Entity) -> Result<Vec<Entity>> {
    if !world.contains(root) {
        return Err(EcsError::StaleEntity(root).into());
    }
    unlink(world, root)?;
    let all = subtree(world, root);
    for e in &all {
        world.despawn(*e)?;
    }
    Ok(all)
}

/// Sets the entity's own active flag; `true` when it changed. The
/// derived [`Inactive`] markers follow at the next [`refresh_active`].
pub fn set_active(world: &mut World, entity: Entity, active: bool) -> Result<bool> {
    if !world.contains(entity) {
        return Err(EcsError::StaleEntity(entity).into());
    }
    let was = !world.has::<InactiveSelf>(entity);
    if was == active {
        return Ok(false);
    }
    if active {
        world.remove::<InactiveSelf>(entity)?;
    } else {
        world.insert(entity, InactiveSelf)?;
    }
    Ok(true)
}

/// The entity's own flag (`activeSelf`).
pub fn is_active_self(world: &World, entity: Entity) -> bool {
    !world.has::<InactiveSelf>(entity)
}

/// Whether the entity and every ancestor are active, as of the last
/// [`refresh_active`].
pub fn is_active_in_hierarchy(world: &World, entity: Entity) -> bool {
    !world.has::<Inactive>(entity)
}

/// Recomputes [`Inactive`] from the [`InactiveSelf`] flags and the
/// hierarchy — after flags changed, entities were spawned or reparented.
/// Returns how many entities changed state. Nothing to walk (no flag and
/// no marker anywhere) costs two archetype checks.
pub fn refresh_active(world: &mut World) -> usize {
    let (any_self, any_derived) = {
        let h = world.hecs();
        (
            h.query::<&InactiveSelf>().iter().next().is_some(),
            h.query::<&Inactive>().iter().next().is_some(),
        )
    };
    if !any_self && !any_derived {
        return 0;
    }
    let mut changes: Vec<(hecs::Entity, bool)> = Vec::new();
    {
        let h = world.hecs();
        let mut stack: Vec<(hecs::Entity, bool)> = h
            .query::<hecs::Without<hecs::Entity, &Parent>>()
            .iter()
            .map(|e| (e, false))
            .collect();
        while let Some((raw, parent_inactive)) = stack.pop() {
            let inactive = parent_inactive || h.satisfies::<&InactiveSelf>(raw);
            let marked = h.satisfies::<&Inactive>(raw);
            if inactive != marked {
                changes.push((raw, inactive));
            }
            if let Ok(children) = h.get::<&Children>(raw) {
                for &c in children.0.iter() {
                    if let Some(craw) = world.raw(c) {
                        stack.push((craw, inactive));
                    }
                }
            }
        }
    }
    let h = world.hecs_mut();
    for &(raw, inactive) in &changes {
        if inactive {
            let _ = h.insert_one(raw, Inactive);
        } else {
            let _ = h.remove_one::<Inactive>(raw);
        }
    }
    changes.len()
}

/// Sets [`Inactive`] on `root`'s subtree alone, from its parent's marker
/// — a freshly spawned subtree, when every other marker is current
/// (foundation 3.1). Returns how many entities changed state.
pub fn refresh_active_subtree(world: &mut World, root: Entity) -> usize {
    let parent_inactive = parent_of(world, root).is_some_and(|p| world.has::<Inactive>(p));
    let Some(raw) = world.raw(root) else {
        return 0;
    };
    let mut changes: Vec<(hecs::Entity, bool)> = Vec::new();
    {
        let h = world.hecs();
        let mut stack = vec![(raw, parent_inactive)];
        while let Some((raw, parent_inactive)) = stack.pop() {
            let inactive = parent_inactive || h.satisfies::<&InactiveSelf>(raw);
            if inactive != h.satisfies::<&Inactive>(raw) {
                changes.push((raw, inactive));
            }
            if let Ok(children) = h.get::<&Children>(raw) {
                for &c in children.0.iter() {
                    if let Some(craw) = world.raw(c) {
                        stack.push((craw, inactive));
                    }
                }
            }
        }
    }
    let h = world.hecs_mut();
    for &(raw, inactive) in &changes {
        if inactive {
            let _ = h.insert_one(raw, Inactive);
        } else {
            let _ = h.remove_one::<Inactive>(raw);
        }
    }
    changes.len()
}

/// Recomputes the world matrices of `root`'s subtree alone, from its
/// parent's world matrix — what a freshly spawned subtree needs, without
/// walking the rest of the world (an instantiate, foundation 3.1). `None`
/// when an ancestor is dirty itself (its matrix is stale): then only a
/// whole [`propagate`] is right. Otherwise how many were recomputed.
pub fn propagate_subtree(world: &mut World, root: Entity) -> Option<usize> {
    let mut parent_matrix = Mat4::IDENTITY;
    let mut first = true;
    let mut up = parent_of(world, root);
    while let Some(p) = up {
        let wt = world.copy::<WorldTransform>(p).ok()?;
        if wt.dirty {
            return None;
        }
        if first {
            parent_matrix = wt.matrix;
            first = false;
        }
        up = parent_of(world, p);
    }
    let raw = world.raw(root)?;
    let mut count = 0;
    let mut stack = vec![(raw, parent_matrix)];
    while let Some((raw, parent_matrix)) = stack.pop() {
        let h = world.hecs();
        let Ok(local) = h.get::<&Transform>(raw).map(|t| t.matrix()) else {
            continue;
        };
        let Ok(mut wt) = h.get::<&mut WorldTransform>(raw) else {
            continue;
        };
        let m = parent_matrix * local;
        wt.matrix = m;
        wt.dirty = false;
        drop(wt);
        count += 1;
        if let Ok(children) = h.get::<&Children>(raw) {
            for &c in children.0.iter().rev() {
                if let Some(craw) = world.raw(c) {
                    stack.push((craw, m));
                }
            }
        }
    }
    Some(count)
}

/// Recomputes world matrices for dirty subtrees. Returns how many entities
/// were recomputed (a measure, and a test hook). A clean world costs one
/// pass over the dirty flags; each subtree under a dirty node is walked
/// once, from its topmost dirty ancestor (foundation 3.3).
pub fn propagate(world: &mut World) -> usize {
    let dirty: Vec<(hecs::Entity, Option<Entity>)> = world
        .hecs()
        .query::<(hecs::Entity, &Transform, &WorldTransform, Option<&Parent>)>()
        .iter()
        .filter(|(_, _, wt, _)| wt.dirty)
        .map(|(e, _, _, p)| (e, p.map(|p| p.0)))
        .collect();
    if dirty.is_empty() {
        return 0;
    }
    // The tops: dirty nodes with no dirty ancestor, found before any flag
    // is cleared. The parent of a top is clean, so its matrix is current.
    let mut tops: Vec<(hecs::Entity, Mat4)> = Vec::new();
    'next: for (raw, parent) in dirty {
        let mut parent_matrix = Mat4::IDENTITY;
        let mut first = true;
        let mut up = parent;
        while let Some(p) = up {
            // An ancestor that is not spatial cuts the chain: its
            // descendants are not reached from a root, as before.
            let Ok(wt) = world.copy::<WorldTransform>(p) else {
                continue 'next;
            };
            if wt.dirty {
                continue 'next;
            }
            if first {
                parent_matrix = wt.matrix;
                first = false;
            }
            up = parent_of(world, p);
        }
        tops.push((raw, parent_matrix));
    }
    let mut count = 0;
    let mut stack = tops;
    while let Some((raw, parent_matrix)) = stack.pop() {
        let h = world.hecs();
        let Ok(local) = h.get::<&Transform>(raw).map(|t| t.matrix()) else {
            continue;
        };
        let Ok(mut wt) = h.get::<&mut WorldTransform>(raw) else {
            continue;
        };
        let m = parent_matrix * local;
        wt.matrix = m;
        wt.dirty = false;
        drop(wt);
        count += 1;
        if let Ok(children) = h.get::<&Children>(raw) {
            for &c in children.0.iter().rev() {
                if let Some(craw) = world.raw(c) {
                    stack.push((craw, m));
                }
            }
        }
    }
    count
}

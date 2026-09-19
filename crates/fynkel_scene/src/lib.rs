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
//! Every mutation goes through `Hierarchy` functions so the dirty flag and
//! links stay right; a system that writes `Transform` directly must call
//! [`mark_dirty`].

use fynkel_contracts::Entity;
use fynkel_ecs::internal::hecs;
use fynkel_ecs::{EcsError, EntityBuilder, World};
use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub position: Vec3,
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
/// Uses cached world matrices: call [`propagate`] before if transforms changed.
pub fn set_parent_keep_world(
    world: &mut World,
    child: Entity,
    parent: Option<Entity>,
) -> Result<()> {
    let child_world = world_matrix(world, child)?;
    let parent_world = match parent {
        Some(p) => world_matrix(world, p)?,
        None => Mat4::IDENTITY,
    };
    set_parent_keep_local(world, child, parent)?;
    let local = Transform::from_matrix(parent_world.inverse() * child_world);
    set_local(world, child, local)
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

/// Recomputes world matrices for dirty subtrees. Returns how many entities
/// were recomputed (a measure, and a test hook).
pub fn propagate(world: &mut World) -> usize {
    // Roots: spatial entities without a parent.
    let roots: Vec<hecs::Entity> = world
        .hecs()
        .query::<hecs::Without<(hecs::Entity, &Transform, &WorldTransform), &Parent>>()
        .iter()
        .map(|(e, _, _)| e)
        .collect();
    let mut count = 0;
    let mut stack: Vec<(hecs::Entity, Mat4, bool)> = roots
        .into_iter()
        .map(|e| (e, Mat4::IDENTITY, false))
        .collect();
    while let Some((raw, parent_matrix, parent_dirty)) = stack.pop() {
        let h = world.hecs();
        let (dirty, local) = {
            let Ok(wt) = h.get::<&WorldTransform>(raw) else {
                continue;
            };
            let Ok(t) = h.get::<&Transform>(raw) else {
                continue;
            };
            (wt.dirty || parent_dirty, t.matrix())
        };
        let matrix = if dirty {
            let m = parent_matrix * local;
            let mut wt = h.get::<&mut WorldTransform>(raw).unwrap();
            wt.matrix = m;
            wt.dirty = false;
            count += 1;
            m
        } else {
            h.get::<&WorldTransform>(raw).unwrap().matrix
        };
        if let Ok(children) = h.get::<&Children>(raw) {
            for &c in children.0.iter().rev() {
                if let Some(craw) = world.raw(c) {
                    stack.push((craw, matrix, dirty));
                }
            }
        }
    }
    count
}

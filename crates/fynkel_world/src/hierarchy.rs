//! Parents, children, names, the active flag (stage 2 plan §3.1, stage 3
//! plan §3.3) and layers (stage 4 plan §3.9).
//!
//! The hierarchy lives beside the components, not in them: setting a parent
//! must check for cycles and keep both sides in step, which a component
//! anyone can overwrite with `insert` couldn't promise. Names are here for a
//! simpler reason: components are `Copy`, and a name is a string.
//!
//! An object's `Transform` is relative to its parent. [`World::world_matrix`]
//! walks up to answer for one object; [`WorldMatrices`] computes them all at
//! once, parents before children, for the extract that runs every frame.
//!
//! An inactive object and everything below it are hidden: not drawn, not
//! lit by, not seen through and not hit by rays. Each object keeps its own
//! flag, so making a parent active again shows exactly the children that
//! were active before.
//!
//! Each object is on one of 32 layers, 0 unless set. A layer is the
//! object's own: its children keep theirs. Cameras draw and rays hit only
//! the layers in their masks.

use glam::Mat4;

use crate::ObjectId;
use crate::components::{Camera, Transform};
use crate::id::IdMap;
use crate::world::{Result, World, WorldError};

/// Who is whose parent, and every object's name.
#[derive(Debug, Default)]
pub(crate) struct Hierarchy {
    /// Only objects with a parent or children have a node, so a world of
    /// loose objects costs nothing here.
    nodes: IdMap<ObjectId, Node>,
    names: IdMap<ObjectId, Box<str>>,
    /// Objects whose own flag is off. Few, usually none, so a world with
    /// every object active pays one `is_empty` a frame.
    inactive: IdMap<ObjectId, ()>,
    /// Objects not on layer 0. Usually few, so a world on layer 0 pays one
    /// `is_empty` per object asked about.
    layers: IdMap<ObjectId, u8>,
    /// Bumped on every change of parent, so [`WorldMatrices`] knows when
    /// its order is stale.
    version: u64,
}

#[derive(Debug, Default)]
struct Node {
    parent: Option<ObjectId>,
    /// In the order they were added.
    children: Vec<ObjectId>,
}

impl Hierarchy {
    fn parent(&self, id: ObjectId) -> Option<ObjectId> {
        self.nodes.get(&id).and_then(|n| n.parent)
    }

    fn children(&self, id: ObjectId) -> &[ObjectId] {
        self.nodes.get(&id).map_or(&[], |n| &n.children)
    }

    /// Whether `id`'s own flag is off. Skips hashing when nothing is.
    fn is_off(&self, id: ObjectId) -> bool {
        !self.inactive.is_empty() && self.inactive.contains_key(&id)
    }

    fn layer(&self, id: ObjectId) -> u8 {
        if self.layers.is_empty() { 0 } else { self.layers.get(&id).copied().unwrap_or(0) }
    }

    /// Drops `id`'s node once it has neither a parent nor children.
    fn prune(&mut self, id: ObjectId) {
        if self.nodes.get(&id).is_some_and(|n| n.parent.is_none() && n.children.is_empty()) {
            self.nodes.remove(&id);
        }
    }

    /// Takes `child` from its parent. The trees change, so the version
    /// moves on: a leaf despawned has no node left to say so later.
    fn detach(&mut self, child: ObjectId) {
        let Some(parent) = self.nodes.get_mut(&child).and_then(|n| n.parent.take()) else { return };
        if let Some(node) = self.nodes.get_mut(&parent) {
            node.children.retain(|&c| c != child);
        }
        self.prune(parent);
        self.prune(child);
        self.version += 1;
    }
}

impl World {
    /// Makes `child` a child of `parent`, or a root with `None`. Its
    /// `Transform` is kept as it is, so it now counts from the new parent.
    ///
    /// Refused with [`WorldError::Cycle`] when `parent` is `child` or below
    /// it. Setting the parent it already has changes nothing, not even its
    /// place among its siblings.
    pub fn set_parent(&mut self, child: ObjectId, parent: Option<ObjectId>) -> Result<()> {
        self.entity(child)?;
        if let Some(parent) = parent {
            self.entity(parent)?;
            if self.is_part_of(parent, child)? {
                return Err(WorldError::Cycle { child, parent });
            }
        }
        let tree = &mut self.hierarchy;
        if tree.parent(child) == parent {
            return Ok(());
        }
        tree.detach(child);
        if let Some(parent) = parent {
            tree.nodes.entry(parent).or_default().children.push(child);
            tree.nodes.entry(child).or_default().parent = Some(parent);
        }
        tree.version += 1;
        self.transforms_version += 1;
        Ok(())
    }

    pub fn parent(&self, id: ObjectId) -> Result<Option<ObjectId>> {
        self.entity(id)?;
        Ok(self.hierarchy.parent(id))
    }

    /// The object's children, in the order they were added.
    pub fn children(&self, id: ObjectId) -> Result<&[ObjectId]> {
        self.entity(id)?;
        Ok(self.hierarchy.children(id))
    }

    /// Whether `id` is `root` or below it.
    pub fn is_part_of(&self, id: ObjectId, root: ObjectId) -> Result<bool> {
        self.entity(id)?;
        self.entity(root)?;
        let mut at = Some(id);
        while let Some(current) = at {
            if current == root {
                return Ok(true);
            }
            at = self.hierarchy.parent(current);
        }
        Ok(false)
    }

    /// Names the object, or clears its name with `None` or `""` (an empty
    /// name can't be found, so it's the same as none). Names needn't be
    /// unique.
    pub fn set_name(&mut self, id: ObjectId, name: Option<&str>) -> Result<()> {
        self.entity(id)?;
        match name.filter(|n| !n.is_empty()) {
            Some(name) => self.hierarchy.names.insert(id, name.into()),
            None => self.hierarchy.names.remove(&id),
        };
        Ok(())
    }

    /// Turns the object on or off. Off hides it and everything below it
    /// (see the [module docs](crate::hierarchy)); its children keep their
    /// own flags.
    pub fn set_active(&mut self, id: ObjectId, active: bool) -> Result<()> {
        self.entity(id)?;
        if active {
            self.hierarchy.inactive.remove(&id);
        } else {
            self.hierarchy.inactive.insert(id, ());
        }
        self.transforms_version += 1;
        Ok(())
    }

    /// The object's own flag, whatever its parents' are.
    pub fn active(&self, id: ObjectId) -> Result<bool> {
        self.entity(id)?;
        Ok(!self.hierarchy.is_off(id))
    }

    /// Whether the object is shown: it and every parent above it active.
    pub fn active_in_hierarchy(&self, id: ObjectId) -> Result<bool> {
        self.entity(id)?;
        let mut at = Some(id);
        while let Some(current) = at {
            if self.hierarchy.is_off(current) {
                return Ok(false);
            }
            at = self.hierarchy.parent(current);
        }
        Ok(true)
    }

    /// Puts the object on `layer`, 0 to 31. Its children keep theirs.
    pub fn set_layer(&mut self, id: ObjectId, layer: u32) -> Result<()> {
        self.entity(id)?;
        if layer >= 32 {
            return Err(WorldError::Layer(layer));
        }
        if layer == 0 {
            self.hierarchy.layers.remove(&id);
        } else {
            self.hierarchy.layers.insert(id, layer as u8);
        }
        Ok(())
    }

    pub fn layer(&self, id: ObjectId) -> Result<u32> {
        self.entity(id)?;
        Ok(self.hierarchy.layer(id).into())
    }

    /// Whether the object's layer is in `mask` (bit `n` for layer `n`).
    /// False for an object that doesn't exist.
    pub fn in_mask(&self, id: ObjectId, mask: u32) -> bool {
        mask == u32::MAX || (mask >> self.hierarchy.layer(id)) & 1 == 1
    }

    /// The camera a view draws from: the shown object with the lowest id
    /// that has both a `Camera` and a `Transform`, or `None`.
    pub fn active_camera(&self) -> Option<ObjectId> {
        let mut best: Option<ObjectId> = None;
        self.for_each2::<Camera, Transform>(|id, _, _| {
            if best.is_none_or(|b| id < b) && self.active_in_hierarchy(id).unwrap_or(false) {
                best = Some(id);
            }
        });
        best
    }

    pub fn name(&self, id: ObjectId) -> Result<Option<&str>> {
        self.entity(id)?;
        Ok(self.hierarchy.names.get(&id).map(|n| &**n))
    }

    /// The first object below `root` named `name`, depth first in child
    /// order. `root` itself isn't searched.
    pub fn find(&self, root: ObjectId, name: &str) -> Result<Option<ObjectId>> {
        self.entity(root)?;
        let mut stack: Vec<ObjectId> = self.hierarchy.children(root).iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            if self.hierarchy.names.get(&id).is_some_and(|n| &**n == name) {
                return Ok(Some(id));
            }
            stack.extend(self.hierarchy.children(id).iter().rev());
        }
        Ok(None)
    }

    /// Model-to-world for one object: its `Transform` after each of its
    /// parents'. An object without a `Transform` counts as not moved.
    pub fn world_matrix(&self, id: ObjectId) -> Result<Mat4> {
        self.entity(id)?;
        let mut matrix = Mat4::IDENTITY;
        let mut at = Some(id);
        while let Some(current) = at {
            matrix = self.local_matrix(self.entities[&current]) * matrix;
            at = self.hierarchy.parent(current);
        }
        Ok(matrix)
    }

    /// One object's place in the world, as Dart's `worldTransform` works
    /// it out: its `Transform` after each of its parents', the rotations
    /// multiplied and the scales too (as Unity's lossy scale). Unlike
    /// taking [`world_matrix`](Self::world_matrix) apart, a parent scaled
    /// to 0 on a side still gives a rotation.
    pub fn world_transform(&self, id: ObjectId) -> Result<Transform> {
        self.entity(id)?;
        let mut world = Transform::default();
        let mut at = Some(id);
        while let Some(current) = at {
            let parent = self.ecs.get::<&Transform>(self.entities[&current]).map_or(Transform::default(), |t| *t);
            world = Transform {
                position: parent.position + parent.rotation * (parent.scale * world.position),
                rotation: (parent.rotation * world.rotation).normalize(),
                scale: parent.scale * world.scale,
            };
            at = self.hierarchy.parent(current);
        }
        Ok(world)
    }

    fn local_matrix(&self, entity: hecs::Entity) -> Mat4 {
        self.ecs.get::<&Transform>(entity).map_or(Mat4::IDENTITY, |t| t.matrix())
    }

    /// Detaches `id` and forgets the parents, children and names of it and
    /// everything below it. Returns everything below it, for `despawn`;
    /// `None` when there's nothing below, without allocating (despawning
    /// loose objects is the common case).
    pub(crate) fn take_subtree(&mut self, id: ObjectId) -> Option<Vec<ObjectId>> {
        let tree = &mut self.hierarchy;
        tree.detach(id);
        if !tree.nodes.contains_key(&id) {
            tree.names.remove(&id);
            tree.inactive.remove(&id);
            tree.layers.remove(&id);
            return None;
        }
        let mut subtree = vec![id];
        let mut next = 0;
        while next < subtree.len() {
            let current = subtree[next];
            if let Some(node) = tree.nodes.remove(&current) {
                subtree.extend(node.children);
                tree.version += 1;
            }
            tree.names.remove(&current);
            tree.inactive.remove(&current);
            tree.layers.remove(&current);
            next += 1;
        }
        subtree.swap_remove(0);
        Some(subtree)
    }
}

/// Every object's model-to-world matrix, and whether it's shown, kept for
/// the extract.
///
/// [`update`](Self::update) recomputes them in one pass, parents before
/// children. The order is rebuilt only after a parent changes, so a steady
/// frame allocates nothing, and turning objects on and off rebuilds
/// nothing. Objects with neither parent nor children aren't stored;
/// [`world`](Self::world) uses their own transform and flag. One of these
/// belongs to one world.
#[derive(Debug, Default)]
pub struct WorldMatrices {
    /// The hierarchy version the order was built for, and the world's
    /// transforms version the matrices were worked out for.
    version: Option<u64>,
    transforms: Option<u64>,
    /// Every object in a tree, parents first, with its parent's index.
    order: Vec<(ObjectId, hecs::Entity, Option<u32>)>,
    matrices: Vec<Mat4>,
    /// Whether each object in [`order`](Self::order) is shown.
    shown: Vec<bool>,
    index: IdMap<ObjectId, u32>,
}

impl WorldMatrices {
    /// Brings every matrix up to date with `world`.
    pub fn update(&mut self, world: &World) {
        let tree = &world.hierarchy;
        if self.version == Some(tree.version) && self.transforms == Some(world.transforms_version) {
            return;
        }
        if self.version != Some(tree.version) {
            self.rebuild(world);
        }
        self.transforms = Some(world.transforms_version);
        for i in 0..self.order.len() {
            let (id, entity, parent) = self.order[i];
            let local = world.local_matrix(entity);
            let own = !tree.is_off(id);
            (self.matrices[i], self.shown[i]) = match parent {
                Some(p) => (self.matrices[p as usize] * local, own && self.shown[p as usize]),
                None => (local, own),
            };
        }
    }

    /// `id`'s model-to-world matrix as of the last [`update`](Self::update),
    /// or `None` when it's hidden: inactive itself or below an inactive
    /// object. `local` is its own transform, used when it's in no tree.
    pub fn world(&self, world: &World, id: ObjectId, local: &Transform) -> Option<Mat4> {
        match self.index.get(&id) {
            Some(&i) => self.shown[i as usize].then(|| self.matrices[i as usize]),
            None => (!world.hierarchy.is_off(id)).then(|| local.matrix()),
        }
    }

    /// `id`'s model-to-world matrix as of the last [`update`](Self::update),
    /// shown or not (a skin's joint bends its mesh either way), or `None`
    /// when it's gone.
    pub fn matrix(&self, world: &World, id: ObjectId) -> Option<Mat4> {
        match self.index.get(&id) {
            Some(&i) => Some(self.matrices[i as usize]),
            None => world.world_matrix(id).ok(),
        }
    }

    /// Whether `id` is shown, as of the last [`update`](Self::update).
    pub fn shown(&self, world: &World, id: ObjectId) -> bool {
        match self.index.get(&id) {
            Some(&i) => self.shown[i as usize],
            None => !world.hierarchy.is_off(id),
        }
    }

    fn rebuild(&mut self, world: &World) {
        let tree = &world.hierarchy;
        self.order.clear();
        self.index.clear();
        // Roots in id order, so the result doesn't depend on hash order.
        let mut roots: Vec<ObjectId> =
            tree.nodes.iter().filter(|(_, n)| n.parent.is_none()).map(|(&id, _)| id).collect();
        roots.sort_unstable();
        // Breadth first: a parent is always placed before its children.
        let mut queue: Vec<(ObjectId, Option<u32>)> = roots.into_iter().map(|id| (id, None)).collect();
        let mut next = 0;
        while next < queue.len() {
            let (id, parent) = queue[next];
            let at = u32::try_from(self.order.len()).expect("fewer than 2^32 objects in trees");
            self.order.push((id, world.entities[&id], parent));
            self.index.insert(id, at);
            queue.extend(tree.children(id).iter().map(|&child| (child, Some(at))));
            next += 1;
        }
        self.matrices.clear();
        self.matrices.resize(self.order.len(), Mat4::IDENTITY);
        self.shown.clear();
        self.shown.resize(self.order.len(), true);
        self.version = Some(tree.version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Despawning forgets the flags, in a tree or loose, so a world that
    /// hides and despawns objects all day doesn't grow.
    #[test]
    fn despawning_forgets_the_flags() {
        let mut world = World::new();
        let [root, child, loose] = std::array::from_fn(|_| world.spawn());
        world.set_parent(child, Some(root)).unwrap();
        for id in [root, child, loose] {
            world.set_active(id, false).unwrap();
        }
        world.despawn(root).unwrap();
        world.despawn(loose).unwrap();
        assert!(world.hierarchy.inactive.is_empty(), "{:?}", world.hierarchy.inactive);
    }
}

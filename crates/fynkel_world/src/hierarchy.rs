//! Parents, children and names (stage 2 plan §3.1).
//!
//! The hierarchy lives beside the components, not in them: setting a parent
//! must check for cycles and keep both sides in step, which a component
//! anyone can overwrite with `insert` couldn't promise. Names are here for a
//! simpler reason: components are `Copy`, and a name is a string.
//!
//! An object's `Transform` is relative to its parent. [`World::world_matrix`]
//! walks up to answer for one object; [`WorldMatrices`] computes them all at
//! once, parents before children, for the extract that runs every frame.

use glam::Mat4;

use crate::ObjectId;
use crate::components::Transform;
use crate::id::IdMap;
use crate::world::{Result, World, WorldError};

/// Who is whose parent, and every object's name.
#[derive(Debug, Default)]
pub(crate) struct Hierarchy {
    /// Only objects with a parent or children have a node, so a world of
    /// loose objects costs nothing here.
    nodes: IdMap<ObjectId, Node>,
    names: IdMap<ObjectId, Box<str>>,
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

    /// Drops `id`'s node once it has neither a parent nor children.
    fn prune(&mut self, id: ObjectId) {
        if self.nodes.get(&id).is_some_and(|n| n.parent.is_none() && n.children.is_empty()) {
            self.nodes.remove(&id);
        }
    }

    fn detach(&mut self, child: ObjectId) {
        let Some(parent) = self.nodes.get_mut(&child).and_then(|n| n.parent.take()) else { return };
        if let Some(node) = self.nodes.get_mut(&parent) {
            node.children.retain(|&c| c != child);
        }
        self.prune(parent);
        self.prune(child);
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
            next += 1;
        }
        subtree.swap_remove(0);
        Some(subtree)
    }
}

/// Every object's model-to-world matrix, kept for the extract.
///
/// [`update`](Self::update) recomputes them in one pass, parents before
/// children. The order is rebuilt only after a parent changes, so a steady
/// frame allocates nothing. Objects with neither parent nor children
/// aren't stored; [`world`](Self::world) uses their own transform. One of
/// these belongs to one world.
#[derive(Debug, Default)]
pub struct WorldMatrices {
    /// The hierarchy version the order was built for.
    version: Option<u64>,
    /// Every object in a tree, parents first, with its parent's index.
    order: Vec<(hecs::Entity, Option<u32>)>,
    matrices: Vec<Mat4>,
    index: IdMap<ObjectId, u32>,
}

impl WorldMatrices {
    /// Brings every matrix up to date with `world`.
    pub fn update(&mut self, world: &World) {
        let tree = &world.hierarchy;
        if self.version != Some(tree.version) {
            self.rebuild(world);
        }
        for i in 0..self.order.len() {
            let (entity, parent) = self.order[i];
            let local = world.local_matrix(entity);
            self.matrices[i] = match parent {
                Some(p) => self.matrices[p as usize] * local,
                None => local,
            };
        }
    }

    /// `id`'s model-to-world matrix as of the last [`update`](Self::update).
    /// `local` is its own transform, used when it's in no tree.
    pub fn world(&self, id: ObjectId, local: &Transform) -> Mat4 {
        self.index.get(&id).map_or_else(|| local.matrix(), |&i| self.matrices[i as usize])
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
            self.order.push((world.entities[&id], parent));
            self.index.insert(id, at);
            queue.extend(tree.children(id).iter().map(|&child| (child, Some(at))));
            next += 1;
        }
        self.matrices.clear();
        self.matrices.resize(self.order.len(), Mat4::IDENTITY);
        self.version = Some(tree.version);
    }
}

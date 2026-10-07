//! The world: objects, their components, meshes and materials.

use crate::components::{Component, MeshRenderer};
use crate::describe::{Describe, FieldError, Value};
use crate::id::{IdCounter, IdMap};
use crate::resources::{Material, MeshSource};
use crate::{MaterialId, MeshId, ObjectId};

/// Why a world operation was refused. On error, the world is unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorldError {
    /// No object has this id: it was despawned, or never existed.
    NoObject(ObjectId),
    NoComponent {
        object: ObjectId,
        component: &'static str,
    },
    NoMesh(MeshId),
    NoMaterial(MaterialId),
    /// The resource can't be removed while an object still uses it.
    MeshInUse {
        mesh: MeshId,
        object: ObjectId,
    },
    MaterialInUse {
        material: MaterialId,
        object: ObjectId,
    },
    /// A field value failed validation.
    Field(FieldError),
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::NoObject(id) => write!(f, "{id} does not exist (despawned, or never spawned)"),
            WorldError::NoComponent { object, component } => write!(f, "{object} has no {component}"),
            WorldError::NoMesh(id) => write!(f, "{id} does not exist"),
            WorldError::NoMaterial(id) => write!(f, "{id} does not exist"),
            WorldError::MeshInUse { mesh, object } => write!(f, "{mesh} is still used by {object}"),
            WorldError::MaterialInUse { material, object } => write!(f, "{material} is still used by {object}"),
            WorldError::Field(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for WorldError {}

impl From<FieldError> for WorldError {
    fn from(e: FieldError) -> Self {
        WorldError::Field(e)
    }
}

pub type Result<T> = std::result::Result<T, WorldError>;

/// Holds every object, component, mesh and material.
///
/// Objects are addressed only by [`ObjectId`]. The `hecs` entity behind an
/// id is a private detail, and no public item names a `hecs` type.
pub struct World {
    ecs: hecs::World,
    /// The only place an `ObjectId` and a `hecs::Entity` meet.
    entities: IdMap<ObjectId, hecs::Entity>,
    object_ids: IdCounter,
    meshes: IdMap<MeshId, MeshSource>,
    mesh_ids: IdCounter,
    materials: IdMap<MaterialId, Material>,
    material_ids: IdCounter,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("objects", &self.entities.len())
            .field("meshes", &self.meshes.len())
            .field("materials", &self.materials.len())
            .finish()
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            ecs: hecs::World::new(),
            entities: IdMap::default(),
            object_ids: IdCounter::new(),
            meshes: IdMap::default(),
            mesh_ids: IdCounter::new(),
            materials: IdMap::default(),
            material_ids: IdCounter::new(),
        }
    }

    // --- Objects -----------------------------------------------------------

    /// Creates an object with no components and returns its new id.
    pub fn spawn(&mut self) -> ObjectId {
        let id = ObjectId::new(self.object_ids.next());
        // The id is also stored on the entity, so queries can report it.
        let entity = self.ecs.spawn((id,));
        self.entities.insert(id, entity);
        id
    }

    /// Removes an object and all its components. Its id is never handed out
    /// again.
    pub fn despawn(&mut self, id: ObjectId) -> Result<()> {
        let entity = self.entities.remove(&id).ok_or(WorldError::NoObject(id))?;
        self.ecs.despawn(entity).expect("the id map only holds live entities");
        Ok(())
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.entities.contains_key(&id)
    }

    /// The number of objects.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn entity(&self, id: ObjectId) -> Result<hecs::Entity> {
        self.entities.get(&id).copied().ok_or(WorldError::NoObject(id))
    }

    // --- Components --------------------------------------------------------

    /// Adds `component` to the object, replacing one of the same type.
    ///
    /// Every field is validated, so values written straight into the
    /// struct's public fields are checked too. Mesh and material ids must
    /// exist.
    pub fn insert<C: Component>(&mut self, id: ObjectId, component: C) -> Result<()> {
        let entity = self.entity(id)?;
        validate_all(&component)?;
        self.check_references(&component)?;
        self.ecs.insert_one(entity, component).expect("the id map only holds live entities");
        Ok(())
    }

    /// Removes the object's `C` and returns it.
    pub fn remove<C: Component>(&mut self, id: ObjectId) -> Result<C> {
        let entity = self.entity(id)?;
        self.ecs.remove_one::<C>(entity).map_err(|_| WorldError::NoComponent { object: id, component: C::NAME })
    }

    /// A copy of the object's `C`.
    pub fn get<C: Component>(&self, id: ObjectId) -> Result<C> {
        let entity = self.entity(id)?;
        self.ecs.get::<&C>(entity).map(|c| *c).map_err(|_| WorldError::NoComponent { object: id, component: C::NAME })
    }

    pub fn has<C: Component>(&self, id: ObjectId) -> Result<bool> {
        let entity = self.entity(id)?;
        Ok(self.ecs.entity(entity).expect("the id map only holds live entities").has::<C>())
    }

    /// Calls `f` for every object that has an `A`.
    pub fn for_each<A: Component>(&self, mut f: impl FnMut(ObjectId, &A)) {
        for (id, a) in self.ecs.query::<(&ObjectId, &A)>().iter() {
            f(*id, a);
        }
    }

    /// Calls `f` for every object that has both an `A` and a `B`.
    pub fn for_each2<A: Component, B: Component>(&self, mut f: impl FnMut(ObjectId, &A, &B)) {
        for (id, a, b) in self.ecs.query::<(&ObjectId, &A, &B)>().iter() {
            f(*id, a, b);
        }
    }

    /// Mesh and material ids in a component must point at live resources.
    /// Only [`MeshRenderer`] holds such ids in stage 1. Checking every
    /// field of kind `Mesh` or `Material` keeps this right for future
    /// components without listing them here.
    fn check_references<C: Component>(&self, component: &C) -> Result<()> {
        for index in 0..C::fields().len() {
            match component.get(index) {
                Some(Value::Mesh(Some(mesh))) if !self.meshes.contains_key(&mesh) => {
                    return Err(WorldError::NoMesh(mesh));
                }
                Some(Value::Material(Some(material))) if !self.materials.contains_key(&material) => {
                    return Err(WorldError::NoMaterial(material));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The first object whose renderer satisfies `uses`, if any.
    fn user_of(&self, uses: impl Fn(&MeshRenderer) -> bool) -> Option<ObjectId> {
        self.ecs.query::<(&ObjectId, &MeshRenderer)>().iter().find(|(_, r)| uses(r)).map(|(id, _)| *id)
    }

    // --- Meshes ------------------------------------------------------------

    pub fn add_mesh(&mut self, source: MeshSource) -> MeshId {
        let id = MeshId::new(self.mesh_ids.next());
        self.meshes.insert(id, source);
        id
    }

    pub fn mesh(&self, id: MeshId) -> Result<MeshSource> {
        self.meshes.get(&id).copied().ok_or(WorldError::NoMesh(id))
    }

    /// Removes a mesh. Refused while any object's [`MeshRenderer`] uses it.
    pub fn remove_mesh(&mut self, id: MeshId) -> Result<()> {
        if !self.meshes.contains_key(&id) {
            return Err(WorldError::NoMesh(id));
        }
        if let Some(object) = self.user_of(|r| r.mesh == Some(id)) {
            return Err(WorldError::MeshInUse { mesh: id, object });
        }
        self.meshes.remove(&id);
        Ok(())
    }

    // --- Materials ---------------------------------------------------------

    /// Adds a material after validating its fields.
    pub fn add_material(&mut self, material: Material) -> Result<MaterialId> {
        validate_all(&material)?;
        let id = MaterialId::new(self.material_ids.next());
        self.materials.insert(id, material);
        Ok(id)
    }

    pub fn material(&self, id: MaterialId) -> Result<Material> {
        self.materials.get(&id).copied().ok_or(WorldError::NoMaterial(id))
    }

    /// Replaces a material's values. Every object using it changes with it.
    pub fn set_material(&mut self, id: MaterialId, material: Material) -> Result<()> {
        let slot = self.materials.get_mut(&id).ok_or(WorldError::NoMaterial(id))?;
        validate_all(&material)?;
        *slot = material;
        Ok(())
    }

    /// Removes a material. Refused while any object's [`MeshRenderer`] uses it.
    pub fn remove_material(&mut self, id: MaterialId) -> Result<()> {
        if !self.materials.contains_key(&id) {
            return Err(WorldError::NoMaterial(id));
        }
        if let Some(object) = self.user_of(|r| r.material == Some(id)) {
            return Err(WorldError::MaterialInUse { material: id, object });
        }
        self.materials.remove(&id);
        Ok(())
    }
}

/// Runs every field of `value` through the same checks [`Describe::set`]
/// uses.
fn validate_all<T: Describe>(value: &T) -> std::result::Result<(), FieldError> {
    for (index, field) in T::fields().iter().enumerate() {
        let current = value.get(index).expect("fields() and get() agree");
        crate::describe::validate(T::NAME, field, current)?;
    }
    Ok(())
}

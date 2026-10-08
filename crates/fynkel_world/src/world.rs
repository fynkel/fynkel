//! The world: objects, their components, and the meshes, materials and
//! textures they use.

use crate::components::{Component, MeshRenderer};
use crate::describe::{Describe, FieldError, Value};
use crate::hierarchy::Hierarchy;
use crate::id::{IdCounter, IdMap};
use std::sync::Arc;

use fynkel_load::ImageData;
use fynkel_mesh::MeshData;

use crate::resources::Material;
use crate::{MaterialId, MeshId, ObjectId, TextureId};

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
    NoTexture(TextureId),
    /// The resource can't be removed while an object still uses it.
    MeshInUse {
        mesh: MeshId,
        object: ObjectId,
    },
    MaterialInUse {
        material: MaterialId,
        object: ObjectId,
    },
    TextureInUse {
        texture: TextureId,
        material: MaterialId,
    },
    /// A field value failed validation.
    Field(FieldError),
    /// `parent` is `child` or below it, so it can't be `child`'s parent.
    Cycle {
        child: ObjectId,
        parent: ObjectId,
    },
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::NoObject(id) => write!(f, "{id} does not exist (despawned, or never spawned)"),
            WorldError::NoComponent { object, component } => write!(f, "{object} has no {component}"),
            WorldError::NoMesh(id) => write!(f, "{id} does not exist"),
            WorldError::NoMaterial(id) => write!(f, "{id} does not exist"),
            WorldError::NoTexture(id) => write!(f, "{id} does not exist"),
            WorldError::MeshInUse { mesh, object } => write!(f, "{mesh} is still used by {object}"),
            WorldError::MaterialInUse { material, object } => write!(f, "{material} is still used by {object}"),
            WorldError::TextureInUse { texture, material } => write!(f, "{texture} is still used by {material}"),
            WorldError::Field(e) => e.fmt(f),
            WorldError::Cycle { child, parent } => {
                write!(f, "{parent} can't be the parent of {child}: it is {child} or below it")
            }
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

/// Holds every object, component, mesh, material and texture.
///
/// Objects are addressed only by [`ObjectId`]. The `hecs` entity behind an
/// id is a private detail, and no public item names a `hecs` type.
pub struct World {
    pub(crate) ecs: hecs::World,
    /// The only place an `ObjectId` and a `hecs::Entity` meet.
    pub(crate) entities: IdMap<ObjectId, hecs::Entity>,
    pub(crate) hierarchy: Hierarchy,
    object_ids: IdCounter,
    meshes: IdMap<MeshId, Arc<MeshData>>,
    mesh_ids: IdCounter,
    materials: IdMap<MaterialId, Material>,
    material_ids: IdCounter,
    textures: IdMap<TextureId, Arc<ImageData>>,
    texture_ids: IdCounter,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("objects", &self.entities.len())
            .field("meshes", &self.meshes.len())
            .field("materials", &self.materials.len())
            .field("textures", &self.textures.len())
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
            hierarchy: Hierarchy::default(),
            object_ids: IdCounter::new(),
            meshes: IdMap::default(),
            mesh_ids: IdCounter::new(),
            materials: IdMap::default(),
            material_ids: IdCounter::new(),
            textures: IdMap::default(),
            texture_ids: IdCounter::new(),
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

    /// Removes an object, its components and its children, theirs too.
    /// None of their ids is ever handed out again.
    pub fn despawn(&mut self, id: ObjectId) -> Result<()> {
        self.entity(id)?;
        let below = self.take_subtree(id);
        for id in std::iter::once(id).chain(below.into_iter().flatten()) {
            let entity = self.entities.remove(&id).expect("the hierarchy only holds live objects");
            self.ecs.despawn(entity).expect("the id map only holds live entities");
        }
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

    pub(crate) fn entity(&self, id: ObjectId) -> Result<hecs::Entity> {
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
        // Replacing is the common case (a transform set every frame), and
        // writing in place skips hecs's archetype lookup.
        if let Ok(mut slot) = self.ecs.get::<&mut C>(entity) {
            *slot = component;
        } else {
            self.ecs.insert_one(entity, component).expect("the id map only holds live entities");
        }
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

    /// Mesh, material and texture ids in a component or material must
    /// point at live resources. Checking every field of those kinds keeps
    /// this right for future types without listing them here.
    fn check_references<T: Describe>(&self, value: &T) -> Result<()> {
        for index in 0..T::fields().len() {
            match value.get(index) {
                Some(Value::Mesh(Some(mesh))) if !self.meshes.contains_key(&mesh) => {
                    return Err(WorldError::NoMesh(mesh));
                }
                Some(Value::Material(Some(material))) if !self.materials.contains_key(&material) => {
                    return Err(WorldError::NoMaterial(material));
                }
                Some(Value::Texture(Some(texture))) if !self.textures.contains_key(&texture) => {
                    return Err(WorldError::NoTexture(texture));
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

    /// Adds a mesh. It's shared, not copied: each view's renderer uploads
    /// it from here, and raycasts read it.
    pub fn add_mesh(&mut self, mesh: impl Into<Arc<MeshData>>) -> MeshId {
        let id = MeshId::new(self.mesh_ids.next());
        self.meshes.insert(id, mesh.into());
        id
    }

    pub fn mesh(&self, id: MeshId) -> Result<&Arc<MeshData>> {
        self.meshes.get(&id).ok_or(WorldError::NoMesh(id))
    }

    /// Every mesh, in no particular order.
    pub fn meshes(&self) -> impl Iterator<Item = (MeshId, &Arc<MeshData>)> + '_ {
        self.meshes.iter().map(|(&id, mesh)| (id, mesh))
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

    /// Adds a material after validating its fields. Its textures must
    /// exist.
    pub fn add_material(&mut self, material: Material) -> Result<MaterialId> {
        validate_all(&material)?;
        self.check_references(&material)?;
        let id = MaterialId::new(self.material_ids.next());
        self.materials.insert(id, material);
        Ok(id)
    }

    pub fn material(&self, id: MaterialId) -> Result<Material> {
        self.materials.get(&id).copied().ok_or(WorldError::NoMaterial(id))
    }

    /// Replaces a material's values. Every object using it changes with it.
    pub fn set_material(&mut self, id: MaterialId, material: Material) -> Result<()> {
        if !self.materials.contains_key(&id) {
            return Err(WorldError::NoMaterial(id));
        }
        validate_all(&material)?;
        self.check_references(&material)?;
        self.materials.insert(id, material);
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

    /// Every material, in no particular order.
    pub fn materials(&self) -> impl Iterator<Item = (MaterialId, &Material)> + '_ {
        self.materials.iter().map(|(&id, material)| (id, material))
    }

    // --- Textures ----------------------------------------------------------

    /// Adds an image for materials to use. It's shared, not copied: each
    /// view's renderer uploads it from here.
    pub fn add_texture(&mut self, image: impl Into<Arc<ImageData>>) -> TextureId {
        let id = TextureId::new(self.texture_ids.next());
        self.textures.insert(id, image.into());
        id
    }

    pub fn texture(&self, id: TextureId) -> Result<&Arc<ImageData>> {
        self.textures.get(&id).ok_or(WorldError::NoTexture(id))
    }

    /// Every texture, in no particular order.
    pub fn textures(&self) -> impl Iterator<Item = (TextureId, &Arc<ImageData>)> + '_ {
        self.textures.iter().map(|(&id, image)| (id, image))
    }

    /// Removes a texture. Refused while any material uses it.
    pub fn remove_texture(&mut self, id: TextureId) -> Result<()> {
        if !self.textures.contains_key(&id) {
            return Err(WorldError::NoTexture(id));
        }
        if let Some((&material, _)) = self.materials.iter().find(|(_, m)| m.textures().contains(&Some(id))) {
            return Err(WorldError::TextureInUse { texture: id, material });
        }
        self.textures.remove(&id);
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

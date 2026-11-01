//! The world: objects, their components, and the meshes, materials and
//! textures they use.

use crate::components::{Component, MeshRenderer, Transform};
use crate::describe::{Describe, FieldError, Value};
use crate::hierarchy::Hierarchy;
use crate::id::{IdCounter, IdMap};
use std::any::TypeId;
use std::sync::Arc;

use fynkel_load::ImageData;
use fynkel_mesh::MeshData;

use crate::resources::{Material, ScreenEffect, Shader};
use crate::{FontId, MaterialId, MeshId, ObjectId, ShaderId, TextureId};

/// Why a world operation was refused. On error, the world is unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorldError {
    /// No object has this id: it was destroyed, or never added.
    NoObject(ObjectId),
    NoComponent {
        object: ObjectId,
        component: &'static str,
    },
    NoMesh(MeshId),
    NoMaterial(MaterialId),
    NoTexture(TextureId),
    NoShader(ShaderId),
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
    ShaderInUse {
        shader: ShaderId,
        material: MaterialId,
    },
    /// The shader can't be removed while a camera's screen effect uses it.
    EffectInUse {
        shader: ShaderId,
        object: ObjectId,
    },
    /// The texture can't be removed while a camera's screen effect uses it.
    EffectTextureInUse {
        texture: TextureId,
        object: ObjectId,
    },
    /// A screen effect's shader given a material, or a material's given a
    /// camera's effects (`effect`: whether it's an effect's).
    ShaderKind {
        shader: ShaderId,
        effect: bool,
    },
    /// A screen effect's values aren't as many bytes as its shader's: it
    /// has `size`.
    EffectValues {
        shader: ShaderId,
        given: usize,
        size: usize,
    },
    /// Values written past the end of a material's shader's: it has
    /// `size` bytes of them.
    Values {
        material: MaterialId,
        end: usize,
        size: usize,
    },
    /// A field value failed validation.
    Field(FieldError),
    /// `parent` is `child` or below it, so it can't be `child`'s parent.
    Cycle {
        child: ObjectId,
        parent: ObjectId,
    },
    /// Layers are 0 to 31.
    Layer(u32),
    /// A model was spawned with a different number of mesh or material
    /// ids than it has meshes or materials.
    ModelIds {
        what: &'static str,
        given: usize,
        needed: usize,
    },
    /// A model's animator has `count` clips; `clip` isn't one.
    NoClip {
        object: ObjectId,
        clip: u32,
        count: u32,
    },
    /// An animator's layer, blend or mask that can't be.
    Animator(&'static str),
    /// A particle system's settings that can't be, or a call it can't take.
    Particles(&'static str),
    NoFont(FontId),
    /// The font can't be removed while an object's text uses it.
    FontInUse {
        font: FontId,
        object: ObjectId,
    },
    /// Bytes that aren't a font the engine reads.
    Font,
    /// A text renderer's settings that can't be.
    Text(&'static str),
    /// A sprite renderer's settings that can't be.
    Sprite(&'static str),
    /// The texture can't be removed while an object's sprite shows it.
    SpriteTextureInUse {
        texture: TextureId,
        object: ObjectId,
    },
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::NoObject(id) => write!(f, "{id} does not exist (destroyed, or never added)"),
            WorldError::NoComponent { object, component } => write!(f, "{object} has no {component}"),
            WorldError::NoMesh(id) => write!(f, "{id} does not exist"),
            WorldError::NoMaterial(id) => write!(f, "{id} does not exist"),
            WorldError::NoTexture(id) => write!(f, "{id} does not exist"),
            WorldError::NoShader(id) => write!(f, "{id} does not exist"),
            WorldError::MeshInUse { mesh, object } => write!(f, "{mesh} is still used by {object}"),
            WorldError::MaterialInUse { material, object } => write!(f, "{material} is still used by {object}"),
            WorldError::TextureInUse { texture, material } => write!(f, "{texture} is still used by {material}"),
            WorldError::ShaderInUse { shader, material } => write!(f, "{shader} is still used by {material}"),
            WorldError::EffectInUse { shader, object } => {
                write!(f, "{shader} is still a screen effect on {object}'s camera")
            }
            WorldError::EffectTextureInUse { texture, object } => {
                write!(f, "{texture} is still used by a screen effect on {object}'s camera")
            }
            WorldError::ShaderKind { shader, effect: true } => {
                write!(f, "{shader} is a screen effect's shader: it goes on a camera, not a material")
            }
            WorldError::ShaderKind { shader, effect: false } => {
                write!(f, "{shader} is a material's shader, not a screen effect's (fn effect)")
            }
            WorldError::EffectValues { shader, given, size } => {
                write!(f, "{shader} has {size} bytes of values; a screen effect gave {given}")
            }
            WorldError::Values { material, end, size } => {
                write!(f, "{material}'s shader has {size} bytes of values; {end} is past them")
            }
            WorldError::Field(e) => e.fmt(f),
            WorldError::Cycle { child, parent } => {
                write!(f, "{parent} can't be the parent of {child}: it is {child} or below it")
            }
            WorldError::Layer(layer) => write!(f, "layer {layer} doesn't exist: layers are 0 to 31"),
            WorldError::ModelIds { what, given, needed } => {
                write!(f, "the model has {needed} {what}, but {given} ids were given for them")
            }
            WorldError::NoClip { object, clip, count } => {
                write!(f, "{object}'s model has {count} clips; there's no clip {clip}")
            }
            WorldError::Particles(why) => f.write_str(why),
            WorldError::NoFont(id) => write!(f, "{id} does not exist"),
            WorldError::FontInUse { font, object } => write!(f, "{font} is still used by {object}'s text"),
            WorldError::Font => f.write_str("not a TrueType or OpenType font the engine can read"),
            WorldError::Text(why) => f.write_str(why),
            WorldError::Sprite(why) => f.write_str(why),
            WorldError::SpriteTextureInUse { texture, object } => {
                write!(f, "{texture} is still shown by {object}'s sprite")
            }
            WorldError::Animator(message) => f.write_str(message),
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
    /// Each mesh, and its version: a new one each time it's set.
    meshes: IdMap<MeshId, (Arc<MeshData>, u64)>,
    /// The materials an object's mesh's submeshes after the first are
    /// drawn with (stage 13), for those that have any.
    submesh_materials: IdMap<ObjectId, Vec<MaterialId>>,
    mesh_ids: IdCounter,
    materials: IdMap<MaterialId, Material>,
    material_ids: IdCounter,
    textures: IdMap<TextureId, Arc<ImageData>>,
    texture_ids: IdCounter,
    /// Each shader, and its version: a new one each time it's set.
    shaders: IdMap<ShaderId, (Shader, u64)>,
    shader_ids: IdCounter,
    shader_versions: u64,
    /// The values of each material with a shader that has some, laid out
    /// as its `Values`.
    material_values: IdMap<MaterialId, Vec<u8>>,
    /// The time and the time since the last frame, as shaders see them.
    shader_time: [f32; 2],
    /// [`World::insert_all`]'s entities, kept between calls so a frame's
    /// batch allocates nothing.
    batch: Vec<hecs::Entity>,
    pub(crate) debug_lines: Vec<crate::DebugLine>,
    /// Each model copy's animator, by its root.
    pub(crate) animators: IdMap<ObjectId, crate::animation::Animator>,
    /// See [`World::transforms_version`].
    pub(crate) transforms_version: u64,
    /// The roots of the animators the last [`World::animate`] posed.
    pub(crate) posed: Vec<ObjectId>,
    /// The skin of each model copy's skinned part, by the part.
    pub(crate) skins: IdMap<ObjectId, crate::skin::Skin>,
    /// Each camera's screen effects, in order, by its object.
    camera_effects: IdMap<ObjectId, Vec<ScreenEffect>>,
    /// Each object's particle system.
    pub(crate) particles: IdMap<ObjectId, crate::particles::ParticleSystem>,
    /// The next new system's own seed.
    pub(crate) particle_seeds: u64,
    /// Each system's sub emitters: (the system, which of its sub
    /// emitters, the sub emitter's system).
    pub(crate) particle_links: Vec<(ObjectId, usize, ObjectId)>,
    /// Each object's trail.
    pub(crate) trails: IdMap<ObjectId, crate::trails::Trail>,
    /// The fonts kept for flat text.
    pub(crate) fonts: IdMap<FontId, crate::texts::WorldFont>,
    pub(crate) font_ids: IdCounter,
    /// Each object's text renderer.
    pub(crate) texts: IdMap<ObjectId, crate::texts::TextRenderer>,
    /// Each object's sprite renderer.
    pub(crate) sprites: IdMap<ObjectId, crate::sprites::SpriteRenderer>,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("objects", &self.entities.len())
            .field("meshes", &self.meshes.len())
            .field("materials", &self.materials.len())
            .field("textures", &self.textures.len())
            .field("shaders", &self.shaders.len())
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
            submesh_materials: IdMap::default(),
            mesh_ids: IdCounter::new(),
            materials: IdMap::default(),
            material_ids: IdCounter::new(),
            textures: IdMap::default(),
            texture_ids: IdCounter::new(),
            shaders: IdMap::default(),
            shader_ids: IdCounter::new(),
            shader_versions: 0,
            material_values: IdMap::default(),
            shader_time: [0.0; 2],
            batch: Vec::new(),
            debug_lines: Vec::new(),
            animators: IdMap::default(),
            transforms_version: 0,
            posed: Vec::new(),
            skins: IdMap::default(),
            camera_effects: IdMap::default(),
            particles: IdMap::default(),
            particle_seeds: 1,
            particle_links: Vec::new(),
            trails: IdMap::default(),
            fonts: IdMap::default(),
            font_ids: IdCounter::new(),
            texts: IdMap::default(),
            sprites: IdMap::default(),
        }
    }

    // --- Objects -----------------------------------------------------------

    /// Creates an object with no components and returns its new id.
    pub fn spawn(&mut self) -> ObjectId {
        let id = ObjectId::new(self.object_ids.next());
        // The id is also stored on the entity, so queries can report it.
        let entity = self.ecs.spawn((id,));
        self.entities.insert(id, entity);
        self.transforms_version += 1;
        id
    }

    /// A number that goes up whenever an object's world matrix, or whether
    /// it's shown, may have changed: a `Transform` set or removed, an
    /// object spawned or despawned, turned on or off, given a new parent,
    /// or posed by its animator. While it and the hierarchy stay the same,
    /// [`WorldMatrices`](crate::hierarchy::WorldMatrices) has nothing to
    /// work out again.
    pub fn transforms_version(&self) -> u64 {
        self.transforms_version
    }

    /// Removes an object, its components and its children, theirs too.
    /// None of their ids is ever handed out again.
    pub fn despawn(&mut self, id: ObjectId) -> Result<()> {
        self.entity(id)?;
        let below = self.take_subtree(id);
        let mut unlinked = false;
        for id in std::iter::once(id).chain(below.into_iter().flatten()) {
            let entity = self.entities.remove(&id).expect("the hierarchy only holds live objects");
            if !self.animators.is_empty() {
                self.animators.remove(&id);
            }
            if !self.skins.is_empty() {
                self.skins.remove(&id);
            }
            if !self.submesh_materials.is_empty() {
                self.submesh_materials.remove(&id);
            }
            if !self.camera_effects.is_empty() {
                self.camera_effects.remove(&id);
            }
            if !self.trails.is_empty() {
                self.trails.remove(&id);
            }
            if !self.texts.is_empty() {
                self.texts.remove(&id);
            }
            if !self.sprites.is_empty() {
                self.sprites.remove(&id);
            }
            if !self.particles.is_empty() && self.particles.remove(&id).is_some() {
                unlinked = true;
            }
            self.ecs.despawn(entity).expect("the id map only holds live entities");
        }
        if unlinked {
            self.relink_particles();
        }
        self.transforms_version += 1;
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
        let entity = self.check(id, &component)?;
        self.write(entity, component);
        Ok(())
    }

    /// [`insert`](Self::insert) for many objects at once: all are checked
    /// first, so if one is refused none is written. `items` is gone through
    /// twice, so it's cloned once.
    pub fn insert_all<C: Component>(&mut self, items: impl Iterator<Item = (ObjectId, C)> + Clone) -> Result<()> {
        let mut entities = std::mem::take(&mut self.batch);
        entities.clear();
        let checked = items.clone().try_for_each(|(id, component)| {
            entities.push(self.check(id, &component)?);
            Ok(())
        });
        if checked.is_ok() {
            for ((_, component), &entity) in items.zip(&entities) {
                self.write(entity, component);
            }
        }
        self.batch = entities;
        checked
    }

    /// Writes a checked component.
    fn write<C: Component>(&mut self, entity: hecs::Entity, component: C) {
        if TypeId::of::<C>() == TypeId::of::<Transform>() {
            self.transforms_version += 1;
        }
        // Replacing is the common case (a transform set every frame), and
        // writing in place skips hecs's archetype lookup; `query_one_mut`
        // also skips its run-time borrow.
        if let Ok(slot) = self.ecs.query_one_mut::<&mut C>(entity) {
            *slot = component;
        } else {
            self.ecs.insert_one(entity, component).expect("the id map only holds live entities");
        }
    }

    /// Whether [`insert`](Self::insert) would take `component` for the
    /// object, without changing anything (so many can be checked before
    /// any is written), and the object's entity if so.
    fn check<C: Component>(&self, id: ObjectId, component: &C) -> Result<hecs::Entity> {
        let entity = self.entity(id)?;
        validate_all(component)?;
        self.check_references(component)?;
        Ok(entity)
    }

    /// Removes the object's `C` and returns it.
    pub fn remove<C: Component>(&mut self, id: ObjectId) -> Result<C> {
        let entity = self.entity(id)?;
        if TypeId::of::<C>() == TypeId::of::<Transform>() {
            self.transforms_version += 1;
        }
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
        self.meshes.insert(id, (mesh.into(), 0));
        id
    }

    pub fn mesh(&self, id: MeshId) -> Result<&Arc<MeshData>> {
        self.meshes.get(&id).map(|(mesh, _)| mesh).ok_or(WorldError::NoMesh(id))
    }

    /// Changes a mesh (stage 13): every object drawn with it, and each
    /// view, has the new one from the next frame. The data is shared, not
    /// copied.
    pub fn set_mesh(&mut self, id: MeshId, mesh: impl Into<Arc<MeshData>>) -> Result<()> {
        let (data, version) = self.meshes.get_mut(&id).ok_or(WorldError::NoMesh(id))?;
        *data = mesh.into();
        *version += 1;
        Ok(())
    }

    /// The version of mesh `id`: a new one each time it's set.
    pub(crate) fn mesh_version(&self, id: MeshId) -> u64 {
        self.meshes.get(&id).map_or(0, |(_, version)| *version)
    }

    /// Whether mesh `id` is in the world.
    pub(crate) fn has_mesh(&self, id: MeshId) -> bool {
        self.meshes.contains_key(&id)
    }

    /// The number of meshes.
    pub(crate) fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Every mesh, and its version, in no particular order.
    pub fn meshes(&self) -> impl Iterator<Item = (MeshId, &Arc<MeshData>, u64)> + '_ {
        self.meshes.iter().map(|(&id, (mesh, version))| (id, mesh, *version))
    }

    /// The materials `object`'s mesh's submeshes after the first are drawn
    /// with, one each (its `MeshRenderer`'s material draws the first), as
    /// Unity's `Renderer.materials`. A submesh without one isn't drawn.
    /// None takes them away.
    pub fn set_submesh_materials(&mut self, object: ObjectId, materials: &[MaterialId]) -> Result<()> {
        self.entity(object)?;
        if let Some(&missing) = materials.iter().find(|m| !self.materials.contains_key(m)) {
            return Err(WorldError::NoMaterial(missing));
        }
        if materials.is_empty() {
            self.submesh_materials.remove(&object);
        } else {
            let kept = self.submesh_materials.entry(object).or_default();
            kept.clear();
            kept.extend_from_slice(materials);
        }
        Ok(())
    }

    /// [`World::set_submesh_materials`]' materials for `object`.
    pub fn submesh_materials(&self, object: ObjectId) -> &[MaterialId] {
        self.submesh_materials.get(&object).map_or(&[], Vec::as_slice)
    }

    /// Whether any object has materials for submeshes after the first.
    pub fn has_submesh_materials(&self) -> bool {
        !self.submesh_materials.is_empty()
    }

    /// Removes a mesh. Refused while any object's [`MeshRenderer`] uses it,
    /// or a particle system draws its particles with it.
    pub fn remove_mesh(&mut self, id: MeshId) -> Result<()> {
        if !self.meshes.contains_key(&id) {
            return Err(WorldError::NoMesh(id));
        }
        if let Some(object) = self.user_of(|r| r.mesh == Some(id)) {
            return Err(WorldError::MeshInUse { mesh: id, object });
        }
        let drawn_with = |s: &crate::ParticleSystem| s.settings().render_mode == crate::RenderMode::Mesh(id);
        if let Some((&object, _)) = self.particles.iter().find(|(_, s)| drawn_with(s)) {
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
        self.check_shader(&material)?;
        let id = MaterialId::new(self.material_ids.next());
        self.materials.insert(id, material);
        self.reset_values(id);
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
        self.check_shader(&material)?;
        let old = self.materials.insert(id, material).expect("checked above");
        // Its values are its shader's: a new shader starts from zeros.
        if old.shader != material.shader {
            self.reset_values(id);
        }
        Ok(())
    }

    fn check_shader(&self, material: &Material) -> Result<()> {
        match material.shader.map(|s| (s, self.shaders.get(&s))) {
            Some((shader, None)) => Err(WorldError::NoShader(shader)),
            Some((shader, Some((s, _)))) if s.effect => Err(WorldError::ShaderKind { shader, effect: true }),
            _ => Ok(()),
        }
    }

    /// Zeros for `id`'s values, as many as its shader has.
    fn reset_values(&mut self, id: MaterialId) {
        let size = self.materials[&id].shader.map_or(0, |s| self.shaders[&s].0.values_size);
        if size == 0 {
            self.material_values.remove(&id);
        } else {
            self.material_values.insert(id, vec![0; size]);
        }
    }

    /// A material's values, laid out as its shader's `Values`: empty
    /// without a shader, or with one that has none.
    pub fn material_values(&self, id: MaterialId) -> Result<&[u8]> {
        if !self.materials.contains_key(&id) {
            return Err(WorldError::NoMaterial(id));
        }
        Ok(self.material_values.get(&id).map_or(&[], Vec::as_slice))
    }

    /// Writes `bytes` into a material's values from `offset`, where its
    /// shader's `Values` puts the one being set. Every object using it
    /// changes with it.
    pub fn set_material_values(&mut self, id: MaterialId, offset: usize, bytes: &[u8]) -> Result<()> {
        if !self.materials.contains_key(&id) {
            return Err(WorldError::NoMaterial(id));
        }
        let values = self.material_values.get_mut(&id);
        let size = values.as_ref().map_or(0, |v| v.len());
        let end = offset.saturating_add(bytes.len());
        if end > size {
            return Err(WorldError::Values { material: id, end, size });
        }
        if let Some(values) = values {
            values[offset..end].copy_from_slice(bytes);
        }
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
        if let Some((&object, _)) = self.particles.iter().find(|(_, s)| s.settings().material == Some(id)) {
            return Err(WorldError::MaterialInUse { material: id, object });
        }
        if let Some((&object, _)) = self.trails.iter().find(|(_, t)| t.settings().material == Some(id)) {
            return Err(WorldError::MaterialInUse { material: id, object });
        }
        if let Some((&object, _)) = self.submesh_materials.iter().find(|(_, m)| m.contains(&id)) {
            return Err(WorldError::MaterialInUse { material: id, object });
        }
        self.materials.remove(&id);
        self.material_values.remove(&id);
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
        if let Some((&material, _)) = self.materials.iter().find(|(_, m)| m.uses_texture(id)) {
            return Err(WorldError::TextureInUse { texture: id, material });
        }
        if let Some((&object, _)) =
            self.camera_effects.iter().find(|(_, e)| e.iter().any(|e| e.textures.contains(&Some(id))))
        {
            return Err(WorldError::EffectTextureInUse { texture: id, object });
        }
        if let Some((&object, _)) = self.sprites.iter().find(|(_, s)| s.settings().texture == id) {
            return Err(WorldError::SpriteTextureInUse { texture: id, object });
        }
        self.textures.remove(&id);
        Ok(())
    }

    // --- Shaders -----------------------------------------------------------

    /// Adds a material's own shader, compiled.
    pub fn add_shader(&mut self, shader: Shader) -> ShaderId {
        let id = ShaderId::new(self.shader_ids.next());
        self.shader_versions += 1;
        self.shaders.insert(id, (shader, self.shader_versions));
        id
    }

    pub fn shader(&self, id: ShaderId) -> Result<&Shader> {
        self.shaders.get(&id).map(|(s, _)| s).ok_or(WorldError::NoShader(id))
    }

    /// Replaces a shader, as when its file changes: its materials draw
    /// with the new one. Their values keep what fits; new ones are 0.
    pub fn set_shader(&mut self, id: ShaderId, shader: Shader) -> Result<()> {
        if !self.shaders.contains_key(&id) {
            return Err(WorldError::NoShader(id));
        }
        // An effect's stays one, and a material's a material's, while used.
        if shader.effect != self.shaders[&id].0.effect {
            let used = if shader.effect {
                self.materials.values().any(|m| m.shader == Some(id))
            } else {
                self.camera_effects.values().flatten().any(|e| e.shader == id)
            };
            if used {
                return Err(WorldError::ShaderKind { shader: id, effect: !shader.effect });
            }
        }
        let size = shader.values_size;
        self.shader_versions += 1;
        self.shaders.insert(id, (shader, self.shader_versions));
        for effect in self.camera_effects.values_mut().flatten().filter(|e| e.shader == id) {
            effect.values.resize(size, 0);
        }
        for (material, m) in &self.materials {
            if m.shader == Some(id) {
                if size == 0 {
                    self.material_values.remove(material);
                } else {
                    self.material_values.entry(*material).or_default().resize(size, 0);
                }
            }
        }
        Ok(())
    }

    /// Removes a shader. Refused while any material uses it.
    pub fn remove_shader(&mut self, id: ShaderId) -> Result<()> {
        if !self.shaders.contains_key(&id) {
            return Err(WorldError::NoShader(id));
        }
        if let Some((&material, _)) = self.materials.iter().find(|(_, m)| m.shader == Some(id)) {
            return Err(WorldError::ShaderInUse { shader: id, material });
        }
        if let Some((&object, _)) = self.camera_effects.iter().find(|(_, e)| e.iter().any(|e| e.shader == id)) {
            return Err(WorldError::EffectInUse { shader: id, object });
        }
        self.shaders.remove(&id);
        Ok(())
    }

    /// Every shader and its version, which changes each time it's set, in
    /// no particular order.
    pub fn shaders(&self) -> impl Iterator<Item = (ShaderId, &Shader, u64)> + '_ {
        self.shaders.iter().map(|(&id, (shader, version))| (id, shader, *version))
    }

    /// Gives the camera on `object` these screen effects, in place of any
    /// it had: run over its picture in order. Each one's shader must be an
    /// effect's, its values as many bytes as the shader's, and its
    /// textures there.
    pub fn set_camera_effects(&mut self, object: ObjectId, effects: Vec<ScreenEffect>) -> Result<()> {
        self.get::<crate::Camera>(object)?;
        for effect in &effects {
            for &texture in effect.textures.iter().flatten() {
                self.texture(texture)?;
            }
            let shader = self.shader(effect.shader)?;
            if !shader.effect {
                return Err(WorldError::ShaderKind { shader: effect.shader, effect: false });
            }
            if effect.values.len() != shader.values_size {
                let (given, size) = (effect.values.len(), shader.values_size);
                return Err(WorldError::EffectValues { shader: effect.shader, given, size });
            }
        }
        if effects.is_empty() {
            self.camera_effects.remove(&object);
        } else {
            self.camera_effects.insert(object, effects);
        }
        Ok(())
    }

    /// The screen effects of the camera on `object`, in order: none when
    /// it has none, or isn't there.
    pub fn camera_effects(&self, object: ObjectId) -> &[ScreenEffect] {
        self.camera_effects.get(&object).map_or(&[], Vec::as_slice)
    }

    /// The time shaders see, and how far it moved since the last frame:
    /// the game's, so it stops while it's paused.
    pub fn shader_time(&self) -> [f32; 2] {
        self.shader_time
    }

    pub fn set_shader_time(&mut self, time: f32, delta: f32) {
        self.shader_time = [time, delta];
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

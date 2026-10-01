//! Decoding is fallible; resource publication consumes validated owned data.
use super::*;
use fynkel_core::{components::ComponentValue, physics::PreparedMesh, ColliderShape, World};

/// A decoded prefab and its resource closure. Preparation never mutates a world.
/// Dropping this value releases its residency lease without registering resources.
pub struct PreparedPrefab {
    prefab: CookedPrefab,
    resources: PreparedGameplayResources,
    residency: Residency,
}

/// Published resource ownership. Keep the lease while instances need the assets.
pub struct PublishedPrefab {
    pub prefab: CookedPrefab,
    pub residency: Residency,
}

pub type PreparedSessionResult =
    Result<(fynkel_core::EditCommit<()>, Vec<PublishedPrefab>), PreparedPrefabError>;

/// A prepared-resource capacity rejection or a rejected gameplay edit.
#[derive(Debug)]
pub enum PreparedPrefabError {
    Resources(Diagnostic),
    Edit(fynkel_core::EditError),
}
impl std::fmt::Display for PreparedPrefabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resources(d) => d.fmt(f),
            Self::Edit(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for PreparedPrefabError {}

#[derive(Default)]
pub struct PreparedGameplayResources {
    meshes: HashMap<AssetRef, PreparedMesh>,
    materials: HashMap<AssetRef, PhysicsMaterial>,
    clips: HashMap<AssetRef, Arc<Clip>>,
    blends: HashMap<AssetRef, Arc<BlendSpace>>,
    controllers: HashMap<AssetRef, Arc<ControllerDef>>,
    skins: HashMap<AssetRef, Arc<Skin>>,
    sounds: HashMap<AssetRef, Arc<AudioClip>>,
    prefabs: HashMap<Uuid, SceneDescription>,
}

impl PreparedPrefab {
    pub fn stage_controllers(
        &self,
        session: &mut fynkel_core::GameplaySession,
    ) -> fynkel_core::EditResult<()> {
        self.resources.stage_controllers(session)
    }

    pub fn prefab(&self) -> &CookedPrefab {
        &self.prefab
    }

    /// Checks the only world-dependent capacity limit, without mutation. Call
    /// again immediately before publication if the world has changed meanwhile.
    pub fn validate_publication(&self, world: &World) -> Result<(), Diagnostic> {
        let additional = self
            .resources
            .sounds
            .keys()
            .filter(|key| !world.has_sound(key))
            .count();
        if additional > fynkel_core::audio::MAX_CLIPS.saturating_sub(world.audio().clip_count()) {
            return Err(Diagnostic::error(
                Code::RUNTIME_PAYLOAD_MALFORMED,
                "prefab resources exceed available audio clip slots",
            )
            .with_id(self.prefab.id.to_string()));
        }
        Ok(())
    }

    /// Commits gameplay configuration and prepared resources together. The
    /// callback uses the ordinary native journal and may instantiate the supplied
    /// description under live or pending parents, or make other configuration
    /// edits. Resource registration occurs only after successful data commit.
    /// The exclusive world borrow prevents capacity changes between preflight
    /// and publication. There is no I/O or user callback after commit.
    pub fn commit<R>(
        self,
        world: &mut World,
        edit: impl FnOnce(
            &mut fynkel_core::GameplayEdit<'_>,
            &CookedPrefab,
        ) -> fynkel_core::EditResult<R>,
    ) -> Result<(fynkel_core::EditCommit<R>, PublishedPrefab), PreparedPrefabError> {
        self.validate_publication(world)
            .map_err(PreparedPrefabError::Resources)?;
        let commit = world
            .edit_gameplay(|access| edit(access, &self.prefab))
            .map_err(PreparedPrefabError::Edit)?;
        Ok((commit, self.publish_validated(world)))
    }

    /// Finishes an owned bridge session with the union of its prepared resources.
    /// Capacity is checked once across all plans before any data publication.
    pub fn finish_session(
        plans: Vec<Self>,
        session: fynkel_core::GameplaySession,
    ) -> (World, PreparedSessionResult) {
        Self::finish_session_with_resources(plans, PreparedGameplayResources::default(), session)
    }

    pub fn finish_session_with_resources(
        plans: Vec<Self>,
        resources: PreparedGameplayResources,
        session: fynkel_core::GameplaySession,
    ) -> (World, PreparedSessionResult) {
        let world = session.committed();
        let sounds: HashSet<_> = plans
            .iter()
            .flat_map(|p| p.resources.sounds.keys())
            .chain(resources.sounds.keys())
            .filter(|key| !world.has_sound(key))
            .collect();
        if sounds.len() > fynkel_core::audio::MAX_CLIPS.saturating_sub(world.audio().clip_count()) {
            return (
                session.abort(),
                Err(PreparedPrefabError::Resources(Diagnostic::error(
                    Code::RUNTIME_PAYLOAD_MALFORMED,
                    "prefab resources exceed available audio clip slots",
                ))),
            );
        }
        let (mut world, result) = session.finish();
        let result = match result {
            Ok(commit) => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                resources.publish_validated(&mut world);
                plans
                    .into_iter()
                    .map(|p| p.publish_validated(&mut world))
                    .collect()
            })) {
                Ok(published) => Ok((commit, published)),
                Err(_) => {
                    world.poison();
                    Err(PreparedPrefabError::Edit(fynkel_core::EditError::Poisoned))
                }
            },
            Err(error) => Err(PreparedPrefabError::Edit(error)),
        };
        (world, result)
    }

    /// Publishes only resource registrations, not prefab entities. All I/O,
    /// decoding and value validation already happened. Capacity is checked before
    /// any registry changes; an error leaves every world registry untouched.
    ///
    /// This is not a gameplay transaction commit hook yet. A caller must not
    /// publish these resources before an edit that can still fail.
    pub fn publish(self, world: &mut World) -> Result<PublishedPrefab, Diagnostic> {
        self.validate_publication(world)?;
        Ok(self.publish_validated(world))
    }

    fn publish_validated(self, world: &mut World) -> PublishedPrefab {
        self.resources.publish_validated(world);
        PublishedPrefab {
            prefab: self.prefab,
            residency: self.residency,
        }
    }
}

impl Assets {
    /// Prepares a cooked prefab and every emitter dependency without registering
    /// resources in a world. Missing/invalid required resources reject preparation;
    /// decoded store caches may warm, but no world state is changed.
    pub fn prepare_prefab(&self, id: Uuid) -> Result<PreparedPrefab, Vec<Diagnostic>> {
        // Acquire before decoding, so failure releases newly cached closure data.
        let closure = self.lock().index.closure(id);
        let residency = self.retain(&closure);
        let prefab = self.prefab(id)?;
        let mut resources = PreparedGameplayResources::default();
        let mut pending = vec![prefab.description.clone()];
        let mut seen = HashSet::from([id]);
        while let Some(scene) = pending.pop() {
            resources.scene(self, &scene).map_err(|d| vec![d])?;
            for e in &scene.entities {
                for c in &e.components {
                    if let ComponentValue::Emitter(em) = c {
                        let id = em.prefab.asset;
                        if !id.is_nil() && seen.insert(id) {
                            let nested = self.prefab(id)?;
                            pending.push(nested.description.clone());
                            resources.prefabs.insert(id, nested.description);
                        }
                    }
                }
            }
        }
        // A recursive emitter naming the root must find its prepared description.
        resources.prefabs.insert(id, prefab.description.clone());
        Ok(PreparedPrefab {
            prefab,
            resources,
            residency,
        })
    }
}

impl PreparedGameplayResources {
    fn publish_validated(self, world: &mut World) {
        let r = self;
        for (key, mesh) in r.meshes {
            if !world.physics().has_mesh(&key) {
                world.physics_mut().register_prepared_mesh(key, mesh);
            }
        }
        for (key, material) in r.materials {
            world
                .physics_mut()
                .register_material(key, material)
                .expect("prepared material validated");
        }
        for (key, clip) in r.clips {
            if !world.has_clip(&key) {
                world.register_clip(key, (*clip).clone());
            }
        }
        for (key, blend) in r.blends {
            if !world.has_blend(&key) {
                world.register_blend(key, (*blend).clone());
            }
        }
        for (key, controller) in r.controllers {
            if !world.has_controller(&key) {
                world
                    .register_controller(key, (*controller).clone())
                    .expect("prepared controller validated");
            }
        }
        for (key, skin) in r.skins {
            if !world.has_skin(&key) {
                world.register_skin(key, (*skin).clone());
            }
        }
        // Audio slot assignment must not depend on randomized HashMap iteration.
        let mut sounds: Vec<_> = r.sounds.into_iter().collect();
        sounds.sort_by(|(a, _), (b, _)| (a.asset, &a.sub).cmp(&(b.asset, &b.sub)));
        for (key, sound) in sounds {
            if !world.has_sound(&key) {
                world
                    .register_sound(key, sound)
                    .expect("audio capacity checked");
            }
        }
        for (id, prefab) in r.prefabs {
            if !world.has_prefab(id) {
                world.register_prefab(id, prefab);
            }
        }
    }

    /// Controller definitions are visible to validation in the owning edit only.
    pub fn stage_controllers(
        &self,
        session: &mut fynkel_core::GameplaySession,
    ) -> fynkel_core::EditResult<()> {
        session.access(|edit| {
            for (key, def) in &self.controllers {
                edit.prepare_controller(key.clone(), def.clone())?;
            }
            Ok(())
        })
    }

    /// Decode dependencies without mutating world registries. Missing assets keep
    /// their existing execution-time diagnostics; successfully decoded resources
    /// publish only if the owning gameplay session commits.
    pub fn component(&mut self, assets: &Assets, world: &World, value: &ComponentValue) {
        let mut value = value.clone();
        match &mut value {
            ComponentValue::Animator(a) => {
                // Clip and controller are independent dependencies. A stale
                // inactive clip must not prevent preparing the selected controller.
                self.motion_request(assets, world, &a.clip);
                a.clip = AssetRef::default();
                if a.controller
                    .as_ref()
                    .is_some_and(|c| world.has_controller(c))
                {
                    a.controller = None;
                }
            }
            ComponentValue::AudioSource(a) if world.has_sound(&a.clip) => return,
            ComponentValue::Skin(s) if world.has_skin(&s.skin) => return,
            _ => {}
        }
        let _ = self.component_value(assets, &value);
    }

    pub fn motion_request(&mut self, assets: &Assets, world: &World, key: &AssetRef) {
        if !world.has_clip(key) && !world.has_blend(key) {
            let _ = self.motion(assets, key);
        }
    }

    pub fn sound_request(&mut self, assets: &Assets, world: &World, key: &AssetRef) {
        if !key.asset.is_nil() && !world.has_sound(key) && !self.sounds.contains_key(key) {
            if let Ok(sound) = assets.sound(key) {
                self.sounds.insert(key.clone(), sound);
            }
        }
    }

    fn motion(&mut self, assets: &Assets, key: &AssetRef) -> Result<(), Diagnostic> {
        if key.asset.is_nil() || self.clips.contains_key(key) || self.blends.contains_key(key) {
            return Ok(());
        }
        if assets.is_blend(key) {
            let blend = assets.blend(key)?;
            for point in &blend.points {
                if !self.clips.contains_key(&point.clip) {
                    self.clips
                        .insert(point.clip.clone(), assets.clip(&point.clip)?);
                }
            }
            self.blends.insert(key.clone(), blend);
        } else {
            self.clips.insert(key.clone(), assets.clip(key)?);
        }
        Ok(())
    }
    fn scene(&mut self, assets: &Assets, scene: &SceneDescription) -> Result<(), Diagnostic> {
        for e in &scene.entities {
            for c in &e.components {
                self.component_value(assets, c)?;
            }
        }
        Ok(())
    }
    fn component_value(&mut self, assets: &Assets, c: &ComponentValue) -> Result<(), Diagnostic> {
        match c {
            ComponentValue::Collider(c) => {
                if let Some(key) = c.material.as_ref().filter(|key| !key.asset.is_nil()) {
                    if !self.materials.contains_key(key) {
                        let material = assets.physics_material(key)?;
                        material
                            .validate()
                            .map_err(|why| assets.malformed(key, "JSON").with_suggestion(why))?;
                        self.materials.insert(key.clone(), material);
                    }
                }
                if c.shape == ColliderShape::Mesh {
                    if let Some(key) = &c.mesh {
                        if !self.meshes.contains_key(key) {
                            let mesh = assets.mesh(key)?;
                            let positions: Vec<_> =
                                mesh.vertices.iter().map(|v| v.position).collect();
                            let prepared =
                                PreparedMesh::new(&positions, &mesh.indices).map_err(|why| {
                                    assets.malformed(key, "MESH").with_suggestion(why)
                                })?;
                            self.meshes.insert(key.clone(), prepared);
                        }
                    }
                }
            }
            ComponentValue::Animator(a) => {
                self.motion(assets, &a.clip)?;
                if let Some(key) = a.controller.as_ref().filter(|key| !key.asset.is_nil()) {
                    if !self.controllers.contains_key(key) {
                        let controller = assets.controller(key)?;
                        for layer in &controller.layers {
                            for state in &layer.states {
                                let mut motions = Vec::new();
                                if let Some(motion) = &state.motion {
                                    motion.assets(&mut motions);
                                }
                                for key in motions {
                                    self.motion(assets, key)?;
                                }
                            }
                        }
                        self.controllers.insert(key.clone(), controller);
                    }
                }
            }
            ComponentValue::Skin(s) if !s.skin.asset.is_nil() => {
                if !self.skins.contains_key(&s.skin) {
                    self.skins.insert(s.skin.clone(), assets.skin(&s.skin)?);
                }
            }
            ComponentValue::AudioSource(a) if !a.clip.asset.is_nil() => {
                if !self.sounds.contains_key(&a.clip) {
                    self.sounds.insert(a.clip.clone(), assets.sound(&a.clip)?);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

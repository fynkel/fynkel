//! The Rapier adapter (the `rapier` feature): see the crate docs.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use fynkel_contracts::{AssetRef, Entity};
use fynkel_ecs::World as Ecs;
use fynkel_scene::{self as scene, Layer, Parent, Transform, WorldTransform};
use glam::Mat4;
use rapier3d::control::{
    CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::*;

use crate::{
    BodyCommand, BodyState, BodyType, CharacterController, CharacterState, Collider, ColliderShape,
    CombineRule, Contact, ContactPoint, Hit, PhysicsEvent, PhysicsMaterial, PhysicsOwned,
    PhysicsStats, Quat, QueryKind, QuerySpec, Rejection, RejectionKind, RigidBody, Vec3,
    MAX_CONTACT_POINTS,
};

/// How far below a character's skin the ground may be for it to count as
/// grounded, metres.
const GROUND_PROBE: f32 = 0.05;
/// The downward translation a resting character asks for (metres), so
/// `move_shape` takes its cast path (see `move_characters`).
const REST_PROBE: f32 = 1e-4;
/// A resting character's motion under this length (metres) is the
/// controller settling into its skin gap, reported as standing still.
const REST_SETTLE: f32 = 1e-3;

struct Entry {
    body: Option<RigidBodyHandle>,
    collider: Option<ColliderHandle>,
    rigid: Option<RigidBody>,
    shape: Option<Collider>,
    /// The character this entry is a capsule for; `rigid`/`shape` are
    /// then only remembered for change detection.
    character: Option<CharacterController>,
    /// World scale the shape was built with.
    scale: Vec3,
    /// The object's layer bit (`fynkel.Layer`) the collider was built with.
    layer: u32,
    /// The body this entity's collider joined (ADR-0034) — its nearest
    /// ancestor with a `RigidBody` — and that body's type when it did.
    joined: Option<(Entity, BodyType)>,
    /// A joined collider's transform relative to its body: the product of
    /// the local transforms between them, exact, so a moving body leaves
    /// it the same.
    rel: Mat4,
    /// The entity's world matrix as recorded after the last propagate; a
    /// different matrix at sync means the entity was moved by a command.
    last_world: Mat4,
    /// A diagnostic already reported for this entry (once).
    reported: bool,
    /// Rebuild at the next sync: the collision mesh it was built from
    /// changed ([`Physics::replace_mesh`]).
    stale: bool,
}

/// Validated ECS inputs gathered once before mutating the physics world.
/// Bodies are applied before nested colliders; only physics ownership markers
/// change in ECS between collection and application.
struct SyncEntry {
    entity: Entity,
    world: Mat4,
    transform: Transform,
    rigid: Option<RigidBody>,
    shape: Option<Collider>,
    character: Option<CharacterController>,
    layer: u32,
}

/// What the sync found on an entity for a new entry.
struct NewEntry {
    rigid: Option<RigidBody>,
    shape: Option<Collider>,
    character: Option<CharacterController>,
    /// The object's layer bit.
    layer: u32,
    joined: Option<(Entity, BodyType)>,
    rel: Mat4,
}

/// The moving state of a character entry.
struct CharacterRun {
    controller: KinematicCharacterController,
    /// Vertical speed, metres per second along +Y.
    vertical: f32,
    /// This tick's `Move`, consumed by the step.
    pending_move: Option<Vec3>,
    /// This tick's `Jump` speed, consumed by the step.
    pending_jump: Option<f32>,
    state: CharacterState,
    /// Created by this tick's sync: its capsule is not in the broad phase
    /// yet, so it stands still until the step has placed it.
    fresh: bool,
    /// Collision scratch, kept for its capacity.
    collisions: Vec<CharacterCollision>,
}

/// Collects the step's collision events; their contact data is read from
/// the narrow phase after the step, once the solver has produced the
/// tick's impulses (the event itself is raised before solving). A start
/// keeps the relative velocity of its two colliders at that moment — the
/// speed of the impact, before the solver stops it (Unity's
/// `relativeVelocity`, foundation 2.9).
struct Collector {
    events: Mutex<Vec<(CollisionEvent, Vec3)>>,
}

impl EventHandler for Collector {
    fn handle_collision_event(
        &self,
        bodies: &RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        pair: Option<&ContactPair>,
    ) {
        let relative = match event {
            CollisionEvent::Started(h1, h2, _) => {
                let at = pair
                    .and_then(|p| deepest_point(colliders, p))
                    .or_else(|| colliders.get(h1).map(|c| c.position().translation))
                    .unwrap_or(Vec3::ZERO);
                velocity_at(bodies, colliders, h2, at) - velocity_at(bodies, colliders, h1, at)
            }
            CollisionEvent::Stopped(..) => Vec3::ZERO,
        };
        self.events.lock().unwrap().push((event, relative));
    }

    fn handle_contact_force_event(
        &self,
        _dt: Real,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _pair: &ContactPair,
        _total_force_magnitude: Real,
    ) {
    }
}

/// The velocity at `point` (world) of the body `collider` is attached to;
/// zero for a collider without one.
fn velocity_at(
    bodies: &RigidBodySet,
    colliders: &ColliderSet,
    collider: ColliderHandle,
    point: Vec3,
) -> Vec3 {
    colliders
        .get(collider)
        .and_then(|c| c.parent())
        .and_then(|b| bodies.get(b))
        .map_or(Vec3::ZERO, |b| b.velocity_at_point(point))
}

/// The pair's deepest point in world space.
fn deepest_point(colliders: &ColliderSet, pair: &ContactPair) -> Option<Vec3> {
    let (_, contact) = pair.find_deepest_contact()?;
    Some(
        colliders
            .get(pair.collider1)?
            .position()
            .transform_point(contact.local_p1),
    )
}

/// Fills `contact`'s geometry from the pair seen from `first` (the
/// collider of side `a`): its points, the deepest first and at most
/// [`MAX_CONTACT_POINTS`], each normal pointing from `a` to `b`; the
/// deepest point and normal; the impulse on `a` (the sum over every
/// point, not only the kept ones).
fn fill_contact(
    colliders: &ColliderSet,
    pair: &ContactPair,
    first: ColliderHandle,
    contact: &mut Contact,
) {
    let Some(c1) = colliders.get(pair.collider1) else {
        return;
    };
    let flip = if pair.collider1 == first { 1.0 } else { -1.0 };
    let mut all: Vec<ContactPoint> = Vec::new();
    let mut impulse = Vec3::ZERO;
    for m in pair.solver_manifolds() {
        let normal = m.data.normal * flip;
        for p in &m.points {
            impulse -= normal * p.data.impulse;
            all.push(ContactPoint {
                point: c1.position().transform_point(p.local_p1),
                normal,
                separation: p.dist,
                impulse: p.data.impulse,
            });
        }
    }
    all.sort_by(|x, y| x.separation.total_cmp(&y.separation));
    let n = all.len().min(MAX_CONTACT_POINTS);
    contact.points[..n].copy_from_slice(&all[..n]);
    contact.point_count = n as u32;
    if let Some(deepest) = all.first() {
        contact.point = deepest.point;
        contact.normal = deepest.normal;
    }
    contact.impulse = impulse;
}

// No clock reads when profiling is disabled.
fn profile_checkpoint(timer: &mut Option<std::time::Instant>) -> std::time::Duration {
    let Some(previous) = timer else {
        return std::time::Duration::ZERO;
    };
    let now = std::time::Instant::now();
    let elapsed = now.duration_since(*previous);
    *previous = now;
    elapsed
}

fn pose_of(m: &Mat4) -> (Vec3, Quat, Vec3) {
    let t = Transform::from_matrix(*m);
    (t.position, t.rotation.normalize(), t.scale)
}

fn pose(position: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(position, rotation)
}

/// A collider's groups: its object's layer bit (ADR-0020) and its mask.
fn groups(layer: u32, mask: u32) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_retain(layer),
        Group::from_bits_retain(mask),
        InteractionTestMode::And,
    )
}

/// The adapter: owns the Rapier world and the entity ↔ handle maps.
pub struct Physics {
    world: PhysicsWorld,
    entries: BTreeMap<Entity, Entry>,
    characters: BTreeMap<Entity, CharacterRun>,
    /// Collider → its entity and the entity whose body it belongs to
    /// (ADR-0034), kept until the step after a removal so the `Stopped`
    /// events Rapier emits for a removed collider still resolve.
    by_collider: HashMap<ColliderHandle, (Entity, Option<Entity>)>,
    retired: Vec<ColliderHandle>,
    meshes: HashMap<AssetRef, (Vec<Vec3>, Vec<[u32; 3]>)>,
    /// Physics materials by reference (ADR-0025), registered by the loader.
    materials: HashMap<AssetRef, PhysicsMaterial>,
    pending: Vec<(u32, Entity, BodyCommand)>,
    /// Root motion the animators extracted this tick (M3-19c): a world
    /// velocity added to the character's `Move` at the step.
    root_motion: Vec<(Entity, Vec3)>,
    rejections: Vec<Rejection>,
    unsupported_transforms: HashSet<Entity>,
    events: Vec<PhysicsEvent>,
    /// Touching pairs `(a, b)` with `a < b`: sensor pair or not.
    touching: BTreeMap<(Entity, Entity), bool>,
    /// Sync scratch, kept for its capacity.
    sync_cursor: Option<fynkel_ecs::ChangeCursor>,
    invalidated: BTreeSet<Entity>,
    sync_candidates: usize,
    writeback_pending: BTreeSet<Entity>,
    scratch_changed: Vec<SyncEntry>,
    scratch_subtree: Vec<Entity>,
    scratch_nested: Vec<SyncEntry>,
    scratch_gone: Vec<Entity>,
    /// Entities whose transform the last step wrote (`record` refreshes
    /// their `last_world`).
    scratch_written: Vec<Entity>,
    scratch_poses: Vec<(Entity, Vec3, Quat)>,
    forced: Vec<RigidBodyHandle>,
    /// Colliders a sync made or moved since the last step: the broad phase
    /// learns of them at the step, or at [`Physics::refresh`] for queries.
    unplaced: Vec<ColliderHandle>,
    stats: PhysicsStats,
    profile_enabled: bool,
    step_profile: Option<crate::PhysicsStepProfile>,
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}

impl Physics {
    /// Remove CPU source data after its owning references have been released.
    pub fn retain_resources(&mut self, mut keep: impl FnMut(&AssetRef) -> bool) {
        self.meshes.retain(|r, _| keep(r));
        self.materials.retain(|r, _| keep(r));
    }

    pub fn new() -> Self {
        Self {
            world: PhysicsWorld::new(),
            entries: BTreeMap::new(),
            characters: BTreeMap::new(),
            by_collider: HashMap::new(),
            retired: Vec::new(),
            meshes: HashMap::new(),
            materials: HashMap::new(),
            pending: Vec::new(),
            root_motion: Vec::new(),
            rejections: Vec::new(),
            unsupported_transforms: HashSet::new(),
            events: Vec::new(),
            touching: BTreeMap::new(),
            sync_cursor: None,
            invalidated: BTreeSet::new(),
            sync_candidates: 0,
            writeback_pending: BTreeSet::new(),
            scratch_changed: Vec::new(),
            scratch_subtree: Vec::new(),
            scratch_nested: Vec::new(),
            scratch_gone: Vec::new(),
            scratch_written: Vec::new(),
            scratch_poses: Vec::new(),
            forced: Vec::new(),
            unplaced: Vec::new(),
            stats: PhysicsStats::default(),
            profile_enabled: false,
            step_profile: None,
        }
    }

    pub fn gravity(&self) -> Vec3 {
        self.world.gravity
    }

    /// Metres per second squared; the project's `physics.gravity`.
    pub fn set_gravity(&mut self, gravity: Vec3) {
        if gravity.is_finite() {
            self.world.gravity = gravity;
        }
    }

    /// Makes a cooked mesh available to `shape: mesh` colliders. Fails
    /// above [`crate::MAX_MESH_COLLIDER_TRIANGLES`] or on an index out of range.
    pub fn register_mesh(
        &mut self,
        key: AssetRef,
        positions: &[[f32; 3]],
        indices: &[u32],
    ) -> Result<usize, String> {
        let mesh = crate::PreparedMesh::new(positions, indices)?;
        Ok(self.register_prepared_mesh(key, mesh))
    }

    /// Publishes already validated collision data without a fallible decode step.
    pub fn register_prepared_mesh(&mut self, key: AssetRef, mesh: crate::PreparedMesh) -> usize {
        let triangles = mesh.triangle_count();
        self.invalidate_mesh_users(&key);
        self.meshes.insert(key, (mesh.vertices, mesh.triangles));
        triangles
    }

    pub fn has_mesh(&self, key: &AssetRef) -> bool {
        self.meshes.contains_key(key)
    }

    /// Whether a collider uses the collision mesh `key` or it is
    /// registered: what [`replace_mesh`] needs the triangles for.
    ///
    /// [`replace_mesh`]: Physics::replace_mesh
    pub fn uses_mesh(&self, key: &AssetRef) -> bool {
        self.meshes.contains_key(key) || self.entries.values().any(|e| Self::built_from(e, key))
    }

    fn built_from(e: &Entry, key: &AssetRef) -> bool {
        e.shape
            .as_ref()
            .is_some_and(|c| c.shape == ColliderShape::Mesh && c.mesh.as_ref() == Some(key))
    }

    fn invalidate_mesh_users(&mut self, key: &AssetRef) {
        for (&entity, e) in &mut self.entries {
            if Self::built_from(e, key) {
                e.stale = true;
                self.invalidated.insert(entity);
            }
        }
    }

    /// Publishes a prevalidated runtime replacement and invalidates built colliders.
    pub fn replace_prepared_mesh(&mut self, key: &AssetRef, mesh: Option<crate::PreparedMesh>) {
        if let Some(mesh) = mesh {
            self.register_prepared_mesh(key.clone(), mesh);
        } else {
            self.invalidate_mesh_users(key);
            self.meshes.remove(key);
        }
    }

    /// The collision mesh `key` changed at runtime (a mesh made in code set
    /// again or removed): `Some((positions, indices))` registers the new
    /// triangles, `None` forgets them, and every collider using `key` is
    /// rebuilt at the next step — one that no longer can reports
    /// `MESH_UNAVAILABLE`. Fails as [`register_mesh`] does (the mesh is
    /// then forgotten).
    ///
    /// [`register_mesh`]: Physics::register_mesh
    pub fn replace_mesh(
        &mut self,
        key: &AssetRef,
        mesh: Option<(&[[f32; 3]], &[u32])>,
    ) -> Result<(), String> {
        match mesh
            .map(|(positions, indices)| crate::PreparedMesh::new(positions, indices))
            .transpose()
        {
            Ok(prepared) => {
                self.replace_prepared_mesh(key, prepared);
                Ok(())
            }
            Err(error) => {
                self.replace_prepared_mesh(key, None);
                Err(error)
            }
        }
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Makes a physics material available to the colliders naming `key`
    /// (ADR-0025). Registering new values for a key already in use (the
    /// document was edited and recooked, the scene reloaded) applies them
    /// to every live collider that names it — no rebuild, velocities kept.
    /// Refused (with the reason) when a value is out of range.
    pub fn register_material(
        &mut self,
        key: AssetRef,
        material: PhysicsMaterial,
    ) -> Result<(), String> {
        material.validate()?;
        if self.materials.get(&key) == Some(&material) {
            return Ok(());
        }
        self.materials.insert(key.clone(), material);
        for entry in self.entries.values() {
            if entry.character.is_some()
                || entry.shape.as_ref().and_then(|s| s.material.as_ref()) != Some(&key)
            {
                continue;
            }
            let Some(h) = entry.collider else { continue };
            let Some(c) = self.world.colliders.get_mut(h) else {
                continue;
            };
            c.set_friction(material.friction);
            c.set_restitution(material.bounciness);
            c.set_friction_combine_rule(material.friction_combine.rapier());
            c.set_restitution_combine_rule(material.bounce_combine.rapier());
            if let Some(b) = entry.body.and_then(|b| self.world.bodies.get_mut(b)) {
                b.wake_up(true);
            }
        }
        Ok(())
    }

    pub fn has_material(&self, key: &AssetRef) -> bool {
        self.materials.contains_key(key)
    }

    /// The registered values of `key`.
    pub fn material(&self, key: &AssetRef) -> Option<PhysicsMaterial> {
        self.materials.get(key).copied()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    /// What a collider touches with: its material's values and rules, or
    /// without one its own `friction`/`restitution`, averaged (Rapier's
    /// and Unity's default). `Err` carries the fallback and the reason
    /// when the named material is not registered.
    fn surface(&self, c: &Collider) -> Result<PhysicsMaterial, (PhysicsMaterial, String)> {
        let own = PhysicsMaterial {
            friction: c.friction,
            bounciness: c.restitution,
            friction_combine: CombineRule::Average,
            bounce_combine: CombineRule::Average,
        };
        match &c.material {
            None => Ok(own),
            Some(key) => self.materials.get(key).copied().ok_or_else(|| {
                (
                    own,
                    format!(
                        "physics material {key} is not registered (not cooked, or not a physics material): the collider's own friction and restitution apply"
                    ),
                )
            }),
        }
    }

    /// Queues a body command for the next [`Physics::step`] (after the
    /// sync, so a body set in the same batch receives it).
    pub fn queue(&mut self, index: u32, entity: Entity, command: BodyCommand) {
        self.pending.push((index, entity, command));
    }

    /// Adds `velocity` (metres per second, world space) to the character's
    /// movement this tick on top of its `Move` (M3-19c root motion); an
    /// entity without a character is ignored — the caller moves it itself.
    pub fn add_root_motion(&mut self, entity: Entity, velocity: Vec3) {
        if velocity.is_finite() {
            self.root_motion.push((entity, velocity));
        }
    }

    /// Whether `entity` is a character the step will move (its controller
    /// has been synced).
    pub fn has_character(&self, entity: Entity) -> bool {
        self.characters.contains_key(&entity)
    }

    pub fn stats(&self) -> PhysicsStats {
        self.stats
    }

    pub fn body_count(&self) -> usize {
        self.entries.values().filter(|e| e.body.is_some()).count()
    }

    /// Whether a step would do anything: bodies exist, commands are queued,
    /// or `ecs` holds a physics component the sync has not seen yet.
    /// Cheap (archetype-level), for skipping the physics phase.
    pub fn has_work(&self, ecs: &Ecs) -> bool {
        if !self.entries.is_empty() || !self.pending.is_empty() {
            return true;
        }
        let h = ecs;
        h.query::<&RigidBody>().iter().next().is_some()
            || h.query::<&Collider>().iter().next().is_some()
            || h.query::<&CharacterController>().iter().next().is_some()
    }

    /// The events of the last step.
    pub fn events(&self) -> &[PhysicsEvent] {
        &self.events
    }

    /// Rejections of the last step (queued commands that could not be
    /// applied, components that could not be realised), then cleared.
    pub fn take_rejections(&mut self) -> Vec<Rejection> {
        std::mem::take(&mut self.rejections)
    }

    pub fn body_state(&self, entity: Entity) -> Option<BodyState> {
        let e = self.entries.get(&entity)?;
        let body = &self.world.bodies[e.body?];
        Some(BodyState {
            body_type: e.rigid?.body_type,
            linear_velocity: body.linvel(),
            angular_velocity: body.angvel(),
            sleeping: body.is_sleeping(),
        })
    }

    /// A character's held state, `None` without a `CharacterController`
    /// (or before its first step).
    pub fn character_state(&self, entity: Entity) -> Option<CharacterState> {
        self.characters.get(&entity).map(|c| c.state)
    }

    pub fn character_count(&self) -> usize {
        self.characters.len()
    }

    /// Whether `entity` currently has a dynamic body (its transform is
    /// physics-owned).
    pub fn owns(&self, entity: Entity) -> bool {
        self.entries
            .get(&entity)
            .and_then(|e| e.rigid)
            .is_some_and(|r| r.body_type == BodyType::Dynamic)
    }

    // ------------------------------------------------------------- the tick

    /// Enables diagnostic timing of future physics steps. Disabled by default;
    /// disabling clears the last sample. This does not alter simulation settings.
    pub fn set_profiling(&mut self, enabled: bool) {
        self.profile_enabled = enabled;
        self.step_profile = None;
    }

    /// Last completed profiled step, or None before a sample/when disabled.
    pub fn step_profile(&self) -> Option<crate::PhysicsStepProfile> {
        self.step_profile
    }

    /// The physics phase of one fixed tick; `watched` selects the entities
    /// whose `CollisionStay` events are reported.
    pub fn step(&mut self, ecs: &mut Ecs, dt: f32, watched: &BTreeSet<Entity>) {
        let mut timer = self.profile_enabled.then(std::time::Instant::now);
        let started = timer;
        let mut profile = crate::PhysicsStepProfile::default();
        self.step_profile = None;
        self.events.clear();
        self.sync(ecs);
        profile.sync = profile_checkpoint(&mut timer);
        // The step's broad-phase update places them.
        self.unplaced.clear();
        self.apply_pending(ecs);
        self.apply_root_motion();
        profile.commands = profile_checkpoint(&mut timer);
        if self.entries.is_empty() {
            self.pending.clear();
            self.stats = PhysicsStats::default();
            self.step_profile = started.map(|start| {
                profile.total = start.elapsed();
                profile
            });
            return;
        }
        self.world.integration_parameters.dt = dt;
        self.move_characters(dt, watched);
        profile.characters = profile_checkpoint(&mut timer);
        let collector = Collector {
            events: Mutex::new(Vec::new()),
        };
        self.world.step_with_events(&(), &collector);
        for h in self.forced.drain(..) {
            if let Some(b) = self.world.bodies.get_mut(h) {
                b.reset_forces(false);
                b.reset_torques(false);
            }
        }
        profile.solver = profile_checkpoint(&mut timer);
        self.write_back(ecs);
        profile.writeback = profile_checkpoint(&mut timer);
        let raw = collector.events.into_inner().unwrap();
        self.collect_events(raw, watched);
        for h in self.retired.drain(..) {
            self.by_collider.remove(&h);
        }
        self.stats = PhysicsStats {
            bodies: self.world.bodies.len() as u32,
            colliders: self.world.colliders.len() as u32,
            characters: self.characters.len() as u32,
            active_bodies: self.world.islands.active_bodies().count() as u32,
            contact_pairs: self.world.narrow_phase.contact_pairs().count() as u32,
            events: self.events.len() as u32,
        };
        profile.events = profile_checkpoint(&mut timer);
        self.step_profile = started.map(|start| {
            profile.total = start.elapsed();
            profile
        });
    }

    /// After the post-physics propagate: every entry remembers its world
    /// matrix so the next sync distinguishes a command's move from its own.
    pub fn record(&mut self, ecs: &Ecs) {
        // Only the bodies the step wrote back: every other entry's world
        // matrix is what the last sync saw, unless gameplay moved it — and
        // that is exactly what the next sync must notice.
        let mut written = std::mem::take(&mut self.scratch_written);
        for entity in written.drain(..) {
            if let (Some(entry), Ok(wt)) = (
                self.entries.get_mut(&entity),
                ecs.get::<WorldTransform>(entity),
            ) {
                entry.last_world = wt.matrix;
            }
        }
        self.scratch_written = written;
    }

    /// Between steps, before a query: brings the physics world in line with
    /// the entities (a collider made by `instantiate` since the last step)
    /// and puts the colliders made or moved into the query tree, so a
    /// query sees them at once, as Unity's (foundation 2.7). Nothing is
    /// simulated and no event fires.
    pub fn refresh(&mut self, ecs: &mut Ecs) {
        if !self.has_work(ecs) {
            return;
        }
        self.sync(ecs);
        for h in self.unplaced.drain(..) {
            if let Some(co) = self.world.colliders.get(h) {
                let aabb = co.compute_broad_phase_aabb(
                    &self.world.integration_parameters,
                    &self.world.bodies,
                );
                self.world
                    .broad_phase
                    .set_aabb(&self.world.integration_parameters, h, aabb);
            }
        }
    }

    /// Number of candidate identities inspected by the last sync, for qualification.
    pub fn sync_candidates(&self) -> usize {
        self.sync_candidates
    }

    fn sync(&mut self, ecs: &mut Ecs) {
        use std::any::TypeId;
        let changes = ecs
            .changes_since(
                self.sync_cursor,
                &[
                    TypeId::of::<WorldTransform>(),
                    TypeId::of::<RigidBody>(),
                    TypeId::of::<Collider>(),
                    TypeId::of::<CharacterController>(),
                    TypeId::of::<Layer>(),
                    TypeId::of::<Parent>(),
                    TypeId::of::<scene::Children>(),
                    TypeId::of::<scene::Inactive>(),
                ],
                false,
            )
            .expect("settled physics storage");
        let mut affected: BTreeSet<Entity> = if changes.reset {
            ecs.entities().chain(self.entries.keys().copied()).collect()
        } else {
            changes.entities.into_iter().collect()
        };
        affected.append(&mut self.invalidated);
        // An ancestor's body/component or hierarchy change can change the home
        // of a nested collider even when its own component did not change.
        let mut subtree = std::mem::take(&mut self.scratch_subtree);
        subtree.clear();
        subtree.extend(affected.iter().copied().filter(|&e| {
            let mut up = scene::parent_of(ecs, e);
            while let Some(p) = up {
                if affected.contains(&p) {
                    return false;
                }
                up = scene::parent_of(ecs, p);
            }
            true
        }));
        // Walk all roots with one retained stack. The affected set is processed
        // in entity order below, so traversal order does not affect physics.
        // Visit descendants even when already affected: their own children may
        // still need a new body home after an ancestor changes.
        while let Some(entity) = subtree.pop() {
            if !ecs.contains(entity) {
                continue;
            }
            affected.insert(entity);
            if let Ok(children) = ecs.get::<scene::Children>(entity) {
                subtree.extend(children.0.iter().copied());
            }
        }
        self.scratch_subtree = subtree;
        self.sync_candidates = affected.len();
        let mut changed = std::mem::take(&mut self.scratch_changed);
        let mut nested = std::mem::take(&mut self.scratch_nested);
        let mut gone = std::mem::take(&mut self.scratch_gone);
        changed.clear();
        nested.clear();
        gone.clear();
        if !affected.is_empty() {
            // Resolve the candidate identity once and reuse one shared archetype
            // query for all sync inputs. The borrow ends before ownership changes.
            let mut inputs = ecs
                .query::<(
                    Option<&RigidBody>,
                    Option<&Collider>,
                    Option<&CharacterController>,
                    Option<&Layer>,
                    Option<&Parent>,
                    &WorldTransform,
                )>()
                .without::<&scene::Inactive>();
            let view = inputs.view();
            for entity in affected {
                let Some((rigid, shape, character, layer, parent, wt)) =
                    ecs.raw(entity).and_then(|raw| view.get(raw))
                else {
                    gone.push(entity);
                    self.unsupported_transforms.remove(&entity);
                    continue;
                };
                if rigid.is_none() && shape.is_none() && character.is_none() {
                    gone.push(entity);
                    self.unsupported_transforms.remove(&entity);
                    continue;
                }
                let was_unsupported = self.unsupported_transforms.contains(&entity);
                // last_world also contains physics writeback: equality is not
                // proof that this matrix passed transform validation.
                let moving =
                    character.is_some() || rigid.is_some_and(|r| r.body_type != BodyType::Static);
                let transform = Transform::try_from_matrix(wt.matrix);
                if transform.is_none()
                    || (moving && !supports_motion_parent(ecs, parent.map(|p| p.0)))
                {
                    self.unsupported_transforms.insert(entity);
                    if !was_unsupported {
                        self.rejections.push(Rejection {
                            index: u32::MAX,
                            entity,
                            kind: RejectionKind::UnsupportedTransform,
                            message: "physics requires a non-singular world TRS; moving bodies require positive uniform scale on every ancestor".into(),
                        });
                    }
                    // Leaving candidates removes any previously valid body
                    // and collider, instead of retaining stale geometry.
                    gone.push(entity);
                    continue;
                }
                self.unsupported_transforms.remove(&entity);
                // A lone collider under a parent may belong to an
                // ancestor's body (ADR-0034): decided once the bodies are
                // synced.
                if rigid.is_none() && character.is_none() && parent.is_some() {
                    nested.push(SyncEntry {
                        entity,
                        world: wt.matrix,
                        transform: transform.unwrap(),
                        rigid: rigid.copied(),
                        shape: shape.cloned(),
                        character: character.cloned(),
                        layer: layer.copied().unwrap_or_default().bit(),
                    });
                    continue;
                }
                let same = self.entries.get(&entity).is_some_and(|entry| {
                    entry.last_world == wt.matrix
                        && !entry.stale
                        && entry.joined.is_none()
                        && entry.rigid.is_some() == rigid.is_some()
                        && entry.shape.is_some() == shape.is_some()
                        && entry.character.is_some() == character.is_some()
                        && entry.shape.as_ref().zip(shape).is_none_or(|(a, b)| a == b)
                        && entry
                            .character
                            .as_ref()
                            .zip(character)
                            .is_none_or(|(a, b)| a == b)
                        && entry.rigid == rigid.copied()
                        && entry.layer == layer.copied().unwrap_or_default().bit()
                });
                if !same {
                    changed.push(SyncEntry {
                        entity,
                        world: wt.matrix,
                        transform: transform.unwrap(),
                        rigid: rigid.copied(),
                        shape: shape.cloned(),
                        character: character.cloned(),
                        layer: layer.copied().unwrap_or_default().bit(),
                    });
                }
            }
        }
        for e in gone.drain(..) {
            self.remove_entry(ecs, e);
        }
        for input in changed.drain(..) {
            self.update_entry(ecs, input, None);
        }
        for input in nested.drain(..) {
            let entity = input.entity;
            let join = home_of(ecs, entity)
                .filter(|b| self.entries.get(b).is_some_and(|e| e.body.is_some()))
                .map(|b| (b, relative(ecs, entity, b)));
            if !self.unchanged(&input, join) {
                self.update_entry(ecs, input, join);
            }
        }
        self.scratch_changed = changed;
        self.scratch_nested = nested;
        self.scratch_gone = gone;
        self.sync_cursor = ecs.change_cursor();
    }

    /// Whether a nested collider's entry still matches its entity: joined
    /// to the same body at the same relative place (so a moving body costs
    /// nothing), or, standing alone, where it was.
    fn unchanged(&self, input: &SyncEntry, join: Option<(Entity, Mat4)>) -> bool {
        let Some(entry) = self.entries.get(&input.entity) else {
            return false;
        };
        if input.shape.is_none()
            || entry.stale
            || entry.shape != input.shape
            || entry.layer != input.layer
        {
            return false;
        }
        match (join, entry.joined) {
            (None, None) => entry.last_world == input.world,
            (Some((body, rel)), Some((was, kind))) => {
                body == was
                    && rel == entry.rel
                    && Some(kind) == self.body_type_of(body)
                    && !rescaled(entry.scale, input.transform.scale)
            }
            _ => false,
        }
    }

    /// The type of the body `entity` has, `None` without one.
    fn body_type_of(&self, entity: Entity) -> Option<BodyType> {
        self.entries
            .get(&entity)
            .and_then(|e| e.rigid)
            .map(|r| r.body_type)
    }

    /// Brings `entity`'s entry in line with its components: a new entry,
    /// one rebuilt because its body or character came or went, or — every
    /// other change — the same body and collider updated in place, so the
    /// body keeps its velocity and a pair that still touches fires nothing
    /// (foundation 2.2). `join` is the body a nested collider joins and its
    /// place relative to it.
    fn update_entry(&mut self, ecs: &mut Ecs, input: SyncEntry, join: Option<(Entity, Mat4)>) {
        let SyncEntry {
            entity,
            world,
            transform,
            rigid,
            shape,
            character,
            layer,
        } = input;
        let Transform {
            position,
            rotation,
            scale,
        } = transform;
        let joined = join.and_then(|(b, _)| Some((b, self.body_type_of(b)?)));
        let rel = join.map_or(Mat4::IDENTITY, |(_, rel)| rel);
        let recreate = match self.entries.get(&entity) {
            None => true,
            Some(e) => {
                e.rigid.is_some() != rigid.is_some()
                    || e.character.is_some() != character.is_some()
                    // A character's capsule follows its every field; its
                    // body and collider components are ignored, so their
                    // changes are only remembered (a new body/collider
                    // means a `CharacterWithBody`).
                    || (e.character.is_some()
                        && (e.stale
                            || e.layer != layer
                            || e.shape != shape
                            || e.character != character
                            || rescaled(e.scale, scale)))
            }
        };
        if recreate {
            self.remove_entry(ecs, entity);
            self.create_entry(
                ecs,
                entity,
                NewEntry {
                    rigid,
                    shape,
                    character,
                    layer,
                    joined,
                    rel,
                },
                position,
                rotation,
                scale,
                world,
            );
            return;
        }
        let entry = self.entries.get_mut(&entity).unwrap();
        let mut moved = entry.last_world != world;
        entry.last_world = world;
        if entry.character.is_some() {
            // The capsule ignores the body component; remember the value
            // so the comparison above stays quiet.
            entry.rigid = rigid;
            if moved {
                // A character teleports (a transform write is not a walk):
                // vertical speed resets.
                let h = entry.body.unwrap();
                let p = pose(position, rotation);
                self.world.bodies[h].set_position(p, true);
                self.world.bodies[h].set_next_kinematic_position(p);
                if let Some(run) = self.characters.get_mut(&entity) {
                    run.vertical = 0.0;
                }
            }
            return;
        }
        let shape_changed = entry.shape != shape;
        let mut refit = entry.stale
            || shape_changed
            || entry.layer != layer
            || entry.joined != joined
            || rescaled(entry.scale, scale);
        if shape_changed {
            // Report a still-broken value again once it changes.
            entry.reported = false;
        }
        if let (Some(h), Some(new), Some(old)) = (entry.body, rigid, entry.rigid) {
            if new != old {
                let body = &mut self.world.bodies[h];
                if new.body_type != old.body_type {
                    body.set_body_type(body_type(new.body_type), true);
                    set_owned(ecs, entity, new.body_type == BodyType::Dynamic);
                    moved = true;
                    // A mesh collider may no longer fit the body.
                    refit = true;
                }
                body.set_linear_damping(new.linear_damping);
                body.set_angular_damping(new.angular_damping);
                body.set_gravity_scale(new.gravity_scale, true);
                body.enable_ccd(new.ccd);
                let (t, r) = freedoms(new.constraints);
                body.set_enabled_translations(t[0], t[1], t[2], true);
                body.set_enabled_rotations(r[0], r[1], r[2], true);
                body.set_additional_mass(mass_of(&new), true);
                entry.rigid = Some(new);
            }
        }
        let placed = entry.rel != rel;
        entry.shape = shape;
        entry.layer = layer;
        entry.joined = joined;
        entry.rel = rel;
        entry.scale = scale;
        entry.stale = false;
        let (body, kind) = (entry.body, entry.rigid.map(|r| r.body_type));
        if moved {
            let p = pose(position, rotation);
            match (body, kind) {
                // A queued `SetKinematicTarget` is applied after the
                // sync, so it wins over a transform write in the same batch.
                (Some(h), Some(BodyType::Kinematic)) => {
                    self.world.bodies[h].set_next_kinematic_position(p);
                }
                (Some(h), _) => self.world.bodies[h].set_position(p, true),
                (None, _) => {}
            }
        }
        if refit {
            self.fit_collider(entity);
        } else if (moved && body.is_none()) || placed {
            self.place_collider(entity, pose(position, rotation));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_entry(
        &mut self,
        ecs: &mut Ecs,
        entity: Entity,
        new: NewEntry,
        position: Vec3,
        rotation: Quat,
        scale: Vec3,
        world: Mat4,
    ) {
        let NewEntry {
            rigid,
            shape,
            character,
            layer,
            joined,
            rel,
        } = new;
        if let Some(c) = character {
            self.create_character(
                ecs, entity, c, rigid, shape, layer, position, rotation, scale, world,
            );
            return;
        }
        let p = pose(position, rotation);
        let user = entity.to_bits();
        let body = rigid.map(|r| {
            let (t, rot) = freedoms(r.constraints);
            let mut b = RigidBodyBuilder::new(body_type(r.body_type))
                .pose(p)
                .enabled_translations(t[0], t[1], t[2])
                .enabled_rotations(rot[0], rot[1], rot[2])
                .linear_damping(r.linear_damping)
                .angular_damping(r.angular_damping)
                .gravity_scale(r.gravity_scale)
                .ccd_enabled(r.ccd)
                .user_data(user);
            if r.body_type == BodyType::Dynamic {
                b = b.additional_mass(r.mass);
            }
            self.world.insert_body(b)
        });
        self.writeback_pending.insert(entity);
        self.entries.insert(
            entity,
            Entry {
                body,
                collider: None,
                rigid,
                shape,
                character: None,
                scale,
                layer,
                joined,
                rel,
                last_world: world,
                reported: false,
                stale: false,
            },
        );
        self.fit_collider(entity);
        set_owned(
            ecs,
            entity,
            rigid.is_some_and(|r| r.body_type == BodyType::Dynamic),
        );
    }

    /// Where an entry's collider goes: the body it is attached to and its
    /// pose relative to that body, or, without one, its world pose.
    fn collider_place(
        &self,
        entry: &Entry,
        world_pose: impl FnOnce() -> Pose,
    ) -> (Option<RigidBodyHandle>, Pose) {
        let offset = Pose::from_translation(
            entry.shape.as_ref().map_or(Vec3::ZERO, |s| s.offset) * entry.scale,
        );
        if let Some((home, _)) = entry.joined {
            let body = &self.entries[&home];
            let (t, r, _) = pose_of(&entry.rel);
            // The body's pose carries no scale: its scale stretches the
            // child's place within it.
            return (body.body, pose(t * body.scale, r) * offset);
        }
        if entry.body.is_some() {
            return (entry.body, offset);
        }
        (None, world_pose() * offset)
    }

    /// Moves an entry's collider to its place (the entity or its place in
    /// the body moved; nothing else changed).
    fn place_collider(&mut self, entity: Entity, world_pose: Pose) {
        let entry = &self.entries[&entity];
        let Some(h) = entry.collider else {
            return;
        };
        let (parent, place) = self.collider_place(entry, || world_pose);
        let co = &mut self.world.colliders[h];
        if parent.is_some() {
            co.set_position_wrt_parent(place);
        } else {
            co.set_position(place);
        }
        self.unplaced.push(h);
    }

    /// Makes an entry's Rapier collider match its `Collider`: built when
    /// missing, else updated in place — the same handle keeps the pairs it
    /// is in, so nothing fires for a pair that still touches (foundation
    /// 2.2) — or removed when the component went or cannot be built.
    fn fit_collider(&mut self, entity: Entity) {
        let entry = &self.entries[&entity];
        let Some(c) = entry.shape.clone() else {
            self.drop_collider(entity);
            return;
        };
        let body_type = match entry.joined {
            Some((_, kind)) => Some(kind),
            None => entry.rigid.map(|r| r.body_type),
        };
        let built = self.build_shape(&c, entry.scale, body_type);
        let surface = self.surface(&c);
        let (parent, place) = self.collider_place(entry, || {
            let (t, r, _) = pose_of(&entry.last_world);
            pose(t, r)
        });
        let body_entity = match entry.joined {
            Some((home, _)) => Some(home),
            None => entry.rigid.is_some().then_some(entity),
        };
        let (layer, existing) = (entry.layer, entry.collider);
        let shape = match built {
            Ok(s) => s,
            Err((kind, message)) => {
                self.report(entity, kind, message);
                self.drop_collider(entity);
                return;
            }
        };
        let surface = match surface {
            Ok(m) => m,
            Err((own, message)) => {
                self.report(entity, RejectionKind::MaterialUnavailable, message);
                own
            }
        };
        // A sensor also sees kinematic bodies — the character controller's
        // capsule walking into a static pickup (Rapier's default is
        // dynamic-vs-anything only).
        let types = if c.is_sensor {
            ActiveCollisionTypes::default()
                | ActiveCollisionTypes::KINEMATIC_FIXED
                | ActiveCollisionTypes::KINEMATIC_KINEMATIC
        } else {
            ActiveCollisionTypes::default()
        };
        let handle = match existing {
            Some(h) => {
                if self.world.colliders[h].parent() != parent {
                    self.world
                        .colliders
                        .set_parent(h, parent, &mut self.world.bodies);
                }
                let co = &mut self.world.colliders[h];
                co.set_shape(shape);
                co.set_friction(surface.friction);
                co.set_friction_combine_rule(surface.friction_combine.rapier());
                co.set_restitution(surface.bounciness);
                co.set_restitution_combine_rule(surface.bounce_combine.rapier());
                co.set_sensor(c.is_sensor);
                co.set_collision_groups(groups(layer, c.mask));
                co.set_active_collision_types(types);
                if parent.is_some() {
                    co.set_position_wrt_parent(place);
                } else {
                    co.set_position(place);
                }
                h
            }
            None => {
                let builder = ColliderBuilder::new(shape)
                    .friction(surface.friction)
                    .friction_combine_rule(surface.friction_combine.rapier())
                    .restitution(surface.bounciness)
                    .restitution_combine_rule(surface.bounce_combine.rapier())
                    .sensor(c.is_sensor)
                    .collision_groups(groups(layer, c.mask))
                    .active_events(ActiveEvents::COLLISION_EVENTS)
                    .active_collision_types(types)
                    .density(0.0)
                    .position(place)
                    .user_data(entity.to_bits());
                let h = self.world.insert_collider(builder, parent);
                self.entries.get_mut(&entity).unwrap().collider = Some(h);
                h
            }
        };
        self.by_collider.insert(handle, (entity, body_entity));
        self.unplaced.push(handle);
        // The effective inverse mass is otherwise refreshed by the step: a
        // body command queued in the tick that created the body (spawn +
        // components + impulse in one batch) must see the mass now.
        if let Some(b) = parent {
            self.world.bodies[b].recompute_mass_properties_from_colliders(&self.world.colliders);
        }
    }

    /// Reports a component `entity`'s entry cannot realise, once.
    fn report(&mut self, entity: Entity, kind: RejectionKind, message: String) {
        let entry = self.entries.get_mut(&entity).unwrap();
        if !entry.reported {
            entry.reported = true;
            self.rejections.push(Rejection {
                index: u32::MAX,
                entity,
                kind,
                message,
            });
        }
    }

    /// Removes an entry's Rapier collider, if it has one.
    fn drop_collider(&mut self, entity: Entity) {
        let Some(h) = self
            .entries
            .get_mut(&entity)
            .and_then(|e| e.collider.take())
        else {
            return;
        };
        let parent = self.world.colliders.get(h).and_then(|c| c.parent());
        self.retired.push(h);
        self.world.remove_collider(h);
        if let Some(b) = parent.filter(|b| self.world.bodies.contains(*b)) {
            self.world.bodies[b].recompute_mass_properties_from_colliders(&self.world.colliders);
        }
    }

    /// A character: a kinematic body with the controller's capsule, a
    /// controller configured from the component, and a run at rest. A
    /// `RigidBody`/`Collider` on the entity is reported once and ignored.
    #[allow(clippy::too_many_arguments)]
    fn create_character(
        &mut self,
        ecs: &mut Ecs,
        entity: Entity,
        c: CharacterController,
        rigid: Option<RigidBody>,
        shape: Option<Collider>,
        layer: u32,
        position: Vec3,
        rotation: Quat,
        scale: Vec3,
        world: Mat4,
    ) {
        let p = pose(position, rotation);
        let user = entity.to_bits();
        let body = self.world.insert_body(
            RigidBodyBuilder::kinematic_position_based()
                .pose(p)
                .user_data(user),
        );
        let s = scale.abs();
        let capsule = SharedShape::capsule_y(c.half_height() * s.y, c.radius * s.x.max(s.z));
        let collider = self.world.insert_collider(
            ColliderBuilder::new(capsule)
                .translation(c.center * scale)
                .friction(0.0)
                .collision_groups(groups(layer, c.mask))
                .active_events(ActiveEvents::COLLISION_EVENTS)
                .density(0.0)
                .user_data(user),
            Some(body),
        );
        self.by_collider.insert(collider, (entity, None));
        let mut reported = false;
        if rigid.is_some() || shape.is_some() {
            reported = true;
            self.rejections.push(Rejection {
                index: u32::MAX,
                entity,
                kind: RejectionKind::CharacterWithBody,
                message: "a CharacterController is its own kinematic capsule: the entity's RigidBody/Collider are ignored".into(),
            });
        }
        let up = Vec3::Y;
        let controller = KinematicCharacterController {
            up,
            offset: CharacterLength::Absolute(c.skin_width.max(1e-4)),
            slide: true,
            autostep: (c.max_step_height > 0.0).then_some(CharacterAutostep {
                max_height: CharacterLength::Absolute(c.max_step_height * s.y),
                min_width: CharacterLength::Absolute(c.radius * s.x.max(s.z)),
                include_dynamic_bodies: true,
            }),
            max_slope_climb_angle: c.max_slope,
            min_slope_slide_angle: c.max_slope,
            snap_to_ground: (c.snap_to_ground > 0.0)
                .then_some(CharacterLength::Absolute(c.snap_to_ground * s.y)),
            normal_nudge_factor: 1.0e-4,
        };
        // A rebuilt character (a field changed) keeps its motion.
        let previous = self.characters.remove(&entity);
        self.characters.insert(
            entity,
            CharacterRun {
                controller,
                vertical: previous.as_ref().map_or(0.0, |r| r.vertical),
                pending_move: None,
                pending_jump: None,
                state: previous
                    .as_ref()
                    .map_or(CharacterState::default(), |r| r.state),
                fresh: previous.is_none(),
                collisions: previous.map_or(Vec::new(), |r| r.collisions),
            },
        );
        set_owned(ecs, entity, false);
        self.writeback_pending.insert(entity);
        self.entries.insert(
            entity,
            Entry {
                body: Some(body),
                collider: Some(collider),
                rigid,
                shape,
                character: Some(c),
                scale,
                layer,
                joined: None,
                rel: Mat4::IDENTITY,
                last_world: world,
                reported,
                stale: false,
            },
        );
    }

    fn build_shape(
        &self,
        c: &Collider,
        scale: Vec3,
        body: Option<BodyType>,
    ) -> Result<SharedShape, (RejectionKind, String)> {
        let s = scale.abs();
        Ok(match c.shape {
            ColliderShape::Box => {
                let h = c.size * s;
                SharedShape::cuboid(h.x, h.y, h.z)
            }
            ColliderShape::Sphere => SharedShape::ball(c.size.x * s.max_element()),
            ColliderShape::Capsule => {
                SharedShape::capsule_y(c.size.y * s.y, c.size.x * s.x.max(s.z))
            }
            ColliderShape::Mesh => {
                if body.is_some_and(|b| b != BodyType::Static) {
                    return Err((
                        RejectionKind::MeshOnMovingBody,
                        "a mesh collider needs a static body (or no RigidBody)".into(),
                    ));
                }
                let key = c.mesh.as_ref().ok_or((
                    RejectionKind::MeshUnavailable,
                    "a mesh collider needs `mesh`".to_string(),
                ))?;
                let (verts, tris) = self.meshes.get(key).ok_or((
                    RejectionKind::MeshUnavailable,
                    format!("mesh {key} is not registered for collision (not cooked, or over the triangle limit)"),
                ))?;
                let scaled: Vec<Vec3> = verts.iter().map(|v| *v * scale).collect();
                SharedShape::trimesh(scaled, tris.clone()).map_err(|e| {
                    (
                        RejectionKind::MeshUnavailable,
                        format!("mesh {key} cannot be a collider: {e:?}"),
                    )
                })?
            }
        })
    }

    fn remove_entry(&mut self, ecs: &mut Ecs, entity: Entity) {
        self.writeback_pending.remove(&entity);
        let Some(entry) = self.entries.remove(&entity) else {
            return;
        };
        if let Some(c) = entry.collider {
            self.retired.push(c);
            self.world.remove_collider(c);
        }
        if let Some(b) = entry.body {
            // Rapier takes the colliders that joined the body with it; their
            // entries rebuild at this sync's nested pass (ADR-0034).
            for &h in self.world.bodies[b].colliders() {
                if let Some(&(e, _)) = self.by_collider.get(&h) {
                    if let Some(j) = self.entries.get_mut(&e) {
                        j.collider = None;
                        j.stale = true;
                        self.invalidated.insert(e);
                    }
                    self.retired.push(h);
                }
            }
            self.world.remove_body(b);
        }
        // A rebuild re-inserts the run (`create_character`); a removal
        // drops it with the entry.
        if entry.character.is_some() && !ecs.has::<CharacterController>(entity) {
            self.characters.remove(&entity);
        }
        if ecs.contains(entity) {
            set_owned(ecs, entity, false);
        }
    }

    fn apply_pending(&mut self, ecs: &mut Ecs) {
        let pending = std::mem::take(&mut self.pending);
        for (index, entity, cmd) in pending {
            let reject = |kind: RejectionKind, message: String| Rejection {
                index,
                entity,
                kind,
                message,
            };
            if ecs.has::<fynkel_scene::Inactive>(entity) {
                // Out of the simulation while inactive: nothing to move,
                // and nothing wrong with asking (Unity drops it too).
                continue;
            }
            if !ecs.contains(entity) {
                // Destroyed after the command in the same batch (a script
                // moves its object, then ends the level): nothing left to
                // move, and nothing wrong with the order.
                continue;
            }
            let body = ecs.get::<RigidBody>(entity).ok().map(|b| b.body_type);
            if let Err((kind, message)) =
                cmd.validate_for(body, ecs.has::<CharacterController>(entity))
            {
                self.rejections.push(reject(kind, message));
                continue;
            }
            if cmd.is_character() {
                match (self.characters.get_mut(&entity), cmd) {
                    (Some(run), BodyCommand::Move(v)) => run.pending_move = Some(v),
                    (Some(run), BodyCommand::Jump(speed)) => run.pending_jump = Some(speed),
                    _ => self.rejections.push(reject(
                        RejectionKind::MissingCharacter,
                        "entity has no CharacterController".into(),
                    )),
                }
                continue;
            }
            let Some(entry) = self.entries.get_mut(&entity) else {
                self.rejections.push(reject(
                    RejectionKind::MissingBody,
                    "entity has no RigidBody".into(),
                ));
                continue;
            };
            if entry.character.is_some() {
                self.rejections.push(reject(
                    RejectionKind::MissingBody,
                    "a CharacterController has no RigidBody: use move/jump or a transform write"
                        .into(),
                ));
                continue;
            }
            let (Some(h), Some(rigid)) = (entry.body, entry.rigid) else {
                self.rejections.push(reject(
                    RejectionKind::MissingBody,
                    "entity has no RigidBody".into(),
                ));
                continue;
            };
            let body = &mut self.world.bodies[h];
            match cmd {
                BodyCommand::ApplyForce(v) => {
                    body.add_force(v, true);
                    self.forced.push(h);
                }
                BodyCommand::ApplyImpulse(v) => body.apply_impulse(v, true),
                BodyCommand::ApplyTorque(v) => {
                    body.add_torque(v, true);
                    self.forced.push(h);
                }
                BodyCommand::ApplyTorqueImpulse(v) => body.apply_torque_impulse(v, true),
                BodyCommand::SetLinearVelocity(v) => {
                    body.set_linvel(masked(v, body.locked_axes(), false), true)
                }
                BodyCommand::SetAngularVelocity(v) => {
                    body.set_angvel(masked(v, body.locked_axes(), true), true)
                }
                BodyCommand::SetKinematicTarget { position, rotation } => {
                    body.set_next_kinematic_position(pose(position, rotation.normalize()));
                }
                BodyCommand::Teleport { position, rotation } => {
                    let p = pose(position, rotation.normalize());
                    body.set_position(p, true);
                    if rigid.body_type == BodyType::Kinematic {
                        body.set_next_kinematic_position(p);
                    }
                    // Static bodies are not written back: move the entity now.
                    if rigid.body_type == BodyType::Static {
                        if let Err(error) = write_pose(ecs, entity, position, rotation.normalize())
                        {
                            self.rejections.push(reject(
                                RejectionKind::UnsupportedTransform,
                                format!("physics pose publication: {error:?}"),
                            ));
                        }
                        if let Ok(wt) = ecs.get::<WorldTransform>(entity) {
                            entry.last_world = wt.matrix;
                        }
                    }
                }
                // Routed to the character above.
                BodyCommand::Move(_) | BodyCommand::Jump(_) => unreachable!(),
            }
        }
    }

    fn apply_root_motion(&mut self) {
        for (entity, v) in self.root_motion.drain(..) {
            if let Some(run) = self.characters.get_mut(&entity) {
                let combined = run.pending_move.unwrap_or(Vec3::ZERO) + v;
                if let Err((kind, message)) = BodyCommand::Move(combined).validate_for(None, true) {
                    self.rejections.push(Rejection {
                        index: u32::MAX,
                        entity,
                        kind,
                        message: format!("root motion: {message}"),
                    });
                } else {
                    run.pending_move = Some(combined);
                }
            }
        }
    }

    fn write_back(&mut self, ecs: &mut Ecs) {
        self.scratch_written.clear();
        self.scratch_poses.clear();
        let mut candidates = std::mem::take(&mut self.writeback_pending);
        candidates.extend(
            self.world
                .active_bodies()
                .map(|(_, body)| Entity::from_bits(body.user_data)),
        );
        for entity in candidates {
            let Some(entry) = self.entries.get(&entity) else {
                continue;
            };
            let Some(h) = entry.body else { continue };
            let body = &self.world.bodies[h];
            if entry.character.is_none()
                && entry.rigid.is_none_or(|r| {
                    r.body_type == BodyType::Static
                        || (r.body_type == BodyType::Dynamic && body.is_sleeping())
                })
            {
                continue;
            }
            let p = body.position();
            self.scratch_poses.push((entity, p.translation, p.rotation));
        }
        let published = scene::prepare_world_poses(ecs, &self.scratch_poses)
            .and_then(|poses| scene::set_local_batch(ecs, &poses));
        match published {
            Ok(()) => self
                .scratch_written
                .extend(self.scratch_poses.iter().map(|p| p.0)),
            Err(error) => {
                let entity = self
                    .scratch_poses
                    .first()
                    .map(|p| p.0)
                    .unwrap_or(Entity::NULL);
                self.rejections.push(Rejection {
                    index: u32::MAX,
                    entity,
                    kind: RejectionKind::UnsupportedTransform,
                    message: format!("physics pose batch rejected: {error:?}"),
                });
            }
        }
    }

    /// The characters' tick: gravity, this tick's `Move`/`Jump`, Rapier's
    /// controller against the last step's geometry (slopes, steps, snap),
    /// the kinematic target for the step, impulses on the dynamic bodies
    /// met, the held state and the hits of watched characters. Entity
    /// order, so equal inputs give equal motion.
    fn move_characters(&mut self, dt: f32, watched: &BTreeSet<Entity>) {
        if self.characters.is_empty() || dt <= 0.0 {
            return;
        }
        let gravity = self.world.gravity;
        let up = Vec3::Y;
        for (entity, run) in &mut self.characters {
            let Some(entry) = self.entries.get(entity) else {
                continue;
            };
            let (Some(spec), Some(body), Some(collider)) =
                (entry.character.as_ref(), entry.body, entry.collider)
            else {
                continue;
            };
            if run.fresh {
                // The initial broad-phase placement takes this tick. Preserve
                // explicit intent until the first tick that can execute it;
                // later move/jump requests still replace the pending values.
                run.fresh = false;
                continue;
            }
            let wanted_move = run.pending_move.take();
            let jump = run.pending_jump.take();
            let flying = spec.gravity_scale == 0.0;
            let mut planar = wanted_move.unwrap_or(Vec3::ZERO);
            if !flying {
                planar.y = 0.0;
                // Gravity accumulates in the air; on the ground the
                // controller's own contact test (and the snap when walking
                // off a bump) keeps `grounded` without a downward probe.
                let g = gravity.y * spec.gravity_scale;
                run.vertical = if run.state.grounded {
                    0.0
                } else {
                    run.vertical + g * dt
                };
                if let Some(speed) = jump {
                    run.vertical = if speed == 0.0 { spec.jump_speed } else { speed };
                }
            }
            // A character at rest still asks for a hair of downward motion:
            // Rapier's `move_shape` depenetrates instead of casting when the
            // desired translation is zero, and parry's contact query is
            // unreliable for a capsule resting exactly on a box (it can
            // report a deep penetration from above and shove the character
            // into the floor); the cast path is robust there.
            let resting = !flying && planar == Vec3::ZERO && run.vertical == 0.0;
            let desired = if resting {
                Vec3::new(0.0, -REST_PROBE, 0.0)
            } else if flying {
                planar * dt
            } else {
                planar * dt + Vec3::Y * (run.vertical * dt)
            };
            let shape = self.world.colliders[collider].shared_shape().clone();
            let start = *self.world.colliders[collider].position();
            let filter = QueryFilter::new()
                .exclude_rigid_body(body)
                .exclude_sensors()
                .groups(groups(entry.layer, spec.mask));
            run.collisions.clear();
            let collisions = &mut run.collisions;
            let moved = {
                let queries = self.world.query_pipeline_with_filter(filter);
                run.controller
                    .move_shape(dt, &queries, &*shape, &start, desired, |c| {
                        collisions.push(c)
                    })
            };
            if spec.mass > 0.0 && !collisions.is_empty() {
                let mut queries = self.world.broad_phase.as_query_pipeline_mut(
                    self.world.narrow_phase.query_dispatcher(),
                    &mut self.world.bodies,
                    &mut self.world.colliders,
                    filter,
                );
                run.controller.solve_character_collision_impulses(
                    dt,
                    &mut queries,
                    &*shape,
                    spec.mass,
                    collisions.iter(),
                );
            }
            let translation = if !moved.translation.is_finite()
                || (resting && moved.translation.length_squared() < REST_SETTLE * REST_SETTLE)
            {
                Vec3::ZERO
            } else {
                moved.translation
            };
            // Grounded is our own test — Rapier's counts any contact with
            // an upward-facing normal (a ledge brushed mid-jump would hold
            // the character): walkable ground (within `max_slope`) just
            // below the feet, and not while rising.
            let ascending = !flying && run.vertical > 0.0 && translation.y > 1e-6;
            let ground = if ascending {
                None
            } else {
                let after = Pose::from_translation(translation) * start;
                let queries = self.world.query_pipeline_with_filter(filter);
                queries
                    .cast_shape(
                        &after,
                        -up,
                        &*shape,
                        ShapeCastOptions {
                            target_distance: 0.0,
                            stop_at_penetration: true,
                            max_time_of_impact: spec.skin_width + GROUND_PROBE,
                            compute_impact_geometry_on_penetration: true,
                        },
                    )
                    .filter(|(_, hit)| hit.normal1.dot(up) >= spec.max_slope.cos() - 1e-3)
            };
            let grounded = ground.is_some();
            if !flying {
                run.vertical = if grounded {
                    0.0
                } else {
                    (translation.y / dt).clamp(run.vertical.min(0.0), run.vertical.max(0.0))
                };
            }
            // On the ground the controller settles into its skin gap by a
            // hair each tick: not motion worth reporting.
            let mut velocity = translation / dt;
            if grounded && translation.y.abs() < REST_SETTLE {
                velocity.y = 0.0;
            }
            // Rapier's flag alone also fires on a landing; sliding means
            // a contact steeper than `max_slope` (a slope or an overhang,
            // not a wall).
            let steep = |n: Vec3| {
                let cos = n.dot(up);
                cos.abs() > 0.05 && cos < spec.max_slope.cos() - 1e-3
            };
            // Where each hit touched the capsule (foundation 2.4, Unity's
            // rule): below its lower sphere's centre, above its upper
            // one's, or on the sides between them.
            let (low, high) = shape
                .as_capsule()
                .map(|c| {
                    (
                        c.segment.a.y.min(c.segment.b.y),
                        c.segment.a.y.max(c.segment.b.y),
                    )
                })
                .unwrap_or((0.0, 0.0));
            let mut collision_flags = if grounded { CharacterState::BELOW } else { 0 };
            for c in collisions.iter() {
                let y = c.hit.witness2.y;
                collision_flags |= if y < low - 1e-3 {
                    CharacterState::BELOW
                } else if y > high + 1e-3 {
                    CharacterState::ABOVE
                } else {
                    CharacterState::SIDES
                };
            }
            if watched.contains(entity) {
                // The ground stood on counts as hit every tick (a Unity
                // move carries gravity into it), found by the probe when
                // the move itself stopped short in the skin gap.
                let ground = ground.filter(|(h, _)| collisions.iter().all(|c| c.handle != *h));
                let hits = collisions.iter().map(|c| (c.handle, &c.hit));
                for (i, (handle, hit)) in hits
                    .chain(ground.iter().map(|(h, hit)| (*h, hit)))
                    .enumerate()
                {
                    if collisions.iter().take(i).any(|d| d.handle == handle) {
                        continue;
                    }
                    let Some(&(b, body_b)) = self.by_collider.get(&handle) else {
                        continue;
                    };
                    let mut contact = Contact::between(*entity, b, None, body_b);
                    contact.point = hit.witness1;
                    contact.normal = -hit.normal1;
                    contact.points[0] = ContactPoint {
                        point: contact.point,
                        normal: contact.normal,
                        separation: 0.0,
                        impulse: 0.0,
                    };
                    contact.point_count = 1;
                    self.events.push(PhysicsEvent::ControllerHit(contact));
                }
            }
            run.state = CharacterState {
                grounded,
                sliding: moved.is_sliding_down_slope
                    && collisions.iter().any(|c| steep(c.hit.normal1)),
                velocity,
                collision_flags,
            };
            let body_ref = &mut self.world.bodies[body];
            let current = *body_ref.position();
            let mut rotation = current.rotation;
            if spec.turn_speed > 0.0 {
                let heading = Vec3::new(planar.x, 0.0, planar.z);
                if heading.length_squared() > 1e-8 {
                    // Face the motion: -Z is forward.
                    let target = Quat::from_rotation_y((-heading.x).atan2(-heading.z));
                    let max = spec.turn_speed * dt;
                    let angle = rotation.angle_between(target);
                    rotation = if angle <= max {
                        target
                    } else {
                        rotation.slerp(target, max / angle)
                    };
                }
            }
            let next = Pose::from_parts(current.translation + translation, rotation.normalize());
            body_ref.set_next_kinematic_position(next);
        }
    }

    /// The velocity of `h2`'s body minus `h1`'s, at `h1`'s position.
    fn relative_velocity(&self, h1: ColliderHandle, h2: ColliderHandle) -> Vec3 {
        let at = self
            .world
            .colliders
            .get(h1)
            .map_or(Vec3::ZERO, |c| c.position().translation);
        self.relative_velocity_at(h1, h2, at)
    }

    /// The velocity of `h2`'s body minus `h1`'s, at `point`.
    fn relative_velocity_at(&self, h1: ColliderHandle, h2: ColliderHandle, point: Vec3) -> Vec3 {
        let w = &self.world;
        velocity_at(&w.bodies, &w.colliders, h2, point)
            - velocity_at(&w.bodies, &w.colliders, h1, point)
    }

    fn collect_events(&mut self, raw: Vec<(CollisionEvent, Vec3)>, watched: &BTreeSet<Entity>) {
        let mut started: HashSet<(Entity, Entity)> = HashSet::new();
        for (event, relative) in raw {
            let (h1, h2, flags, start) = match event {
                CollisionEvent::Started(a, b, f) => (a, b, f, true),
                CollisionEvent::Stopped(a, b, f) => (a, b, f, false),
            };
            let (Some(&(a, body_a)), Some(&(b, body_b))) =
                (self.by_collider.get(&h1), self.by_collider.get(&h2))
            else {
                continue;
            };
            let sensor = flags.contains(CollisionEventFlags::SENSOR);
            let mut contact = Contact::between(a, b, body_a, body_b);
            if start && !sensor {
                if let Some(pair) = self.world.narrow_phase.contact_pair(h1, h2) {
                    fill_contact(&self.world.colliders, pair, h1, &mut contact);
                }
                contact.relative_velocity = relative;
            } else if !sensor {
                // An exit: no geometry, the velocities as they are now.
                contact.relative_velocity = self.relative_velocity(h1, h2);
            }
            let key = if a < b { (a, b) } else { (b, a) };
            self.events.push(match (start, sensor) {
                (true, false) => PhysicsEvent::CollisionEnter(contact),
                (true, true) => PhysicsEvent::TriggerEnter(contact),
                (false, false) => PhysicsEvent::CollisionExit(contact),
                (false, true) => PhysicsEvent::TriggerExit(contact),
            });
            if start {
                self.touching.insert(key, sensor);
                started.insert(key);
            } else {
                self.touching.remove(&key);
            }
        }
        // Stay events for watched, still-touching pairs: a sensor pair
        // while it overlaps (no geometry), a collision pair while it has a
        // contact.
        let mut stays = Vec::new();
        for (&(a, b), &sensor) in &self.touching {
            if started.contains(&(a, b)) {
                continue;
            }
            let (Some(ca), Some(cb)) = (
                self.entries.get(&a).and_then(|e| e.collider),
                self.entries.get(&b).and_then(|e| e.collider),
            ) else {
                continue;
            };
            let body_a = self.by_collider.get(&ca).and_then(|&(_, body)| body);
            let body_b = self.by_collider.get(&cb).and_then(|&(_, body)| body);
            // Watching a body watches the colliders that joined it.
            let seen = |e: Option<Entity>| e.is_some_and(|e| watched.contains(&e));
            if !(seen(Some(a)) || seen(Some(b)) || seen(body_a) || seen(body_b)) {
                continue;
            }
            if sensor {
                if self.world.narrow_phase.intersection_pair(ca, cb) == Some(true) {
                    stays.push(PhysicsEvent::TriggerStay(Contact::between(
                        a, b, body_a, body_b,
                    )));
                }
                continue;
            }
            let Some(pair) = self.world.narrow_phase.contact_pair(ca, cb) else {
                continue;
            };
            if !pair.has_any_active_contact() {
                continue;
            }
            let mut contact = Contact::between(a, b, body_a, body_b);
            fill_contact(&self.world.colliders, pair, ca, &mut contact);
            contact.relative_velocity = self.relative_velocity_at(ca, cb, contact.point);
            stays.push(PhysicsEvent::CollisionStay(contact));
        }
        self.events.extend(stays);
    }

    // ------------------------------------------------------------- queries

    /// Answers a spatial query against the last stepped state; hits are
    /// appended to `out` (nearest first for casts).
    pub fn query(&self, q: &QuerySpec, out: &mut Vec<Hit>) {
        let max_hits = q.max_hits.max(1) as usize;
        let mask = q.mask;
        // As Unity, a ray or a cast does not detect a collider it starts
        // in (foundation 2.8). A mesh is a surface: nothing is inside it.
        let start = pose(q.origin, q.rotation.normalize());
        let start_shape = match q.kind {
            QueryKind::ShapeCast => query_shape(q),
            _ => None,
        };
        let (kind, origin) = (q.kind, q.origin);
        let predicate = move |_: ColliderHandle, co: &rapier3d::geometry::Collider| {
            if co.collision_groups().memberships.bits() & mask == 0 {
                return false;
            }
            if co.shape().as_trimesh().is_some() {
                return true;
            }
            match (kind, &start_shape) {
                (QueryKind::Raycast, _) => !co.shape().contains_point(co.position(), origin),
                (QueryKind::ShapeCast, Some(s)) => !rapier3d::parry::query::intersection_test(
                    &start,
                    &**s,
                    co.position(),
                    co.shape(),
                )
                .unwrap_or(false),
                _ => true,
            }
        };
        let mut filter = QueryFilter::new().predicate(&predicate);
        if !q.hit_sensors {
            filter = filter.exclude_sensors();
        }
        if let Some(entry) = q.exclude.and_then(|e| self.entries.get(&e)) {
            if let Some(b) = entry.body {
                filter = filter.exclude_rigid_body(b);
            }
            if let Some(c) = entry.collider {
                filter = filter.exclude_collider(c);
            }
        }
        let entity_of = |h: ColliderHandle| -> Option<Entity> {
            self.world
                .colliders
                .get(h)
                .map(|c| Entity::from_bits(c.user_data))
        };
        if !q.origin.is_finite() {
            return;
        }
        match q.kind {
            QueryKind::Raycast => {
                let Some(dir) = q.direction.try_normalize() else {
                    return;
                };
                let ray = Ray::new(q.origin, dir);
                let pipeline = self.world.query_pipeline_with_filter(filter);
                if max_hits == 1 {
                    if let Some((h, hit)) =
                        pipeline.cast_ray_and_get_normal(&ray, q.max_distance, true)
                    {
                        if let Some(entity) = entity_of(h) {
                            out.push(Hit {
                                entity,
                                point: ray.point_at(hit.time_of_impact),
                                normal: hit.normal,
                                distance: hit.time_of_impact,
                            });
                        }
                    }
                } else {
                    let mut hits: Vec<Hit> = pipeline
                        .intersect_ray(ray, q.max_distance, true)
                        .filter_map(|(h, _, hit)| {
                            Some(Hit {
                                entity: entity_of(h)?,
                                point: ray.point_at(hit.time_of_impact),
                                normal: hit.normal,
                                distance: hit.time_of_impact,
                            })
                        })
                        .collect();
                    hits.sort_by(|x, y| {
                        x.distance
                            .total_cmp(&y.distance)
                            .then(x.entity.cmp(&y.entity))
                    });
                    hits.truncate(max_hits);
                    out.extend(hits);
                }
            }
            QueryKind::ShapeCast => {
                let Some(dir) = q.direction.try_normalize() else {
                    return;
                };
                let Some(shape) = query_shape(q) else {
                    return;
                };
                let p = pose(q.origin, q.rotation.normalize());
                let options = ShapeCastOptions::with_max_time_of_impact(q.max_distance);
                if let Some((h, hit)) = self.world.cast_shape(&p, dir, &*shape, options, filter) {
                    if let Some(entity) = entity_of(h) {
                        out.push(Hit {
                            entity,
                            point: hit.witness1,
                            normal: hit.normal1,
                            distance: hit.time_of_impact,
                        });
                    }
                }
            }
            QueryKind::Overlap => {
                let Some(shape) = query_shape(q) else {
                    return;
                };
                let p = pose(q.origin, q.rotation.normalize());
                let pipeline = self.world.query_pipeline_with_filter(filter);
                let mut hits: Vec<Hit> = pipeline
                    .intersect_shape(p, &*shape)
                    .filter_map(|(h, c)| {
                        Some(Hit {
                            entity: entity_of(h)?,
                            // The collider's origin (its offset included).
                            point: c.position().translation,
                            normal: Vec3::ZERO,
                            distance: 0.0,
                        })
                    })
                    .collect();
                hits.sort_by_key(|h| h.entity);
                hits.truncate(max_hits);
                out.extend(hits);
            }
        }
    }

    // ------------------------------------------------------------- debug

    /// Line segments outlining every collider (world space) with whether
    /// it is a sensor, for a host's debug draw.
    pub fn debug_lines(&self, out: &mut Vec<(Vec3, Vec3, bool)>) {
        for (_, co) in self.world.colliders.iter() {
            let p = co.position();
            let sensor = co.is_sensor();
            let shape = co.shape();
            if let Some(c) = shape.as_cuboid() {
                let h = c.half_extents;
                let corners: Vec<Vec3> = (0..8)
                    .map(|i| {
                        let v = Vec3::new(
                            if i & 1 == 0 { -h.x } else { h.x },
                            if i & 2 == 0 { -h.y } else { h.y },
                            if i & 4 == 0 { -h.z } else { h.z },
                        );
                        p.transform_point(v)
                    })
                    .collect();
                for (a, b) in [
                    (0, 1),
                    (2, 3),
                    (4, 5),
                    (6, 7),
                    (0, 2),
                    (1, 3),
                    (4, 6),
                    (5, 7),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ] {
                    out.push((corners[a], corners[b], sensor));
                }
            } else if let Some(b) = shape.as_ball() {
                circles(out, p, b.radius, 0.0, sensor);
            } else if let Some(c) = shape.as_capsule() {
                circles(out, p, c.radius, c.half_height(), sensor);
            } else {
                let aabb = co.compute_aabb();
                let (min, max) = (aabb.mins, aabb.maxs);
                let corner = |i: usize| {
                    Vec3::new(
                        if i & 1 == 0 { min.x } else { max.x },
                        if i & 2 == 0 { min.y } else { max.y },
                        if i & 4 == 0 { min.z } else { max.z },
                    )
                };
                for (a, b) in [
                    (0, 1),
                    (2, 3),
                    (4, 5),
                    (6, 7),
                    (0, 2),
                    (1, 3),
                    (4, 6),
                    (5, 7),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ] {
                    out.push((corner(a), corner(b), sensor));
                }
            }
        }
    }
}

/// Three great circles (a sphere) or, with a half height, the capsule's
/// two end circles plus its side lines, 16 segments each.
fn circles(out: &mut Vec<(Vec3, Vec3, bool)>, p: &Pose, r: f32, half_height: f32, sensor: bool) {
    const N: usize = 16;
    let ring = |axis: usize, center: Vec3, out: &mut Vec<(Vec3, Vec3, bool)>| {
        let mut prev = None;
        for i in 0..=N {
            let t = i as f32 / N as f32 * std::f32::consts::TAU;
            let (s, c) = t.sin_cos();
            let v = match axis {
                0 => Vec3::new(0.0, c * r, s * r),
                1 => Vec3::new(c * r, 0.0, s * r),
                _ => Vec3::new(c * r, s * r, 0.0),
            };
            let w = p.transform_point(center + v);
            if let Some(q) = prev {
                out.push((q, w, sensor));
            }
            prev = Some(w);
        }
    };
    if half_height == 0.0 {
        ring(0, Vec3::ZERO, out);
        ring(1, Vec3::ZERO, out);
        ring(2, Vec3::ZERO, out);
    } else {
        let top = Vec3::Y * half_height;
        ring(1, top, out);
        ring(1, -top, out);
        for v in [Vec3::X * r, -Vec3::X * r, Vec3::Z * r, -Vec3::Z * r] {
            out.push((
                p.transform_point(top + v),
                p.transform_point(-top + v),
                sensor,
            ));
        }
    }
}

fn query_shape(q: &QuerySpec) -> Option<SharedShape> {
    if !q.size.is_finite() {
        return None;
    }
    Some(match q.shape {
        ColliderShape::Box => {
            if q.size.min_element() <= 0.0 {
                return None;
            }
            SharedShape::cuboid(q.size.x, q.size.y, q.size.z)
        }
        ColliderShape::Sphere => {
            if q.size.x <= 0.0 {
                return None;
            }
            SharedShape::ball(q.size.x)
        }
        ColliderShape::Capsule => {
            if q.size.x <= 0.0 || q.size.y < 0.0 {
                return None;
            }
            SharedShape::capsule_y(q.size.y, q.size.x)
        }
        ColliderShape::Mesh => return None,
    })
}

/// Whether a world scale differs from the one a shape was built with by
/// more than float noise.
fn rescaled(was: Vec3, now: Vec3) -> bool {
    (was - now).abs().max_element() > 1e-4 * now.abs().max_element().max(1.0)
}

/// `v` without its components along a body's frozen axes (translations,
/// or rotations when `angular`): Rapier's locks act through the mass, so a
/// velocity set directly would still move a frozen axis (foundation 2.5).
fn masked(v: Vec3, locked: LockedAxes, angular: bool) -> Vec3 {
    let axes = if angular {
        [
            LockedAxes::ROTATION_LOCKED_X,
            LockedAxes::ROTATION_LOCKED_Y,
            LockedAxes::ROTATION_LOCKED_Z,
        ]
    } else {
        [
            LockedAxes::TRANSLATION_LOCKED_X,
            LockedAxes::TRANSLATION_LOCKED_Y,
            LockedAxes::TRANSLATION_LOCKED_Z,
        ]
    };
    let keep = |i: usize| if locked.contains(axes[i]) { 0.0 } else { 1.0 };
    v * Vec3::new(keep(0), keep(1), keep(2))
}

/// Which axes a body may translate along and rotate about, from Unity's
/// `RigidbodyConstraints` bits (foundation 2.5).
fn freedoms(bits: u32) -> ([bool; 3], [bool; 3]) {
    use crate::components::constraints as c;
    let free = |b| bits & b == 0;
    (
        [
            free(c::FREEZE_POSITION_X),
            free(c::FREEZE_POSITION_Y),
            free(c::FREEZE_POSITION_Z),
        ],
        [
            free(c::FREEZE_ROTATION_X),
            free(c::FREEZE_ROTATION_Y),
            free(c::FREEZE_ROTATION_Z),
        ],
    )
}

/// The body a lone collider on `entity` joins (ADR-0034): its nearest
/// ancestor with a `RigidBody`. A `CharacterController` on the way keeps
/// its children out (its capsule is its whole shape). An active entity's
/// ancestors are active, so the body is too.
fn home_of(ecs: &Ecs, entity: Entity) -> Option<Entity> {
    let mut e = entity;
    while let Ok(Parent(p)) = ecs.copy::<Parent>(e) {
        if ecs.has::<CharacterController>(p) {
            return None;
        }
        if ecs.has::<RigidBody>(p) {
            return Some(p);
        }
        e = p;
    }
    None
}

/// `entity`'s transform relative to its ancestor `body`: the product of
/// the local transforms between them.
fn relative(ecs: &Ecs, entity: Entity, body: Entity) -> Mat4 {
    let local = |e: Entity| {
        ecs.copy::<Transform>(e)
            .map_or(Mat4::IDENTITY, |t| t.matrix())
    };
    let mut m = local(entity);
    let mut e = entity;
    while let Ok(Parent(p)) = ecs.copy::<Parent>(e) {
        if p == body {
            break;
        }
        m = local(p) * m;
        e = p;
    }
    m
}

fn body_type(t: BodyType) -> RigidBodyType {
    match t {
        BodyType::Static => RigidBodyType::Fixed,
        BodyType::Dynamic => RigidBodyType::Dynamic,
        BodyType::Kinematic => RigidBodyType::KinematicPositionBased,
    }
}

fn mass_of(r: &RigidBody) -> f32 {
    if r.body_type == BodyType::Dynamic {
        r.mass
    } else {
        0.0
    }
}

fn set_owned(ecs: &mut Ecs, entity: Entity, owned: bool) {
    if owned {
        let _ = ecs.insert(entity, PhysicsOwned);
    } else if ecs.has::<PhysicsOwned>(entity) {
        let _ = ecs.remove::<PhysicsOwned>(entity);
    }
}

/// Writes a world pose as the entity's local transform (scale kept).
// A currently orthogonal world basis can become sheared as soon as a body
// rotates under nonuniform scale. Reject that hierarchy before simulation.
fn supports_motion_parent(ecs: &Ecs, mut parent: Option<Entity>) -> bool {
    while let Some(entity) = parent {
        let Ok(local) = ecs.get::<Transform>(entity) else {
            return false;
        };
        let scale = local.scale;
        if !scale.is_finite()
            || scale.min_element() <= 0.0
            || scale.max_element() - scale.min_element() > 1e-5 * scale.max_element()
        {
            return false;
        }
        parent = scene::parent_of(ecs, entity);
    }
    true
}

fn write_pose(ecs: &mut Ecs, entity: Entity, position: Vec3, rotation: Quat) -> scene::Result<()> {
    let poses = scene::prepare_world_poses(ecs, &[(entity, position, rotation)])?;
    scene::set_local_batch(ecs, &poses)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn world() -> Ecs {
        Ecs::new()
    }

    pub(crate) fn spawn(
        ecs: &mut Ecs,
        at: Vec3,
        rigid: Option<RigidBody>,
        shape: Option<Collider>,
    ) -> Entity {
        let e = scene::spawn(ecs, Transform::from_position(at), None).unwrap();
        if let Some(r) = rigid {
            ecs.insert(e, r).unwrap();
        }
        if let Some(c) = shape {
            ecs.insert(e, c).unwrap();
        }
        e
    }

    pub(crate) fn tick(p: &mut Physics, ecs: &mut Ecs, watched: &BTreeSet<Entity>) {
        scene::propagate(ecs);
        p.step(ecs, 1.0 / 60.0, watched);
        scene::propagate(ecs);
        p.record(ecs);
    }

    #[test]
    fn moving_bodies_reject_unsafe_ancestors_before_rotation_and_recover() {
        for body_type in [BodyType::Dynamic, BodyType::Kinematic] {
            for scale in [Vec3::new(2.0, 1.0, 1.0), Vec3::splat(-1.0)] {
                let mut ecs = world();
                let mut p = Physics::new();
                p.set_gravity(Vec3::ZERO);
                let root = scene::spawn(
                    &mut ecs,
                    Transform {
                        scale,
                        ..Transform::IDENTITY
                    },
                    None,
                )
                .unwrap();
                let middle = scene::spawn(&mut ecs, Transform::IDENTITY, Some(root)).unwrap();
                let body = scene::spawn(&mut ecs, Transform::IDENTITY, Some(middle)).unwrap();
                ecs.insert(
                    body,
                    RigidBody {
                        body_type,
                        ..RigidBody::default()
                    },
                )
                .unwrap();
                ecs.insert(body, Collider::default()).unwrap();
                let watched = BTreeSet::new();
                tick(&mut p, &mut ecs, &watched);
                assert_eq!(p.stats().bodies, 0, "{body_type:?}, {scale:?}");
                assert_eq!(p.stats().colliders, 0);
                let rejected = p.take_rejections();
                assert_eq!(rejected.len(), 1);
                assert_eq!(rejected[0].kind, RejectionKind::UnsupportedTransform);
                tick(&mut p, &mut ecs, &watched);
                assert!(
                    p.take_rejections().is_empty(),
                    "report once while unsupported"
                );

                scene::set_local(
                    &ecs,
                    root,
                    Transform {
                        scale: Vec3::splat(2.0),
                        ..Transform::IDENTITY
                    },
                )
                .unwrap();
                tick(&mut p, &mut ecs, &watched);
                assert_eq!(p.stats().bodies, 1);
                assert_eq!(p.stats().colliders, 1);
                let rotation = Quat::from_rotation_z(0.7);
                let command = if body_type == BodyType::Dynamic {
                    BodyCommand::SetAngularVelocity(Vec3::new(0.0, 0.0, 3.0))
                } else {
                    BodyCommand::SetKinematicTarget {
                        position: Vec3::ZERO,
                        rotation,
                    }
                };
                p.queue(0, body, command);
                for _ in 0..3 {
                    tick(&mut p, &mut ecs, &watched);
                    assert!(Transform::try_from_matrix(
                        ecs.get::<WorldTransform>(body).unwrap().matrix
                    )
                    .is_some());
                    assert!(p.take_rejections().is_empty());
                    assert_eq!(p.stats().bodies, 1);
                }
                scene::set_local(
                    &ecs,
                    root,
                    Transform {
                        scale,
                        ..Transform::IDENTITY
                    },
                )
                .unwrap();
                tick(&mut p, &mut ecs, &watched);
                assert_eq!(p.stats().bodies, 0, "an existing body must also be removed");
                assert_eq!(
                    p.take_rejections()[0].kind,
                    RejectionKind::UnsupportedTransform
                );
            }
        }
    }

    #[test]
    fn unsafe_parent_scale_rejects_characters_but_allows_static_trs_colliders() {
        let mut ecs = world();
        let mut p = Physics::new();
        let root = scene::spawn(
            &mut ecs,
            Transform {
                scale: Vec3::new(2.0, 1.0, 1.0),
                ..Transform::IDENTITY
            },
            None,
        )
        .unwrap();
        let character = scene::spawn(&mut ecs, Transform::IDENTITY, Some(root)).unwrap();
        ecs.insert(character, CharacterController::default())
            .unwrap();
        let fixed = scene::spawn(
            &mut ecs,
            Transform::from_position(Vec3::X * 10.0),
            Some(root),
        )
        .unwrap();
        ecs.insert(
            fixed,
            RigidBody {
                body_type: BodyType::Static,
                ..RigidBody::default()
            },
        )
        .unwrap();
        ecs.insert(fixed, Collider::default()).unwrap();
        tick(&mut p, &mut ecs, &BTreeSet::new());
        assert!(!p.entries.contains_key(&character));
        assert!(p.entries.contains_key(&fixed));
        let rejected = p.take_rejections();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entity, character);
        assert_eq!(rejected[0].kind, RejectionKind::UnsupportedTransform);
        scene::set_local(&ecs, root, Transform::IDENTITY).unwrap();
        tick(&mut p, &mut ecs, &BTreeSet::new());
        assert!(p.entries.contains_key(&character));
        assert!(p.take_rejections().is_empty());
    }

    #[test]
    fn recording_a_matrix_does_not_certify_it_as_valid() {
        let mut ecs = world();
        let mut p = Physics::new();
        let body = spawn(
            &mut ecs,
            Vec3::ZERO,
            Some(RigidBody::default()),
            Some(Collider::default()),
        );
        scene::propagate(&mut ecs);
        p.step(&mut ecs, 1.0 / 60.0, &BTreeSet::new());
        scene::propagate(&mut ecs);
        // Model an unsupported post-physics result entering the writeback
        // cache. The next sync must validate it even though it is unchanged.
        ecs.get_mut::<WorldTransform>(body).unwrap().matrix.y_axis.x = 0.5;
        p.record(&ecs);
        p.refresh(&mut ecs);
        assert!(p.entries.is_empty());
        assert_eq!(
            p.take_rejections()[0].kind,
            RejectionKind::UnsupportedTransform
        );
    }

    #[test]
    fn unsupported_affine_colliders_are_removed_reported_once_and_can_recover() {
        let mut ecs = world();
        let mut physics = Physics::new();
        let root = scene::spawn(&mut ecs, Transform::IDENTITY, None).unwrap();
        let child = scene::spawn(
            &mut ecs,
            Transform {
                rotation: Quat::from_rotation_z(0.7),
                ..Transform::IDENTITY
            },
            Some(root),
        )
        .unwrap();
        ecs.insert(child, Collider::default()).unwrap();
        let watched = BTreeSet::new();
        tick(&mut physics, &mut ecs, &watched);
        assert_eq!(physics.world.colliders.len(), 1);
        scene::set_local(
            &ecs,
            root,
            Transform {
                scale: Vec3::new(2.0, 1.0, 1.0),
                ..Transform::IDENTITY
            },
        )
        .unwrap();
        tick(&mut physics, &mut ecs, &watched);
        assert_eq!(physics.world.colliders.len(), 0);
        let rejected = physics.take_rejections();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].kind, RejectionKind::UnsupportedTransform);
        tick(&mut physics, &mut ecs, &watched);
        assert!(physics.take_rejections().is_empty());
        scene::set_local(&ecs, root, Transform::IDENTITY).unwrap();
        tick(&mut physics, &mut ecs, &watched);
        assert_eq!(physics.world.colliders.len(), 1);
        scene::set_local(
            &ecs,
            root,
            Transform {
                scale: Vec3::new(0.0, 1.0, 1.0),
                ..Transform::IDENTITY
            },
        )
        .unwrap();
        tick(&mut physics, &mut ecs, &watched);
        assert_eq!(physics.world.colliders.len(), 0);
        assert_eq!(
            physics.take_rejections()[0].kind,
            RejectionKind::UnsupportedTransform
        );
    }

    #[test]
    fn a_crate_falls_onto_a_floor_deterministically() {
        let run = || {
            let mut ecs = world();
            let mut p = Physics::new();
            let floor = spawn(
                &mut ecs,
                Vec3::ZERO,
                None,
                Some(Collider {
                    size: Vec3::new(10.0, 0.5, 10.0),
                    ..Collider::default()
                }),
            );
            let crate_ = spawn(
                &mut ecs,
                Vec3::new(0.0, 3.0, 0.0),
                Some(RigidBody::default()),
                Some(Collider::default()),
            );
            let none = BTreeSet::new();
            let mut enters = 0;
            for _ in 0..240 {
                tick(&mut p, &mut ecs, &none);
                enters += p
                    .events()
                    .iter()
                    .filter(|e| matches!(e, PhysicsEvent::CollisionEnter(c) if c.a == crate_ || c.b == crate_))
                    .count();
            }
            assert!(ecs.has::<PhysicsOwned>(crate_));
            assert!(!ecs.has::<PhysicsOwned>(floor));
            (ecs.copy::<Transform>(crate_).unwrap(), enters)
        };
        let (a, enters) = run();
        let (b, _) = run();
        assert_eq!(a, b, "same inputs, same transform");
        assert_eq!(enters, 1, "one enter for the landing");
        // Resting on the floor: half extents 0.5 on a slab whose top is y = 0.5
        // (a box landing flat drifts a few millimetres while its contact settles).
        assert!((a.position.y - 1.0).abs() < 0.02, "{:?}", a.position);
        assert!(
            a.position.x.abs() < 0.05 && a.position.z.abs() < 0.05,
            "{:?}",
            a.position
        );
    }

    #[test]
    fn triggers_and_stays() {
        let mut ecs = world();
        let mut p = Physics::new();
        let floor = spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                size: Vec3::new(10.0, 0.5, 10.0),
                ..Collider::default()
            }),
        );
        let zone = spawn(
            &mut ecs,
            Vec3::new(0.0, 2.0, 0.0),
            None,
            Some(Collider {
                size: Vec3::new(1.0, 0.25, 1.0),
                is_sensor: true,
                ..Collider::default()
            }),
        );
        let ball = spawn(
            &mut ecs,
            Vec3::new(0.0, 4.0, 0.0),
            Some(RigidBody::default()),
            Some(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.25, 0.0, 0.0),
                ..Collider::default()
            }),
        );
        let watched: BTreeSet<Entity> = [ball].into_iter().collect();
        let (mut t_enter, mut t_exit, mut stays, mut c_enter, mut t_stays) = (0, 0, 0, 0, 0);
        for _ in 0..240 {
            tick(&mut p, &mut ecs, &watched);
            for e in p.events() {
                match e {
                    PhysicsEvent::TriggerEnter(c) => {
                        t_enter += 1;
                        assert!((c.a == zone && c.b == ball) || (c.a == ball && c.b == zone));
                        assert_eq!(c.impulse, Vec3::ZERO);
                    }
                    PhysicsEvent::TriggerExit(_) => t_exit += 1,
                    PhysicsEvent::CollisionStay(c) => {
                        stays += 1;
                        assert!(c.a == floor || c.b == floor);
                    }
                    PhysicsEvent::CollisionEnter(c) => {
                        c_enter += 1;
                        assert!(c.impulse.is_finite());
                        assert!(c.normal.length() > 0.9);
                    }
                    PhysicsEvent::CollisionExit(_) | PhysicsEvent::ControllerHit(_) => {}
                    PhysicsEvent::TriggerStay(c) => {
                        t_stays += 1;
                        assert!((c.a == zone && c.b == ball) || (c.a == ball && c.b == zone));
                    }
                }
            }
        }
        assert_eq!(t_enter, 1, "the trigger fires exactly once");
        assert!(
            t_stays > 0,
            "the watched ball stays in the zone while it falls through"
        );
        assert_eq!(t_exit, 1);
        assert_eq!(c_enter, 1);
        assert!(stays > 100, "resting on the floor reports stays: {stays}");
        // Unwatched: no stays.
        let none = BTreeSet::new();
        tick(&mut p, &mut ecs, &none);
        assert!(!p
            .events()
            .iter()
            .any(|e| matches!(e, PhysicsEvent::CollisionStay(_))));
    }

    #[test]
    fn commands_kinematics_and_rejections() {
        let mut ecs = world();
        let mut p = Physics::new();
        let none = BTreeSet::new();
        let ball = spawn(
            &mut ecs,
            Vec3::ZERO,
            Some(RigidBody {
                gravity_scale: 0.0,
                angular_damping: 0.0,
                ..RigidBody::default()
            }),
            Some(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.5, 0.0, 0.0),
                ..Collider::default()
            }),
        );
        let platform = spawn(
            &mut ecs,
            Vec3::new(5.0, 0.0, 0.0),
            Some(RigidBody {
                body_type: BodyType::Kinematic,
                ..RigidBody::default()
            }),
            Some(Collider::default()),
        );
        let plain = spawn(&mut ecs, Vec3::ZERO, None, None);
        // Queued before the bodies exist: applied after the sync.
        p.queue(0, ball, BodyCommand::SetLinearVelocity(Vec3::X * 6.0));
        p.queue(1, plain, BodyCommand::ApplyImpulse(Vec3::X));
        p.queue(2, platform, BodyCommand::ApplyForce(Vec3::X));
        p.queue(
            3,
            ball,
            BodyCommand::ApplyForce(Vec3::new(f32::NAN, 0.0, 0.0)),
        );
        p.queue(
            4,
            platform,
            BodyCommand::SetKinematicTarget {
                position: Vec3::new(5.0, 1.0, 0.0),
                rotation: Quat::IDENTITY,
            },
        );
        tick(&mut p, &mut ecs, &none);
        let rej = p.take_rejections();
        assert_eq!(rej.len(), 3);
        assert_eq!(rej[0].kind, RejectionKind::MissingBody);
        assert_eq!(rej[1].kind, RejectionKind::NotDynamic);
        assert_eq!(rej[2].kind, RejectionKind::NotFinite);
        let s = p.body_state(ball).unwrap();
        assert!((s.linear_velocity.x - 6.0).abs() < 1e-5);
        assert!(!s.sleeping);
        assert_eq!(p.body_state(plain), None);
        let t = ecs.copy::<Transform>(ball).unwrap();
        assert!((t.position.x - 0.1).abs() < 1e-4, "{:?}", t.position);
        let k = ecs.copy::<Transform>(platform).unwrap();
        assert!(
            (k.position.y - 1.0).abs() < 1e-5,
            "kinematic reached its target {:?}",
            k.position
        );
        assert!(!ecs.has::<PhysicsOwned>(platform));

        // A simulation-lane transform write teleports a dynamic body and a
        // kinematic body follows its entity.
        scene::set_local(
            &ecs,
            ball,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0)),
        )
        .unwrap();
        scene::set_local(
            &ecs,
            platform,
            Transform::from_position(Vec3::new(7.0, 1.0, 0.0)),
        )
        .unwrap();
        tick(&mut p, &mut ecs, &none);
        let t = ecs.copy::<Transform>(ball).unwrap();
        assert!(
            (t.position.y - 10.0).abs() < 1e-3 && (t.position.x - 0.1).abs() < 1e-3,
            "{:?}",
            t.position
        );
        let k = ecs.copy::<Transform>(platform).unwrap();
        assert!((k.position.x - 7.0).abs() < 1e-5, "{:?}", k.position);

        // Teleport keeps velocity; impulse on 1 kg adds 1 m/s.
        p.queue(
            0,
            ball,
            BodyCommand::Teleport {
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            },
        );
        p.queue(1, ball, BodyCommand::ApplyImpulse(Vec3::X));
        tick(&mut p, &mut ecs, &none);
        assert!(p.take_rejections().is_empty());
        let s = p.body_state(ball).unwrap();
        assert!(
            (s.linear_velocity.x - 7.0).abs() < 1e-4,
            "{:?}",
            s.linear_velocity
        );

        // Removing the component removes the body and the marker.
        ecs.remove::<RigidBody>(ball).unwrap();
        tick(&mut p, &mut ecs, &none);
        assert!(!ecs.has::<PhysicsOwned>(ball));
        assert_eq!(p.body_state(ball), None);
        assert_eq!(p.stats().bodies, 1);
        // Type change in place: static → dynamic marks ownership.
        ecs.insert(
            platform,
            RigidBody {
                body_type: BodyType::Dynamic,
                ..RigidBody::default()
            },
        )
        .unwrap();
        tick(&mut p, &mut ecs, &none);
        assert!(ecs.has::<PhysicsOwned>(platform));
        assert_eq!(p.body_state(platform).unwrap().body_type, BodyType::Dynamic);
    }

    #[test]
    fn queries_and_layers() {
        let mut ecs = world();
        let mut p = Physics::new();
        let none = BTreeSet::new();
        let floor = spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                size: Vec3::new(10.0, 0.5, 10.0),
                ..Collider::default()
            }),
        );
        let wall = spawn(
            &mut ecs,
            Vec3::new(0.0, 3.0, -5.0),
            None,
            Some(Collider {
                size: Vec3::new(3.0, 3.0, 0.5),
                ..Collider::default()
            }),
        );
        let sensor = spawn(
            &mut ecs,
            Vec3::new(0.0, 3.0, -2.0),
            None,
            Some(Collider {
                size: Vec3::splat(0.5),
                is_sensor: true,
                ..Collider::default()
            }),
        );
        // The floor stays on layer 0 (no component); the wall and the
        // sensor go on layer 1 — a collider's membership is its object's.
        ecs.insert(wall, Layer { layer: 1 }).unwrap();
        ecs.insert(sensor, Layer { layer: 1 }).unwrap();
        tick(&mut p, &mut ecs, &none);
        let q = QuerySpec {
            kind: QueryKind::Raycast,
            shape: ColliderShape::Box,
            size: Vec3::ZERO,
            origin: Vec3::new(0.0, 3.0, 0.0),
            rotation: Quat::IDENTITY,
            direction: -Vec3::Z,
            max_distance: 100.0,
            mask: u32::MAX,
            exclude: None,
            max_hits: 1,
            hit_sensors: false,
        };
        let mut hits = Vec::new();
        p.query(&q, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity, wall);
        assert!((hits[0].distance - 4.5).abs() < 1e-4);
        assert!((hits[0].normal - Vec3::Z).length() < 1e-4);
        hits.clear();
        p.query(
            &QuerySpec {
                hit_sensors: true,
                max_hits: 8,
                ..q
            },
            &mut hits,
        );
        assert_eq!(
            hits.iter().map(|h| h.entity).collect::<Vec<_>>(),
            vec![sensor, wall]
        );
        hits.clear();
        p.query(&QuerySpec { mask: 1, ..q }, &mut hits);
        assert!(hits.is_empty(), "the wall is on layer 1 (bit 1), not bit 0");
        hits.clear();
        p.query(
            &QuerySpec {
                direction: -Vec3::Y,
                ..q
            },
            &mut hits,
        );
        assert_eq!(hits[0].entity, floor);
        assert!((hits[0].distance - 2.5).abs() < 1e-4);
        // Shape cast: a box sweeping towards the wall stops early.
        hits.clear();
        p.query(
            &QuerySpec {
                kind: QueryKind::ShapeCast,
                size: Vec3::splat(0.5),
                ..q
            },
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity, wall);
        assert!(
            (hits[0].distance - 4.0).abs() < 1e-3,
            "{}",
            hits[0].distance
        );
        // Overlap: a sphere at the sensor's position, sensors included.
        hits.clear();
        p.query(
            &QuerySpec {
                kind: QueryKind::Overlap,
                shape: ColliderShape::Sphere,
                size: Vec3::new(1.0, 0.0, 0.0),
                origin: Vec3::new(0.0, 3.0, -2.0),
                hit_sensors: true,
                max_hits: 8,
                ..q
            },
            &mut hits,
        );
        assert_eq!(
            hits.iter().map(|h| h.entity).collect::<Vec<_>>(),
            vec![sensor]
        );
        // An overlap's point is the collider's origin (foundation 2.11).
        assert_eq!(hits[0].point, Vec3::new(0.0, 3.0, -2.0));
        // Layers on contacts: a ball on layer 2 with mask 1 (only the floor)
        // passes through the wall's layer mate? No — both sides must agree:
        // the wall's mask is all, the ball's mask excludes bit 1.
        let ball = spawn(
            &mut ecs,
            Vec3::new(0.0, 3.0, -4.0),
            Some(RigidBody {
                gravity_scale: 0.0,
                ..RigidBody::default()
            }),
            Some(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.25, 0.0, 0.0),
                mask: 1,
                ..Collider::default()
            }),
        );
        ecs.insert(ball, Layer { layer: 2 }).unwrap();
        p.queue(0, ball, BodyCommand::SetLinearVelocity(-Vec3::Z * 10.0));
        for _ in 0..30 {
            tick(&mut p, &mut ecs, &none);
        }
        let t = ecs.copy::<Transform>(ball).unwrap();
        assert!(
            t.position.z < -6.0,
            "passed through the wall: {:?}",
            t.position
        );
        // Moving an object to another layer at run time rebuilds its
        // collider's groups: the sensor on layer 0 now answers a bit-0 ray.
        ecs.insert(sensor, Layer { layer: 0 }).unwrap();
        tick(&mut p, &mut ecs, &none);
        hits.clear();
        p.query(
            &QuerySpec {
                mask: 1,
                hit_sensors: true,
                ..q
            },
            &mut hits,
        );
        assert_eq!(
            hits.iter().map(|h| h.entity).collect::<Vec<_>>(),
            vec![sensor]
        );
        let mut lines = Vec::new();
        p.debug_lines(&mut lines);
        assert!(lines.len() > 12 * 3);
    }

    #[test]
    fn mesh_colliders() {
        let mut ecs = world();
        let mut p = Physics::new();
        let none = BTreeSet::new();
        let key = AssetRef::with_sub(
            "70d7ba8b-f7fb-4eba-920d-ec9e6b968330".parse().unwrap(),
            "mesh/0",
        );
        // A 10×10 quad at y = 0.
        let quad = [
            [-5.0, 0.0, -5.0],
            [5.0, 0.0, -5.0],
            [5.0, 0.0, 5.0],
            [-5.0, 0.0, 5.0],
        ];
        assert_eq!(
            p.register_mesh(key.clone(), &quad, &[0, 2, 1, 0, 3, 2])
                .unwrap(),
            2
        );
        assert!(p.register_mesh(key.clone(), &quad, &[0, 9, 1]).is_err());
        let big: Vec<u32> = (0..(crate::MAX_MESH_COLLIDER_TRIANGLES as u32 + 1) * 3)
            .map(|i| i % 4)
            .collect();
        assert!(p.register_mesh(key.clone(), &quad, &big).is_err());
        let ground = spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                shape: ColliderShape::Mesh,
                mesh: Some(key.clone()),
                ..Collider::default()
            }),
        );
        let ball = spawn(
            &mut ecs,
            Vec3::new(1.0, 2.0, 1.0),
            Some(RigidBody::default()),
            Some(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.25, 0.0, 0.0),
                ..Collider::default()
            }),
        );
        let moving = spawn(
            &mut ecs,
            Vec3::new(0.0, 5.0, 0.0),
            Some(RigidBody::default()),
            Some(Collider {
                shape: ColliderShape::Mesh,
                mesh: Some(key.clone()),
                ..Collider::default()
            }),
        );
        let missing = spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                shape: ColliderShape::Mesh,
                mesh: Some(AssetRef::new(
                    "0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff".parse().unwrap(),
                )),
                ..Collider::default()
            }),
        );
        for _ in 0..180 {
            tick(&mut p, &mut ecs, &none);
        }
        let rej = p.take_rejections();
        assert_eq!(rej.len(), 2, "{rej:?}");
        assert!(rej
            .iter()
            .any(|r| r.entity == moving && r.kind == RejectionKind::MeshOnMovingBody));
        assert!(rej
            .iter()
            .any(|r| r.entity == missing && r.kind == RejectionKind::MeshUnavailable));
        let t = ecs.copy::<Transform>(ball).unwrap();
        assert!(
            (t.position.y - 0.25).abs() < 0.02,
            "rests on the mesh: {:?}",
            t.position
        );
        assert!(p.stats().colliders >= 2);
        let _ = ground;
    }

    /// A mesh made in code changed under its collider (ADR-0026 §4): the
    /// collider is rebuilt from the new triangles at the next step, and
    /// reports the mesh unavailable once it is removed.
    #[test]
    fn a_replaced_mesh_rebuilds_its_colliders() {
        let mut ecs = world();
        let mut p = Physics::new();
        let none = BTreeSet::new();
        let key = AssetRef::new("5c4e3f2a-1b0d-4c9e-8a7b-6f5e4d3c2b1a".parse().unwrap());
        let quad = |y: f32| {
            [
                [-5.0, y, -5.0],
                [5.0, y, -5.0],
                [5.0, y, 5.0],
                [-5.0, y, 5.0],
            ]
        };
        let tris = [0, 2, 1, 0, 3, 2];
        assert!(!p.uses_mesh(&key));
        p.register_mesh(key.clone(), &quad(0.0), &tris).unwrap();
        assert!(p.uses_mesh(&key));
        spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                shape: ColliderShape::Mesh,
                mesh: Some(key.clone()),
                ..Collider::default()
            }),
        );
        let ball = spawn(
            &mut ecs,
            Vec3::new(1.0, 1.0, 1.0),
            Some(RigidBody::default()),
            Some(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.25, 0.0, 0.0),
                ..Collider::default()
            }),
        );
        let settle = |p: &mut Physics, ecs: &mut Ecs| {
            for _ in 0..120 {
                tick(p, ecs, &none);
            }
            ecs.copy::<Transform>(ball).unwrap().position.y
        };
        assert!((settle(&mut p, &mut ecs) - 0.25).abs() < 0.02);

        // Lowered by 3 m: the ball falls onto the new floor.
        p.replace_mesh(&key, Some((&quad(-3.0), &tris))).unwrap();
        assert!((settle(&mut p, &mut ecs) + 2.75).abs() < 0.02);
        assert!(p.take_rejections().is_empty());

        // Removed: the collider says so, the ball falls through.
        p.replace_mesh(&key, None).unwrap();
        assert!(settle(&mut p, &mut ecs) < -5.0);
        assert!(p
            .take_rejections()
            .iter()
            .any(|r| r.kind == RejectionKind::MeshUnavailable));
    }

    #[test]
    fn mesh_registration_invalidates_existing_and_unavailable_colliders() {
        let mut ecs = world();
        let mut p = Physics::new();
        let key = AssetRef::new("5c4e3f2a-1b0d-4c9e-8a7b-6f5e4d3c2b1a".parse().unwrap());
        let e = spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                shape: ColliderShape::Mesh,
                mesh: Some(key.clone()),
                ..Default::default()
            }),
        );
        p.refresh(&mut ecs);
        assert!(p.entries[&e].collider.is_none());
        let quad = |y| [[-5., y, -5.], [5., y, -5.], [5., y, 5.], [-5., y, 5.]];
        let tris = [0, 2, 1, 0, 3, 2];
        p.register_mesh(key.clone(), &quad(0.), &tris).unwrap();
        p.refresh(&mut ecs);
        let h = p.entries[&e]
            .collider
            .expect("late registration must retry unavailable collider");
        assert_eq!(p.world.colliders[h].compute_aabb().center().y, 0.);
        p.register_prepared_mesh(
            key.clone(),
            crate::PreparedMesh::new(&quad(-3.), &tris).unwrap(),
        );
        p.refresh(&mut ecs);
        let h = p.entries[&e].collider.unwrap();
        assert_eq!(p.world.colliders[h].compute_aabb().center().y, -3.);
        p.refresh(&mut ecs);
        assert_eq!(p.sync_candidates(), 0);
    }

    #[test]
    fn a_character_walks_jumps_and_lands_on_a_platform() {
        let run = |jump_tick: u64| {
            let mut ecs = world();
            let mut p = Physics::new();
            let floor = spawn(
                &mut ecs,
                Vec3::new(0.0, -0.5, 0.0),
                None,
                Some(Collider {
                    size: Vec3::new(10.0, 0.5, 10.0),
                    ..Collider::default()
                }),
            );
            // A platform 0.8 m high, 3 m ahead (-Z).
            let platform = spawn(
                &mut ecs,
                Vec3::new(0.0, 0.4, -4.0),
                None,
                Some(Collider {
                    size: Vec3::new(1.5, 0.4, 1.5),
                    ..Collider::default()
                }),
            );
            let player =
                scene::spawn(&mut ecs, Transform::from_position(Vec3::ZERO), None).unwrap();
            ecs.insert(player, CharacterController::default()).unwrap();
            let watched = BTreeSet::new();
            let mut log = Vec::new();
            for t in 1..=240u64 {
                if t > 1 && t < 90 {
                    p.queue(
                        t as u32,
                        player,
                        BodyCommand::Move(Vec3::new(0.0, 7.0, -3.0)),
                    );
                }
                if t == jump_tick {
                    p.queue(t as u32, player, BodyCommand::Jump(0.0));
                }
                tick(&mut p, &mut ecs, &watched);
                let st = p.character_state(player).unwrap();
                let pos = ecs.copy::<Transform>(player).unwrap().position;
                log.push((t, st, pos));
            }
            assert!(p.take_rejections().is_empty());
            (log, player, floor, platform, p, ecs)
        };
        let (log, player, _floor, _platform, mut p, mut ecs) = run(60);
        // Tick 1 is the fresh tick: standing, not yet grounded.
        assert!(!log[0].1.grounded);
        // Walking on the floor at 3 m/s along -Z, grounded, facing -Z.
        let (_, st, pos) = log[20];
        assert!(st.grounded, "{:?}", log[20]);
        assert!((st.velocity.z + 3.0).abs() < 1e-3, "{st:?}");
        assert!(
            st.velocity.y.abs() < 1e-3,
            "the vertical move is ignored: {st:?}"
        );
        assert!(pos.y.abs() < 0.03, "{pos:?}");
        assert!(pos.z < -0.5 && pos.z > -1.2, "{pos:?}");
        // The jump leaves the ground for a while, then lands on the
        // platform (its top is y = 0.8) still walking.
        let airborne: Vec<_> = log
            .iter()
            .filter(|(t, st, _)| *t >= 60 && *t < 120 && !st.grounded)
            .collect();
        assert!(airborne.len() > 20, "airborne {}", airborne.len());
        let (_, st61, pos61) = log[60];
        assert!(st61.velocity.y > 3.0, "{st61:?}");
        assert!(pos61.y > 0.05, "{pos61:?}");
        let landed = log
            .iter()
            .find(|(t, st, _)| *t > 61 && st.grounded)
            .unwrap();
        assert!((landed.2.y - 0.8).abs() < 0.05, "{landed:?}");
        assert!(landed.2.z < -2.5 && landed.2.z > -5.5, "{landed:?}");
        // After the walk it stands on the platform, still.
        let (_, st, pos) = log[239];
        assert!(st.grounded && st.velocity.length() < 1e-3, "{st:?}");
        assert!((pos.y - 0.8).abs() < 0.05 && pos.z < -2.5, "{pos:?}");
        let rot = ecs.copy::<Transform>(player).unwrap().rotation;
        let forward = rot * Vec3::NEG_Z;
        assert!(forward.z < -0.99, "faces -Z: {forward:?}");
        // The same inputs give the same walk; a different jump tick does not.
        let (again, ..) = run(60);
        assert_eq!(again, log);
        let (other, ..) = run(70);
        assert_ne!(other, log);
        // A move on a plain entity, a force on the character: rejected.
        let floor2 = spawn(
            &mut ecs,
            Vec3::new(0.0, 0.0, 5.0),
            None,
            Some(Collider::default()),
        );
        p.queue(1, floor2, BodyCommand::Move(Vec3::X));
        p.queue(2, player, BodyCommand::ApplyImpulse(Vec3::X));
        p.queue(3, player, BodyCommand::Jump(f32::NAN));
        tick(&mut p, &mut ecs, &BTreeSet::new());
        let r = p.take_rejections();
        assert_eq!(
            r.iter().map(|r| (r.index, r.kind)).collect::<Vec<_>>(),
            vec![
                (1, RejectionKind::MissingCharacter),
                (2, RejectionKind::MissingBody),
                (3, RejectionKind::NotFinite),
            ]
        );
        assert_eq!(p.stats().characters, 1);
        assert!(p.body_state(player).is_none());
        // A transform write teleports the character and it lands again.
        scene::set_local(
            &ecs,
            player,
            Transform::from_position(Vec3::new(3.0, 2.0, 3.0)),
        )
        .unwrap();
        for _ in 0..90 {
            tick(&mut p, &mut ecs, &BTreeSet::new());
        }
        let pos = ecs.copy::<Transform>(player).unwrap().position;
        assert!(pos.y.abs() < 0.03 && (pos.x - 3.0).abs() < 1e-3, "{pos:?}");
        assert!(p.character_state(player).unwrap().grounded);
        // A RigidBody next to it is reported once and ignored; removing
        // the controller drops the capsule.
        ecs.insert(player, RigidBody::default()).unwrap();
        tick(&mut p, &mut ecs, &BTreeSet::new());
        let r = p.take_rejections();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, RejectionKind::CharacterWithBody);
        assert_eq!(r[0].index, u32::MAX);
        assert!(!ecs.has::<PhysicsOwned>(player));
        tick(&mut p, &mut ecs, &BTreeSet::new());
        assert!(p.take_rejections().is_empty());
        ecs.remove::<CharacterController>(player).unwrap();
        tick(&mut p, &mut ecs, &BTreeSet::new());
        assert_eq!(p.character_count(), 0);
        assert!(p.character_state(player).is_none());
        // ... and the dynamic body takes over (it now owns the transform).
        assert!(ecs.has::<PhysicsOwned>(player));
    }

    #[test]
    fn a_character_pushes_a_crate_and_slides_off_a_steep_slope() {
        let mut ecs = world();
        let mut p = Physics::new();
        spawn(
            &mut ecs,
            Vec3::new(0.0, -0.5, 0.0),
            None,
            Some(Collider {
                size: Vec3::new(20.0, 0.5, 20.0),
                ..Collider::default()
            }),
        );
        let crate_ = spawn(
            &mut ecs,
            Vec3::new(0.0, 0.5, -2.0),
            Some(RigidBody {
                mass: 5.0,
                ..RigidBody::default()
            }),
            Some(Collider::default()),
        );
        // A 60° ramp (steeper than the 45° limit) to the right.
        let ramp = scene::spawn(
            &mut ecs,
            Transform {
                position: Vec3::new(6.0, 0.0, 0.0),
                rotation: Quat::from_rotation_z(-std::f32::consts::FRAC_PI_3),
                scale: Vec3::ONE,
            },
            None,
        )
        .unwrap();
        ecs.insert(
            ramp,
            Collider {
                size: Vec3::new(3.0, 0.2, 3.0),
                ..Collider::default()
            },
        )
        .unwrap();
        let player = scene::spawn(&mut ecs, Transform::from_position(Vec3::ZERO), None).unwrap();
        ecs.insert(player, CharacterController::default()).unwrap();
        let watched = BTreeSet::new();
        for t in 1..=120u32 {
            p.queue(t, player, BodyCommand::Move(Vec3::new(0.0, 0.0, -2.0)));
            tick(&mut p, &mut ecs, &watched);
        }
        let c = ecs.copy::<Transform>(player).unwrap().position;
        let k = ecs.copy::<Transform>(crate_).unwrap().position;
        assert!(c.z < -1.0, "walked into the crate: {c:?}");
        assert!(k.z < -2.5, "the crate was pushed ahead: {k:?}");
        assert!(k.y > 0.3, "and stays on the floor: {k:?}");
        // Back to the origin, then into the ramp: it slides, never climbs.
        scene::set_local(&ecs, player, Transform::from_position(Vec3::ZERO)).unwrap();
        let mut max_y: f32 = 0.0;
        let mut slid = false;
        for t in 1..=180u32 {
            p.queue(t, player, BodyCommand::Move(Vec3::new(3.0, 0.0, 0.0)));
            tick(&mut p, &mut ecs, &watched);
            let st = p.character_state(player).unwrap();
            slid |= st.sliding;
            max_y = max_y.max(ecs.copy::<Transform>(player).unwrap().position.y);
        }
        assert!(max_y < 1.0, "did not climb the 60° ramp: {max_y}");
        assert!(slid, "reported sliding");
        assert!(p.take_rejections().is_empty());
    }

    // ---------------------------------------------------- physics materials

    fn material_key(n: u8) -> AssetRef {
        AssetRef::new(
            format!("0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0{n:02x}")
                .parse()
                .unwrap(),
        )
    }

    fn floor_with(ecs: &mut Ecs, material: Option<AssetRef>) -> Entity {
        spawn(
            ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                size: Vec3::new(50.0, 0.5, 50.0),
                material,
                ..Collider::default()
            }),
        )
    }

    /// The highest the ball climbs after its first landing, dropped from
    /// 3 m onto the floor (its bottom 2.25 m above the floor's top).
    fn rebound(floor: PhysicsMaterial, ball: PhysicsMaterial) -> f32 {
        let mut ecs = world();
        let mut p = Physics::new();
        p.register_material(material_key(1), floor).unwrap();
        p.register_material(material_key(2), ball).unwrap();
        floor_with(&mut ecs, Some(material_key(1)));
        let b = spawn(
            &mut ecs,
            Vec3::new(0.0, 3.0, 0.0),
            Some(RigidBody::default()),
            Some(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.25, 0.0, 0.0),
                material: Some(material_key(2)),
                ..Collider::default()
            }),
        );
        let none = BTreeSet::new();
        let mut landed = false;
        let mut top = f32::MIN;
        for _ in 0..180 {
            tick(&mut p, &mut ecs, &none);
            landed |= p
                .events()
                .iter()
                .any(|e| matches!(e, PhysicsEvent::CollisionEnter(_)));
            if landed {
                top = top.max(ecs.copy::<Transform>(b).unwrap().position.y);
            }
        }
        assert!(landed);
        assert!(p.take_rejections().is_empty());
        top - 0.75
    }

    /// How far a box launched at 4 m/s along +X slides in two seconds.
    fn slide(floor: PhysicsMaterial, block: PhysicsMaterial) -> f32 {
        let mut ecs = world();
        let mut p = Physics::new();
        p.register_material(material_key(1), floor).unwrap();
        p.register_material(material_key(2), block).unwrap();
        floor_with(&mut ecs, Some(material_key(1)));
        let b = spawn(
            &mut ecs,
            Vec3::new(0.0, 1.0, 0.0),
            Some(RigidBody::default()),
            Some(Collider {
                material: Some(material_key(2)),
                ..Collider::default()
            }),
        );
        let none = BTreeSet::new();
        // Settle, then launch.
        for _ in 0..30 {
            tick(&mut p, &mut ecs, &none);
        }
        let start = ecs.copy::<Transform>(b).unwrap().position.x;
        p.queue(0, b, BodyCommand::SetLinearVelocity(Vec3::X * 4.0));
        for _ in 0..120 {
            tick(&mut p, &mut ecs, &none);
        }
        assert!(p.take_rejections().is_empty());
        ecs.copy::<Transform>(b).unwrap().position.x - start
    }

    fn mat(friction: f32, bounciness: f32) -> PhysicsMaterial {
        PhysicsMaterial {
            friction,
            bounciness,
            ..PhysicsMaterial::default()
        }
    }

    #[test]
    fn a_materials_bounciness_makes_a_ball_bounce() {
        let dead = rebound(mat(0.6, 0.0), mat(0.6, 0.0));
        let live = rebound(mat(0.6, 1.0), mat(0.6, 1.0));
        assert!(dead < 0.05, "bounciness 0 stays down: {dead}");
        assert!(
            live > 1.5,
            "bounciness 1 comes most of the way back: {live}"
        );
        // Averaged with a dead floor: half the restitution, a lower bounce.
        let half = rebound(mat(0.6, 0.0), mat(0.6, 1.0));
        assert!(half > 0.2 && half < live * 0.6, "{half} vs {live}");
    }

    #[test]
    fn a_materials_friction_decides_how_far_a_box_slides() {
        let ice = slide(mat(0.0, 0.0), mat(0.0, 0.0));
        let rubber = slide(mat(1.0, 0.0), mat(1.0, 0.0));
        assert!(ice > 7.0, "frictionless keeps going: {ice}");
        assert!(rubber < 1.5, "friction 1 stops it: {rubber}");
    }

    #[test]
    fn combine_rules_the_higher_one_wins() {
        let floor = mat(1.0, 0.0);
        let with = |rule| {
            slide(
                floor,
                PhysicsMaterial {
                    friction_combine: rule,
                    ..mat(0.0, 0.0)
                },
            )
        };
        // The floor averages; the box's rule is the higher one either way.
        let minimum = with(CombineRule::Minimum);
        let maximum = with(CombineRule::Maximum);
        let average = with(CombineRule::Average);
        assert!(minimum > 7.0, "min(1, 0) = 0 slides on: {minimum}");
        assert!(maximum < 1.5, "max(1, 0) = 1 stops: {maximum}");
        assert!(
            average > maximum && average < minimum,
            "average 0.5 is between: {maximum} < {average} < {minimum}"
        );
        // Bounciness likewise: a bouncy ball on a dead floor.
        let bouncy = |rule| {
            rebound(
                mat(0.6, 0.0),
                PhysicsMaterial {
                    bounce_combine: rule,
                    ..mat(0.6, 1.0)
                },
            )
        };
        assert!(bouncy(CombineRule::Maximum) > 1.5);
        assert!(bouncy(CombineRule::Minimum) < 0.05);
        assert!(bouncy(CombineRule::Multiply) < 0.05);
    }

    #[test]
    fn a_collider_without_a_material_keeps_its_own_values_and_a_missing_one_is_reported() {
        let mut ecs = world();
        let mut p = Physics::new();
        let own = spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                friction: 0.3,
                restitution: 0.4,
                ..Collider::default()
            }),
        );
        let missing = spawn(
            &mut ecs,
            Vec3::X * 5.0,
            None,
            Some(Collider {
                friction: 0.2,
                material: Some(material_key(9)),
                ..Collider::default()
            }),
        );
        let none = BTreeSet::new();
        tick(&mut p, &mut ecs, &none);
        let coefficients = |p: &Physics, e: Entity| {
            let c = &p.world.colliders[p.entries[&e].collider.unwrap()];
            (
                c.friction(),
                c.restitution(),
                c.friction_combine_rule(),
                c.restitution_combine_rule(),
            )
        };
        let avg = CoefficientCombineRule::Average;
        assert_eq!(coefficients(&p, own), (0.3, 0.4, avg, avg));
        assert_eq!(coefficients(&p, missing), (0.2, 0.0, avg, avg));
        let r = p.take_rejections();
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(
            (r[0].entity, r[0].kind),
            (missing, RejectionKind::MaterialUnavailable)
        );
        tick(&mut p, &mut ecs, &none);
        assert!(p.take_rejections().is_empty(), "reported once");
        // Registered later (a hot reload): the live collider takes it,
        // and new values for the same key apply to it again.
        p.register_material(
            material_key(9),
            PhysicsMaterial {
                friction: 0.9,
                bounciness: 0.5,
                friction_combine: CombineRule::Maximum,
                bounce_combine: CombineRule::Multiply,
            },
        )
        .unwrap();
        assert_eq!(
            coefficients(&p, missing),
            (
                0.9,
                0.5,
                CoefficientCombineRule::Max,
                CoefficientCombineRule::Multiply
            )
        );
        p.register_material(material_key(9), mat(0.1, 0.0)).unwrap();
        assert_eq!(coefficients(&p, missing), (0.1, 0.0, avg, avg));
        assert_eq!(coefficients(&p, own), (0.3, 0.4, avg, avg));
        assert!(p
            .register_material(material_key(3), mat(-1.0, 0.0))
            .is_err());
        assert!(p.register_material(material_key(3), mat(0.5, 2.0)).is_err());
        assert!(!p.has_material(&material_key(3)));
        assert_eq!(p.material_count(), 1);
    }

    #[test]
    fn optional_step_profile_resets_and_covers_only_executed_phases() {
        let mut ecs = world();
        let e = spawn(
            &mut ecs,
            Vec3::Y,
            Some(RigidBody::default()),
            Some(Collider::default()),
        );
        scene::propagate(&mut ecs);
        let mut physics = Physics::new();
        let watched = BTreeSet::new();
        physics.step(&mut ecs, 1.0 / 60.0, &watched);
        assert!(physics.step_profile().is_none());
        physics.set_profiling(true);
        assert!(physics.step_profile().is_none());
        physics.step(&mut ecs, 1.0 / 60.0, &watched);
        let p = physics.step_profile().unwrap();
        assert!(p.total >= p.sync + p.commands + p.characters + p.solver + p.writeback + p.events);
        ecs.despawn(e).unwrap();
        physics.step(&mut ecs, 1.0 / 60.0, &watched);
        let empty = physics.step_profile().unwrap();
        assert_eq!(empty.solver, std::time::Duration::ZERO);
        assert_eq!(empty.writeback, std::time::Duration::ZERO);
        assert_eq!(empty.events, std::time::Duration::ZERO);
        physics.set_profiling(false);
        assert!(physics.step_profile().is_none());
        physics.step(&mut ecs, 1.0 / 60.0, &watched);
        assert!(physics.step_profile().is_none());
    }

    #[test]
    fn changed_ancestor_visits_grandchildren_through_an_already_affected_child() {
        let mut ecs = world();
        let root = spawn(&mut ecs, Vec3::ZERO, None, None);
        let middle = spawn(&mut ecs, Vec3::X, None, Some(Collider::default()));
        let leaf = spawn(&mut ecs, Vec3::Y, None, Some(Collider::default()));
        scene::set_parent_keep_local(&mut ecs, middle, Some(root)).unwrap();
        scene::set_parent_keep_local(&mut ecs, leaf, Some(middle)).unwrap();
        scene::propagate(&mut ecs);
        let mut physics = Physics::new();
        physics.sync(&mut ecs);
        for attached in [true, false, true] {
            if attached {
                ecs.insert(
                    root,
                    RigidBody {
                        body_type: BodyType::Static,
                        ..RigidBody::default()
                    },
                )
                .unwrap();
            } else {
                ecs.remove::<RigidBody>(root).unwrap();
            }
            // Middle is independently dirty, but leaf has no direct changes.
            ecs.get_mut::<Collider>(middle).unwrap().friction = if attached { 0.3 } else { 0.7 };
            physics.sync(&mut ecs);
            for e in [middle, leaf] {
                let collider = &physics.world.colliders[physics.entries[&e].collider.unwrap()];
                assert_eq!(
                    collider.parent(),
                    physics.entries.get(&root).and_then(|entry| entry.body)
                );
                assert_eq!(collider.parent().is_some(), attached);
            }
            assert_eq!(physics.sync_candidates(), 3);
            physics.sync(&mut ecs);
            // Removing a body queues its two former colliders for reconciliation.
            assert_eq!(physics.sync_candidates(), if attached { 0 } else { 2 });
            physics.sync(&mut ecs);
            assert_eq!(physics.sync_candidates(), 0);
        }
        ecs.despawn(leaf).unwrap();
        physics.sync(&mut ecs);
        assert_eq!(physics.sync_candidates(), 1);
        assert!(!physics.entries.contains_key(&leaf));
        assert!(physics.entries.contains_key(&middle));
    }

    #[test]
    #[ignore = "manual nested-collider synchronization qualification"]
    fn nested_sync_qualification() {
        let mut ecs = world();
        let parent = spawn(
            &mut ecs,
            Vec3::ZERO,
            Some(RigidBody {
                body_type: BodyType::Static,
                ..RigidBody::default()
            }),
            None,
        );
        for i in 0..5000 {
            let child = spawn(
                &mut ecs,
                Vec3::X * (i * 3) as f32,
                None,
                Some(Collider::default()),
            );
            scene::set_parent_keep_local(&mut ecs, child, Some(parent)).unwrap();
        }
        scene::propagate(&mut ecs);
        let mut physics = Physics::new();
        physics.sync(&mut ecs);
        let mut samples = Vec::new();
        for i in 0..80 {
            scene::set_local(&ecs, parent, Transform::from_position(Vec3::Y * i as f32)).unwrap();
            scene::propagate(&mut ecs);
            let start = std::time::Instant::now();
            physics.sync(&mut ecs);
            let micros = start.elapsed().as_secs_f64() * 1e6;
            if i >= 20 {
                samples.push(micros);
            }
            assert_eq!(physics.world.colliders.len(), 5000);
        }
        println!("nested_sync_samples_us={samples:?}");
    }

    #[test]
    fn sync_inputs_follow_component_membership_activity_and_reused_entities() {
        let mut ecs = world();
        let mut physics = Physics::new();
        let a = spawn(&mut ecs, Vec3::ZERO, None, Some(Collider::default()));
        let b = spawn(&mut ecs, Vec3::X * 3.0, None, None);
        scene::propagate(&mut ecs);
        physics.sync(&mut ecs);
        assert!(physics.entries.contains_key(&a));
        assert!(!physics.entries.contains_key(&b));

        ecs.remove::<Collider>(a).unwrap();
        ecs.insert(b, Collider::default()).unwrap();
        physics.sync(&mut ecs);
        assert!(!physics.entries.contains_key(&a));
        assert!(physics.entries.contains_key(&b));
        assert_eq!(physics.world.colliders.len(), 1);

        ecs.insert(b, scene::Inactive).unwrap();
        physics.sync(&mut ecs);
        assert!(physics.world.colliders.is_empty());
        ecs.remove::<scene::Inactive>(b).unwrap();
        let transform = ecs.remove::<WorldTransform>(b).unwrap();
        physics.sync(&mut ecs);
        assert!(!physics.entries.contains_key(&b));
        ecs.insert(b, transform).unwrap();
        physics.sync(&mut ecs);
        assert!(physics.entries.contains_key(&b));

        ecs.despawn(b).unwrap();
        let replacement = spawn(&mut ecs, Vec3::Y * 5.0, None, Some(Collider::default()));
        assert_ne!(replacement, b);
        scene::propagate(&mut ecs);
        physics.sync(&mut ecs);
        assert!(!physics.entries.contains_key(&b));
        assert!(physics.entries.contains_key(&replacement));
        assert_eq!(physics.world.colliders.len(), 1);
        let collider = &physics.world.colliders[physics.entries[&replacement].collider.unwrap()];
        assert!((collider.position().translation.y - 5.0).abs() < 1e-5);
        physics.sync(&mut ecs);
        assert_eq!(physics.sync_candidates(), 0);
        assert_eq!(physics.world.colliders.len(), 1);
    }

    #[test]
    #[ignore = "manual moving-collider synchronization qualification"]
    fn moving_collider_sync_qualification() {
        let mut ecs = world();
        let entities: Vec<_> = (0..5000)
            .map(|i| {
                spawn(
                    &mut ecs,
                    Vec3::X * (i * 3) as f32,
                    None,
                    Some(Collider::default()),
                )
            })
            .collect();
        scene::propagate(&mut ecs);
        let mut physics = Physics::new();
        physics.sync(&mut ecs);
        let mut samples = Vec::new();
        for i in 0..80 {
            for &entity in &entities {
                let mut local = ecs.copy::<Transform>(entity).unwrap();
                local.position.y = i as f32;
                local.rotation = Quat::from_rotation_y(i as f32 * 0.01);
                scene::set_local(&ecs, entity, local).unwrap();
            }
            scene::propagate(&mut ecs);
            let start = std::time::Instant::now();
            physics.sync(&mut ecs);
            let micros = start.elapsed().as_secs_f64() * 1e6;
            if i >= 20 {
                samples.push(micros);
            }
            assert_eq!(physics.world.colliders.len(), 5000);
        }
        assert_eq!(physics.sync_candidates(), 5000);
        let collider = &physics.world.colliders[physics.entries[&entities[0]].collider.unwrap()];
        assert!((collider.position().translation.y - 79.0).abs() < 1e-5);
        println!("moving_sync_samples_us={samples:?}");
    }

    #[test]
    fn collider_motion_reuses_pose_without_losing_scaled_rotated_offset() {
        let mut ecs = world();
        let entity = spawn(
            &mut ecs,
            Vec3::ZERO,
            None,
            Some(Collider {
                offset: Vec3::new(1.0, 2.0, 3.0),
                ..Collider::default()
            }),
        );
        let mut physics = Physics::new();
        scene::set_local(
            &ecs,
            entity,
            Transform {
                scale: Vec3::new(2.0, 3.0, 4.0),
                ..Transform::IDENTITY
            },
        )
        .unwrap();
        scene::propagate(&mut ecs);
        physics.sync(&mut ecs);
        let handle = physics.entries[&entity].collider.unwrap();
        for angle in [0.3, 1.2, -2.0] {
            let transform = Transform {
                position: Vec3::new(4.0, -1.0, 2.0),
                rotation: Quat::from_rotation_y(angle),
                scale: Vec3::new(2.0, 3.0, 4.0),
            };
            scene::set_local(&ecs, entity, transform).unwrap();
            scene::propagate(&mut ecs);
            physics.sync(&mut ecs);
            assert_eq!(physics.entries[&entity].collider, Some(handle));
            let collider = &physics.world.colliders[handle];
            let expected = transform
                .matrix()
                .transform_point3(Vec3::new(1.0, 2.0, 3.0));
            assert!((collider.position().translation - expected).length() < 1e-4);
            assert!(collider.position().rotation.dot(transform.rotation).abs() > 0.99999);
        }
    }

    #[test]
    fn nested_collider_refresh_applies_shape_layer_scale_and_removal() {
        let mut ecs = world();
        let parent = spawn(
            &mut ecs,
            Vec3::ZERO,
            Some(RigidBody {
                body_type: BodyType::Static,
                ..RigidBody::default()
            }),
            None,
        );
        let child = spawn(&mut ecs, Vec3::X, None, Some(Collider::default()));
        scene::set_parent_keep_local(&mut ecs, child, Some(parent)).unwrap();
        scene::propagate(&mut ecs);
        let mut physics = Physics::new();
        physics.sync(&mut ecs);
        let handle = physics.entries[&child].collider.unwrap();
        ecs.get_mut::<Collider>(child).unwrap().friction = 0.7;
        ecs.insert(child, Layer { layer: 3 }).unwrap();
        scene::set_local(
            &ecs,
            child,
            Transform {
                position: Vec3::Y,
                scale: Vec3::splat(2.0),
                ..Transform::IDENTITY
            },
        )
        .unwrap();
        scene::propagate(&mut ecs);
        physics.sync(&mut ecs);
        assert_eq!(physics.entries[&child].collider, Some(handle));
        let collider = &physics.world.colliders[handle];
        assert_eq!(collider.friction(), 0.7);
        assert_eq!(collider.collision_groups().memberships.bits(), 1 << 3);
        assert!((collider.position_wrt_parent().unwrap().translation - Vec3::Y).length() < 1e-5);
        let bounds = collider.shape().compute_local_aabb();
        assert!((bounds.half_extents() - Vec3::ONE).length() < 1e-5);
        ecs.remove::<Collider>(child).unwrap();
        physics.sync(&mut ecs);
        assert!(!physics.entries.contains_key(&child));
        assert!(physics.world.colliders.get(handle).is_none());
    }

    #[test]
    fn gathered_child_inputs_follow_same_sync_body_creation_and_removal() {
        let mut ecs = world();
        // Allocate the child first: synchronization cannot depend on entity order.
        let child = spawn(&mut ecs, Vec3::X, None, Some(Collider::default()));
        let parent = spawn(&mut ecs, Vec3::ZERO, None, None);
        scene::set_parent_keep_local(&mut ecs, child, Some(parent)).unwrap();
        let mut physics = Physics::new();
        let watched = BTreeSet::new();
        tick(&mut physics, &mut ecs, &watched);
        for attached in [true, false, true] {
            if attached {
                ecs.insert(
                    parent,
                    RigidBody {
                        body_type: BodyType::Static,
                        ..RigidBody::default()
                    },
                )
                .unwrap();
            } else {
                ecs.remove::<RigidBody>(parent).unwrap();
            }
            scene::set_local(
                &ecs,
                parent,
                Transform {
                    position: Vec3::new(10.0, 0.0, 0.0),
                    rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                    scale: Vec3::splat(2.0),
                },
            )
            .unwrap();
            scene::set_local(
                &ecs,
                child,
                Transform::from_position(Vec3::new(2.0, 0.0, 0.0)),
            )
            .unwrap();
            ecs.insert(
                child,
                Collider {
                    offset: Vec3::X,
                    ..Collider::default()
                },
            )
            .unwrap();
            tick(&mut physics, &mut ecs, &watched);
            let collider = &physics.world.colliders[physics.entries[&child].collider.unwrap()];
            assert_eq!(collider.parent().is_some(), attached);
            assert!((collider.position().translation - Vec3::new(10.0, 6.0, 0.0)).length() < 1e-4);
            assert!(physics.take_rejections().is_empty());
        }
    }

    #[test]
    fn invalid_solver_pose_rejects_the_entire_ecs_writeback_batch() {
        let mut ecs = world();
        let a = spawn(&mut ecs, Vec3::ZERO, Some(RigidBody::default()), None);
        let b = spawn(&mut ecs, Vec3::X, Some(RigidBody::default()), None);
        scene::propagate(&mut ecs);
        let mut physics = Physics::new();
        physics.sync(&mut ecs);
        for (entity, x) in [(a, 7.0), (b, f32::NAN)] {
            let handle = physics.entries[&entity].body.unwrap();
            physics.world.bodies[handle]
                .set_position(pose(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY), true);
        }
        physics.write_back(&mut ecs);
        assert_eq!(ecs.copy::<Transform>(a).unwrap().position, Vec3::ZERO);
        assert_eq!(ecs.copy::<Transform>(b).unwrap().position, Vec3::X);
        assert!(physics.scratch_written.is_empty());
        assert_eq!(physics.take_rejections().len(), 1);
    }

    #[test]
    fn root_motion_accumulation_rejects_overflow_without_replacing_valid_intent() {
        let mut ecs = world();
        let entity = spawn(&mut ecs, Vec3::ZERO, None, None);
        ecs.insert(entity, CharacterController::default()).unwrap();
        scene::propagate(&mut ecs);
        let mut physics = Physics::new();
        physics.sync(&mut ecs);
        let velocity = Vec3::new(f32::MAX, 0.0, 0.0);
        physics.characters.get_mut(&entity).unwrap().pending_move = Some(velocity);
        physics.add_root_motion(entity, velocity);
        physics.apply_root_motion();
        assert_eq!(physics.characters[&entity].pending_move, Some(velocity));
        let errors = physics.take_rejections();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, RejectionKind::NotFinite);
    }
}

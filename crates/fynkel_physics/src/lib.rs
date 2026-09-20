//! Fynkel physics (blueprint §9, §19.2d item 8): the `RigidBody`/`Collider`
//! components and the Rapier adapter that steps them inside the fixed tick.
//!
//! The adapter is a system the core calls at the §5.1 physics slot:
//!
//! ```text
//! World::step
//!   apply C[n]                 body commands are queued here (`queue`)
//!   propagate                  world matrices reflect the commands
//!   Physics::step(ecs, dt)     sync components → bodies, apply the queued
//!                              commands, Rapier step, write dynamic and
//!                              kinematic poses back to the entities' local
//!                              transforms, collect the tick's events
//!   propagate                  the written locals become world matrices
//!   Physics::record(ecs)       remember every body's world matrix, so the
//!                              next sync sees only external moves
//! ```
//!
//! Ownership (§5.1): a dynamic body's transform is physics-owned — the
//! entity carries [`PhysicsOwned`] and presentation writes are rejected;
//! a simulation-lane transform write (or [`BodyCommand::Teleport`]) moves
//! the body. Kinematic bodies follow the entity's transform (or a
//! [`BodyCommand::SetKinematicTarget`]); static bodies and standalone
//! colliders move only when their entity is moved.
//!
//! Determinism: bodies are created in entity order and Rapier is
//! deterministic on one platform, so equal inputs give equal transforms
//! (the headless checksum). Nothing here allocates per tick in steady
//! state beyond Rapier's own working sets.

pub mod components;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use fynkel_contracts::{AssetRef, Entity};
use fynkel_ecs::internal::Handle;
use fynkel_ecs::World as Ecs;
use fynkel_scene::{self as scene, Parent, Transform, WorldTransform};
use glam::Mat4;
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::*;

pub use components::{BodyType, Collider, ColliderShape, RigidBody};
pub use glam::{Quat, Vec3};

/// A mesh collider is refused above this many triangles (the explicit
/// limit of the bounded `mesh` shape; §9 "bounded MeshCollider support").
pub const MAX_MESH_COLLIDER_TRIANGLES: usize = 32_768;

/// Marker: the transform is written by physics; presentation-lane transform
/// commands are rejected with `PHYSICS_OWNED` (ADR-0008). Carried by every
/// entity with a dynamic `RigidBody`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicsOwned;

/// How gameplay moves a body (simulation lane; §9 "forces, impulses,
/// velocities, kinematic targets, or an explicit teleport").
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyCommand {
    /// Newtons, for this tick only (dynamic bodies).
    ApplyForce(Vec3),
    /// Newton-seconds, once (dynamic bodies).
    ApplyImpulse(Vec3),
    /// Newton-metres, for this tick only (dynamic bodies).
    ApplyTorque(Vec3),
    /// Newton-metre-seconds, once (dynamic bodies).
    ApplyTorqueImpulse(Vec3),
    /// Metres per second (dynamic bodies).
    SetLinearVelocity(Vec3),
    /// Radians per second (dynamic bodies).
    SetAngularVelocity(Vec3),
    /// World pose the kinematic body reaches at the end of this tick (its
    /// velocity follows, so it pushes what it meets).
    SetKinematicTarget { position: Vec3, rotation: Quat },
    /// World pose, now, without interpolation (any body type); velocities
    /// are kept.
    Teleport { position: Vec3, rotation: Quat },
}

impl BodyCommand {
    pub fn is_finite(&self) -> bool {
        match self {
            BodyCommand::ApplyForce(v)
            | BodyCommand::ApplyImpulse(v)
            | BodyCommand::ApplyTorque(v)
            | BodyCommand::ApplyTorqueImpulse(v)
            | BodyCommand::SetLinearVelocity(v)
            | BodyCommand::SetAngularVelocity(v) => v.is_finite(),
            BodyCommand::SetKinematicTarget { position, rotation }
            | BodyCommand::Teleport { position, rotation } => {
                position.is_finite() && rotation.is_finite()
            }
        }
    }
}

/// One contact as an event carries it: `point` and `normal` in world
/// space, the normal pointing from `a` towards `b`, `impulse` the total
/// impulse of the pair over the last step (0 for triggers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub a: Entity,
    pub b: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub impulse: f32,
}

/// The physics events of one tick (§5.1 physics lane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhysicsEvent {
    CollisionEnter(Contact),
    /// Reported while a pair keeps touching, only for pairs where at
    /// least one entity is watched (`World::subscribe`): the stay stream
    /// scales with what gameplay observes, not with the scene.
    CollisionStay(Contact),
    CollisionExit(Contact),
    TriggerEnter(Contact),
    TriggerExit(Contact),
}

/// Why a body command or a component could not be honoured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionKind {
    /// The entity has no `RigidBody`.
    MissingBody,
    /// The command needs a dynamic body.
    NotDynamic,
    /// The command needs a kinematic body.
    NotKinematic,
    /// A vector or pose is not finite.
    NotFinite,
    /// A mesh collider whose mesh is not registered (or too large).
    MeshUnavailable,
    /// A mesh collider on a body that is not static.
    MeshOnMovingBody,
}

/// A rejected body command (`index` is the command's batch index) or a
/// component the sync could not realise (`index == u32::MAX`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub index: u32,
    pub entity: Entity,
    pub kind: RejectionKind,
    pub message: String,
}

/// A body's motion state (`FYNKEL_QUERY_BODY`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyState {
    pub body_type: BodyType,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub sleeping: bool,
}

/// What a spatial query asks (`fynkel.rec.PhysicsQuery`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QueryKind {
    Raycast,
    ShapeCast,
    Overlap,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuerySpec {
    pub kind: QueryKind,
    /// `ShapeCast`/`Overlap`: the shape (not `Mesh`) and its `size`.
    pub shape: ColliderShape,
    pub size: Vec3,
    pub origin: Vec3,
    /// The shape's orientation.
    pub rotation: Quat,
    /// `Raycast`/`ShapeCast`: need not be normalised.
    pub direction: Vec3,
    pub max_distance: f32,
    /// Colliders whose `layer` shares a bit with this are candidates.
    pub mask: u32,
    /// An entity whose colliders are skipped (the caster itself).
    pub exclude: Option<Entity>,
    /// At most this many hits (`Raycast`/`Overlap`; a shape cast reports
    /// the first hit only); 0 means 1.
    pub max_hits: u32,
    pub hit_sensors: bool,
}

/// One query hit: `distance` along the ray/cast (0 for overlaps),
/// `point`/`normal` on the hit collider in world space (zero for overlaps).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub entity: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicsStats {
    pub bodies: u32,
    pub colliders: u32,
    pub active_bodies: u32,
    pub contact_pairs: u32,
    /// Events collected by the last step.
    pub events: u32,
}

struct Entry {
    body: Option<RigidBodyHandle>,
    collider: Option<ColliderHandle>,
    rigid: Option<RigidBody>,
    shape: Option<Collider>,
    /// World scale the shape was built with.
    scale: Vec3,
    /// The entity's world matrix as recorded after the last propagate; a
    /// different matrix at sync means the entity was moved by a command.
    last_world: Mat4,
    /// A diagnostic already reported for this entry (once).
    reported: bool,
}

/// Collects the step's collision events; their contact data is read from
/// the narrow phase after the step, once the solver has produced the
/// tick's impulses (the event itself is raised before solving).
struct Collector {
    events: Mutex<Vec<CollisionEvent>>,
}

impl EventHandler for Collector {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        event: CollisionEvent,
        _pair: Option<&ContactPair>,
    ) {
        self.events.lock().unwrap().push(event);
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

/// Deepest contact point (world), its normal (from collider 1 to 2) and
/// the pair's impulse.
fn contact_data(colliders: &ColliderSet, pair: &ContactPair) -> Option<(Vec3, Vec3, f32)> {
    let (manifold, contact) = pair.find_deepest_contact()?;
    let c1 = colliders.get(pair.collider1)?;
    let point = c1.position().transform_point(contact.local_p1);
    let normal = manifold.data.normal;
    Some((point, normal, pair.total_impulse_magnitude()))
}

fn pose_of(m: &Mat4) -> (Vec3, Quat, Vec3) {
    let t = Transform::from_matrix(*m);
    (t.position, t.rotation.normalize(), t.scale)
}

fn pose(position: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(position, rotation)
}

fn groups(c: &Collider) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_retain(c.layer),
        Group::from_bits_retain(c.mask),
        InteractionTestMode::And,
    )
}

/// The adapter: owns the Rapier world and the entity ↔ handle maps.
pub struct Physics {
    world: PhysicsWorld,
    entries: BTreeMap<Entity, Entry>,
    /// Collider → entity, kept until the step after a removal so the
    /// `Stopped` events Rapier emits for a removed collider still resolve.
    by_collider: HashMap<ColliderHandle, Entity>,
    retired: Vec<ColliderHandle>,
    meshes: HashMap<AssetRef, (Vec<Vec3>, Vec<[u32; 3]>)>,
    pending: Vec<(u32, Entity, BodyCommand)>,
    rejections: Vec<Rejection>,
    events: Vec<PhysicsEvent>,
    /// Touching pairs `(a, b)` with `a < b`: sensor pair or not.
    touching: BTreeMap<(Entity, Entity), bool>,
    /// Sync scratch, kept for its capacity.
    scratch_entities: Vec<Entity>,
    scratch_changed: Vec<Entity>,
    scratch_gone: Vec<Entity>,
    /// Entities whose transform the last step wrote (`record` refreshes
    /// their `last_world`).
    scratch_written: Vec<Entity>,
    forced: Vec<RigidBodyHandle>,
    stats: PhysicsStats,
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}

impl Physics {
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld::new(),
            entries: BTreeMap::new(),
            by_collider: HashMap::new(),
            retired: Vec::new(),
            meshes: HashMap::new(),
            pending: Vec::new(),
            rejections: Vec::new(),
            events: Vec::new(),
            touching: BTreeMap::new(),
            scratch_entities: Vec::new(),
            scratch_changed: Vec::new(),
            scratch_gone: Vec::new(),
            scratch_written: Vec::new(),
            forced: Vec::new(),
            stats: PhysicsStats::default(),
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
    /// above [`MAX_MESH_COLLIDER_TRIANGLES`] or on an index out of range.
    pub fn register_mesh(
        &mut self,
        key: AssetRef,
        positions: &[[f32; 3]],
        indices: &[u32],
    ) -> Result<usize, String> {
        let triangles = indices.len() / 3;
        if triangles == 0 {
            return Err("mesh has no triangles".into());
        }
        if triangles > MAX_MESH_COLLIDER_TRIANGLES {
            return Err(format!(
                "mesh has {triangles} triangles, more than the {MAX_MESH_COLLIDER_TRIANGLES} a mesh collider allows"
            ));
        }
        let n = positions.len() as u32;
        let mut tris = Vec::with_capacity(triangles);
        for t in indices.chunks_exact(3) {
            if t.iter().any(|i| *i >= n) {
                return Err("mesh index out of range".into());
            }
            tris.push([t[0], t[1], t[2]]);
        }
        let verts = positions.iter().map(|p| Vec3::from(*p)).collect();
        self.meshes.insert(key, (verts, tris));
        Ok(triangles)
    }

    pub fn has_mesh(&self, key: &AssetRef) -> bool {
        self.meshes.contains_key(key)
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Queues a body command for the next [`Physics::step`] (after the
    /// sync, so a body set in the same batch receives it).
    pub fn queue(&mut self, index: u32, entity: Entity, command: BodyCommand) {
        self.pending.push((index, entity, command));
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
        let h = ecs.hecs();
        h.query::<&RigidBody>().iter().next().is_some()
            || h.query::<&Collider>().iter().next().is_some()
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

    /// Whether `entity` currently has a dynamic body (its transform is
    /// physics-owned).
    pub fn owns(&self, entity: Entity) -> bool {
        self.entries
            .get(&entity)
            .and_then(|e| e.rigid)
            .is_some_and(|r| r.body_type == BodyType::Dynamic)
    }

    // ------------------------------------------------------------- the tick

    /// The physics phase of one fixed tick; `watched` selects the entities
    /// whose `CollisionStay` events are reported.
    pub fn step(&mut self, ecs: &mut Ecs, dt: f32, watched: &BTreeSet<Entity>) {
        self.events.clear();
        self.sync(ecs);
        self.apply_pending(ecs);
        if self.entries.is_empty() {
            self.pending.clear();
            self.stats = PhysicsStats::default();
            return;
        }
        self.world.integration_parameters.dt = dt;
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
        self.write_back(ecs);
        let raw = collector.events.into_inner().unwrap();
        self.collect_events(raw, watched);
        for h in self.retired.drain(..) {
            self.by_collider.remove(&h);
        }
        self.stats = PhysicsStats {
            bodies: self.world.bodies.len() as u32,
            colliders: self.world.colliders.len() as u32,
            active_bodies: self.world.islands.active_bodies().count() as u32,
            contact_pairs: self.world.narrow_phase.contact_pairs().count() as u32,
            events: self.events.len() as u32,
        };
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

    fn sync(&mut self, ecs: &mut Ecs) {
        // Every entity with a physics component, in handle order. The
        // scratch vectors keep their capacity: with thousands of static
        // colliders this runs every tick and must not allocate.
        let mut candidates = std::mem::take(&mut self.scratch_entities);
        let mut changed = std::mem::take(&mut self.scratch_changed);
        candidates.clear();
        changed.clear();
        {
            // One pass over the archetypes: every entity with a physics
            // component and a world transform, and — the fast path — which
            // of them differ from their entry (new, a component changed,
            // the world matrix moved by gameplay). A resting or static
            // collider costs a few comparisons here and nothing below.
            let h = ecs.hecs();
            for (handle, wt, rigid, shape) in h
                .query::<(
                    &Handle,
                    &WorldTransform,
                    Option<&RigidBody>,
                    Option<&Collider>,
                )>()
                .iter()
            {
                if rigid.is_none() && shape.is_none() {
                    continue;
                }
                let entity = handle.0;
                candidates.push(entity);
                let same = self.entries.get(&entity).is_some_and(|entry| {
                    entry.rigid.is_some() == rigid.is_some()
                        && entry.shape.is_some() == shape.is_some()
                        && entry.shape.as_ref().zip(shape).is_none_or(|(a, b)| a == b)
                        && entry.rigid == rigid.copied()
                        && entry.last_world == wt.matrix
                });
                if !same {
                    changed.push(entity);
                }
            }
        }
        candidates.sort_unstable();
        changed.sort_unstable();
        // Entries whose entity lost its components or was destroyed.
        let mut gone = std::mem::take(&mut self.scratch_gone);
        gone.clear();
        if self.entries.len() != candidates.len()
            || self.entries.keys().zip(&candidates).any(|(a, b)| a != b)
        {
            gone.extend(
                self.entries
                    .keys()
                    .filter(|e| candidates.binary_search(e).is_err() || !ecs.contains(**e))
                    .copied(),
            );
        }
        for e in gone.drain(..) {
            self.remove_entry(ecs, e);
        }
        for &entity in &changed {
            let Ok(world) = ecs.get::<WorldTransform>(entity).map(|w| w.matrix) else {
                continue;
            };
            let rigid = ecs.copy::<RigidBody>(entity).ok();
            let shape = ecs.get::<Collider>(entity).ok();
            let existing = self.entries.get(&entity);
            let structural = match existing {
                None => true,
                Some(entry) => {
                    entry.rigid.is_some() != rigid.is_some()
                        || entry.shape.is_some() != shape.is_some()
                        || entry
                            .shape
                            .as_ref()
                            .zip(shape.as_deref())
                            .is_some_and(|(a, b)| a != b)
                }
            };
            let moved = existing.is_none_or(|e| e.last_world != world);
            let (position, rotation, scale) = pose_of(&world);
            let rebuild = structural
                || existing.is_some_and(|e| {
                    (e.scale - scale).abs().max_element()
                        > 1e-4 * scale.abs().max_element().max(1.0)
                });
            if rebuild {
                let shape = shape.map(|s| s.clone());
                self.remove_entry(ecs, entity);
                self.create_entry(ecs, entity, rigid, shape, position, rotation, scale, world);
                continue;
            }
            drop(shape);
            let entry = self.entries.get_mut(&entity).unwrap();
            let mut moved = moved;
            if let (Some(h), Some(new), Some(old)) = (entry.body, rigid, entry.rigid) {
                if new != old {
                    let body = &mut self.world.bodies[h];
                    if new.body_type != old.body_type {
                        body.set_body_type(body_type(new.body_type), true);
                        set_owned(ecs, entity, new.body_type == BodyType::Dynamic);
                        moved = true;
                    }
                    body.set_linear_damping(new.linear_damping);
                    body.set_angular_damping(new.angular_damping);
                    body.set_gravity_scale(new.gravity_scale, true);
                    body.enable_ccd(new.ccd);
                    body.set_additional_mass(mass_of(&new), true);
                    entry.rigid = Some(new);
                }
            }
            if moved {
                let p = pose(position, rotation);
                match (entry.body, entry.collider, entry.rigid.map(|r| r.body_type)) {
                    // A queued `SetKinematicTarget` is applied after the
                    // sync, so it wins over a transform write in the same batch.
                    (Some(h), _, Some(BodyType::Kinematic)) => {
                        self.world.bodies[h].set_next_kinematic_position(p);
                    }
                    (Some(h), _, _) => self.world.bodies[h].set_position(p, true),
                    (None, Some(c), _) => {
                        let offset = entry
                            .shape
                            .as_ref()
                            .map(|s| s.offset * scale)
                            .unwrap_or(Vec3::ZERO);
                        self.world.colliders[c].set_position(p * Pose::from_translation(offset));
                    }
                    (None, None, _) => {}
                }
                entry.last_world = world;
            }
        }
        self.scratch_entities = candidates;
        self.scratch_changed = changed;
        self.scratch_gone = gone;
    }

    #[allow(clippy::too_many_arguments)]
    fn create_entry(
        &mut self,
        ecs: &mut Ecs,
        entity: Entity,
        rigid: Option<RigidBody>,
        shape: Option<Collider>,
        position: Vec3,
        rotation: Quat,
        scale: Vec3,
        world: Mat4,
    ) {
        let p = pose(position, rotation);
        let user = entity.to_bits() as u128;
        let body = rigid.map(|r| {
            let mut b = RigidBodyBuilder::new(body_type(r.body_type))
                .pose(p)
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
        let mut entry = Entry {
            body,
            collider: None,
            rigid,
            shape: shape.clone(),
            scale,
            last_world: world,
            reported: false,
        };
        if let Some(c) = &shape {
            match self.build_shape(c, scale, rigid.map(|r| r.body_type)) {
                Ok(s) => {
                    let offset = c.offset * scale;
                    let builder = ColliderBuilder::new(s)
                        .friction(c.friction)
                        .restitution(c.restitution)
                        .sensor(c.is_sensor)
                        .collision_groups(groups(c))
                        .active_events(ActiveEvents::COLLISION_EVENTS)
                        .density(0.0)
                        .user_data(user);
                    let handle = match body {
                        Some(b) => self
                            .world
                            .insert_collider(builder.translation(offset), Some(b)),
                        None => self.world.insert_collider(
                            builder.position(p * Pose::from_translation(offset)),
                            None,
                        ),
                    };
                    self.by_collider.insert(handle, entity);
                    entry.collider = Some(handle);
                    // The effective inverse mass is otherwise refreshed by
                    // the step: a body command queued in the tick that
                    // created the body (spawn + components + impulse in one
                    // batch) must see the mass now.
                    if let Some(b) = body {
                        self.world.bodies[b]
                            .recompute_mass_properties_from_colliders(&self.world.colliders);
                    }
                }
                Err((kind, message)) => {
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
            }
        }
        set_owned(
            ecs,
            entity,
            rigid.is_some_and(|r| r.body_type == BodyType::Dynamic),
        );
        self.entries.insert(entity, entry);
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
        let Some(entry) = self.entries.remove(&entity) else {
            return;
        };
        if let Some(c) = entry.collider {
            self.retired.push(c);
            self.world.remove_collider(c);
        }
        if let Some(b) = entry.body {
            self.world.remove_body(b);
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
            if !cmd.is_finite() {
                self.rejections.push(reject(
                    RejectionKind::NotFinite,
                    "body command value is not finite".into(),
                ));
                continue;
            }
            let Some(entry) = self.entries.get_mut(&entity) else {
                self.rejections.push(reject(
                    RejectionKind::MissingBody,
                    "entity has no RigidBody".into(),
                ));
                continue;
            };
            let (Some(h), Some(rigid)) = (entry.body, entry.rigid) else {
                self.rejections.push(reject(
                    RejectionKind::MissingBody,
                    "entity has no RigidBody".into(),
                ));
                continue;
            };
            let body = &mut self.world.bodies[h];
            let needs_dynamic = |kind: &str| {
                if rigid.body_type != BodyType::Dynamic {
                    Some(reject(
                        RejectionKind::NotDynamic,
                        format!("{kind} needs a dynamic body"),
                    ))
                } else {
                    None
                }
            };
            let err =
                match cmd {
                    BodyCommand::ApplyForce(v) => needs_dynamic("applyForce").or_else(|| {
                        body.add_force(v, true);
                        self.forced.push(h);
                        None
                    }),
                    BodyCommand::ApplyImpulse(v) => needs_dynamic("applyImpulse").or_else(|| {
                        body.apply_impulse(v, true);
                        None
                    }),
                    BodyCommand::ApplyTorque(v) => needs_dynamic("applyTorque").or_else(|| {
                        body.add_torque(v, true);
                        self.forced.push(h);
                        None
                    }),
                    BodyCommand::ApplyTorqueImpulse(v) => needs_dynamic("applyTorqueImpulse")
                        .or_else(|| {
                            body.apply_torque_impulse(v, true);
                            None
                        }),
                    BodyCommand::SetLinearVelocity(v) => needs_dynamic("setLinearVelocity")
                        .or_else(|| {
                            body.set_linvel(v, true);
                            None
                        }),
                    BodyCommand::SetAngularVelocity(v) => needs_dynamic("setAngularVelocity")
                        .or_else(|| {
                            body.set_angvel(v, true);
                            None
                        }),
                    BodyCommand::SetKinematicTarget { position, rotation } => {
                        if rigid.body_type != BodyType::Kinematic {
                            Some(reject(
                                RejectionKind::NotKinematic,
                                "setKinematicTarget needs a kinematic body".into(),
                            ))
                        } else {
                            body.set_next_kinematic_position(pose(position, rotation.normalize()));
                            None
                        }
                    }
                    BodyCommand::Teleport { position, rotation } => {
                        let p = pose(position, rotation.normalize());
                        body.set_position(p, true);
                        if rigid.body_type == BodyType::Kinematic {
                            body.set_next_kinematic_position(p);
                        }
                        // Static bodies are not written back: move the entity now.
                        if rigid.body_type == BodyType::Static {
                            write_pose(ecs, entity, position, rotation.normalize());
                            if let Ok(wt) = ecs.get::<WorldTransform>(entity) {
                                entry.last_world = wt.matrix;
                            }
                        }
                        None
                    }
                };
            if let Some(r) = err {
                self.rejections.push(r);
            }
        }
    }

    fn write_back(&mut self, ecs: &mut Ecs) {
        self.scratch_written.clear();
        for (entity, entry) in &mut self.entries {
            let (Some(h), Some(rigid)) = (entry.body, entry.rigid) else {
                continue;
            };
            if rigid.body_type == BodyType::Static {
                continue;
            }
            let body = &self.world.bodies[h];
            if rigid.body_type == BodyType::Dynamic && body.is_sleeping() {
                continue;
            }
            let p = body.position();
            write_pose(ecs, *entity, p.translation, p.rotation);
            self.scratch_written.push(*entity);
        }
    }

    fn collect_events(&mut self, raw: Vec<CollisionEvent>, watched: &BTreeSet<Entity>) {
        let mut started: HashSet<(Entity, Entity)> = HashSet::new();
        for event in raw {
            let (h1, h2, flags, start) = match event {
                CollisionEvent::Started(a, b, f) => (a, b, f, true),
                CollisionEvent::Stopped(a, b, f) => (a, b, f, false),
            };
            let (Some(&a), Some(&b)) = (self.by_collider.get(&h1), self.by_collider.get(&h2))
            else {
                continue;
            };
            let sensor = flags.contains(CollisionEventFlags::SENSOR);
            let (point, mut normal, impulse) = self
                .world
                .narrow_phase
                .contact_pair(h1, h2)
                .and_then(|p| {
                    let d = contact_data(&self.world.colliders, p)?;
                    // The pair's collider1 may be either side; the normal
                    // reported points a → b.
                    Some(if p.collider1 == h1 {
                        d
                    } else {
                        (d.0, -d.1, d.2)
                    })
                })
                .unwrap_or((Vec3::ZERO, Vec3::ZERO, 0.0));
            if !start {
                normal = Vec3::ZERO;
            }
            let contact = Contact {
                a,
                b,
                point,
                normal,
                impulse: if sensor { 0.0 } else { impulse },
            };
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
        // Stay events for watched, still-touching, non-sensor pairs.
        let mut stays = Vec::new();
        for (&(a, b), &sensor) in &self.touching {
            if sensor || started.contains(&(a, b)) {
                continue;
            }
            if !(watched.contains(&a) || watched.contains(&b)) {
                continue;
            }
            let (Some(ca), Some(cb)) = (
                self.entries.get(&a).and_then(|e| e.collider),
                self.entries.get(&b).and_then(|e| e.collider),
            ) else {
                continue;
            };
            let Some(pair) = self.world.narrow_phase.contact_pair(ca, cb) else {
                continue;
            };
            if !pair.has_any_active_contact() {
                continue;
            }
            let (point, mut normal, impulse) =
                contact_data(&self.world.colliders, pair).unwrap_or((Vec3::ZERO, Vec3::ZERO, 0.0));
            // The pair's collider1 may be either entity; the normal points a → b.
            if pair.collider1 != ca {
                normal = -normal;
            }
            stays.push(PhysicsEvent::CollisionStay(Contact {
                a,
                b,
                point,
                normal,
                impulse,
            }));
        }
        self.events.extend(stays);
    }

    // ------------------------------------------------------------- queries

    /// Answers a spatial query against the last stepped state; hits are
    /// appended to `out` (nearest first for casts).
    pub fn query(&self, q: &QuerySpec, out: &mut Vec<Hit>) {
        let max_hits = q.max_hits.max(1) as usize;
        let mask = q.mask;
        let predicate = move |_: ColliderHandle, co: &rapier3d::geometry::Collider| {
            co.collision_groups().memberships.bits() & mask != 0
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
                .map(|c| Entity::from_bits(c.user_data as u64))
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
                    .filter_map(|(h, _)| {
                        Some(Hit {
                            entity: entity_of(h)?,
                            point: Vec3::ZERO,
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

/// Writes a world pose as the entity's local transform (scale kept),
/// relative to its parent's cached world matrix.
fn write_pose(ecs: &mut Ecs, entity: Entity, position: Vec3, rotation: Quat) {
    let Ok(current) = ecs.copy::<Transform>(entity) else {
        return;
    };
    let parent = ecs.get::<Parent>(entity).ok().map(|p| p.0);
    let local = match parent.and_then(|p| ecs.copy::<WorldTransform>(p).ok()) {
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
    if local != current && local.is_finite() {
        let _ = scene::set_local(ecs, entity, local);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::WorldId;

    fn world() -> Ecs {
        Ecs::new(WorldId(1))
    }

    fn spawn(ecs: &mut Ecs, at: Vec3, rigid: Option<RigidBody>, shape: Option<Collider>) -> Entity {
        let e = scene::spawn(ecs, Transform::from_position(at), None).unwrap();
        if let Some(r) = rigid {
            ecs.insert(e, r).unwrap();
        }
        if let Some(c) = shape {
            ecs.insert(e, c).unwrap();
        }
        e
    }

    fn tick(p: &mut Physics, ecs: &mut Ecs, watched: &BTreeSet<Entity>) {
        scene::propagate(ecs);
        p.step(ecs, 1.0 / 60.0, watched);
        scene::propagate(ecs);
        p.record(ecs);
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
        let (mut t_enter, mut t_exit, mut stays, mut c_enter) = (0, 0, 0, 0);
        for _ in 0..240 {
            tick(&mut p, &mut ecs, &watched);
            for e in p.events() {
                match e {
                    PhysicsEvent::TriggerEnter(c) => {
                        t_enter += 1;
                        assert!((c.a == zone && c.b == ball) || (c.a == ball && c.b == zone));
                        assert_eq!(c.impulse, 0.0);
                    }
                    PhysicsEvent::TriggerExit(_) => t_exit += 1,
                    PhysicsEvent::CollisionStay(c) => {
                        stays += 1;
                        assert!(c.a == floor || c.b == floor);
                    }
                    PhysicsEvent::CollisionEnter(c) => {
                        c_enter += 1;
                        assert!(c.impulse >= 0.0);
                        assert!(c.normal.length() > 0.9);
                    }
                    PhysicsEvent::CollisionExit(_) => {}
                }
            }
        }
        assert_eq!(t_enter, 1, "the trigger fires exactly once");
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
                layer: 1,
                ..Collider::default()
            }),
        );
        let wall = spawn(
            &mut ecs,
            Vec3::new(0.0, 3.0, -5.0),
            None,
            Some(Collider {
                size: Vec3::new(3.0, 3.0, 0.5),
                layer: 2,
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
                layer: 2,
                ..Collider::default()
            }),
        );
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
                layer: 4,
                mask: 1,
                ..Collider::default()
            }),
        );
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
        let big: Vec<u32> = (0..(MAX_MESH_COLLIDER_TRIANGLES as u32 + 1) * 3)
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
}

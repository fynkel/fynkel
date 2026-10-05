//! Fynkel physics (blueprint §9, §19.2d items 8 and 9): the
//! `RigidBody`/`Collider`/`CharacterController` components and the Rapier
//! adapter that steps them inside the fixed tick.
//!
//! The adapter is a system the core calls at the §5.1 physics slot:
//!
//! ```text
//! World::step
//!   apply C[n]                 body commands are queued here (`queue`)
//!   propagate                  world matrices reflect the commands
//!   Physics::step(ecs, dt)     sync components → bodies, apply the queued
//!                              commands, move the characters (Rapier's
//!                              kinematic controller against the last
//!                              step's geometry), Rapier step, write
//!                              dynamic, kinematic and character poses back
//!                              to the entities' local transforms, collect
//!                              the tick's events
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
//! colliders move only when their entity is moved. A character is a
//! kinematic capsule the controller moves by [`BodyCommand::Move`] /
//! [`BodyCommand::Jump`] with gravity of its own; a transform write
//! teleports it (its vertical velocity resets).
//!
//! Determinism: bodies are created in entity order and Rapier is
//! deterministic on one platform, so equal inputs give equal transforms
//! (the headless checksum). Nothing here allocates per tick in steady
//! state beyond Rapier's own working sets.

pub mod components;
pub mod material;

use fynkel_contracts::Entity;

// The adapter (Rapier) is the `rapier` feature, on by default; without it
// (a runtime built with `modules.strip` and physics off, ADR-0029) a
// stand-in of the same API keeps no bodies, answers no queries and costs
// no Rapier. The types above and below are the contract and stay.
#[cfg(feature = "rapier")]
mod sim;
#[cfg(feature = "rapier")]
pub use sim::Physics;
#[cfg(not(feature = "rapier"))]
mod stub;
#[cfg(not(feature = "rapier"))]
pub use stub::Physics;

pub use components::{BodyType, CharacterController, Collider, ColliderShape, RigidBody};
pub use glam::{Quat, Vec3};
pub use material::{CombineRule, PhysicsMaterial};

/// A mesh collider is refused above this many triangles (the explicit
/// limit of the bounded `mesh` shape; §9 "bounded MeshCollider support").
pub const MAX_MESH_COLLIDER_TRIANGLES: usize = 32_768;

/// Validated collision mesh data, prepared without changing a physics world.
#[derive(Clone, Debug)]
pub struct PreparedMesh {
    vertices: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}
impl PreparedMesh {
    pub fn new(positions: &[[f32; 3]], indices: &[u32]) -> Result<Self, String> {
        let triangles = indices.len() / 3;
        if triangles == 0 {
            return Err("mesh has no triangles".into());
        }
        if triangles > MAX_MESH_COLLIDER_TRIANGLES {
            return Err(format!("mesh has {triangles} triangles, more than the {MAX_MESH_COLLIDER_TRIANGLES} a mesh collider allows"));
        }
        if !indices.len().is_multiple_of(3) {
            return Err("mesh has incomplete triangle indices".into());
        }
        if indices.iter().any(|&i| i as usize >= positions.len()) {
            return Err("mesh index out of range".into());
        }
        if positions.iter().flatten().any(|v| !v.is_finite()) {
            return Err("mesh has non-finite vertices".into());
        }
        Ok(Self {
            vertices: positions.iter().copied().map(Vec3::from).collect(),
            triangles: indices
                .chunks_exact(3)
                .map(|t| [t[0], t[1], t[2]])
                .collect(),
        })
    }
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
}

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
    /// A `CharacterController`'s velocity for this tick, metres per second
    /// in world space; the vertical component is ignored unless the
    /// character has no gravity. Without one the character stands still.
    Move(Vec3),
    /// A `CharacterController` takes off at this vertical speed (m/s; `0`
    /// uses its `jump_speed`) — whether it is grounded is gameplay's
    /// check (`grounded` in its state).
    Jump(f32),
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
            BodyCommand::Move(v) => v.is_finite(),
            BodyCommand::Jump(s) => s.is_finite(),
        }
    }

    /// Shared configuration validation for transactional gameplay and execution.
    /// Runtime readiness (for example an unsupported world transform) is checked
    /// separately by the physics adapter after component synchronization.
    pub fn validate_for(
        &self,
        body: Option<BodyType>,
        character: bool,
    ) -> Result<(), (RejectionKind, String)> {
        let invalid = |kind, message: &str| Err((kind, message.to_owned()));
        if !self.is_finite() {
            return invalid(RejectionKind::NotFinite, "body command value is not finite");
        }
        if let Self::SetKinematicTarget { rotation, .. } | Self::Teleport { rotation, .. } = self {
            let norm = rotation.length_squared();
            if !norm.is_finite() || norm <= f32::MIN_POSITIVE {
                return invalid(
                    RejectionKind::NotFinite,
                    "body rotation cannot be normalized",
                );
            }
        }
        if self.is_character() {
            return if character {
                Ok(())
            } else {
                invalid(
                    RejectionKind::MissingCharacter,
                    "entity has no CharacterController",
                )
            };
        }
        let Some(body) = body.filter(|_| !character) else {
            return invalid(RejectionKind::MissingBody, "entity has no usable RigidBody");
        };
        match self {
            Self::Teleport { .. } => Ok(()),
            Self::SetKinematicTarget { .. } if body != BodyType::Kinematic => invalid(
                RejectionKind::NotKinematic,
                "setKinematicTarget needs a kinematic body",
            ),
            Self::SetKinematicTarget { .. } => Ok(()),
            _ if body != BodyType::Dynamic => invalid(
                RejectionKind::NotDynamic,
                "body command needs a dynamic body",
            ),
            _ => Ok(()),
        }
    }

    /// Whether the command addresses a `CharacterController` rather than
    /// a `RigidBody`.
    pub fn is_character(&self) -> bool {
        matches!(self, BodyCommand::Move(_) | BodyCommand::Jump(_))
    }
}

/// The most contact points one event carries: the deepest four of the
/// pair (a face on a face has four corners; a mesh pair keeps its four
/// deepest).
pub const MAX_CONTACT_POINTS: usize = 4;

/// One point of a contact (Unity's `ContactPoint`): `point` in world
/// space, `normal` from `a` towards `b`, `separation` the distance between
/// the shapes there (negative when they overlap), `impulse` the solver's
/// impulse along the normal at this point over the last step (N·s).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactPoint {
    pub point: Vec3,
    pub normal: Vec3,
    pub separation: f32,
    pub impulse: f32,
}

/// One contact as an event carries it (Unity's `Collision`): `a` and `b`
/// the colliders' entities, `body_a`/`body_b` the entity whose `RigidBody`
/// each collider belongs to (its own, or its parent's when it joined it —
/// ADR-0034; `None` for static geometry and characters), `point` and
/// `normal` the deepest point in world space, the normal pointing from `a`
/// towards `b`; `points` the pair's contact points (the deepest first,
/// `point_count` of them, at most [`MAX_CONTACT_POINTS`]); `impulse` the
/// total impulse the solver applied to `a` over the last step (N·s, zero
/// for triggers and exits; `b` got its opposite); `relative_velocity` the
/// velocity of `b` minus that of `a` at the deepest point — at an enter,
/// before the step's solver stopped them (foundation 2.9).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub a: Entity,
    pub b: Entity,
    pub body_a: Option<Entity>,
    pub body_b: Option<Entity>,
    pub point: Vec3,
    pub normal: Vec3,
    pub impulse: Vec3,
    pub relative_velocity: Vec3,
    pub points: [ContactPoint; MAX_CONTACT_POINTS],
    pub point_count: u32,
}

impl Contact {
    /// A contact between two sides with no geometry (triggers, exits).
    pub fn between(a: Entity, b: Entity, body_a: Option<Entity>, body_b: Option<Entity>) -> Self {
        Self {
            a,
            b,
            body_a,
            body_b,
            point: Vec3::ZERO,
            normal: Vec3::ZERO,
            impulse: Vec3::ZERO,
            relative_velocity: Vec3::ZERO,
            points: [ContactPoint::default(); MAX_CONTACT_POINTS],
            point_count: 0,
        }
    }
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
    /// A sensor pair still overlapping (Unity's `OnTriggerStay`, foundation
    /// 2.3): watched pairs only, as [`PhysicsEvent::CollisionStay`]; no
    /// contact geometry.
    TriggerStay(Contact),
    /// A character's move touched a collider (Unity's
    /// `OnControllerColliderHit`, foundation 2.4): `a` the character, `b`
    /// the collider, `body_b` its body, `point` on the collider, `normal`
    /// from the character into it, `impulse` 0. Static and kinematic
    /// geometry included; once per collider per tick, watched characters
    /// only, as the stays.
    ControllerHit(Contact),
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
    /// Physics cannot represent a singular or sheared world basis.
    UnsupportedTransform,
    /// A mesh collider whose mesh is not registered (or too large).
    MeshUnavailable,
    /// A mesh collider on a body that is not static.
    MeshOnMovingBody,
    /// A `move`/`jump` on an entity without a `CharacterController`.
    MissingCharacter,
    /// A `CharacterController` next to a `RigidBody` or `Collider`: the
    /// character's own capsule is used and they are ignored.
    CharacterWithBody,
    /// A collider naming a physics material that is not registered (not
    /// cooked, or not a physics material): the collider's own `friction`
    /// and `restitution` apply until it is (ADR-0025).
    MaterialUnavailable,
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

/// A character's held state as the last step left it
/// (`FYNKEL_QUERY_CHARACTER`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CharacterState {
    /// Standing on walkable ground after the last move.
    pub grounded: bool,
    /// Sliding down a slope steeper than `max_slope`.
    pub sliding: bool,
    /// The movement actually applied over the last tick, metres per
    /// second (gravity included).
    pub velocity: Vec3,
    /// Where the last move touched something (Unity's `collisionFlags`):
    /// [`CharacterState::SIDES`], [`CharacterState::ABOVE`],
    /// [`CharacterState::BELOW`], or 0.
    pub collision_flags: u32,
}

impl CharacterState {
    /// Unity's `CollisionFlags` values.
    pub const SIDES: u32 = 1;
    pub const ABOVE: u32 = 2;
    pub const BELOW: u32 = 4;
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
    /// `CharacterController`s (their capsules count as bodies and
    /// colliders too).
    pub characters: u32,
    pub active_bodies: u32,
    pub contact_pairs: u32,
    /// Events collected by the last step.
    pub events: u32,
}

/// Optional timings of the last physics step. Phases do not overlap; `total`
/// also includes timer/bookkeeping overhead. Excludes the core's preceding
/// scene propagation and subsequent event publication. Not persisted game state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicsStepProfile {
    /// Nested phases contained within `sync`; never add these to the outer phases.
    pub synchronization: PhysicsSyncProfile,
    pub sync: std::time::Duration,
    pub commands: std::time::Duration,
    pub characters: std::time::Duration,
    pub solver: std::time::Duration,
    pub writeback: std::time::Duration,
    pub events: std::time::Duration,
    pub total: std::time::Duration,
}

/// Non-overlapping parts of scene-to-physics synchronization, diagnostic only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicsSyncProfile {
    pub collect: std::time::Duration,
    pub hierarchy: std::time::Duration,
    pub classify: std::time::Duration,
    pub apply: std::time::Duration,
}

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

    /// Whether the command addresses a `CharacterController` rather than
    /// a `RigidBody`.
    pub fn is_character(&self) -> bool {
        matches!(self, BodyCommand::Move(_) | BodyCommand::Jump(_))
    }
}

/// One contact as an event carries it: `a` and `b` the colliders'
/// entities, `body_a`/`body_b` the entity whose `RigidBody` each collider
/// belongs to (its own, or its parent's when it joined it — ADR-0034;
/// `None` for static geometry and characters), `point` and `normal` in
/// world space, the normal pointing from `a` towards `b`, `impulse` the
/// total impulse of the pair over the last step (0 for triggers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub a: Entity,
    pub b: Entity,
    pub body_a: Option<Entity>,
    pub body_b: Option<Entity>,
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

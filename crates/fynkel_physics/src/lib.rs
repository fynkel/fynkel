//! Fynkel's physics world: rigid bodies and colliders on Rapier, stepped
//! by the caller and read out in one buffer per step (docs/stage-5.md
//! §3.1). It knows nothing of the engine: `package:fynkel_physics` moves
//! poses between the two.
//!
//! Ids are `u64`, never 0, and never reused while a world lives: each is a
//! Rapier handle's index and generation. Each body carries a key, the
//! caller's own number for it (the object it moves), which [`Physics::moved`]
//! hands back with the pose.

use std::fmt;

use rapier3d::prelude::*;

pub use rapier3d::math::{Pose, Rotation, Vector};

mod bodies;
mod colliders;
mod contacts;
mod queries;

pub use bodies::{BodySpec, Constraints, ForceMode, Push, Settings};
pub use colliders::{Axis, ColliderShape, ColliderSpec, Combine, Material, ObjectPose};
pub use contacts::{ContactEvent, ContactKind, ContactPoint, LISTEN_COLLISIONS, LISTEN_TRIGGERS};
pub use queries::{Hit, QueryOptions, QueryShape};

/// What moves a body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// Moved by the simulation: gravity, forces, contacts.
    Dynamic,
    /// Moved by the game, through the step; pushes dynamic bodies.
    Kinematic,
    /// Doesn't move.
    Static,
}

/// A collider's shape, in its body's space before the collider's own pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A box, by half its size on each axis.
    Box {
        half_extents: Vector,
    },
    Sphere {
        radius: f32,
    },
    /// A capsule along Y: two half-spheres of `radius` with `half_height`
    /// of cylinder between their centres, on each side of the middle.
    Capsule {
        half_height: f32,
        radius: f32,
    },
}

/// Why a call was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum PhysicsError {
    NoBody(u64),
    NoCollider(u64),
    /// A value was not finite, out of range, or a rotation not of unit length.
    Value(String),
}

impl fmt::Display for PhysicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoBody(id) => write!(f, "no body {id} (removed, or never added)"),
            Self::NoCollider(id) => write!(f, "no collider {id} (removed, or never added)"),
            Self::Value(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PhysicsError {}

/// A body's key, where it is and how it moves, as [`Physics::moved`]
/// gives them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    pub key: u64,
    pub pose: Pose,
    pub linear_velocity: Vector,
    pub angular_velocity: Vector,
}

/// A physics world.
pub struct Physics {
    world: PhysicsWorld,
    // The colliders that follow an object ([`Self::add_static_collider`]),
    // by their handle's index: their spec and the object's last pose.
    followed: Vec<Option<colliders::Followed>>,
    // Colliders added or moved since the last step, whose bounds the
    // queries' tree doesn't have yet ([`Self::sync`]).
    dirty: Vec<ColliderHandle>,
    // What rigidbodies (bodies.rs) have beyond Rapier's, by their handle's
    // index, and the step's bookkeeping for them.
    bodies: Vec<Option<bodies::BodyData>>,
    steps: bodies::StepWork,
    settings: Settings,
    // Who touches whom (contacts.rs), the layer matrix and ignored pairs.
    contacts: contacts::Contacts,
}

impl fmt::Debug for Physics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Physics")
            .field("bodies", &self.world.bodies.len())
            .field("colliders", &self.world.colliders.len())
            .finish()
    }
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}

impl Physics {
    /// An empty world with Unity's gravity, (0, -9.81, 0).
    pub fn new() -> Self {
        let mut world = PhysicsWorld::new();
        let settings = Settings::default();
        world.integration_parameters.num_solver_iterations = settings.solver_iterations as usize;
        Self {
            world,
            followed: Vec::new(),
            dirty: Vec::new(),
            bodies: Vec::new(),
            steps: bodies::StepWork::default(),
            settings,
            contacts: contacts::Contacts::default(),
        }
    }

    pub fn gravity(&self) -> Vector {
        self.world.gravity
    }

    pub fn set_gravity(&mut self, gravity: Vector) -> Result<(), PhysicsError> {
        check_vector("gravity", gravity)?;
        self.world.gravity = gravity;
        Ok(())
    }

    pub fn body_count(&self) -> usize {
        self.world.bodies.len()
    }

    pub fn collider_count(&self) -> usize {
        self.world.colliders.len()
    }

    /// Adds a body at `pose`, with `key` to know it by in [`Self::moved`].
    pub fn add_body(&mut self, kind: BodyKind, key: u64, pose: Pose) -> Result<u64, PhysicsError> {
        check_pose(pose)?;
        let builder = match kind {
            BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
            BodyKind::Kinematic => RigidBodyBuilder::kinematic_position_based(),
            BodyKind::Static => RigidBodyBuilder::fixed(),
        };
        let handle = self.world.bodies.insert(builder.pose(pose).user_data(u128::from(key)));
        Ok(body_id(handle))
    }

    /// Removes a body and the colliders on it.
    pub fn remove_body(&mut self, id: u64) -> Result<(), PhysicsError> {
        let handle = self.body_handle(id)?;
        let w = &mut self.world;
        for collider in w.bodies[handle].colliders() {
            if let Some(slot) = self.followed.get_mut(collider.into_raw_parts().0 as usize) {
                *slot = None;
            }
        }
        if let Some(slot) = self.bodies.get_mut(handle.into_raw_parts().0 as usize) {
            *slot = None;
        }
        w.bodies.remove(
            handle,
            &mut w.islands,
            &mut w.colliders,
            &mut w.impulse_joints,
            &mut w.multibody_joints,
            &mut w.soft_bodies,
            true,
        );
        Ok(())
    }

    /// Adds a collider of `shape` at `pose`: relative to `body` when there
    /// is one, else in the world, as a static collider.
    pub fn add_collider(&mut self, body: Option<u64>, shape: Shape, pose: Pose) -> Result<u64, PhysicsError> {
        check_pose(pose)?;
        let builder = match shape {
            Shape::Box { half_extents: h } => {
                check_positive("a box's half extents", h.min_element())?;
                check_vector("a box's half extents", h)?;
                ColliderBuilder::cuboid(h.x, h.y, h.z)
            }
            Shape::Sphere { radius } => {
                check_positive("a sphere's radius", radius)?;
                ColliderBuilder::ball(radius)
            }
            Shape::Capsule { half_height, radius } => {
                check_positive("a capsule's radius", radius)?;
                if !(half_height >= 0.0 && half_height.is_finite()) {
                    return Err(PhysicsError::Value(format!(
                        "a capsule's half height must be finite and 0 or more, not {half_height}"
                    )));
                }
                ColliderBuilder::capsule_y(half_height, radius)
            }
        };
        let builder = builder.position(pose);
        let w = &mut self.world;
        let handle = match body {
            Some(body) => {
                let parent = body_handle(&w.bodies, body)?;
                w.colliders.insert_with_parent(builder, parent, &mut w.bodies)
            }
            None => w.colliders.insert(builder),
        };
        self.dirty.push(handle);
        Ok(collider_id(handle))
    }

    pub fn remove_collider(&mut self, id: u64) -> Result<(), PhysicsError> {
        let handle = self.collider_handle(id)?;
        let w = &mut self.world;
        let body = w.colliders[handle].parent();
        w.colliders.remove(handle, &mut w.islands, &mut w.bodies, &mut w.soft_bodies, true);
        if let Some(slot) = self.followed.get_mut(handle.into_raw_parts().0 as usize)
            && let Some(followed) = slot.take()
        {
            self.listening_changed(followed.spec.listens, 0);
        }
        if let Some(body) = body {
            self.mass_changed(body);
        }
        Ok(())
    }

    /// Where a body is now.
    pub fn body_pose(&self, id: u64) -> Result<Pose, PhysicsError> {
        Ok(*self.world.bodies[self.body_handle(id)?].position())
    }

    /// Puts a body at `pose` at once (a teleport), and wakes it.
    pub fn set_body_pose(&mut self, id: u64, pose: Pose) -> Result<(), PhysicsError> {
        check_pose(pose)?;
        let handle = self.body_handle(id)?;
        self.world.bodies[handle].set_position(pose, true);
        Ok(())
    }

    pub fn is_sleeping(&self, id: u64) -> Result<bool, PhysicsError> {
        Ok(self.world.bodies[self.body_handle(id)?].is_sleeping())
    }

    /// Moves the world on by `dt` seconds.
    pub fn step(&mut self, dt: f32) -> Result<(), PhysicsError> {
        check_positive("a step's time", dt)?;
        self.before_step(dt);
        self.contacts_before_step();
        self.world.integration_parameters.dt = dt;
        self.world.step_with_events(&self.contacts.filter(), &());
        self.after_step();
        self.contacts_after_step();
        // The step put every collider's bounds in the tree.
        self.dirty.clear();
        Ok(())
    }

    /// The bodies the last step may have moved, their keys and poses, into
    /// `out` (cleared first): the awake dynamic and kinematic bodies.
    /// Sleeping and static bodies aren't in it, nor an awake rigidbody the
    /// step left where it was with no velocity (a kinematic body not moved,
    /// or one with every axis frozen).
    pub fn moved(&self, out: &mut Vec<Moved>) {
        out.clear();
        let w = &self.world;
        out.extend(w.islands.active_bodies().filter_map(|handle| {
            let body = w.bodies.get(handle)?;
            if let Some(Some(data)) = self.bodies.get(handle.into_raw_parts().0 as usize)
                && data.still
            {
                return None;
            }
            // The key went in as a u64, so it comes back whole.
            #[allow(clippy::cast_possible_truncation)]
            let key = body.user_data as u64;
            Some(Moved { key, pose: *body.position(), linear_velocity: body.linvel(), angular_velocity: body.angvel() })
        }));
    }

    fn body_handle(&self, id: u64) -> Result<RigidBodyHandle, PhysicsError> {
        body_handle(&self.world.bodies, id)
    }

    fn collider_handle(&self, id: u64) -> Result<ColliderHandle, PhysicsError> {
        let (index, generation) = split(id).ok_or(PhysicsError::NoCollider(id))?;
        let handle = ColliderHandle::from_raw_parts(index, generation);
        if self.world.colliders.contains(handle) { Ok(handle) } else { Err(PhysicsError::NoCollider(id)) }
    }
}

fn body_handle(bodies: &RigidBodySet, id: u64) -> Result<RigidBodyHandle, PhysicsError> {
    let (index, generation) = split(id).ok_or(PhysicsError::NoBody(id))?;
    let handle = RigidBodyHandle::from_raw_parts(index, generation);
    if bodies.contains(handle) { Ok(handle) } else { Err(PhysicsError::NoBody(id)) }
}

// An id is a handle's index + 1 in the low 32 bits, so none is 0, and its
// generation in the high 32.
fn join(index: u32, generation: u32) -> u64 {
    (u64::from(generation) << 32) | (u64::from(index) + 1)
}

fn split(id: u64) -> Option<(u32, u32)> {
    let low = id & 0xFFFF_FFFF;
    if low == 0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(((low - 1) as u32, (id >> 32) as u32))
}

fn body_id(handle: RigidBodyHandle) -> u64 {
    let (index, generation) = handle.into_raw_parts();
    join(index, generation)
}

fn collider_id(handle: ColliderHandle) -> u64 {
    let (index, generation) = handle.into_raw_parts();
    join(index, generation)
}

fn check_positive(what: &str, value: f32) -> Result<(), PhysicsError> {
    if value > 0.0 && value.is_finite() {
        Ok(())
    } else {
        Err(PhysicsError::Value(format!("{what} must be finite and more than 0, not {value}")))
    }
}

fn check_vector(what: &str, v: Vector) -> Result<(), PhysicsError> {
    if v.is_finite() { Ok(()) } else { Err(PhysicsError::Value(format!("{what}: NaN or infinity in {v}"))) }
}

fn check_pose(pose: Pose) -> Result<(), PhysicsError> {
    check_vector("position", pose.translation)?;
    let r = pose.rotation;
    if !r.is_finite() {
        return Err(PhysicsError::Value(format!("rotation: NaN or infinity in {r}")));
    }
    if (r.length() - 1.0).abs() > 1e-3 {
        return Err(PhysicsError::Value(format!("rotation: {r} is not a unit quaternion")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f32 = 1.0 / 50.0;

    fn at(x: f32, y: f32, z: f32) -> Pose {
        Pose::from_translation(Vector::new(x, y, z))
    }

    fn ground(physics: &mut Physics) {
        physics
            .add_collider(None, Shape::Box { half_extents: Vector::new(50.0, 0.5, 50.0) }, at(0.0, -0.5, 0.0))
            .unwrap();
    }

    #[test]
    fn a_body_falls_as_the_formula_says() {
        let mut physics = Physics::new();
        let ball = physics.add_body(BodyKind::Dynamic, 7, at(0.0, 10.0, 0.0)).unwrap();
        physics.add_collider(Some(ball), Shape::Sphere { radius: 0.5 }, Pose::IDENTITY).unwrap();
        // After 1 s of free fall: 10 - g/2, within what the integrator
        // (semi-implicit Euler, 50 steps) gets wrong: g·t·dt/2 ≈ 0.1 m.
        for _ in 0..50 {
            physics.step(STEP).unwrap();
        }
        let y = physics.body_pose(ball).unwrap().translation.y;
        assert!((y - (10.0 - 9.81 / 2.0)).abs() < 0.11, "y = {y}");
    }

    #[test]
    fn a_dropped_ball_lands_within_a_step_of_the_formula_and_rests_on_the_ground() {
        let mut physics = Physics::new();
        ground(&mut physics);
        let ball = physics.add_body(BodyKind::Dynamic, 1, at(0.0, 10.5, 0.0)).unwrap();
        physics.add_collider(Some(ball), Shape::Sphere { radius: 0.5 }, Pose::IDENTITY).unwrap();
        let expected = (2.0 * 10.0 / 9.81_f32).sqrt();
        let mut landed = None;
        for i in 1..=200 {
            physics.step(STEP).unwrap();
            if landed.is_none() && physics.body_pose(ball).unwrap().translation.y < 0.5 + 0.01 {
                landed = Some(i as f32 * STEP);
            }
        }
        let landed = landed.expect("it landed");
        assert!((landed - expected).abs() <= STEP * 1.5, "landed at {landed} s, expected {expected} s");
        let y = physics.body_pose(ball).unwrap().translation.y;
        assert!((y - 0.5).abs() < 0.02, "rests at y = {y}");
    }

    #[test]
    fn resting_bodies_fall_asleep_and_stop_being_reported() {
        let mut physics = Physics::new();
        ground(&mut physics);
        let cube = physics.add_body(BodyKind::Dynamic, 42, at(0.0, 2.0, 0.0)).unwrap();
        physics.add_collider(Some(cube), Shape::Box { half_extents: Vector::splat(0.5) }, Pose::IDENTITY).unwrap();
        let mut moved = Vec::new();
        physics.step(STEP).unwrap();
        physics.moved(&mut moved);
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].key, 42);
        assert_eq!(moved[0].pose, physics.body_pose(cube).unwrap());
        for _ in 0..500 {
            physics.step(STEP).unwrap();
        }
        assert!(physics.is_sleeping(cube).unwrap());
        physics.moved(&mut moved);
        assert!(moved.is_empty(), "{moved:?}");
        // A teleport wakes it, and it's reported again.
        physics.set_body_pose(cube, at(3.0, 2.0, 0.0)).unwrap();
        physics.step(STEP).unwrap();
        physics.moved(&mut moved);
        assert_eq!(moved.len(), 1);
        assert!((moved[0].pose.translation.x - 3.0).abs() < 1e-4);
    }

    #[test]
    fn static_and_kinematic_bodies_stay_where_they_are_put() {
        let mut physics = Physics::new();
        let wall = physics.add_body(BodyKind::Static, 1, at(1.0, 2.0, 3.0)).unwrap();
        let lift = physics.add_body(BodyKind::Kinematic, 2, at(0.0, 0.0, 0.0)).unwrap();
        for _ in 0..10 {
            physics.step(STEP).unwrap();
        }
        assert_eq!(physics.body_pose(wall).unwrap(), at(1.0, 2.0, 3.0));
        assert_eq!(physics.body_pose(lift).unwrap(), at(0.0, 0.0, 0.0));
    }

    #[test]
    fn a_capsule_stands_on_its_end() {
        let mut physics = Physics::new();
        ground(&mut physics);
        let body = physics.add_body(BodyKind::Dynamic, 1, at(0.0, 3.0, 0.0)).unwrap();
        physics.add_collider(Some(body), Shape::Capsule { half_height: 0.5, radius: 0.5 }, Pose::IDENTITY).unwrap();
        for _ in 0..200 {
            physics.step(STEP).unwrap();
        }
        // Its bottom is at y - half_height - radius.
        let y = physics.body_pose(body).unwrap().translation.y;
        assert!((y - 1.0).abs() < 0.02, "y = {y}");
    }

    #[test]
    fn ids_are_never_0_and_removed_ones_are_refused() {
        let mut physics = Physics::new();
        let a = physics.add_body(BodyKind::Dynamic, 1, Pose::IDENTITY).unwrap();
        assert_ne!(a, 0);
        let c = physics.add_collider(Some(a), Shape::Sphere { radius: 1.0 }, Pose::IDENTITY).unwrap();
        assert_ne!(c, 0);
        physics.remove_body(a).unwrap();
        assert_eq!(physics.collider_count(), 0, "its colliders went with it");
        assert_eq!(physics.body_pose(a), Err(PhysicsError::NoBody(a)));
        assert_eq!(physics.remove_collider(c), Err(PhysicsError::NoCollider(c)));
        assert_eq!(physics.body_pose(0), Err(PhysicsError::NoBody(0)));
        // A new body in the same slot gets a new id.
        let b = physics.add_body(BodyKind::Dynamic, 1, Pose::IDENTITY).unwrap();
        assert_ne!(a, b);
        assert_eq!(physics.body_pose(a), Err(PhysicsError::NoBody(a)));
    }

    #[test]
    fn bad_values_are_refused() {
        let mut physics = Physics::new();
        let bad_rotation = Pose::from_parts(Vector::ZERO, Rotation::from_xyzw(0.0, 0.0, 0.0, 2.0));
        assert!(matches!(physics.add_body(BodyKind::Dynamic, 1, bad_rotation), Err(PhysicsError::Value(_))));
        assert!(matches!(physics.add_body(BodyKind::Dynamic, 1, at(f32::NAN, 0.0, 0.0)), Err(PhysicsError::Value(_))));
        for shape in [
            Shape::Sphere { radius: 0.0 },
            Shape::Sphere { radius: f32::INFINITY },
            Shape::Box { half_extents: Vector::new(1.0, -1.0, 1.0) },
            Shape::Capsule { half_height: -1.0, radius: 1.0 },
        ] {
            assert!(
                matches!(physics.add_collider(None, shape, Pose::IDENTITY), Err(PhysicsError::Value(_))),
                "{shape:?}"
            );
        }
        assert!(matches!(physics.step(0.0), Err(PhysicsError::Value(_))));
        assert!(matches!(physics.set_gravity(Vector::new(0.0, f32::NAN, 0.0)), Err(PhysicsError::Value(_))));
        assert_eq!(physics.body_count(), 0);
        assert_eq!(physics.collider_count(), 0);
    }

    #[test]
    fn gravity_can_change() {
        let mut physics = Physics::new();
        assert_eq!(physics.gravity(), Vector::new(0.0, -9.81, 0.0));
        physics.set_gravity(Vector::ZERO).unwrap();
        let body = physics.add_body(BodyKind::Dynamic, 1, at(0.0, 5.0, 0.0)).unwrap();
        physics.add_collider(Some(body), Shape::Sphere { radius: 0.5 }, Pose::IDENTITY).unwrap();
        for _ in 0..50 {
            physics.step(STEP).unwrap();
        }
        assert_eq!(physics.body_pose(body).unwrap().translation, Vector::new(0.0, 5.0, 0.0));
    }

    #[test]
    fn the_same_input_gives_the_same_output() {
        let run = || {
            let mut physics = Physics::new();
            ground(&mut physics);
            let mut bodies = Vec::new();
            for i in 0..20 {
                let body =
                    physics.add_body(BodyKind::Dynamic, i, at((i % 3) as f32 * 0.3, 1.0 + i as f32, 0.0)).unwrap();
                physics
                    .add_collider(Some(body), Shape::Box { half_extents: Vector::splat(0.5) }, Pose::IDENTITY)
                    .unwrap();
                bodies.push(body);
            }
            for _ in 0..300 {
                physics.step(STEP).unwrap();
            }
            bodies.iter().map(|&b| physics.body_pose(b).unwrap()).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }
}

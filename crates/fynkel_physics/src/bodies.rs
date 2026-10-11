//! Rigidbodies as Unity has them (docs/stage-5.md §3.3): a mass the
//! colliders share out by volume, gravity on or off, damping, forces in
//! Unity's four modes, frozen axes, continuous detection, a top angular
//! speed, and the world's settings. A body's colliders are the followed
//! colliders put on it (colliders.rs).

use rapier3d::prelude::*;

use crate::{Physics, PhysicsError, Pose, Vector, body_handle, body_id, check_pose, check_positive, check_vector};

/// Unity's `RigidbodyConstraints`, with Unity's values: frozen axes, in
/// the world's axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Constraints(pub u32);

impl Constraints {
    pub const NONE: Self = Self(0);
    pub const FREEZE_POSITION_X: Self = Self(2);
    pub const FREEZE_POSITION_Y: Self = Self(4);
    pub const FREEZE_POSITION_Z: Self = Self(8);
    pub const FREEZE_ROTATION_X: Self = Self(16);
    pub const FREEZE_ROTATION_Y: Self = Self(32);
    pub const FREEZE_ROTATION_Z: Self = Self(64);
    pub const FREEZE_ALL: Self = Self(126);

    fn locked(self) -> LockedAxes {
        // Rapier's bits are Unity's, one place lower.
        LockedAxes::from_bits_truncate(((self.0 >> 1) & 0x3F) as u8)
    }
}

/// What a rigidbody is, as the game sets it: Unity's `Rigidbody`
/// properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodySpec {
    /// Moved by the game (`movePosition`), not by forces or contacts.
    pub kinematic: bool,
    pub mass: f32,
    pub use_gravity: bool,
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// Radians a second it never spins faster than.
    pub max_angular_velocity: f32,
    /// Its center of mass in its object's space, or worked out from its
    /// colliders.
    pub center_of_mass: Option<Vector>,
    pub constraints: Constraints,
    /// Continuous collision detection: it doesn't pass through thin things
    /// however fast it goes.
    pub continuous: bool,
}

impl Default for BodySpec {
    /// Unity's defaults: 1 kg, gravity, no linear and 0.05 angular damping,
    /// at most 50 radians a second.
    fn default() -> Self {
        Self {
            kinematic: false,
            mass: 1.0,
            use_gravity: true,
            linear_damping: 0.0,
            angular_damping: 0.05,
            max_angular_velocity: 50.0,
            center_of_mass: None,
            constraints: Constraints::NONE,
            continuous: false,
        }
    }
}

impl BodySpec {
    fn check(&self) -> Result<(), PhysicsError> {
        check_positive("a rigidbody's mass", self.mass)?;
        for (what, value) in [
            ("a rigidbody's linear damping", self.linear_damping),
            ("a rigidbody's angular damping", self.angular_damping),
        ] {
            if !(value >= 0.0 && value.is_finite()) {
                return Err(PhysicsError::Value(format!("{what} must be finite and 0 or more, not {value}")));
            }
        }
        if self.max_angular_velocity.is_nan() || self.max_angular_velocity < 0.0 {
            return Err(PhysicsError::Value(format!(
                "a rigidbody's max angular velocity must be 0 or more, not {}",
                self.max_angular_velocity
            )));
        }
        if let Some(c) = self.center_of_mass {
            check_vector("a rigidbody's center of mass", c)?;
        }
        Ok(())
    }
}

/// How a push changes a body's motion (Unity's `ForceMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceMode {
    /// Newtons (or newton metres) over the next step: mass matters.
    Force,
    /// Metres (or radians) per second squared over the next step: mass
    /// doesn't.
    Acceleration,
    /// Newton seconds, at once: mass matters.
    Impulse,
    /// Metres (or radians) per second, at once: mass doesn't.
    VelocityChange,
}

/// A push on a body: Unity's `AddForce`, `AddTorque`,
/// `AddForceAtPosition` and `AddExplosionForce`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Push {
    Force {
        force: Vector,
        mode: ForceMode,
    },
    Torque {
        torque: Vector,
        mode: ForceMode,
    },
    AtPosition {
        force: Vector,
        position: Vector,
        mode: ForceMode,
    },
    /// Away from `center`, as strong as `force` there and nothing at
    /// `radius` (all of it everywhere with a radius of 0), from a point
    /// `upwards` below the center so it lifts too. It acts on the center of
    /// mass.
    Explosion {
        force: f32,
        center: Vector,
        radius: f32,
        upwards: f32,
        mode: ForceMode,
    },
}

/// The world's settings (Unity's Physics settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// How many times the solver goes over the contacts each step (Unity's
    /// `defaultSolverIterations`, 6).
    pub solver_iterations: u32,
    /// The mass-normalized kinetic energy under which a body falls asleep
    /// (Unity's `sleepThreshold`, 0.005).
    pub sleep_threshold: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { solver_iterations: 6, sleep_threshold: 0.005 }
    }
}

/// A rigidbody's spec, and whether its mass must be worked out again.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BodyData {
    spec: BodySpec,
    mass_dirty: bool,
}

/// The bodies a step has to do something for: those pushed with forces
/// that last a step, whose masses changed, and whose angular velocity
/// changes wait for the step.
#[derive(Debug, Default)]
pub(crate) struct StepWork {
    forced: Vec<RigidBodyHandle>,
    mass_dirty: Vec<RigidBodyHandle>,
    /// Angular velocity changes, and whether each acts over the step.
    spins: Vec<(RigidBodyHandle, Vector, bool)>,
}

impl Physics {
    /// Adds a rigidbody at `pose` (its object's world position and
    /// rotation), with `key` to know it by in [`Self::moved`]. Its colliders
    /// are added with [`Self::add_object_collider`].
    pub fn add_rigidbody(&mut self, key: u64, spec: BodySpec, pose: Pose) -> Result<u64, PhysicsError> {
        spec.check()?;
        check_pose(pose)?;
        let handle = self.world.bodies.insert(RigidBodyBuilder::dynamic().pose(pose).user_data(u128::from(key)));
        let index = handle.into_raw_parts().0 as usize;
        if self.bodies.len() <= index {
            self.bodies.resize(index + 1, None);
        }
        self.bodies[index] = Some(BodyData { spec, mass_dirty: false });
        self.apply_spec(handle, spec);
        self.apply_sleep_threshold(handle);
        self.mass_changed(handle);
        Ok(body_id(handle))
    }

    /// Changes what a rigidbody is.
    pub fn set_body_spec(&mut self, id: u64, spec: BodySpec) -> Result<(), PhysicsError> {
        spec.check()?;
        let handle = self.rigidbody(id)?;
        self.data_mut(handle).spec = spec;
        self.apply_spec(handle, spec);
        self.mass_changed(handle);
        Ok(())
    }

    pub fn body_spec(&self, id: u64) -> Result<BodySpec, PhysicsError> {
        let handle = self.rigidbody(id)?;
        Ok(self.bodies[handle.into_raw_parts().0 as usize].expect("a rigidbody").spec)
    }

    fn apply_spec(&mut self, handle: RigidBodyHandle, spec: BodySpec) {
        let body = &mut self.world.bodies[handle];
        let kind = if spec.kinematic { RigidBodyType::KinematicPositionBased } else { RigidBodyType::Dynamic };
        if body.body_type() != kind {
            body.set_body_type(kind, true);
        }
        body.set_gravity_scale(if spec.use_gravity { 1.0 } else { 0.0 }, true);
        body.set_linear_damping(spec.linear_damping);
        body.set_angular_damping(spec.angular_damping);
        body.set_locked_axes(spec.constraints.locked(), true);
        body.enable_ccd(spec.continuous);
    }

    /// The body's mass is worked out again before the next step or push.
    pub(crate) fn mass_changed(&mut self, handle: RigidBodyHandle) {
        let Some(Some(data)) = self.bodies.get_mut(handle.into_raw_parts().0 as usize) else { return };
        if !data.mass_dirty {
            data.mass_dirty = true;
            self.steps.mass_dirty.push(handle);
        }
    }

    /// Shares each changed body's mass out over its solid colliders by
    /// volume, as Unity does, so a long box turns harder end over end than
    /// about its length. A body with no solid collider spins as a ball.
    fn update_masses(&mut self) {
        for handle in std::mem::take(&mut self.steps.mass_dirty) {
            let Some(Some(data)) = self.bodies.get_mut(handle.into_raw_parts().0 as usize) else { continue };
            data.mass_dirty = false;
            let spec = data.spec;
            let w = &mut self.world;
            let Some(body) = w.bodies.get(handle) else { continue };
            let mut shape = MassProperties::default();
            for &collider in body.colliders() {
                let c = &w.colliders[collider];
                if c.is_sensor() {
                    continue;
                }
                let local = c.position_wrt_parent().copied().unwrap_or(Pose::IDENTITY);
                shape += c.shape().mass_properties(1.0).transform_by(&local);
            }
            let mut props = if shape.mass() > 0.0 {
                let mut p = shape;
                p.set_mass(spec.mass, true);
                p
            } else {
                MassProperties::new(Vector::ZERO, spec.mass, Vector::splat(spec.mass * 0.1))
            };
            if let Some(center) = spec.center_of_mass {
                props.local_com = center;
            }
            let body = &mut w.bodies[handle];
            body.set_additional_mass_properties(props, true);
            body.recompute_mass_properties_from_colliders(&w.colliders);
        }
    }

    /// Pushes a body. Kinematic bodies aren't pushed (as in Unity).
    pub fn push(&mut self, id: u64, push: Push) -> Result<(), PhysicsError> {
        let handle = self.rigidbody(id)?;
        check_push(push)?;
        self.update_masses();
        let body = &mut self.world.bodies[handle];
        if !body.is_dynamic() {
            return Ok(());
        }
        let mass = body.mass();
        let mut lasts = false;
        match push {
            Push::Force { force, mode } => match mode {
                ForceMode::Force => (body.add_force(force, true), lasts = true).1,
                ForceMode::Acceleration => (body.add_force(force * mass, true), lasts = true).1,
                ForceMode::Impulse => body.apply_impulse(force, true),
                ForceMode::VelocityChange => body.apply_impulse(force * mass, true),
            },
            Push::Torque { torque, mode } => match mode {
                ForceMode::Force => (body.add_torque(torque, true), lasts = true).1,
                ForceMode::Impulse => body.apply_torque_impulse(torque, true),
                // Mass doesn't matter: the change goes straight to the
                // angular velocity, over the step or at once.
                ForceMode::Acceleration | ForceMode::VelocityChange => {
                    body.wake_up(true);
                    self.steps.spins.push((handle, torque, mode == ForceMode::Acceleration));
                }
            },
            Push::AtPosition { force, position, mode } => match mode {
                ForceMode::Force => (body.add_force_at_point(force, position, true), lasts = true).1,
                ForceMode::Acceleration => (body.add_force_at_point(force * mass, position, true), lasts = true).1,
                ForceMode::Impulse => body.apply_impulse_at_point(force, position, true),
                ForceMode::VelocityChange => body.apply_impulse_at_point(force * mass, position, true),
            },
            Push::Explosion { force, center, radius, upwards, mode } => {
                let from = center - Vector::Y * upwards;
                let to = body.center_of_mass();
                let away = to - from;
                let distance = away.length();
                if radius > 0.0 && distance > radius {
                    return Ok(());
                }
                let strength = if radius > 0.0 { force * (1.0 - distance / radius) } else { force };
                let direction = if distance > 1e-6 { away / distance } else { Vector::Y };
                return self.push(id, Push::Force { force: direction * strength, mode });
            }
        }
        if lasts {
            self.steps.forced.push(handle);
        }
        Ok(())
    }

    /// Where a rigidbody is: its position and rotation, its velocities, its
    /// center of mass in the world, and whether it sleeps.
    pub fn body_state(&mut self, id: u64) -> Result<(Pose, Vector, Vector, Vector, bool), PhysicsError> {
        let handle = self.rigidbody(id)?;
        self.update_masses();
        let body = &self.world.bodies[handle];
        Ok((*body.position(), body.linvel(), body.angvel(), body.center_of_mass(), body.is_sleeping()))
    }

    pub fn set_velocities(&mut self, id: u64, linear: Vector, angular: Vector) -> Result<(), PhysicsError> {
        check_vector("a linear velocity", linear)?;
        check_vector("an angular velocity", angular)?;
        let handle = self.rigidbody(id)?;
        let body = &mut self.world.bodies[handle];
        body.set_linvel(linear, true);
        body.set_angvel(angular, true);
        Ok(())
    }

    /// Moves a kinematic body to `pose` through the next step, pushing what's
    /// in the way (Unity's `MovePosition` and `MoveRotation`); a dynamic one
    /// is put there at once.
    pub fn move_body(&mut self, id: u64, pose: Pose) -> Result<(), PhysicsError> {
        check_pose(pose)?;
        let handle = self.rigidbody(id)?;
        let body = &mut self.world.bodies[handle];
        if body.is_kinematic() {
            body.set_next_kinematic_position(pose);
        } else {
            body.set_position(pose, true);
        }
        Ok(())
    }

    pub fn set_sleeping(&mut self, id: u64, asleep: bool) -> Result<(), PhysicsError> {
        let handle = self.rigidbody(id)?;
        let body = &mut self.world.bodies[handle];
        if asleep {
            body.sleep()
        } else {
            body.wake_up(true)
        }
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) -> Result<(), PhysicsError> {
        if settings.solver_iterations == 0 {
            return Err(PhysicsError::Value("the solver needs at least 1 iteration".into()));
        }
        if !(settings.sleep_threshold >= 0.0 && settings.sleep_threshold.is_finite()) {
            return Err(PhysicsError::Value(format!(
                "the sleep threshold must be finite and 0 or more, not {}",
                settings.sleep_threshold
            )));
        }
        self.settings = settings;
        self.world.integration_parameters.num_solver_iterations = settings.solver_iterations as usize;
        let handles: Vec<_> = self.world.bodies.iter().map(|(handle, _)| handle).collect();
        for handle in handles {
            self.apply_sleep_threshold(handle);
        }
        Ok(())
    }

    /// Unity's threshold is an energy per kilogram, v²/2: a body sleeps once
    /// it moves and turns slower than √(2e).
    fn apply_sleep_threshold(&mut self, handle: RigidBodyHandle) {
        let speed = (2.0 * self.settings.sleep_threshold).sqrt();
        let activation = self.world.bodies[handle].activation_mut();
        activation.normalized_linear_threshold = speed;
        activation.angular_threshold = speed;
    }

    pub(crate) fn before_step(&mut self, dt: f32) {
        self.update_masses();
        for (handle, change, over_step) in std::mem::take(&mut self.steps.spins) {
            let change = if over_step { change * dt } else { change };
            if let Some(body) = self.world.bodies.get_mut(handle) {
                let locked = body.locked_axes();
                let mut spin = body.angvel() + change;
                for (axis, flag) in [
                    (0, LockedAxes::ROTATION_LOCKED_X),
                    (1, LockedAxes::ROTATION_LOCKED_Y),
                    (2, LockedAxes::ROTATION_LOCKED_Z),
                ] {
                    if locked.contains(flag) {
                        spin[axis] = 0.0;
                    }
                }
                body.set_angvel(spin, true);
            }
        }
    }

    pub(crate) fn after_step(&mut self) {
        for handle in std::mem::take(&mut self.steps.forced) {
            if let Some(body) = self.world.bodies.get_mut(handle) {
                body.reset_forces(false);
                body.reset_torques(false);
            }
        }
        let w = &mut self.world;
        for handle in w.islands.active_bodies() {
            let Some(Some(data)) = self.bodies.get(handle.into_raw_parts().0 as usize) else { continue };
            let top = data.spec.max_angular_velocity;
            let Some(body) = w.bodies.get_mut(handle) else { continue };
            let spin = body.angvel();
            let speed = spin.length();
            if speed > top {
                body.set_angvel(spin * (top / speed), false);
            }
        }
    }

    fn rigidbody(&self, id: u64) -> Result<RigidBodyHandle, PhysicsError> {
        let handle = body_handle(&self.world.bodies, id)?;
        match self.bodies.get(handle.into_raw_parts().0 as usize) {
            Some(Some(_)) => Ok(handle),
            _ => Err(PhysicsError::Value(format!("body {id} isn't a rigidbody"))),
        }
    }

    fn data_mut(&mut self, handle: RigidBodyHandle) -> &mut BodyData {
        self.bodies[handle.into_raw_parts().0 as usize].as_mut().expect("a rigidbody")
    }
}

fn check_push(push: Push) -> Result<(), PhysicsError> {
    match push {
        Push::Force { force, .. } => check_vector("a force", force),
        Push::Torque { torque, .. } => check_vector("a torque", torque),
        Push::AtPosition { force, position, .. } => {
            check_vector("a force", force)?;
            check_vector("a force's position", position)
        }
        Push::Explosion { force, center, radius, upwards, .. } => {
            check_vector("an explosion's center", center)?;
            if !(force.is_finite() && upwards.is_finite() && radius >= 0.0 && radius.is_finite()) {
                return Err(PhysicsError::Value(format!(
                    "an explosion needs a finite force and lift and a radius of 0 or more, not {force}, {upwards}, {radius}"
                )));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColliderShape, ColliderSpec, Material, ObjectPose};

    const STEP: f32 = 1.0 / 50.0;

    fn collider(shape: ColliderShape) -> ColliderSpec {
        ColliderSpec {
            shape,
            center: Vector::ZERO,
            is_trigger: false,
            layer: 0,
            material: Material::DEFAULT,
            listens: 0,
        }
    }

    fn at(position: Vector) -> ObjectPose {
        ObjectPose { position, ..ObjectPose::IDENTITY }
    }

    fn floating() -> BodySpec {
        BodySpec { use_gravity: false, angular_damping: 0.0, ..BodySpec::default() }
    }

    /// A rigidbody with one collider, its object at `position`.
    fn body(physics: &mut Physics, spec: BodySpec, shape: ColliderShape, position: Vector) -> u64 {
        let id = physics.add_rigidbody(1, spec, Pose::from_translation(position)).unwrap();
        physics.add_object_collider(1, collider(shape), at(position), Some(id)).unwrap();
        id
    }

    fn cube() -> ColliderShape {
        ColliderShape::Box { size: Vector::ONE }
    }

    fn ground(physics: &mut Physics) {
        let floor = ColliderShape::Box { size: Vector::new(100.0, 1.0, 100.0) };
        physics.add_static_collider(0, collider(floor), at(Vector::new(0.0, -0.5, 0.0))).unwrap();
    }

    fn velocity(physics: &mut Physics, id: u64) -> Vector {
        physics.body_state(id).unwrap().1
    }

    fn spin(physics: &mut Physics, id: u64) -> Vector {
        physics.body_state(id).unwrap().2
    }

    fn position(physics: &mut Physics, id: u64) -> Vector {
        physics.body_state(id).unwrap().0.translation
    }

    #[test]
    fn a_body_weighs_its_mass_however_many_colliders_it_has() {
        let mut physics = Physics::new();
        let id = body(&mut physics, BodySpec { mass: 3.0, ..floating() }, cube(), Vector::ZERO);
        physics.add_object_collider(1, collider(cube()), at(Vector::X * 2.0), Some(id)).unwrap();
        physics.step(STEP).unwrap();
        let handle = physics.rigidbody(id).unwrap();
        assert!((physics.world.bodies[handle].mass() - 3.0).abs() < 1e-5);
        // Two equal cubes: the center of mass is halfway between them.
        let center = physics.body_state(id).unwrap().3;
        assert!(center.abs_diff_eq(Vector::X, 1e-5), "{center}");
    }

    #[test]
    fn a_long_box_turns_harder_end_over_end_than_about_its_length() {
        let mut physics = Physics::new();
        let long = ColliderShape::Box { size: Vector::new(4.0, 1.0, 1.0) };
        let id = body(&mut physics, floating(), long, Vector::ZERO);
        for torque in [Vector::X, Vector::Y] {
            physics.push(id, Push::Torque { torque, mode: ForceMode::Impulse }).unwrap();
        }
        physics.step(STEP).unwrap();
        let w = spin(&mut physics, id);
        assert!(w.x > 4.0 * w.y, "about its length {} vs end over end {}", w.x, w.y);
    }

    #[test]
    fn force_modes_change_the_velocity_as_unity_says() {
        // 10 N, 10 m/s², 10 N·s and 10 m/s on 2 kg, over one step of 0.02 s.
        for (mode, expected) in [
            (ForceMode::Force, 10.0 / 2.0 * STEP),
            (ForceMode::Acceleration, 10.0 * STEP),
            (ForceMode::Impulse, 10.0 / 2.0),
            (ForceMode::VelocityChange, 10.0),
        ] {
            let mut physics = Physics::new();
            let id = body(&mut physics, BodySpec { mass: 2.0, ..floating() }, cube(), Vector::ZERO);
            physics.push(id, Push::Force { force: Vector::X * 10.0, mode }).unwrap();
            physics.step(STEP).unwrap();
            let v = velocity(&mut physics, id).x;
            assert!((v - expected).abs() < 1e-4, "{mode:?}: {v}, expected {expected}");
            // Forces last one step; the rest are done at once.
            physics.step(STEP).unwrap();
            assert!((velocity(&mut physics, id).x - v).abs() < 1e-5, "{mode:?} acted again");
        }
    }

    #[test]
    fn torque_modes_change_the_spin_as_unity_says() {
        // A 1 m cube of 6 kg: I = m·(1² + 1²)/12 = 1 kg·m².
        for (mode, expected) in [
            (ForceMode::Force, 10.0 * STEP),
            (ForceMode::Acceleration, 10.0 * STEP),
            (ForceMode::Impulse, 10.0),
            (ForceMode::VelocityChange, 10.0),
        ] {
            let mut physics = Physics::new();
            let id = body(&mut physics, BodySpec { mass: 6.0, ..floating() }, cube(), Vector::ZERO);
            physics.push(id, Push::Torque { torque: Vector::Y * 10.0, mode }).unwrap();
            physics.step(STEP).unwrap();
            let w = spin(&mut physics, id).y;
            assert!((w - expected).abs() < 1e-3, "{mode:?}: {w}, expected {expected}");
        }
        // Mass doesn't matter to the last two.
        let mut physics = Physics::new();
        let id = body(&mut physics, BodySpec { mass: 60.0, ..floating() }, cube(), Vector::ZERO);
        physics.push(id, Push::Torque { torque: Vector::Y * 10.0, mode: ForceMode::Acceleration }).unwrap();
        physics.step(STEP).unwrap();
        assert!((spin(&mut physics, id).y - 10.0 * STEP).abs() < 1e-3);
    }

    #[test]
    fn a_push_off_center_turns_the_body_and_an_explosion_pushes_away_from_it() {
        let mut physics = Physics::new();
        let id = body(&mut physics, floating(), cube(), Vector::ZERO);
        let push = Push::AtPosition { force: Vector::Z, position: Vector::X * 0.5, mode: ForceMode::Impulse };
        physics.push(id, push).unwrap();
        physics.step(STEP).unwrap();
        assert!(velocity(&mut physics, id).z > 0.0 && spin(&mut physics, id).y < 0.0, "{}", spin(&mut physics, id));

        let mut physics = Physics::new();
        let id = body(&mut physics, floating(), cube(), Vector::X * 2.0);
        let far = body(&mut physics, floating(), cube(), Vector::X * 20.0);
        for target in [id, far] {
            let blast = Push::Explosion {
                force: 10.0,
                center: Vector::ZERO,
                radius: 4.0,
                upwards: 0.0,
                mode: ForceMode::VelocityChange,
            };
            physics.push(target, blast).unwrap();
        }
        physics.step(STEP).unwrap();
        // Halfway out, half the force.
        assert!(velocity(&mut physics, id).abs_diff_eq(Vector::X * 5.0, 1e-3), "{}", velocity(&mut physics, id));
        assert_eq!(velocity(&mut physics, far), Vector::ZERO);
    }

    #[test]
    fn kinematic_bodies_are_not_pushed_and_move_where_they_are_sent_pushing_others() {
        let mut physics = Physics::new();
        let mover = body(&mut physics, BodySpec { kinematic: true, ..floating() }, cube(), Vector::ZERO);
        let other = body(&mut physics, floating(), cube(), Vector::X * 1.5);
        physics.push(mover, Push::Force { force: Vector::X, mode: ForceMode::VelocityChange }).unwrap();
        physics.step(STEP).unwrap();
        assert_eq!(position(&mut physics, mover), Vector::ZERO);
        for i in 1..=25 {
            physics.move_body(mover, Pose::from_translation(Vector::X * (i as f32 * 0.04))).unwrap();
            physics.step(STEP).unwrap();
        }
        assert!(position(&mut physics, mover).abs_diff_eq(Vector::X, 1e-5));
        assert!(position(&mut physics, other).x > 1.9, "pushed to {}", position(&mut physics, other));
    }

    #[test]
    fn frozen_axes_do_not_move() {
        let mut physics = Physics::new();
        let frozen = Constraints(Constraints::FREEZE_POSITION_X.0 | Constraints::FREEZE_ROTATION_Y.0);
        let id = body(&mut physics, BodySpec { constraints: frozen, ..floating() }, cube(), Vector::ZERO);
        physics.push(id, Push::Force { force: Vector::new(5.0, 5.0, 0.0), mode: ForceMode::VelocityChange }).unwrap();
        for mode in [ForceMode::Impulse, ForceMode::VelocityChange] {
            physics.push(id, Push::Torque { torque: Vector::new(1.0, 1.0, 0.0), mode }).unwrap();
        }
        for _ in 0..10 {
            physics.step(STEP).unwrap();
        }
        let (pose, _, w, _, _) = physics.body_state(id).unwrap();
        assert_eq!(pose.translation.x, 0.0);
        assert!(pose.translation.y > 0.5);
        assert_eq!(w.y, 0.0);
        assert!(w.x > 0.0);
    }

    #[test]
    fn a_fast_small_ball_passes_a_thin_moving_wall_unless_it_is_continuous_and_never_a_static_one() {
        // §3.8: a 0.1 m ball at 300 m/s, a 0.1 m wall. Rapier sweeps any
        // fast body against static colliders; continuous detection sweeps
        // it against the others too.
        for (wall_body, continuous, passes) in [(false, false, false), (true, false, true), (true, true, false)] {
            let mut physics = Physics::new();
            let wall = ColliderShape::Box { size: Vector::new(0.1, 10.0, 10.0) };
            let at_wall = Vector::X * 5.0;
            let on = wall_body.then(|| {
                let spec = BodySpec { kinematic: true, ..floating() };
                physics.add_rigidbody(0, spec, Pose::from_translation(at_wall)).unwrap()
            });
            physics.add_object_collider(0, collider(wall), at(at_wall), on).unwrap();
            let spec = BodySpec { continuous, ..floating() };
            let ball = body(&mut physics, spec, ColliderShape::Sphere { radius: 0.05 }, Vector::ZERO);
            physics.set_velocities(ball, Vector::X * 300.0, Vector::ZERO).unwrap();
            for _ in 0..10 {
                physics.step(STEP).unwrap();
            }
            let x = position(&mut physics, ball).x;
            assert_eq!(x > 5.0, passes, "wall on a body {wall_body}, continuous {continuous}: x = {x}");
        }
    }

    #[test]
    fn a_stack_of_ten_boxes_stands_for_ten_seconds() {
        let mut physics = Physics::new();
        ground(&mut physics);
        let boxes: Vec<_> =
            (0..10).map(|i| body(&mut physics, BodySpec::default(), cube(), Vector::Y * (0.5 + i as f32))).collect();
        for _ in 0..500 {
            physics.step(STEP).unwrap();
        }
        for (i, &id) in boxes.iter().enumerate() {
            let p = position(&mut physics, id);
            let drift = (p - Vector::Y * (0.5 + i as f32)).length();
            assert!(drift < 0.05, "box {i} moved {drift} to {p}");
        }
    }

    #[test]
    fn spin_is_capped_and_gravity_and_damping_can_change() {
        let mut physics = Physics::new();
        let spec = BodySpec { max_angular_velocity: 7.0, ..floating() };
        let id = body(&mut physics, spec, cube(), Vector::ZERO);
        physics.set_velocities(id, Vector::ZERO, Vector::Y * 100.0).unwrap();
        physics.step(STEP).unwrap();
        assert!((spin(&mut physics, id).length() - 7.0).abs() < 1e-4);

        physics.set_body_spec(id, BodySpec { use_gravity: true, ..spec }).unwrap();
        physics.step(STEP).unwrap();
        assert!(velocity(&mut physics, id).y < 0.0);

        let damped = BodySpec { linear_damping: 10.0, use_gravity: false, ..spec };
        physics.set_body_spec(id, damped).unwrap();
        let before = velocity(&mut physics, id).y;
        physics.step(STEP).unwrap();
        assert!(velocity(&mut physics, id).y.abs() < before.abs());
    }

    #[test]
    fn a_resting_body_falls_asleep_by_the_threshold_and_wakes_when_told() {
        let mut physics = Physics::new();
        ground(&mut physics);
        let id = body(&mut physics, BodySpec::default(), cube(), Vector::Y * 0.5);
        for _ in 0..200 {
            physics.step(STEP).unwrap();
        }
        assert!(physics.body_state(id).unwrap().4, "asleep");
        physics.set_sleeping(id, false).unwrap();
        assert!(!physics.body_state(id).unwrap().4);

        // With no threshold it never sleeps... unless told to.
        physics.set_settings(Settings { sleep_threshold: 0.0, ..physics.settings() }).unwrap();
        for _ in 0..200 {
            physics.step(STEP).unwrap();
        }
        assert!(!physics.body_state(id).unwrap().4, "awake");
        physics.set_sleeping(id, true).unwrap();
        assert!(physics.body_state(id).unwrap().4);
    }

    #[test]
    fn a_set_center_of_mass_is_kept_and_a_body_without_colliders_still_falls() {
        let mut physics = Physics::new();
        let low = Vector::new(0.0, -0.4, 0.0);
        let id = body(&mut physics, BodySpec { center_of_mass: Some(low), ..floating() }, cube(), Vector::ZERO);
        physics.step(STEP).unwrap();
        assert!(physics.body_state(id).unwrap().3.abs_diff_eq(low, 1e-5));

        let bare = physics.add_rigidbody(2, BodySpec::default(), Pose::IDENTITY).unwrap();
        physics.step(STEP).unwrap();
        assert!(velocity(&mut physics, bare).y < 0.0);
    }

    #[test]
    fn removing_a_collider_shares_the_mass_out_again_and_removed_bodies_are_refused() {
        let mut physics = Physics::new();
        let id = body(&mut physics, floating(), cube(), Vector::ZERO);
        let second = physics.add_object_collider(1, collider(cube()), at(Vector::X * 2.0), Some(id)).unwrap();
        physics.step(STEP).unwrap();
        physics.remove_collider(second).unwrap();
        physics.step(STEP).unwrap();
        assert!(physics.body_state(id).unwrap().3.abs_diff_eq(Vector::ZERO, 1e-5));

        physics.remove_body(id).unwrap();
        assert_eq!(
            physics.push(id, Push::Force { force: Vector::X, mode: ForceMode::Force }),
            Err(PhysicsError::NoBody(id))
        );
        // A plain body isn't a rigidbody.
        let plain = physics.add_body(crate::BodyKind::Dynamic, 3, Pose::IDENTITY).unwrap();
        assert!(matches!(physics.body_spec(plain), Err(PhysicsError::Value(_))));
    }

    #[test]
    fn bad_values_are_refused() {
        let mut physics = Physics::new();
        for spec in [
            BodySpec { mass: 0.0, ..BodySpec::default() },
            BodySpec { mass: f32::NAN, ..BodySpec::default() },
            BodySpec { linear_damping: -1.0, ..BodySpec::default() },
            BodySpec { angular_damping: f32::INFINITY, ..BodySpec::default() },
            BodySpec { max_angular_velocity: -1.0, ..BodySpec::default() },
            BodySpec { center_of_mass: Some(Vector::NAN), ..BodySpec::default() },
        ] {
            assert!(physics.add_rigidbody(1, spec, Pose::IDENTITY).is_err(), "{spec:?}");
        }
        let id = physics.add_rigidbody(1, BodySpec::default(), Pose::IDENTITY).unwrap();
        for push in [
            Push::Force { force: Vector::NAN, mode: ForceMode::Force },
            Push::Explosion { force: 1.0, center: Vector::ZERO, radius: -1.0, upwards: 0.0, mode: ForceMode::Force },
        ] {
            assert!(physics.push(id, push).is_err(), "{push:?}");
        }
        assert!(physics.set_settings(Settings { solver_iterations: 0, ..Settings::default() }).is_err());
        assert!(physics.set_settings(Settings { sleep_threshold: -1.0, ..Settings::default() }).is_err());
    }
}

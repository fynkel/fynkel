//! Joints, as Unity's (docs/stage-5.md §3.7): a fixed joint, a hinge with
//! limits, a motor and a spring, and a spring between two anchors, each
//! between a rigidbody and another or the world, and each breaking when
//! it holds harder than its break force or torque. On Rapier's impulse
//! joints.
//!
//! A joint holds its bodies as they are when it's made: its connected
//! anchor is where its anchor is then (unless given), and a hinge's angle
//! is 0 then. In Rapier's terms the connected body is body 1 and the
//! joint's own body 2, so the hinge's angle is its own body's turn about
//! the axis relative to the connected one.

use rapier3d::prelude::*;

use crate::{Physics, PhysicsError, Pose, Rotation, Vector, check_vector, join, split};

/// What a joint lets its body do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointKind {
    /// Nothing: the two move as one.
    Fixed,
    /// Turn about the axis through the anchor.
    Hinge,
    /// Anything, pulled back while its anchors are further apart than its
    /// max distance or nearer than its min.
    Spring,
}

/// A hinge's motor: it turns the hinge at `velocity` (radians a second),
/// with no more than `force` (newton metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointMotor {
    pub velocity: f32,
    pub force: f32,
}

/// A spring: `spring` newtons a metre (newton metres a radian on a hinge)
/// towards its target, slowed by `damper` per metre (radian) a second.
/// A hinge's spring turns it to `target` radians; a spring joint's has
/// none (its distances say where it rests).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointSpring {
    pub spring: f32,
    pub damper: f32,
    pub target: f32,
}

/// What a joint is: Unity's joint properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointSpec {
    pub kind: JointKind,
    /// Its own rigidbody.
    pub body: u64,
    /// The rigidbody it's joined to, or 0 for the world.
    pub connected: u64,
    /// Where it holds, in its own body's space.
    pub anchor: Vector,
    /// Where it holds on the connected body, in that body's space (in the
    /// world's for the world), or where the anchor is when it's made.
    pub connected_anchor: Option<Vector>,
    /// A hinge's axis, in its own body's space.
    pub axis: Vector,
    /// Whether the two bodies collide with each other.
    pub enable_collision: bool,
    /// The force (newtons) and torque (newton metres) it breaks at:
    /// infinity for never.
    pub break_force: f32,
    pub break_torque: f32,
    /// A hinge's limits, in radians from where it was made: -π to π.
    pub limits: Option<[f32; 2]>,
    /// A hinge's motor. It turns the hinge in place of its spring.
    pub motor: Option<JointMotor>,
    /// A hinge's spring, or a spring joint's.
    pub spring: Option<JointSpring>,
    /// A spring joint's distances between its anchors, in metres, between
    /// which it doesn't pull.
    pub min_distance: f32,
    pub max_distance: f32,
}

impl JointSpec {
    /// Unity's defaults for a joint of `kind` on `body`: joined to the
    /// world at its middle, about its X axis, unbreakable; a spring joint's
    /// spring 10 and damper 0.2.
    pub fn new(kind: JointKind, body: u64) -> Self {
        Self {
            kind,
            body,
            connected: 0,
            anchor: Vector::ZERO,
            connected_anchor: None,
            axis: Vector::X,
            enable_collision: false,
            break_force: f32::INFINITY,
            break_torque: f32::INFINITY,
            limits: None,
            motor: None,
            spring: (kind == JointKind::Spring).then_some(JointSpring { spring: 10.0, damper: 0.2, target: 0.0 }),
            min_distance: 0.0,
            max_distance: 0.0,
        }
    }

    fn check(&self) -> Result<(), PhysicsError> {
        check_vector("a joint's anchor", self.anchor)?;
        if let Some(a) = self.connected_anchor {
            check_vector("a joint's connected anchor", a)?;
        }
        check_vector("a joint's axis", self.axis)?;
        if self.kind == JointKind::Hinge && self.axis.length() < 1e-6 {
            return Err(PhysicsError::Value("a hinge's axis can't be (0, 0, 0)".into()));
        }
        for (what, value) in [("break force", self.break_force), ("break torque", self.break_torque)] {
            if value.is_nan() || value <= 0.0 {
                return Err(PhysicsError::Value(format!("a joint's {what} must be more than 0, not {value}")));
            }
        }
        if let Some([min, max]) = self.limits {
            let pi = std::f32::consts::PI + 1e-4;
            if !(min >= -pi && max <= pi && min <= max) {
                return Err(PhysicsError::Value(format!(
                    "a hinge's limits must be -180° to 180°, the min no more than the max, not {} to {}",
                    min.to_degrees(),
                    max.to_degrees()
                )));
            }
        }
        if let Some(m) = self.motor
            && !(m.velocity.is_finite() && m.force >= 0.0 && m.force.is_finite())
        {
            return Err(PhysicsError::Value(format!(
                "a motor needs a finite velocity and a finite force of 0 or more, not {} and {}",
                m.velocity, m.force
            )));
        }
        if let Some(s) = self.spring
            && !(s.spring >= 0.0
                && s.spring.is_finite()
                && s.damper >= 0.0
                && s.damper.is_finite()
                && s.target.is_finite())
        {
            return Err(PhysicsError::Value(format!(
                "a spring needs a finite spring and damper of 0 or more, and a finite target, not {:?}",
                self.spring
            )));
        }
        if !(self.min_distance >= 0.0 && self.max_distance >= self.min_distance && self.max_distance.is_finite()) {
            return Err(PhysicsError::Value(format!(
                "a spring's distances must be finite, 0 or more, the min no more than the max, not {} to {}",
                self.min_distance, self.max_distance
            )));
        }
        Ok(())
    }

    /// Whether `other` differs only in what can change on a joint as it
    /// is: not its bodies, anchors or axis.
    fn same_frame(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.body == other.body
            && self.connected == other.connected
            && near(self.anchor, other.anchor)
            && match (self.connected_anchor, other.connected_anchor) {
                (_, None) => true,
                (Some(a), Some(b)) => near(a, b),
                (None, Some(_)) => false,
            }
            && near(self.axis, other.axis)
    }
}

/// Whether two of a joint's points or axes are the same, but for the float
/// noise of a scale read back from a turned object's matrix.
fn near(a: Vector, b: Vector) -> bool {
    (a - b).length() <= 1e-4 * (1.0 + a.length())
}

/// How hard a joint holds, and where a hinge is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointState {
    /// Where it holds on the connected body: as given, or where the anchor
    /// was when it was made.
    pub connected_anchor: Vector,
    /// The force and torque it held its body with in the last step, in the
    /// world.
    pub force: Vector,
    pub torque: Vector,
    /// A hinge's angle (radians, -π to π) and how fast it turns (radians a
    /// second); 0 for the others.
    pub angle: f32,
    pub velocity: f32,
}

/// A joint the last step broke: the key it was added with, and the force
/// and torque it broke at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrokenJoint {
    pub key: u64,
    pub force: f32,
    pub torque: f32,
}

#[derive(Clone, Copy, Debug)]
struct JointData {
    key: u64,
    spec: JointSpec,
}

/// The joints the world has, beyond what Rapier keeps.
#[derive(Debug, Default)]
pub(crate) struct Joints {
    /// By their handle's index.
    data: Vec<Option<JointData>>,
    /// The fixed body joints to the world are joined to.
    ground: Option<RigidBodyHandle>,
    /// How many are driven, by a spring or motor (looked at before each
    /// step), and how many can break (checked after each step).
    driven: usize,
    breakable: usize,
    /// The bodies a drive keeps awake this step.
    wake: Vec<RigidBodyHandle>,
    /// What the last step broke.
    broken: Vec<BrokenJoint>,
    /// The last step's substeps' time: the joints' impulses are the last
    /// substep's.
    substep: f32,
}

impl Physics {
    /// Joins `spec.body` to `spec.connected` (or the world) as they are
    /// now, with `key` to know the joint by when it breaks.
    pub fn add_joint(&mut self, key: u64, spec: JointSpec) -> Result<u64, PhysicsError> {
        spec.check()?;
        let own = self.rigidbody(spec.body)?;
        let connected = match spec.connected {
            0 => self.ground(),
            id if id == spec.body => {
                return Err(PhysicsError::Value("a joint can't join a rigidbody to itself".into()));
            }
            id => self.rigidbody(id)?,
        };
        let own_pose = *self.world.bodies[own].position();
        let connected_pose = *self.world.bodies[connected].position();
        let connected_anchor =
            spec.connected_anchor.unwrap_or_else(|| connected_pose.inverse() * (own_pose * spec.anchor));
        let spec = JointSpec { connected_anchor: Some(connected_anchor), ..spec };
        // Its own frame: X along the hinge's axis. The connected one: turned
        // as the own one is now, so the joint holds them as they are.
        let turn = match spec.kind {
            JointKind::Hinge => Rotation::from_rotation_arc(Vector::X, spec.axis.normalize()),
            _ => Rotation::IDENTITY,
        };
        let frame2 = Pose::from_parts(spec.anchor, turn);
        let frame1 = Pose::from_parts(
            connected_anchor,
            (connected_pose.rotation.inverse() * own_pose.rotation * turn).normalize(),
        );
        let locked = match spec.kind {
            JointKind::Fixed => JointAxesMask::LOCKED_FIXED_AXES,
            JointKind::Hinge => JointAxesMask::LOCKED_REVOLUTE_AXES,
            JointKind::Spring => JointAxesMask::empty(),
        };
        let mut joint = GenericJointBuilder::new(locked)
            .local_frame1(frame1)
            .local_frame2(frame2)
            .user_data(u128::from(key))
            .build();
        if spec.kind == JointKind::Spring {
            joint.coupled_axes = JointAxesMask::LIN_AXES;
        }
        apply(&mut joint, &spec);
        let handle = self.world.impulse_joints.insert(connected, own, joint, true);
        let index = handle.into_raw_parts().0 as usize;
        if self.joints.data.len() <= index {
            self.joints.data.resize(index + 1, None);
        }
        self.joints.data[index] = Some(JointData { key, spec });
        self.count(&spec, 1);
        Ok(joint_id(handle))
    }

    /// Changes a joint's limits, motor, spring, break force and torque, and
    /// whether its bodies collide. Its bodies, anchors and axis stay: to
    /// change those, remove it and add it again.
    pub fn set_joint_spec(&mut self, id: u64, spec: JointSpec) -> Result<(), PhysicsError> {
        spec.check()?;
        let handle = self.joint_handle(id)?;
        let data = self.joint_data(handle);
        if !data.spec.same_frame(&spec) {
            return Err(PhysicsError::Value(
                "a joint's bodies, anchors and axis can't change: remove it and add it again".into(),
            ));
        }
        let spec = JointSpec {
            anchor: data.spec.anchor,
            connected_anchor: data.spec.connected_anchor,
            axis: data.spec.axis,
            ..spec
        };
        let before = data.spec;
        self.count(&before, -1);
        self.count(&spec, 1);
        self.joints.data[handle.into_raw_parts().0 as usize] = Some(JointData { key: data.key, spec });
        let joint = self.world.impulse_joints.get_mut(handle, true).expect("a joint");
        apply(&mut joint.data, &spec);
        Ok(())
    }

    pub fn joint_spec(&self, id: u64) -> Result<JointSpec, PhysicsError> {
        Ok(self.joint_data(self.joint_handle(id)?).spec)
    }

    pub fn remove_joint(&mut self, id: u64) -> Result<(), PhysicsError> {
        let handle = self.joint_handle(id)?;
        self.forget_joint(handle);
        self.world.impulse_joints.remove(handle, true);
        Ok(())
    }

    pub fn joint_count(&self) -> usize {
        self.world.impulse_joints.len()
    }

    /// How hard a joint held in the last step, and where a hinge is now.
    pub fn joint_state(&self, id: u64) -> Result<JointState, PhysicsError> {
        let handle = self.joint_handle(id)?;
        let data = self.joint_data(handle);
        let joint = &self.world.impulse_joints.get(handle).expect("a joint");
        let bodies = &self.world.bodies;
        let (connected, own) = (&bodies[joint.body1()], &bodies[joint.body2()]);
        let frame1 = connected.rotation() * joint.data.local_frame1.rotation;
        let frame2 = own.rotation() * joint.data.local_frame2.rotation;
        let (mut angle, mut velocity) = (0.0, 0.0);
        if data.spec.kind == JointKind::Hinge {
            angle = hinge_angle(frame1, frame2);
            velocity = (own.angvel() - connected.angvel()).dot(frame2 * Vector::X);
        }
        let (force, torque) = self.held(joint, &data.spec);
        Ok(JointState {
            connected_anchor: data.spec.connected_anchor.unwrap_or(Vector::ZERO),
            force,
            torque,
            angle,
            velocity,
        })
    }

    /// The force and torque (about its anchor) `joint` held its own body
    /// with in the last step, in the world. Rapier's impulses are the last
    /// substep's, on the connected body, along its rows once they're made
    /// independent: the locked turns' rows first, pure torques, then the
    /// locked moves', which then push at the body's center of mass.
    fn held(&self, joint: &ImpulseJoint, spec: &JointSpec) -> (Vector, Vector) {
        let per = if self.joints.substep > 0.0 { -1.0 / self.joints.substep } else { 0.0 };
        let bodies = &self.world.bodies;
        let (connected, own) = (&bodies[joint.body1()], &bodies[joint.body2()]);
        let anchor = own.position() * joint.data.local_frame2.translation;
        if spec.kind == JointKind::Spring {
            // Its pull, along the line between its anchors.
            let to = connected.position() * joint.data.local_frame1.translation - anchor;
            let motor = &joint.data.motors[JointAxis::LinX as usize];
            let pull = motor.impulse.abs() * -per;
            let towards = if to.length() > motor.target_pos { 1.0 } else { -1.0 };
            return (to.normalize_or_zero() * pull * towards, Vector::ZERO);
        }
        let frame1 = connected.rotation() * joint.data.local_frame1.rotation;
        let i = joint.impulses;
        let force = frame1 * Vector::new(i[0], i[1], i[2]) * per;
        let mut torque = frame1 * Vector::new(i[3], i[4], i[5]) * per + (own.center_of_mass() - anchor).cross(force);
        if spec.kind == JointKind::Hinge {
            // Not about its axis, where it turns freely.
            let axis = own.rotation() * joint.data.local_frame2.rotation * Vector::X;
            torque -= axis * torque.dot(axis);
        }
        (force, torque)
    }

    /// The joints the last step broke, and removed.
    pub fn broken_joints(&self) -> &[BrokenJoint] {
        &self.joints.broken
    }

    /// Before a step: each spring joint pulls towards the nearest distance
    /// between its min and max.
    pub(crate) fn joints_before_step(&mut self, dt: f32) {
        self.joints.broken.clear();
        self.joints.substep = dt / self.world.integration_parameters.num_solver_iterations as f32;
        if self.joints.driven == 0 {
            return;
        }
        // Rapier lets a body sleep while its joint's drive still pulls, as
        // when a spring nears its target slowly, or a motor turns slowly:
        // such a body is kept awake, as Unity's is.
        let mut wake = std::mem::take(&mut self.joints.wake);
        let bodies = &self.world.bodies;
        for (handle, joint) in self.world.impulse_joints.iter_mut() {
            let Some(Some(data)) = self.joints.data.get(handle.into_raw_parts().0 as usize) else { continue };
            let (connected, own) = (&bodies[joint.body1()], &bodies[joint.body2()]);
            let pulls = match data.spec.kind {
                JointKind::Spring => {
                    let a = connected.position() * joint.data.local_frame1.translation;
                    let b = own.position() * joint.data.local_frame2.translation;
                    // Within its distances it doesn't pull, nor slow down.
                    let d = a.distance(b);
                    let (min, max) = (data.spec.min_distance, data.spec.max_distance);
                    let s = data.spec.spring.unwrap_or(SLACK);
                    let (target, s) = if d < min || d > max { (d.clamp(min, max), s) } else { (d, SLACK) };
                    let motor = &mut joint.data.motors[JointAxis::LinX as usize];
                    motor.target_pos = target;
                    motor.stiffness = s.spring;
                    motor.damping = s.damper;
                    motor.max_force = slack_force(s);
                    (d - target).abs() > AWAKE_DISTANCE
                }
                JointKind::Hinge => {
                    let angle = || {
                        let frame1 = connected.rotation() * joint.data.local_frame1.rotation;
                        hinge_angle(frame1, own.rotation() * joint.data.local_frame2.rotation)
                    };
                    match (data.spec.motor, data.spec.spring) {
                        // A motor turns until a limit stops it.
                        (Some(m), _) if m.velocity != 0.0 && m.force > 0.0 => match data.spec.limits {
                            Some([min, max]) => {
                                let angle = angle();
                                if m.velocity > 0.0 { angle < max - AWAKE_ANGLE } else { angle > min + AWAKE_ANGLE }
                            }
                            None => true,
                        },
                        (None, Some(s)) if s.spring > 0.0 => {
                            let target = match data.spec.limits {
                                Some([min, max]) => s.target.clamp(min, max),
                                None => s.target,
                            };
                            (angle() - target).abs() > AWAKE_ANGLE
                        }
                        _ => false,
                    }
                }
                JointKind::Fixed => false,
            };
            if pulls {
                wake.extend([joint.body1(), joint.body2()]);
            }
        }
        for handle in wake.drain(..) {
            if let Some(body) = self.world.bodies.get_mut(handle)
                && body.is_dynamic()
            {
                body.wake_up(true);
            }
        }
        self.joints.wake = wake;
    }

    /// After a step: the joints that held harder than they can break, and
    /// are removed.
    pub(crate) fn joints_after_step(&mut self) {
        if self.joints.breakable == 0 {
            return;
        }
        let mut broken = Vec::new();
        for (handle, joint) in self.world.impulse_joints.iter() {
            let Some(Some(data)) = self.joints.data.get(handle.into_raw_parts().0 as usize) else { continue };
            let (force, torque) = self.held(joint, &data.spec);
            let (force, torque) = (force.length(), torque.length());
            if force > data.spec.break_force || torque > data.spec.break_torque {
                broken.push((handle, BrokenJoint { key: data.key, force, torque }));
            }
        }
        for (handle, b) in broken {
            self.forget_joint(handle);
            self.world.impulse_joints.remove(handle, true);
            self.joints.broken.push(b);
        }
    }

    /// Forgets the joints on a body about to be removed: Rapier removes
    /// them with it.
    pub(crate) fn forget_joints_of(&mut self, body: RigidBodyHandle) {
        let handles: Vec<_> = self.world.impulse_joints.attached_joints(body).map(|(_, _, handle, _)| handle).collect();
        for handle in handles {
            self.forget_joint(handle);
        }
    }

    fn forget_joint(&mut self, handle: ImpulseJointHandle) {
        if let Some(slot) = self.joints.data.get_mut(handle.into_raw_parts().0 as usize)
            && let Some(data) = slot.take()
        {
            self.count(&data.spec, -1);
        }
    }

    fn count(&mut self, spec: &JointSpec, by: isize) {
        let j = &mut self.joints;
        if spec.kind == JointKind::Spring || spec.motor.is_some() || spec.spring.is_some() {
            j.driven = j.driven.wrapping_add_signed(by);
        }
        if spec.break_force.is_finite() || spec.break_torque.is_finite() {
            j.breakable = j.breakable.wrapping_add_signed(by);
        }
    }

    /// The fixed body joints to the world are joined to, made the first
    /// time it's needed.
    fn ground(&mut self) -> RigidBodyHandle {
        *self.joints.ground.get_or_insert_with(|| self.world.bodies.insert(RigidBodyBuilder::fixed()))
    }

    fn joint_handle(&self, id: u64) -> Result<ImpulseJointHandle, PhysicsError> {
        let (index, generation) = split(id).ok_or(PhysicsError::NoJoint(id))?;
        let handle = ImpulseJointHandle::from_raw_parts(index, generation);
        if self.world.impulse_joints.contains(handle) { Ok(handle) } else { Err(PhysicsError::NoJoint(id)) }
    }

    fn joint_data(&self, handle: ImpulseJointHandle) -> JointData {
        self.joints.data[handle.into_raw_parts().0 as usize].expect("a joint's data")
    }
}

/// How far from its target a drive keeps its bodies awake: 1 mm, 0.1°.
const AWAKE_DISTANCE: f32 = 1e-3;
const AWAKE_ANGLE: f32 = 0.1 * std::f32::consts::PI / 180.0;

const SLACK: JointSpring = JointSpring { spring: 0.0, damper: 0.0, target: 0.0 };

/// The most force a spring's motor may give: none for a spring of no
/// stiffness and no damping, which Rapier would otherwise hold still, as a
/// rod.
fn slack_force(s: JointSpring) -> f32 {
    if s.spring > 0.0 || s.damper > 0.0 { f32::MAX } else { 0.0 }
}

fn joint_id(handle: ImpulseJointHandle) -> u64 {
    let (index, generation) = handle.into_raw_parts();
    join(index, generation)
}

/// Puts `spec`'s limits, motor, spring and collision on `joint`.
fn apply(joint: &mut GenericJoint, spec: &JointSpec) {
    joint.set_contacts_enabled(spec.enable_collision);
    match spec.kind {
        JointKind::Fixed => {}
        JointKind::Hinge => {
            let axis = JointAxis::AngX;
            match spec.limits {
                Some(limits) => {
                    joint.set_limits(axis, limits);
                }
                None => joint.limit_axes.remove(axis.into()),
            }
            joint.motor_axes.remove(axis.into());
            if let Some(m) = spec.motor {
                // As fast as it can up to its force: a stiff velocity motor.
                joint.set_motor(axis, 0.0, m.velocity, 0.0, 1.0e6);
                joint.set_motor_max_force(axis, m.force);
                joint.set_motor_model(axis, MotorModel::ForceBased);
            } else if let Some(s) = spec.spring {
                joint.set_motor(axis, s.target, 0.0, s.spring, s.damper);
                joint.set_motor_max_force(axis, slack_force(s));
                joint.set_motor_model(axis, MotorModel::ForceBased);
            }
        }
        JointKind::Spring => {
            let axis = JointAxis::LinX;
            let s = spec.spring.unwrap_or(SLACK);
            let target = joint.motor(axis).map_or(0.0, |m| m.target_pos);
            joint.set_motor(axis, target, 0.0, s.spring, s.damper);
            joint.set_motor_max_force(axis, slack_force(s));
            joint.set_motor_model(axis, MotorModel::ForceBased);
        }
    }
}

/// How far `frame2` is turned from `frame1` about their X axes: Rapier's
/// revolute joint's angle.
fn hinge_angle(frame1: Rotation, frame2: Rotation) -> f32 {
    let error = frame1.inverse() * frame2;
    let half = error.x.clamp(-1.0, 1.0).asin() * 2.0;
    if frame1.dot(frame2) < 0.0 { -half } else { half }
}

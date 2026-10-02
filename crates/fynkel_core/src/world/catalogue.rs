//! The native component catalogue's systems (blueprint §3.5, ADR-0011,
//! M3-10): `Rotator`, `Oscillate`, `LifetimeDespawn` and `Emitter` run in
//! the §5.1 pre-tick slot (after the tick's commands, before physics, so a
//! kinematic body on a mover follows it this tick); `Follow` and
//! `LookAtCamera` run after physics and the camera rigs, on the tick's
//! propagated poses. Each pose pass computes proposals before one atomic journal publication.
//! Runtime oscillator state advances only after successful publication;
//! lifetime removal uses the same validated boundary.
//! Every pass returns how many components it
//! ticked for the per-system stats.

use std::sync::Arc;

use fynkel_contracts::{ComponentId, DiagnosticCode, Entity, ObjectRef, Tick, Uuid};
use fynkel_ecs::internal::{hecs, Handle};
use fynkel_physics::{BodyCommand, BodyType};
use fynkel_scene::{self as scene, Parent, Transform, WorldTransform};
use glam::{Mat3, Quat, Vec3};

use super::{Diagnostic, World};
use crate::command::Lane;
use crate::components::{
    Camera, Emitter, Follow, LifetimeDespawn, LookAtCamera, MotionSpace, Oscillate, Rotator,
};
use crate::load::SceneDescription;

/// Runtime state of an `Oscillate` (never authored, never on the wire):
/// seconds since the component appeared and the offset last written, in
/// the parent's frame, so the rest position is `position − offset`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Oscillation {
    pub elapsed: f32,
    pub offset: Vec3,
}

/// Runtime state of an `Emitter`: the fractional instance carried over
/// and the anchors it spawned that may still live.
#[derive(Clone, Debug, Default)]
pub struct EmitterRun {
    pub accumulator: f32,
    pub alive: Vec<Entity>,
    /// `PREFAB_UNAVAILABLE` was reported for this emitter.
    pub reported: bool,
}

/// One emitter's spawns of a tick, computed while the ECS is borrowed.
pub(super) struct Emission {
    emitter: Entity,
    prefab: Uuid,
    position: Vec3,
    rotation: Quat,
    count: u32,
    lifetime: f32,
    velocity: Vec3,
}

/// A world pose to write after a read-only pass.
pub(super) struct Placement {
    pub entity: Entity,
    pub position: Vec3,
    pub rotation: Quat,
}

fn rotation_of(matrix: &glam::Mat4) -> Quat {
    let (_, r, _) = matrix.to_scale_rotation_translation();
    r
}

/// The rotation whose local `forward` points along `dir` with `up` as
/// close to +Y as possible; `None` when `dir` is degenerate — shorter than
/// a millimetre, where float noise would set the heading (a smoothed
/// follower converging on its target keeps its last heading instead).
fn facing(dir: Vec3, forward_is_neg_z: bool) -> Option<Quat> {
    if dir.length_squared() < 1e-6 {
        return None;
    }
    let z = if forward_is_neg_z { -dir } else { dir }.try_normalize()?;
    let x = Vec3::Y.cross(z).try_normalize()?;
    let y = z.cross(x);
    Some(Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize())
}

impl World {
    /// Gives the world a prefab description `Emitter`s may instantiate
    /// (the loaders register every prefab a loaded scene's or an
    /// instantiated prefab's emitters name). Replaces an earlier one.
    pub fn register_prefab(&mut self, id: Uuid, prefab: SceneDescription) {
        self.prefabs.insert(id, Arc::new(prefab));
    }

    pub fn has_prefab(&self, id: Uuid) -> bool {
        self.prefabs.contains_key(&id)
    }

    /// Every `Rotator`: the local rotation turns by `speed · dt` about the
    /// axis (in the entity's or the world's frame); the subtree is marked
    /// for propagation.
    pub(super) fn run_rotators(&mut self, tick: Tick) -> u32 {
        let dt = self.config.fixed_dt;
        let mut updates = Vec::new();
        for (handle, rot, original, parent) in self
            .gameplay
            .ecs
            .query::<(&Handle, &Rotator, &Transform, Option<&Parent>)>()
            .without::<&scene::Inactive>()
            .iter()
        {
            let mut pose = *original;
            if let Some(axis) = rot.axis.try_normalize() {
                let angle = rot.speed * dt;
                let rotation = match rot.space {
                    MotionSpace::Local => pose.rotation * Quat::from_axis_angle(axis, angle),
                    MotionSpace::World => {
                        let axis = parent
                            .and_then(|p| self.gameplay.ecs.copy::<WorldTransform>(p.0).ok())
                            .map(|w| rotation_of(&w.matrix).inverse() * axis)
                            .unwrap_or(axis);
                        Quat::from_axis_angle(axis, angle) * pose.rotation
                    }
                };
                if angle != 0.0 {
                    pose.rotation = rotation.normalize();
                }
            }
            updates.push((handle.0, *rot, pose));
        }
        let count = updates.len() as u32;
        if !updates.is_empty() {
            updates.sort_unstable_by_key(|u| u.0);
            let result = self.edit_native_data_at_tick(tick, |edit| {
                for &(entity, rot, pose) in &updates {
                    rot.validate().map_err(|message| {
                        super::gameplay::EditError::InvalidComponent {
                            entity: entity.into(),
                            component: ComponentId::ROTATOR,
                            message,
                        }
                    })?;
                    edit.set_transform(entity, pose)?;
                }
                Ok(())
            });
            if let Err(error) = result {
                self.native_pose_rejection(ComponentId::ROTATOR, "rotator", error);
            }
        }
        count
    }

    /// Calculates poses and oscillator clocks together; neither publishes when
    /// any member of the native stage fails validation.
    pub(super) fn run_oscillators(&mut self, tick: Tick) -> u32 {
        let dt = self.config.fixed_dt;
        let mut updates = Vec::new();
        for (handle, osc, state, original, parent) in self
            .gameplay
            .ecs
            .query::<(
                &Handle,
                &Oscillate,
                Option<&Oscillation>,
                &Transform,
                Option<&Parent>,
            )>()
            .without::<&scene::Inactive>()
            .iter()
        {
            let mut pose = *original;
            let mut run = state.copied().unwrap_or_default();
            run.elapsed += dt;
            if let Some(axis) = osc.axis.try_normalize() {
                let axis = match osc.space {
                    MotionSpace::Local => pose.rotation * axis,
                    MotionSpace::World => parent
                        .and_then(|p| self.gameplay.ecs.copy::<WorldTransform>(p.0).ok())
                        .map(|w| rotation_of(&w.matrix).inverse() * axis)
                        .unwrap_or(axis),
                };
                let phase = core::f32::consts::TAU * osc.frequency * run.elapsed + osc.phase;
                let offset = axis * (osc.amplitude * phase.sin());
                pose.position += offset - run.offset;
                run.offset = offset;
            }
            updates.push((handle.0, *osc, pose, run));
        }
        let count = updates.len() as u32;
        if updates.is_empty() {
            self.attach_state::<Oscillate, Oscillation>();
            return count;
        }
        updates.sort_unstable_by_key(|u| u.0);
        let result = self.edit_native_data_at_tick(tick, |edit| {
            for &(entity, osc, pose, _) in &updates {
                osc.validate()
                    .map_err(|message| super::gameplay::EditError::InvalidComponent {
                        entity: entity.into(),
                        component: ComponentId::OSCILLATE,
                        message,
                    })?;
                edit.set_transform(entity, pose)?;
            }
            Ok(())
        });
        match result {
            Ok(_) => {
                self.attach_state::<Oscillate, Oscillation>();
                for (entity, _, _, state) in updates {
                    self.gameplay
                        .ecs
                        .insert(entity, state)
                        .expect("validated oscillator");
                }
            }
            Err(error) => self.native_pose_rejection(ComponentId::OSCILLATE, "oscillator", error),
        }
        count
    }

    pub(super) fn native_pose_rejection(
        &mut self,
        component: ComponentId,
        stage: &str,
        error: super::gameplay::EditError,
    ) {
        self.publication.diagnostics.push(Diagnostic {
            command_index: u32::MAX,
            lane: Lane::Simulation,
            code: DiagnosticCode::INVALID_PAYLOAD,
            entity: Entity::NULL,
            component,
            message: format!("{stage} pass: {error}"),
        });
    }

    /// Every `LifetimeDespawn` counts down; those at zero destroy their
    /// subtree exactly as `Command::Destroy` would.
    pub(super) fn run_lifetimes(&mut self, tick: Tick) -> u32 {
        let dt = self.config.fixed_dt;
        let mut lifetimes = std::mem::take(&mut self.scratch.lifetimes);
        lifetimes.clear();
        for (handle, life) in self
            .gameplay
            .ecs
            .query::<(&Handle, &LifetimeDespawn)>()
            .without::<&scene::Inactive>()
            .iter()
        {
            lifetimes.push((handle.0, *life));
        }
        let count = lifetimes.len() as u32;
        if !lifetimes.is_empty() {
            lifetimes.sort_unstable_by_key(|(entity, _)| *entity);
            let result = self.edit_gameplay_at_tick(tick, |edit| {
                // Validate every countdown before staging any removals. A child
                // expiring with its parent must not become a stale second write.
                for &(entity, life) in &lifetimes {
                    life.validate().map_err(|message| {
                        super::gameplay::EditError::InvalidComponent {
                            entity: entity.into(),
                            component: ComponentId::LIFETIME_DESPAWN,
                            message,
                        }
                    })?;
                    edit.set_component(
                        entity,
                        crate::ComponentValue::LifetimeDespawn(LifetimeDespawn {
                            seconds: (life.seconds - dt).max(0.0),
                        }),
                    )?;
                }
                for &(entity, life) in &lifetimes {
                    if life.seconds <= dt && edit.contains(entity)? {
                        edit.destroy(entity)?;
                    }
                }
                Ok(())
            });
            if let Err(error) = result {
                self.publication.diagnostics.push(Diagnostic {
                    command_index: u32::MAX,
                    lane: Lane::Simulation,
                    code: DiagnosticCode::INVALID_PAYLOAD,
                    entity: Entity::NULL,
                    component: ComponentId::LIFETIME_DESPAWN,
                    message: format!("lifetime pass: {error}"),
                });
            }
        }
        lifetimes.clear();
        self.scratch.lifetimes = lifetimes;
        count
    }

    /// Every enabled `Emitter` instantiates its prefab `rate · dt` times
    /// (fraction carried), each instance an unparented anchor at the
    /// emitter's world pose with a `LifetimeDespawn` and its dynamic
    /// bodies at `velocity`. Each emitter's burst is one atomic journal; invalid
    /// instances publish nothing. Scheduling still consumes the attempted burst
    /// (no catch-up backlog); only committed roots enter the live-instance ceiling.
    pub(super) fn run_emitters(&mut self, tick: Tick) -> u32 {
        let dt = self.config.fixed_dt;
        self.attach_state::<Emitter, EmitterRun>();
        let mut emissions = std::mem::take(&mut self.scratch.emissions);
        emissions.clear();
        let mut count = 0;
        {
            let h = &self.gameplay.ecs;
            for (handle, em, wt) in h
                .query::<(&Handle, &Emitter, &WorldTransform)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                let Ok(mut run) = self.gameplay.ecs.get_mut::<EmitterRun>(handle.0) else {
                    continue;
                };
                count += 1;
                if !em.enabled || em.rate <= 0.0 {
                    continue;
                }
                run.alive.retain(|e| self.gameplay.ecs.contains(*e));
                run.accumulator += em.rate * dt;
                let mut n = run.accumulator.floor();
                run.accumulator -= n;
                if em.max_alive > 0 {
                    let room = em.max_alive.saturating_sub(run.alive.len() as u32) as f32;
                    n = n.min(room);
                }
                if n < 1.0 {
                    continue;
                }
                let (_, rotation, position) = wt.matrix.to_scale_rotation_translation();
                emissions.push(Emission {
                    emitter: handle.0,
                    prefab: em.prefab.asset,
                    position,
                    rotation,
                    count: n as u32,
                    lifetime: em.lifetime,
                    velocity: em.velocity,
                });
            }
        }
        for e in emissions.drain(..) {
            let Some(prefab) = self.prefabs.get(&e.prefab).cloned() else {
                let reported = self
                    .gameplay
                    .ecs
                    .get::<EmitterRun>(e.emitter)
                    .map(|r| r.reported)
                    .unwrap_or(true);
                if !reported {
                    if let Ok(mut run) = self.gameplay.ecs.get_mut::<EmitterRun>(e.emitter) {
                        run.reported = true;
                    }
                    self.publication.diagnostics.push(Diagnostic {
                        command_index: u32::MAX,
                        lane: Lane::Simulation,
                        code: DiagnosticCode::PREFAB_UNAVAILABLE,
                        entity: e.emitter,
                        component: ComponentId::EMITTER,
                        message: format!(
                            "emitter: prefab {} is not registered with the world (not in the loaded scene's closure?)",
                            e.prefab
                        ),
                    });
                }
                continue;
            };
            // The prefab's root keeps its own rotation and scale, placed
            // at the emitter's pose (ADR-0022: the instance is the root).
            let own = prefab
                .entities
                .iter()
                .find(|d| d.parent.is_none())
                .and_then(|d| {
                    d.components.iter().find_map(|c| match c {
                        crate::ComponentValue::Transform(t) => Some(*t),
                        _ => None,
                    })
                })
                .unwrap_or(Transform::IDENTITY);
            let pose = Transform {
                position: e.position,
                rotation: e.rotation,
                scale: Vec3::ONE,
            };
            let transform = Transform::from_matrix(pose.matrix() * own.matrix());
            let physics_enabled = self.modules.physics;
            let result = self.edit_gameplay_at_tick(tick, |edit| {
                let mut roots = Vec::new();
                for _ in 0..e.count {
                    let instance = edit.instantiate(&prefab, &prefab.name, transform, None)?;
                    if e.lifetime > 0.0 {
                        edit.set_component(
                            instance.root,
                            crate::ComponentValue::LifetimeDespawn(LifetimeDespawn {
                                seconds: e.lifetime,
                            }),
                        )?;
                    }
                    if physics_enabled && e.velocity != Vec3::ZERO {
                        for &(_, entity) in &instance.entities {
                            if matches!(edit.component(entity, ComponentId::RIGID_BODY)?,
                                Some(crate::GameplayComponentValue::RigidBody(body))
                                if body.body_type == BodyType::Dynamic)
                            {
                                edit.body_effect(
                                    entity,
                                    BodyCommand::SetLinearVelocity(e.velocity),
                                )?;
                            }
                        }
                    }
                    roots.push(instance.root);
                }
                Ok(roots)
            });
            match result {
                Ok(commit) => {
                    if let Ok(mut run) = self.gameplay.ecs.get_mut::<EmitterRun>(e.emitter) {
                        run.alive.extend(
                            commit
                                .value
                                .iter()
                                .map(|&root| commit.resolve(root).expect("committed emission")),
                        );
                    }
                }
                Err(error) => self.publication.diagnostics.push(Diagnostic {
                    command_index: u32::MAX,
                    lane: Lane::Simulation,
                    code: DiagnosticCode::INVALID_PAYLOAD,
                    entity: e.emitter,
                    component: ComponentId::EMITTER,
                    message: format!("emitter burst: {error}"),
                }),
            }
        }

        self.scratch.emissions = emissions;
        count
    }

    /// Every `Follow` moves towards its target's propagated world
    /// position plus the offset. Returns the count and whether any moved.
    pub(super) fn run_followers(&mut self, tick: Tick) -> (u32, bool) {
        let dt = self.config.fixed_dt;
        let mut placements = std::mem::take(&mut self.scratch.placements);
        placements.clear();
        let mut count = 0;
        let mut invalid = None;
        {
            let h = &self.gameplay.ecs;
            for (handle, follow, wt) in h
                .query::<(&Handle, &Follow, &WorldTransform)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                count += 1;
                let entity = handle.0;
                if let Err(message) = follow.validate() {
                    invalid = Some(super::gameplay::EditError::InvalidComponent {
                        entity: entity.into(),
                        component: ComponentId::FOLLOW,
                        message,
                    });
                    break;
                }
                let Some(target) = self.live(follow.target) else {
                    continue;
                };
                let Ok(target_world) = self.gameplay.ecs.get::<WorldTransform>(target) else {
                    continue;
                };
                let target_pos = target_world.matrix.w_axis.truncate();
                let desired = target_pos + follow.offset;
                let (_, current_rot, current) = wt.matrix.to_scale_rotation_translation();
                let position = if follow.smoothing > 0.0 {
                    current + (desired - current) * (1.0 - (-follow.smoothing * dt).exp())
                } else {
                    desired
                };
                let rotation = if follow.look_at {
                    facing(target_pos - position, true).unwrap_or(current_rot)
                } else {
                    current_rot
                };
                placements.push(Placement {
                    entity,
                    position,
                    rotation,
                });
            }
        }
        if let Some(error) = invalid {
            self.native_pose_rejection(ComponentId::FOLLOW, "follower", error);
            placements.clear();
            self.scratch.placements = placements;
            return (count, false);
        }
        let moved = self.apply_placements(tick, ComponentId::FOLLOW, &mut placements);
        self.scratch.placements = placements;
        (count, moved)
    }

    /// Every `LookAtCamera` turns its +Z towards the active camera's
    /// propagated position.
    pub(super) fn run_billboards(&mut self, tick: Tick) -> (u32, bool) {
        let Some(camera) = self.active_camera_position() else {
            return (0, false);
        };
        let mut placements = std::mem::take(&mut self.scratch.placements);
        placements.clear();
        let mut count = 0;
        {
            let h = &self.gameplay.ecs;
            for (handle, look, wt) in h
                .query::<(&Handle, &LookAtCamera, &WorldTransform)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                count += 1;
                let position = wt.matrix.w_axis.truncate();
                let mut dir = camera - position;
                if look.lock_y {
                    dir.y = 0.0;
                }
                let Some(rotation) = facing(dir, false) else {
                    continue;
                };
                placements.push(Placement {
                    entity: handle.0,
                    position,
                    rotation,
                });
            }
        }
        let moved = self.apply_placements(tick, ComponentId::LOOK_AT_CAMERA, &mut placements);
        self.scratch.placements = placements;
        (count, moved)
    }

    /// The world position of the active camera: the `Camera` with the
    /// highest priority, the lowest entity on a tie.
    fn active_camera_position(&self) -> Option<Vec3> {
        let h = &self.gameplay.ecs;
        let mut best: Option<(i32, Entity, Vec3)> = None;
        for (handle, cam, wt) in h
            .query::<(&Handle, &Camera, &WorldTransform)>()
            .without::<&scene::Inactive>()
            .iter()
        {
            let candidate = (cam.priority, handle.0, wt.matrix.w_axis.truncate());
            let better = match best {
                None => true,
                Some((p, e, _)) => cam.priority > p || (cam.priority == p && handle.0 < e),
            };
            if better {
                best = Some(candidate);
            }
        }
        best.map(|(_, _, p)| p)
    }

    /// Writes world poses; `true` when at least one entity changed.
    fn apply_placements(
        &mut self,
        tick: Tick,
        component: ComponentId,
        placements: &mut [Placement],
    ) -> bool {
        if placements.is_empty() {
            return false;
        }
        let desired: Vec<_> = placements
            .iter()
            .map(|p| (p.entity, p.position, p.rotation))
            .collect();
        let poses = match scene::prepare_world_poses(&self.gameplay.ecs, &desired) {
            Ok(poses) => poses,
            Err(error) => {
                self.native_pose_rejection(
                    component,
                    "placement",
                    super::gameplay::EditError::InvalidEffect(
                        Entity::NULL.into(),
                        format!("{error:?}"),
                    ),
                );
                return false;
            }
        };
        let result = self.edit_native_data_at_tick(tick, |edit| {
            let mut moved = false;
            for &(entity, pose) in &poses {
                moved |= edit.local_transform(entity)? != pose;
                edit.set_transform(entity, pose)?;
            }
            Ok(moved)
        });
        match result {
            Ok(commit) => commit.value,
            Err(error) => {
                self.native_pose_rejection(component, "placement", error);
                false
            }
        }
    }

    /// The object a reference names while it lives (ADR-0035): a
    /// destroyed target is missing, never replaced by a search.
    pub(super) fn live(&self, r: ObjectRef) -> Option<Entity> {
        r.get().filter(|e| self.gameplay.ecs.contains(*e))
    }

    /// Gives every entity with a `C` a default `S` (its runtime state) and
    /// takes it away from those that lost their `C`.
    pub(super) fn attach_state<C: hecs::Component, S: hecs::Component + Default>(&mut self) {
        let mut pending = std::mem::take(&mut self.scratch.entities);
        pending.clear();
        {
            let h = &self.gameplay.ecs;
            pending.extend(
                h.query::<hecs::Without<&Handle, &S>>()
                    .with::<&C>()
                    .iter()
                    .map(|h| h.0),
            );
        }
        for e in pending.drain(..) {
            let _ = self.gameplay.ecs.insert(e, S::default());
        }
        {
            let h = &self.gameplay.ecs;
            pending.extend(
                h.query::<hecs::Without<&Handle, &C>>()
                    .with::<&S>()
                    .iter()
                    .map(|h| h.0),
            );
        }
        for e in pending.drain(..) {
            let _ = self.gameplay.ecs.remove::<S>(e);
        }
        self.scratch.entities = pending;
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    use crate::{ComponentValue, WorldConfig};

    #[test]
    fn lifetime_commit_keeps_native_transform_propagation_at_its_declared_stage() {
        let mut world = World::new(WorldConfig::default());
        let (entity, _) = world
            .spawn_now(
                "moving",
                Transform::IDENTITY,
                None,
                vec![ComponentValue::LifetimeDespawn(LifetimeDespawn {
                    seconds: 1.0,
                })],
                None,
            )
            .unwrap();
        scene::set_local(world.ecs(), entity, Transform::from_position(Vec3::X)).unwrap();
        let before = world.ecs().copy::<WorldTransform>(entity).unwrap().matrix;
        let tick = world.tick();
        world.run_lifetimes(tick + 1);
        assert_eq!(world.tick(), tick);
        assert_eq!(
            world.ecs().copy::<WorldTransform>(entity).unwrap().matrix,
            before
        );
        assert!(world.ecs().copy::<LifetimeDespawn>(entity).unwrap().seconds < 1.0);
        scene::propagate(world.ecs_mut());
        assert_eq!(
            world
                .ecs()
                .copy::<WorldTransform>(entity)
                .unwrap()
                .matrix
                .w_axis
                .truncate(),
            Vec3::X
        );
    }
}

//! The native component catalogue's systems (blueprint §3.5, ADR-0011,
//! M3-10): `Rotator`, `Oscillate`, `LifetimeDespawn` and `Emitter` run in
//! the §5.1 pre-tick slot (after the tick's commands, before physics, so a
//! kinematic body on a mover follows it this tick); `Follow` and
//! `LookAtCamera` run after physics and the camera rigs, on the tick's
//! propagated poses. Each pass is one or two archetype queries and no
//! per-entity allocation — thousands of movers cost microseconds, which is
//! the point of the catalogue. Every pass returns how many components it
//! ticked for the per-system stats.

use std::sync::Arc;

use fynkel_contracts::{ComponentId, DiagnosticCode, Entity, ObjectRef, Tick, Uuid};
use fynkel_ecs::internal::{hecs, Handle};
use fynkel_physics::{BodyCommand, BodyType, RigidBody};
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
    pub(super) fn run_rotators(&mut self) -> u32 {
        let dt = self.config.fixed_dt;
        let mut count = 0;
        let h = self.ecs.hecs();
        for (rot, t, parent) in h
            .query::<(&Rotator, &mut Transform, Option<&Parent>)>()
            .without::<&scene::Inactive>()
            .iter()
        {
            count += 1;
            let Some(axis) = rot.axis.try_normalize() else {
                continue;
            };
            let angle = rot.speed * dt;
            if angle == 0.0 {
                continue;
            }
            match rot.space {
                MotionSpace::Local => {
                    t.rotation = (t.rotation * Quat::from_axis_angle(axis, angle)).normalize();
                }
                MotionSpace::World => {
                    // The world axis expressed in the parent's frame (the
                    // parent's matrix as the last propagate left it).
                    let axis = match parent.and_then(|p| self.ecs.raw(p.0)) {
                        Some(raw) => h
                            .get::<&WorldTransform>(raw)
                            .map(|w| rotation_of(&w.matrix).inverse() * axis)
                            .unwrap_or(axis),
                        None => axis,
                    };
                    t.rotation = (Quat::from_axis_angle(axis, angle) * t.rotation).normalize();
                }
            }
        }
        for (_, wt) in h
            .query::<(&Rotator, &mut WorldTransform)>()
            .without::<&scene::Inactive>()
            .iter()
        {
            wt.dirty = true;
        }
        count
    }

    /// Every `Oscillate`: advances its clock and moves the entity by the
    /// change in offset, so gameplay writes shift the rest position
    /// instead of fighting the motion.
    pub(super) fn run_oscillators(&mut self) -> u32 {
        let dt = self.config.fixed_dt;
        self.attach_state::<Oscillate, Oscillation>();
        let mut count = 0;
        let h = self.ecs.hecs();
        for (osc, run, t, parent) in h
            .query::<(
                &Oscillate,
                &mut Oscillation,
                &mut Transform,
                Option<&Parent>,
            )>()
            .without::<&scene::Inactive>()
            .iter()
        {
            count += 1;
            run.elapsed += dt;
            let Some(axis) = osc.axis.try_normalize() else {
                continue;
            };
            let axis = match osc.space {
                MotionSpace::Local => t.rotation * axis,
                MotionSpace::World => match parent.and_then(|p| self.ecs.raw(p.0)) {
                    Some(raw) => h
                        .get::<&WorldTransform>(raw)
                        .map(|w| rotation_of(&w.matrix).inverse() * axis)
                        .unwrap_or(axis),
                    None => axis,
                },
            };
            let phase = core::f32::consts::TAU * osc.frequency * run.elapsed + osc.phase;
            let offset = axis * (osc.amplitude * phase.sin());
            t.position += offset - run.offset;
            run.offset = offset;
        }
        for (_, wt) in h
            .query::<(&Oscillate, &mut WorldTransform)>()
            .without::<&scene::Inactive>()
            .iter()
        {
            wt.dirty = true;
        }
        count
    }

    /// Every `LifetimeDespawn` counts down; those at zero destroy their
    /// subtree exactly as `Command::Destroy` would.
    pub(super) fn run_lifetimes(&mut self, tick: Tick) -> u32 {
        let dt = self.config.fixed_dt;
        let mut expired = std::mem::take(&mut self.entity_scratch);
        expired.clear();
        let mut count = 0;
        {
            let h = self.ecs.hecs();
            for (handle, life) in h
                .query::<(&Handle, &mut LifetimeDespawn)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                count += 1;
                life.seconds = (life.seconds - dt).max(0.0);
                if life.seconds <= 0.0 {
                    expired.push(handle.0);
                }
            }
        }
        // Entity order, so a parent and its child expiring together
        // destroy once (the child is gone with the parent's subtree).
        expired.sort_unstable();
        for e in expired.drain(..) {
            if self.ecs.contains(e) {
                let _ = self.destroy_subtree(e, tick);
            }
        }
        self.entity_scratch = expired;
        count
    }

    /// Every enabled `Emitter` instantiates its prefab `rate · dt` times
    /// (fraction carried), each instance an unparented anchor at the
    /// emitter's world pose with a `LifetimeDespawn` and its dynamic
    /// bodies at `velocity`.
    pub(super) fn run_emitters(&mut self, tick: Tick) -> u32 {
        let dt = self.config.fixed_dt;
        self.attach_state::<Emitter, EmitterRun>();
        let mut emissions = std::mem::take(&mut self.emission_scratch);
        emissions.clear();
        let mut count = 0;
        {
            let h = self.ecs.hecs();
            for (handle, em, run, wt) in h
                .query::<(&Handle, &Emitter, &mut EmitterRun, &WorldTransform)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                count += 1;
                if !em.enabled || em.rate <= 0.0 {
                    continue;
                }
                run.alive.retain(|e| self.ecs.contains(*e));
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
                    .ecs
                    .get::<EmitterRun>(e.emitter)
                    .map(|r| r.reported)
                    .unwrap_or(true);
                if !reported {
                    if let Ok(mut run) = self.ecs.get_mut::<EmitterRun>(e.emitter) {
                        run.reported = true;
                    }
                    self.diagnostics.push(Diagnostic {
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
            for _ in 0..e.count {
                let Ok((root, loaded)) =
                    self.instantiate_inner(&prefab, &prefab.name, transform, None, tick, false)
                else {
                    break;
                };
                if e.lifetime > 0.0 {
                    let _ = self.ecs.insert(
                        root,
                        LifetimeDespawn {
                            seconds: e.lifetime,
                        },
                    );
                }
                if e.velocity != Vec3::ZERO {
                    for &(_, entity) in loaded.entities() {
                        let dynamic = self
                            .ecs
                            .get::<RigidBody>(entity)
                            .is_ok_and(|b| b.body_type == BodyType::Dynamic);
                        if dynamic {
                            self.physics.queue(
                                u32::MAX,
                                entity,
                                BodyCommand::SetLinearVelocity(e.velocity),
                            );
                        }
                    }
                }
                if let Ok(mut run) = self.ecs.get_mut::<EmitterRun>(e.emitter) {
                    run.alive.push(root);
                }
            }
        }
        self.emission_scratch = emissions;
        count
    }

    /// Every `Follow` moves towards its target's propagated world
    /// position plus the offset. Returns the count and whether any moved.
    pub(super) fn run_followers(&mut self) -> (u32, bool) {
        let dt = self.config.fixed_dt;
        let mut placements = std::mem::take(&mut self.placement_scratch);
        placements.clear();
        let mut count = 0;
        {
            let h = self.ecs.hecs();
            for (handle, follow, wt) in h
                .query::<(&Handle, &Follow, &WorldTransform)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                count += 1;
                let entity = handle.0;
                let Some(target) = self.live(follow.target) else {
                    continue;
                };
                let Ok(target_world) = self.ecs.get::<WorldTransform>(target) else {
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
        let moved = self.apply_placements(&mut placements);
        self.placement_scratch = placements;
        (count, moved)
    }

    /// Every `LookAtCamera` turns its +Z towards the active camera's
    /// propagated position.
    pub(super) fn run_billboards(&mut self) -> (u32, bool) {
        let Some(camera) = self.active_camera_position() else {
            return (0, false);
        };
        let mut placements = std::mem::take(&mut self.placement_scratch);
        placements.clear();
        let mut count = 0;
        {
            let h = self.ecs.hecs();
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
        let moved = self.apply_placements(&mut placements);
        self.placement_scratch = placements;
        (count, moved)
    }

    /// The world position of the active camera: the `Camera` with the
    /// highest priority, the lowest entity on a tie.
    fn active_camera_position(&self) -> Option<Vec3> {
        let h = self.ecs.hecs();
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
    fn apply_placements(&mut self, placements: &mut Vec<Placement>) -> bool {
        let mut moved = false;
        for p in placements.drain(..) {
            let before = self.ecs.copy::<Transform>(p.entity).ok();
            if scene::set_world_pose(&self.ecs, p.entity, p.position, p.rotation).is_ok()
                && self.ecs.copy::<Transform>(p.entity).ok() != before
            {
                moved = true;
            }
        }
        moved
    }

    /// The object a reference names while it lives (ADR-0035): a
    /// destroyed target is missing, never replaced by a search.
    pub(super) fn live(&self, r: ObjectRef) -> Option<Entity> {
        r.get().filter(|e| self.ecs.contains(*e))
    }

    /// Gives every entity with a `C` a default `S` (its runtime state) and
    /// takes it away from those that lost their `C`.
    pub(super) fn attach_state<C: hecs::Component, S: hecs::Component + Default>(&mut self) {
        let mut pending = std::mem::take(&mut self.entity_scratch);
        pending.clear();
        {
            let h = self.ecs.hecs();
            pending.extend(
                h.query::<hecs::Without<&Handle, &S>>()
                    .with::<&C>()
                    .iter()
                    .map(|h| h.0),
            );
        }
        for e in pending.drain(..) {
            let _ = self.ecs.insert(e, S::default());
        }
        {
            let h = self.ecs.hecs();
            pending.extend(
                h.query::<hecs::Without<&Handle, &C>>()
                    .with::<&S>()
                    .iter()
                    .map(|h| h.0),
            );
        }
        for e in pending.drain(..) {
            let _ = self.ecs.remove::<S>(e);
        }
        self.entity_scratch = pending;
    }
}

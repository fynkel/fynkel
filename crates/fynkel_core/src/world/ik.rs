//! IK after all layers (ADR-0033 § 5): an animator whose controller has
//! `ik` goals bends the pose its layers left in the transforms, goal by
//! goal, before the skins read it.
//!
//! - `twoBone`: an arm or a leg reaches its target — the upper bone
//!   turns so the middle lands where the two lengths meet the target,
//!   the middle turns so the tip lands on it; the bend stays on the side
//!   of the pole (else of the way it bends now).
//! - `lookAt`: the head (and the spine, `body` of the turn) yaws and
//!   pitches towards the target, from the character's forward (−Z), at
//!   most `limit`, eased so a new target is not a snap.
//! - `footPlacement`: a ray down at each foot, from `reach` above the
//!   character's feet; the hips go down as far as the lower foot needs,
//!   each leg reaches its ground (a two-bone solve), the foot tilts to the
//!   ground's slope. It needs the physics module; the rays see last
//!   tick's colliders.
//!
//! A goal's weight is a number or a float parameter, times the weight of
//! a target set from code. What a goal wrote is taken back the next tick
//! when no layer posed the node again, so it never compounds.

#![cfg_attr(not(feature = "animation"), allow(dead_code, unused_imports))]

use fynkel_contracts::{ComponentId, DiagnosticCode, Entity};
use fynkel_ecs::World as Ecs;
use fynkel_physics::{CharacterController, ColliderShape, Hit, QueryKind, QuerySpec};
use fynkel_scene::{self as scene, Parent, Transform};
use glam::{Mat4, Quat, Vec3};

use super::animation::{find_named, AnimatorRun};
use super::{Diagnostic, Name, World};
use crate::animation::controller::{ControllerDef, IkKind, IkWeight};
use crate::command::Lane;

/// How fast a look-at and the feet follow what they want, per second.
const EASE: f32 = 10.0;
/// The most a foot tilts to the ground, radians.
const MAX_TILT: f32 = 0.6;

/// One animator's IK state (on its controller run).
#[derive(Clone, Debug, Default)]
pub struct IkRun {
    /// Per goal: a target set from code (`setIKTarget`) and its weight.
    pub targets: Vec<Option<(Vec3, f32)>>,
    /// Per goal: the nodes and objects it names, resolved.
    nodes: Vec<Vec<Option<Entity>>>,
    /// Per goal: what it moved last tick, eased towards what it wants
    /// (look-at: yaw, pitch; foot placement: the hips' drop and each
    /// foot's lift).
    eased: Vec<[f32; 3]>,
    /// Per goal: `IK_UNAVAILABLE` was reported.
    reported: Vec<bool>,
    /// Every node written last tick: before and after.
    touched: Vec<(Entity, Transform, Transform)>,
}

impl IkRun {
    pub fn new(goals: usize) -> IkRun {
        IkRun {
            targets: vec![None; goals],
            nodes: vec![Vec::new(); goals],
            eased: vec![[0.0; 3]; goals],
            reported: vec![false; goals],
            touched: Vec::new(),
        }
    }
}

/// A node named `name` under `under`, cached in `slot` while it keeps
/// the name.
fn node(ecs: &Ecs, under: Entity, slot: &mut Option<Entity>, name: &str) -> Option<Entity> {
    if let Some(e) = *slot {
        if ecs.get::<Name>(e).is_ok_and(|n| n.value == name) {
            return Some(e);
        }
    }
    *slot = find_named(ecs, under, name);
    *slot
}

/// An object named `name`: under the animator first, else the first in
/// the world.
fn object(ecs: &Ecs, animator: Entity, slot: &mut Option<Entity>, name: &str) -> Option<Entity> {
    if let Some(e) = *slot {
        if ecs.get::<Name>(e).is_ok_and(|n| n.value == name) {
            return Some(e);
        }
    }
    *slot = find_named(ecs, animator, name).or_else(|| {
        ecs.hecs()
            .query::<(&fynkel_ecs::internal::Handle, &Name)>()
            .iter()
            .find(|(_, n)| n.value == name)
            .map(|(h, _)| h.0)
    });
    *slot
}

/// `e`'s world matrix from the local transforms now (this tick's pose;
/// the cached world matrices are last tick's).
fn world(ecs: &Ecs, e: Entity) -> Mat4 {
    let local = |e: Entity| {
        ecs.copy::<Transform>(e)
            .map_or(Mat4::IDENTITY, |t| t.matrix())
    };
    let mut m = local(e);
    let mut at = e;
    while let Ok(p) = ecs.get::<Parent>(at) {
        at = p.0;
        m = local(at) * m;
    }
    m
}

fn position(ecs: &Ecs, e: Entity) -> Vec3 {
    world(ecs, e).w_axis.truncate()
}

fn rotation(ecs: &Ecs, e: Entity) -> Quat {
    world(ecs, e).to_scale_rotation_translation().1
}

fn parent_world(ecs: &Ecs, e: Entity) -> Mat4 {
    ecs.get::<Parent>(e)
        .map(|p| p.0)
        .map_or(Mat4::IDENTITY, |p| world(ecs, p))
}

/// Writes what IK moves, remembering each node's transform before.
struct Writer<'a> {
    ecs: &'a Ecs,
    touched: &'a mut Vec<(Entity, Transform, Transform)>,
}

impl Writer<'_> {
    fn set(&mut self, e: Entity, next: Transform) {
        let Ok(now) = self.ecs.copy::<Transform>(e) else {
            return;
        };
        if !self.touched.iter().any(|(t, _, _)| *t == e) {
            self.touched.push((e, now, now));
        }
        let _ = scene::set_local(self.ecs, e, next);
    }

    /// Turns `e` by `q` in world space.
    fn rotate(&mut self, e: Entity, q: Quat) {
        let Ok(mut t) = self.ecs.copy::<Transform>(e) else {
            return;
        };
        let parent = parent_world(self.ecs, e).to_scale_rotation_translation().1;
        t.rotation = (parent.inverse() * q * parent * t.rotation).normalize();
        self.set(e, t);
    }

    /// Sets `e`'s world rotation.
    fn set_rotation(&mut self, e: Entity, r: Quat) {
        let Ok(mut t) = self.ecs.copy::<Transform>(e) else {
            return;
        };
        let parent = parent_world(self.ecs, e).to_scale_rotation_translation().1;
        t.rotation = (parent.inverse() * r).normalize();
        self.set(e, t);
    }

    /// Moves `e` by `d` in world space.
    fn translate(&mut self, e: Entity, d: Vec3) {
        let Ok(mut t) = self.ecs.copy::<Transform>(e) else {
            return;
        };
        t.position += parent_world(self.ecs, e).inverse().transform_vector3(d);
        self.set(e, t);
    }
}

/// An arm or a leg (`root` → `middle` → `tip`) reaching `target`, the
/// bend towards `pole` (else as it bends now), by `w`.
fn two_bone(
    w: &mut Writer,
    root: Entity,
    middle: Entity,
    tip: Entity,
    target: Vec3,
    pole: Option<Vec3>,
    weight: f32,
) {
    let ecs = w.ecs;
    let (a, b, c) = (
        position(ecs, root),
        position(ecs, middle),
        position(ecs, tip),
    );
    let (lab, lbc) = ((b - a).length(), (c - b).length());
    let to = target - a;
    if lab < 1e-5 || lbc < 1e-5 || to.length() < 1e-5 {
        return;
    }
    let dir = to.normalize();
    let dist = to
        .length()
        .clamp((lab - lbc).abs() + 1e-4, lab + lbc - 1e-4);
    // The bend's side: the pole's, else the middle's now.
    let off_line = |v: Vec3| v - dir * v.dot(dir);
    let mut side = off_line(pole.map_or(b - a, |p| p - a));
    if side.length_squared() < 1e-10 {
        side = off_line(b - a);
    }
    let side = if side.length_squared() < 1e-10 {
        dir.any_orthonormal_vector()
    } else {
        side.normalize()
    };
    let cos = ((lab * lab + dist * dist - lbc * lbc) / (2.0 * lab * dist)).clamp(-1.0, 1.0);
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let b_goal = a + (dir * cos + side * sin) * lab;
    let q_root = Quat::from_rotation_arc((b - a).normalize(), (b_goal - a).normalize());
    let c_turned = a + q_root * (c - a);
    let q_middle = Quat::from_rotation_arc(
        (c_turned - b_goal).normalize(),
        (a + dir * dist - b_goal).normalize(),
    );
    w.rotate(root, Quat::IDENTITY.slerp(q_root, weight));
    w.rotate(middle, Quat::IDENTITY.slerp(q_middle, weight));
}

/// The character's nearest ancestor (itself included) with a character
/// controller: what the feet's rays skip.
fn body_of(ecs: &Ecs, animator: Entity) -> Option<Entity> {
    let mut at = animator;
    loop {
        if ecs.get::<CharacterController>(at).is_ok() {
            return Some(at);
        }
        at = ecs.get::<Parent>(at).ok()?.0;
    }
}

impl World {
    /// Solves `entity`'s IK goals over the pose its layers wrote.
    pub(super) fn run_ik(&mut self, entity: Entity, dt: f32) {
        let (def, mut ik, weights) = {
            let Ok(mut run) = self.ecs.get_mut::<AnimatorRun>(entity) else {
                return;
            };
            let Some(play) = run.controller.as_mut() else {
                return;
            };
            if play.def.ik.is_empty() {
                return;
            }
            let weights: Vec<f32> = play
                .def
                .ik
                .iter()
                .map(|g| match g.weight {
                    IkWeight::Value(v) => v,
                    IkWeight::Parameter(p) => play.run.values[p].as_f32(),
                })
                .map(|v| v.clamp(0.0, 1.0))
                .collect();
            (play.def.clone(), std::mem::take(&mut play.ik), weights)
        };
        let mut problems = Vec::new();
        self.solve(entity, &def, &mut ik, &weights, dt, &mut problems);
        if let Ok(mut run) = self.ecs.get_mut::<AnimatorRun>(entity) {
            if let Some(play) = run.controller.as_mut() {
                play.ik = ik;
            }
        }
        for message in problems {
            self.diagnostics.push(Diagnostic {
                command_index: u32::MAX,
                lane: Lane::Simulation,
                code: DiagnosticCode::IK_UNAVAILABLE,
                entity,
                component: ComponentId::ANIMATOR,
                message: format!("FYN-R0015: {message}"),
            });
        }
    }

    fn solve(
        &self,
        animator: Entity,
        def: &ControllerDef,
        ik: &mut IkRun,
        weights: &[f32],
        dt: f32,
        problems: &mut Vec<String>,
    ) {
        let ecs = &self.ecs;
        // Last tick's writes that no layer posed again are taken back.
        for (e, before, after) in ik.touched.drain(..) {
            if ecs.copy::<Transform>(e).is_ok_and(|now| now == after) {
                let _ = scene::set_local(ecs, e, before);
            }
        }
        let mut touched = std::mem::take(&mut ik.touched);
        let mut w = Writer {
            ecs,
            touched: &mut touched,
        };
        let ease = 1.0 - (-EASE * dt).exp();
        for (i, goal) in def.ik.iter().enumerate() {
            let mut report = |message: String| {
                if !ik.reported[i] {
                    ik.reported[i] = true;
                    problems.push(format!("IK goal `{}`: {message}", goal.name));
                }
            };
            let slots = &mut ik.nodes[i];
            slots.resize(12, None);
            let mut weight = weights[i];
            // The target: code's, else the object named.
            let target = match ik.targets[i] {
                Some((p, cw)) => {
                    weight *= cw.clamp(0.0, 1.0);
                    Some(p)
                }
                None => goal.target.as_deref().and_then(|name| {
                    let e = object(ecs, animator, &mut slots[11], name)?;
                    Some(position(ecs, e))
                }),
            };
            let missing = |name: &str| format!("no node `{name}` under the animator");
            match &goal.kind {
                IkKind::TwoBone {
                    root,
                    middle,
                    tip,
                    pole,
                } => {
                    let Some(t) = node(ecs, animator, &mut slots[0], tip) else {
                        report(missing(tip));
                        continue;
                    };
                    let parent = |e: Entity| ecs.get::<Parent>(e).ok().map(|p| p.0);
                    let m = match middle {
                        Some(n) => node(ecs, animator, &mut slots[1], n).ok_or_else(|| missing(n)),
                        None => parent(t).ok_or_else(|| format!("`{tip}` has no parent")),
                    };
                    let r = m.and_then(|m| {
                        let r = match root {
                            Some(n) => {
                                node(ecs, animator, &mut slots[2], n).ok_or_else(|| missing(n))
                            }
                            None => parent(m).ok_or_else(|| "the middle has no parent".into()),
                        };
                        r.map(|r| (m, r))
                    });
                    let (m, r) = match r {
                        Ok(v) => v,
                        Err(e) => {
                            report(e);
                            continue;
                        }
                    };
                    let (Some(target), true) = (target, weight > 0.0) else {
                        continue;
                    };
                    let pole = pole
                        .as_deref()
                        .and_then(|n| object(ecs, animator, &mut slots[3], n))
                        .map(|e| position(ecs, e));
                    two_bone(&mut w, r, m, t, target, pole, weight);
                }
                IkKind::LookAt {
                    head,
                    spine,
                    limit,
                    body,
                } => {
                    let Some(h) = node(ecs, animator, &mut slots[0], head) else {
                        report(missing(head));
                        continue;
                    };
                    let mut bones = Vec::with_capacity(spine.len());
                    let mut gone = None;
                    for (k, n) in spine.iter().enumerate().take(8) {
                        match node(ecs, animator, &mut slots[1 + k], n) {
                            Some(e) => bones.push(e),
                            None => gone = Some(n),
                        }
                    }
                    if let Some(n) = gone {
                        report(missing(n));
                        continue;
                    }
                    let up = Vec3::Y;
                    let forward = rotation(ecs, animator) * Vec3::NEG_Z;
                    let mut want = (0.0, 0.0);
                    if let (Some(target), true) = (target, weight > 0.0) {
                        let d = target - position(ecs, h);
                        let flat = |v: Vec3| (v - up * v.dot(up)).normalize_or_zero();
                        let (f, g) = (flat(forward), flat(d));
                        if d.length_squared() > 1e-8 && f != Vec3::ZERO {
                            let yaw = if g == Vec3::ZERO {
                                0.0
                            } else {
                                f.cross(g).dot(up).atan2(f.dot(g))
                            };
                            let pitch = d.normalize().dot(up).clamp(-1.0, 1.0).asin()
                                - forward.normalize().dot(up).clamp(-1.0, 1.0).asin();
                            want = (
                                yaw.clamp(-limit, *limit) * weight,
                                pitch.clamp(-limit, *limit) * weight,
                            );
                        }
                    }
                    let e = &mut ik.eased[i];
                    e[0] += (want.0 - e[0]) * ease;
                    e[1] += (want.1 - e[1]) * ease;
                    let (yaw, pitch) = (e[0], e[1]);
                    if yaw.abs() < 1e-5 && pitch.abs() < 1e-5 {
                        continue;
                    }
                    let right = forward.cross(up).normalize_or_zero();
                    let turn = |s: f32| {
                        Quat::from_axis_angle(up, yaw * s) * Quat::from_axis_angle(right, pitch * s)
                    };
                    let share = if bones.is_empty() {
                        0.0
                    } else {
                        body / bones.len() as f32
                    };
                    for b in bones {
                        w.rotate(b, turn(share));
                    }
                    let rest = if share > 0.0 { 1.0 - body } else { 1.0 };
                    w.rotate(h, turn(rest));
                }
                IkKind::FootPlacement {
                    hips,
                    left,
                    right,
                    reach,
                } => {
                    if !self.modules.physics {
                        report("foot placement needs the physics module; it is skipped".into());
                        continue;
                    }
                    let mut named = [None; 3];
                    let mut gone = None;
                    for (k, n) in [hips, left, right].into_iter().enumerate() {
                        named[k] = node(ecs, animator, &mut slots[k], n);
                        if named[k].is_none() {
                            gone = Some(n);
                        }
                    }
                    if let Some(n) = gone {
                        report(missing(n));
                        continue;
                    }
                    let [Some(hips), Some(left), Some(right)] = named else {
                        continue;
                    };
                    let parent = |e: Entity| ecs.get::<Parent>(e).ok().map(|p| p.0);
                    let legs = [left, right].map(|f| {
                        let knee = parent(f)?;
                        Some((parent(knee)?, knee, f))
                    });
                    let [Some(l), Some(r)] = legs else {
                        report("a foot needs a knee and a thigh above it".into());
                        continue;
                    };
                    let up = Vec3::Y;
                    let ground = position(ecs, animator).y;
                    let skip = body_of(ecs, animator);
                    let mut hits: Vec<Hit> = Vec::new();
                    let mut lifts = [0.0f32; 2];
                    let mut normals = [up; 2];
                    let feet = [l.2, r.2].map(|f| position(ecs, f));
                    for (k, p) in feet.iter().enumerate() {
                        hits.clear();
                        self.physics.query(
                            &QuerySpec {
                                kind: QueryKind::Raycast,
                                shape: ColliderShape::Box,
                                size: Vec3::ONE,
                                origin: Vec3::new(p.x, ground + reach, p.z),
                                rotation: Quat::IDENTITY,
                                direction: -up,
                                max_distance: reach * 2.0,
                                mask: u32::MAX,
                                exclude: skip,
                                max_hits: 1,
                                hit_sensors: false,
                            },
                            &mut hits,
                        );
                        if let Some(hit) = hits.first() {
                            lifts[k] = hit.point.y - ground;
                            normals[k] = hit.normal.normalize_or(up);
                        }
                    }
                    // The hips go down as far as the lower foot needs.
                    let e = &mut ik.eased[i];
                    let want = [lifts[0].min(lifts[1]).min(0.0), lifts[0], lifts[1]];
                    for k in 0..3 {
                        e[k] += (want[k] * weight - e[k]) * ease;
                    }
                    let [drop, lift_l, lift_r] = *e;
                    if drop.abs() < 1e-5 && lift_l.abs() < 1e-5 && lift_r.abs() < 1e-5 {
                        continue;
                    }
                    w.translate(hips, up * drop);
                    for ((leg, p), (lift, normal)) in [l, r]
                        .into_iter()
                        .zip(feet)
                        .zip([(lift_l, normals[0]), (lift_r, normals[1])])
                    {
                        let keep = rotation(ecs, leg.2);
                        two_bone(&mut w, leg.0, leg.1, leg.2, p + up * lift, None, 1.0);
                        let tilt = Quat::from_rotation_arc(up, normal);
                        let (axis, angle) = tilt.to_axis_angle();
                        let tilt = Quat::from_axis_angle(axis, angle.min(MAX_TILT) * weight);
                        w.set_rotation(leg.2, tilt * keep);
                    }
                }
            }
        }
        // What IK wrote, to take back next tick if no layer poses it.
        for (e, _, after) in touched.iter_mut() {
            if let Ok(now) = ecs.copy::<Transform>(*e) {
                *after = now;
            }
        }
        ik.touched = touched;
    }
}

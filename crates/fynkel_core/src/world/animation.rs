//! The animator and skin systems (M3-19, `docs/animation.md`): clip
//! playback onto the animators' subtrees in the §5.1 pre-tick slot —
//! before the catalogue's movers and physics, so a collider on a posed
//! joint is where the pose put it this tick — and the skins' joint
//! lookups, whose result the frame's extract turns into a matrix palette.
//!
//! Clip targets are cached on `AnimatorRun` and skin joints on `SkinRun`,
//! re-resolved when assets change or cached entities die or are renamed.
//! Authored pose/configuration candidates publish through the native data
//! journal; playback clocks and lookup caches remain animation-owned runtime.
//!
//! M3-19c adds layers (an animator under another applies after it, at
//! its `weight`, additively when asked), clip markers (`ANIMATION_MARKER`
//! per crossing) and root motion (the root channel's planar translation
//! and yaw taken out of the pose and handed to the entity's character
//! controller, or to its transform).

// Without the `animation` feature (a runtime built with `modules.strip`
// and the animation module off, ADR-0029) the system does not run, so
// the sampling it reaches is left out of the library; the tables, the
// commands' bookkeeping and the queries stay.
#![cfg_attr(not(feature = "animation"), allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::sync::Arc;

use fynkel_contracts::{AssetRef, ComponentId, DiagnosticCode, Entity, Tick};
use fynkel_ecs::internal::Handle;
use fynkel_ecs::World as Ecs;
use fynkel_physics::CharacterController;
use fynkel_scene::{self as scene, Children, Parent, Transform};
use glam::{Mat4, Quat, Vec3};

use super::{controller, Diagnostic, Name, World};
use crate::animation::{BlendSpace, Clip, Pose, Property, Skin};
use crate::command::{AnimateOp, Lane};
use crate::components::{Animator, ComponentValue, Skin as SkinComponent};
use crate::event::{Event, EventPayload};
use crate::schema::AnimatorStateRecord;

/// Runtime state of an `Animator` (never authored, never on the wire).
#[derive(Clone, Debug)]
pub struct AnimatorRun {
    /// Seconds into the clip.
    pub time: f32,
    /// The clip `targets` were resolved for.
    pub resolved: Option<AssetRef>,
    /// One entry per channel of the clip: the entity it drives.
    pub targets: Vec<Option<Entity>>,
    /// A crossfade in progress: the clip being blended out.
    pub fade: Option<Fade>,
    /// The clock was (re)set and its first tick has not run yet: that
    /// tick samples the start (`seek`, else 0 — the end for a negative
    /// speed) without advancing. True for a new animator, a `Play`, a
    /// `Crossfade` and a `SetTime`.
    pub fresh: bool,
    /// The time a `SetTime` asked for, taken by the next fresh tick.
    pub seek: Option<f32>,
    /// `CLIP_UNAVAILABLE` was reported for this clip.
    pub reported: Option<AssetRef>,
    /// The clip's first frame over `targets` (M3-19c): what an additive
    /// layer's delta is measured from. Resolved with the targets.
    pub reference: Pose,
    /// Blend-space parameters set on this animator (M3-19c), by name.
    pub parameters: Vec<(String, f32)>,
    /// The clip's root channels (M3-19c), resolved with the targets.
    pub root: Option<RootChannels>,
    /// Root motion taken out over the last tick, in the animator's parent
    /// space; zero without `root_motion` or when the clock did not move.
    pub root_delta: Vec3,
    /// Root yaw taken out over the last tick, radians.
    pub root_yaw: f32,
    /// The blend space's clips (M3-19c), one per point, resolved with
    /// the space; empty while a clip plays.
    pub blend: Vec<BlendSlot>,
    /// The blend's duration over the last tick: interpolated between the
    /// two active clips' (the clock runs over it; the clips are sampled
    /// at the same phase of their own).
    pub blend_duration: f32,
    /// The blend's active segment: the two points and the fraction.
    pub blend_segment: (usize, usize, f32),
    /// The Animator Controller it runs (ADR-0032), made on first use.
    pub controller: Option<super::controller::ControllerPlay>,
}

impl Default for AnimatorRun {
    fn default() -> Self {
        AnimatorRun {
            time: 0.0,
            resolved: None,
            targets: Vec::new(),
            fade: None,
            fresh: true,
            seek: None,
            reported: None,
            reference: Pose::default(),
            parameters: Vec::new(),
            root: None,
            root_delta: Vec3::ZERO,
            root_yaw: 0.0,
            blend: Vec::new(),
            blend_duration: 0.0,
            blend_segment: (0, 0, 0.0),
            controller: None,
        }
    }
}

/// One clip of a blend space as an animator resolved it (M3-19c).
#[derive(Clone, Debug)]
pub struct BlendSlot {
    pub clip_ref: AssetRef,
    pub clip: Arc<Clip>,
    /// One entry per channel of the clip: the entity it drives.
    pub targets: Vec<Option<Entity>>,
    pub root: Option<RootChannels>,
}

impl BlendSlot {
    pub(super) fn resolve(
        ecs: &Ecs,
        animator: Entity,
        clip_ref: AssetRef,
        clip: &Arc<Clip>,
    ) -> BlendSlot {
        let names: Vec<&str> = clip.channels.iter().map(|c| c.target.as_str()).collect();
        let targets = resolve_names(ecs, animator, &names);
        let root = RootChannels::resolve(ecs, animator, clip, &targets);
        BlendSlot {
            clip_ref,
            clip: clip.clone(),
            targets,
            root,
        }
    }

    pub(super) fn valid(&self, ecs: &Ecs) -> bool {
        let names: Vec<&str> = self
            .clip
            .channels
            .iter()
            .map(|c| c.target.as_str())
            .collect();
        targets_valid(ecs, &self.targets, &names)
    }
}

/// What an animator's `clip` resolved to this tick.
enum Playing {
    Clip(Arc<Clip>),
    Blend(Arc<BlendSpace>),
}

impl AnimatorRun {
    pub fn parameter(&self, name: &str) -> Option<f32> {
        self.parameters
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
    }

    pub fn set_parameter(&mut self, name: &str, value: f32) {
        match self.parameters.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = value,
            None => self.parameters.push((name.to_string(), value)),
        }
    }
}

/// The clip's root for root motion (M3-19c): the translation channel
/// whose target is the shallowest under the animator, and the rotation
/// channel of the same target. The root's motion is measured in its
/// parent's space, assumed Y-up: the planar (x, z) translation and the
/// twist about Y relative to the clip's first frame.
#[derive(Clone, Debug)]
pub struct RootChannels {
    pub entity: Entity,
    pub translation: usize,
    pub rotation: Option<usize>,
    /// The root's translation on the clip's first frame.
    pub start: Vec3,
    /// The root's rotation on the clip's first frame.
    pub start_rotation: Quat,
    /// Translation gained over one full lap (`T(duration) - T(0)`).
    pub lap: Vec3,
    /// Yaw gained over one full lap.
    pub lap_yaw: f32,
}

/// The twist of `q` about +Y, in `(-π, π]`; `0` when undefined.
fn yaw_of(q: Quat) -> f32 {
    let (y, w) = if q.w < 0.0 { (-q.y, -q.w) } else { (q.y, q.w) };
    if y.abs() < 1e-9 && w.abs() < 1e-9 {
        0.0
    } else {
        2.0 * y.atan2(w)
    }
}

impl RootChannels {
    fn resolve(
        ecs: &Ecs,
        animator: Entity,
        clip: &Clip,
        targets: &[Option<Entity>],
    ) -> Option<Self> {
        let mut best: Option<(usize, usize)> = None;
        for (i, (channel, target)) in clip.channels.iter().zip(targets).enumerate() {
            let Some(target) = target else {
                continue;
            };
            if channel.property != Property::Translation {
                continue;
            }
            // Hops from the animator down to the target.
            let mut depth = 0;
            let mut e = *target;
            while e != animator {
                let Ok(p) = ecs.get::<Parent>(e) else {
                    break;
                };
                e = p.0;
                depth += 1;
            }
            if best.is_none_or(|(_, d)| depth < d) {
                best = Some((i, depth));
            }
        }
        let (translation, _) = best?;
        let entity = targets[translation]?;
        let rotation = clip
            .channels
            .iter()
            .zip(targets)
            .position(|(c, t)| c.property == Property::Rotation && *t == Some(entity));
        let duration = clip.duration.max(0.0);
        let t = |time: f32| {
            let v = clip.channels[translation].sample(time);
            Vec3::new(v[0], v[1], v[2])
        };
        let r = |time: f32| {
            rotation
                .map(|c| {
                    let v = clip.channels[c].sample(time);
                    Quat::from_xyzw(v[0], v[1], v[2], v[3]).normalize()
                })
                .unwrap_or(Quat::IDENTITY)
        };
        let start = t(0.0);
        let start_rotation = r(0.0);
        Some(RootChannels {
            entity,
            translation,
            rotation,
            start,
            start_rotation,
            lap: t(duration) - start,
            lap_yaw: yaw_of(r(duration) * start_rotation.inverse()),
        })
    }

    /// The root's translation at an unwrapped `time` of a looping clock
    /// (`k` laps add `k` times the lap's gain) or a clamped one.
    fn translation_at(&self, clip: &Clip, time: f32, looping: bool) -> Vec3 {
        let duration = clip.duration.max(0.0);
        let sample = |t: f32| {
            let v = clip.channels[self.translation].sample(t);
            Vec3::new(v[0], v[1], v[2])
        };
        if !looping || duration <= 0.0 {
            sample(time.clamp(0.0, duration))
        } else {
            let k = (time / duration).floor();
            sample(time - k * duration) + self.lap * k
        }
    }

    /// The root's yaw relative to the first frame at an unwrapped `time`.
    fn yaw_at(&self, clip: &Clip, time: f32, looping: bool) -> f32 {
        let Some(c) = self.rotation else {
            return 0.0;
        };
        let duration = clip.duration.max(0.0);
        let sample = |t: f32| {
            let v = clip.channels[c].sample(t);
            yaw_of(
                Quat::from_xyzw(v[0], v[1], v[2], v[3]).normalize() * self.start_rotation.inverse(),
            )
        };
        if !looping || duration <= 0.0 {
            sample(time.clamp(0.0, duration))
        } else {
            let k = (time / duration).floor();
            sample(time - k * duration) + self.lap_yaw * k
        }
    }
}

/// Root motion out of a sampled pose (M3-19c): the root keeps its
/// first-frame place and heading — its height and the rest of its
/// rotation stay — so the motion reaches the entity as a delta instead.
pub(super) fn pin_root(pose: &mut Pose, clip: &Clip, root: &RootChannels, time: f32) {
    let yaw_now = root.yaw_at(clip, time, false);
    if let Some(e) = pose.entries.iter_mut().find(|e| e.entity == root.entity) {
        if let Some(t) = e.translation.as_mut() {
            t.x = root.start.x;
            t.z = root.start.z;
        }
        if let Some(q) = e.rotation.as_mut() {
            *q = (Quat::from_rotation_y(-yaw_now) * *q).normalize();
        }
    }
}

/// The root's planar motion and yaw between two unwrapped clocks, in
/// the animator's parent space (through the locals from the animator
/// down to the root's parent): `(delta, yaw axis, yaw)`.
pub(super) fn root_delta(
    ecs: &Ecs,
    animator: Entity,
    clip: &Clip,
    root: &RootChannels,
    prev: f32,
    next: f32,
    looping: bool,
) -> Option<(Vec3, Vec3, f32)> {
    let mut delta =
        root.translation_at(clip, next, looping) - root.translation_at(clip, prev, looping);
    delta.y = 0.0;
    let yaw = root.yaw_at(clip, next, looping) - root.yaw_at(clip, prev, looping);
    let mut chain = Vec::new();
    let mut e = root.entity;
    while e != animator {
        let Ok(p) = ecs.get::<Parent>(e) else {
            break;
        };
        e = p.0;
        chain.push(e);
    }
    let mut m = Mat4::IDENTITY;
    for e in chain.iter().rev() {
        if let Ok(t) = ecs.copy::<Transform>(*e) {
            m *= t.matrix();
        }
    }
    let delta = m.transform_vector3(delta);
    let axis = m.transform_vector3(Vec3::Y).normalize_or_zero();
    (delta.is_finite() && yaw.is_finite()).then_some((delta, axis, yaw))
}

/// The outgoing side of a crossfade: it keeps playing on its own clock
/// while its weight falls to zero.
#[derive(Clone, Debug)]
pub struct Fade {
    pub clip: Arc<Clip>,
    pub clip_ref: AssetRef,
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
    pub targets: Vec<Option<Entity>>,
    pub remaining: f32,
    pub total: f32,
}

/// Runtime state of a `Skin` component: the joints resolved to entities.
#[derive(Clone, Debug, Default)]
pub struct SkinRun {
    /// The skin `joints` were resolved for.
    pub resolved: Option<AssetRef>,
    /// The entity the joints were searched under.
    pub root: Option<Entity>,
    /// One per joint of the skin.
    pub joints: Vec<Option<Entity>>,
    /// `SKIN_UNAVAILABLE` was reported for this skin.
    pub reported: Option<AssetRef>,
}

/// The first entity in depth-first order under `root` (itself included)
/// named `name`.
pub(super) fn find_named(ecs: &Ecs, root: Entity, name: &str) -> Option<Entity> {
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if ecs.get::<Name>(e).is_ok_and(|n| n.value == name) {
            return Some(e);
        }
        if let Ok(children) = ecs.get::<Children>(e) {
            stack.extend(children.0.iter().rev().copied());
        }
    }
    None
}

/// Every name under `root` (itself included) resolved in one walk: the
/// first entity in depth-first order per name.
fn resolve_names(ecs: &Ecs, root: Entity, names: &[&str]) -> Vec<Option<Entity>> {
    let mut out = vec![None; names.len()];
    let mut left = names.len();
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if left == 0 {
            break;
        }
        if let Ok(n) = ecs.get::<Name>(e) {
            for (i, name) in names.iter().enumerate() {
                if out[i].is_none() && n.value == *name {
                    out[i] = Some(e);
                    left -= 1;
                }
            }
        }
        if let Ok(children) = ecs.get::<Children>(e) {
            stack.extend(children.0.iter().rev().copied());
        }
    }
    out
}

/// Whether every resolved target still lives under its name.
/// Every target still lives under its name; a channel that resolved to
/// nothing is looked up again (a node spawned later is picked up).
fn targets_valid(ecs: &Ecs, targets: &[Option<Entity>], names: &[&str]) -> bool {
    targets.len() == names.len()
        && targets.iter().zip(names).all(|(t, name)| match t {
            Some(e) => ecs.get::<Name>(*e).is_ok_and(|n| n.value == *name),
            None => false,
        })
}

fn tree_root(ecs: &Ecs, entity: Entity) -> Entity {
    let mut e = entity;
    while let Ok(p) = ecs.get::<Parent>(e) {
        e = p.0;
    }
    e
}

fn at_end(time: f32, speed: f32, duration: f32) -> bool {
    if speed < 0.0 {
        time <= 0.0
    } else {
        time >= duration
    }
}

/// One animator's tick, computed while the ECS is borrowed.
pub(super) struct AnimatorTick {
    entity: Entity,
    ended: Option<AssetRef>,
    pub(super) unavailable: Option<AssetRef>,
    /// Markers crossed this tick, in order: the clip and the name.
    pub(super) markers: Vec<(AssetRef, String)>,
    /// Root motion to apply: the delta and the yaw axis in the animator's
    /// parent space, the yaw in radians.
    pub(super) root: Option<(Vec3, Vec3, f32)>,
    /// A controller's state events (ADR-0032), in order: the layer,
    /// entered (else exited), the state and the state left.
    pub(super) states: Vec<(u32, bool, String, String)>,
    /// One-shots that reached their end (ADR-0033 § 2): each an
    /// `AnimationEnded`, the animator playing on.
    pub(super) one_shots_ended: Vec<AssetRef>,
}

/// Hops from the tree root down to `entity`.
fn depth_of(ecs: &Ecs, entity: Entity) -> u32 {
    let mut d = 0;
    let mut e = entity;
    while let Ok(p) = ecs.get::<Parent>(e) {
        e = p.0;
        d += 1;
    }
    d
}

impl World {
    /// Makes a clip available to `Animator`s that name `clip`
    /// (`docs/animation.md`; the loaders do this for every clip a loaded
    /// scene's animators name and the bridge for every `Play`).
    pub fn register_clip(&mut self, clip: AssetRef, data: Clip) {
        self.anim.clips.insert(clip, Arc::new(data));
    }

    pub fn has_clip(&self, clip: &AssetRef) -> bool {
        self.anim.clips.contains_key(clip)
    }

    pub fn clip(&self, clip: &AssetRef) -> Option<&Arc<Clip>> {
        self.anim.clips.get(clip)
    }

    /// A registered clip's root travel on `animator`'s hierarchy, per
    /// second of one lap: the root's translation (in its parent's space)
    /// and its yaw in radians — what a blend tree's Compute Positions
    /// reads (ADR-0033). The root is the one root motion takes. `None`
    /// when the clip is not registered, has no length or keys no
    /// translation under `animator`.
    pub fn clip_root_travel(&self, animator: Entity, clip: &AssetRef) -> Option<(Vec3, f32)> {
        let data = self.anim.clips.get(clip)?;
        if data.duration <= 0.0 {
            return None;
        }
        let root = BlendSlot::resolve(&self.ecs, animator, clip.clone(), data).root?;
        Some((root.lap / data.duration, root.lap_yaw / data.duration))
    }

    /// Makes a blend space available to `Animator`s that name `blend` as
    /// their clip (M3-19c); its points' clips are registered separately.
    pub fn register_blend(&mut self, blend: AssetRef, data: BlendSpace) {
        self.anim.blends.insert(blend, Arc::new(data));
    }

    pub fn has_blend(&self, blend: &AssetRef) -> bool {
        self.anim.blends.contains_key(blend)
    }

    pub fn blend(&self, blend: &AssetRef) -> Option<&Arc<BlendSpace>> {
        self.anim.blends.get(blend)
    }

    pub fn registered_blends(&self) -> impl Iterator<Item = &AssetRef> {
        self.anim.blends.keys()
    }

    /// Makes an Animator Controller available to `Animator`s that name it
    /// (ADR-0032); its motions' clips and blend spaces are registered
    /// separately. `Err` for a def the runtime cannot run.
    pub fn register_controller(
        &mut self,
        controller: AssetRef,
        data: crate::animation::controller::ControllerDef,
    ) -> Result<(), String> {
        data.validate()?;
        self.anim.controllers.insert(controller, Arc::new(data));
        Ok(())
    }

    pub fn has_controller(&self, controller: &AssetRef) -> bool {
        self.anim.controllers.contains_key(controller)
    }

    pub fn controller(
        &self,
        controller: &AssetRef,
    ) -> Option<&Arc<crate::animation::controller::ControllerDef>> {
        self.anim.controllers.get(controller)
    }

    pub fn registered_controllers(&self) -> impl Iterator<Item = &AssetRef> {
        self.anim.controllers.keys()
    }

    /// `FYNKEL_QUERY_ANIMATOR_CONTROLLER` (ADR-0032): the layers' states
    /// and the parameters of the controller the animator runs; `None`
    /// before its first tick with one, or without one.
    pub fn animator_controller_state(
        &self,
        entity: Entity,
    ) -> Option<(
        Vec<crate::schema::AnimatorLayerStateRecord>,
        Vec<crate::schema::AnimatorParameterRecord>,
    )> {
        let animator = self.ecs.get::<Animator>(entity).ok()?;
        let run = self.ecs.get::<AnimatorRun>(entity).ok()?;
        let play = run.controller.as_ref()?;
        (animator.controller.as_ref() == Some(&play.controller))
            .then(|| controller::state(entity, play))
    }

    /// Makes a skin available to `Skin` components that name it.
    pub fn register_skin(&mut self, skin: AssetRef, data: Skin) {
        self.anim.skins.insert(skin, Arc::new(data));
    }

    pub fn has_skin(&self, skin: &AssetRef) -> bool {
        self.anim.skins.contains_key(skin)
    }

    pub fn skin(&self, skin: &AssetRef) -> Option<&Arc<Skin>> {
        self.anim.skins.get(skin)
    }

    /// The clips and skins the world knows, for reports.
    pub fn registered_clips(&self) -> impl Iterator<Item = &AssetRef> {
        self.anim.clips.keys()
    }

    pub fn registered_skins(&self) -> impl Iterator<Item = &AssetRef> {
        self.anim.skins.keys()
    }

    /// Validate controller transport against its immutable definition.
    pub(super) fn validate_controller_transport(
        &self,
        controller_ref: &AssetRef,
        op: &AnimateOp,
    ) -> Result<(), String> {
        match self.anim.controllers.get(controller_ref) {
            Some(def) => controller::validate_transport(def, op),
            None => Ok(()), // Resource readiness is checked at execution, after prepared resources publish.
        }
    }

    pub(super) fn apply_animate_intent(
        &mut self,
        entity: Entity,
        op: AnimateOp,
        before: Animator,
        invalidated: bool,
    ) -> bool {
        if !self.module_allows(
            u32::MAX,
            Lane::Simulation,
            &crate::Command::Animate {
                entity,
                op: op.clone(),
            },
        ) {
            return false;
        }
        if invalidated {
            return self.reject(
                u32::MAX,
                Lane::Simulation,
                DiagnosticCode::MISSING_COMPONENT,
                entity,
                ComponentId::ANIMATOR,
                "animator was removed before request execution",
            );
        }
        if op.controller_transport()
            && self
                .ecs
                .get::<Animator>(entity)
                .is_ok_and(|a| a.controller != before.controller)
        {
            return self.reject(
                u32::MAX,
                Lane::Simulation,
                DiagnosticCode::INVALID_PAYLOAD,
                entity,
                ComponentId::ANIMATOR,
                "animator controller changed before request execution",
            );
        }
        self.apply_animate_control(u32::MAX, entity, op, before)
    }

    fn apply_animate_control(
        &mut self,
        index: u32,
        entity: Entity,
        op: AnimateOp,
        before: Animator,
    ) -> bool {
        let lane = Lane::Simulation;
        if !self.check_entity(index, lane, entity, "animate") {
            return false;
        }
        if !self.ecs.has::<Animator>(entity) {
            return self.reject(
                index,
                lane,
                DiagnosticCode::MISSING_COMPONENT,
                entity,
                ComponentId::ANIMATOR,
                "entity has no fynkel.Animator",
            );
        }
        let bad = |what: &str| format!("animate: {what}");
        if let Err(message) = op.validate_clip_transport() {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_PAYLOAD,
                entity,
                ComponentId::ANIMATOR,
                bad(&message),
            );
        }
        if let Err(message) = op.validate_controller_transport() {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_PAYLOAD,
                entity,
                ComponentId::ANIMATOR,
                bad(&message),
            );
        }
        match &op {
            AnimateOp::PlayOneShot { motion, .. }
                if !self.anim.clips.contains_key(motion)
                    && !self.anim.blends.contains_key(motion) =>
            {
                return self.reject(
                    index,
                    lane,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::ANIMATOR,
                    bad(&format!(
                        "the one-shot's motion {motion} is not a clip or blend space the world has"
                    )),
                );
            }
            _ => {}
        }
        // ADR-0032: a controller owns what plays; the clip ops are for an
        // animator without one, the state and parameter ops for one with.
        let controlled = self
            .ecs
            .get::<Animator>(entity)
            .ok()
            .and_then(|a| a.controller.clone())
            .filter(|c| !c.is_nil());
        let clip_op = op.requires_uncontrolled();
        let controller_op = matches!(
            op,
            AnimateOp::SetControllerParameter { .. }
                | AnimateOp::PlayState { .. }
                | AnimateOp::PlayOneShot { .. }
                | AnimateOp::StopOneShot { .. }
                | AnimateOp::SetIkTarget { .. }
                | AnimateOp::ClearIkTarget { .. }
        );
        if clip_op && controlled.is_some() {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_PAYLOAD,
                entity,
                ComponentId::ANIMATOR,
                bad("the animator runs a controller: play its states by name and set its parameters (setFloat, setInteger, setBool, setTrigger)"),
            );
        }
        if !self.ecs.has::<AnimatorRun>(entity) {
            let _ = self.ecs.insert(entity, AnimatorRun::default());
        }
        if controller_op {
            let Some(c) = controlled else {
                return self.reject(
                    index,
                    lane,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::ANIMATOR,
                    bad(if matches!(op, AnimateOp::PlayOneShot { .. } | AnimateOp::StopOneShot { .. }) {
                        "the animator has no controller: a one-shot plays in a controller's slot (play the clip instead)"
                    } else {
                        "the animator has no controller"
                    }),
                );
            };
            let result = {
                let Ok(mut run) = self.ecs.get_mut::<AnimatorRun>(entity) else {
                    return false;
                };
                match controller::play_for(&self.anim.controllers, &c, &mut run.controller) {
                    Err(missing) => Err(format!(
                        "controller {missing} is not registered with the world"
                    )),
                    Ok(play) => match op {
                        AnimateOp::SetControllerParameter { name, value } => play
                            .run
                            .set(&play.def, &name, value)
                            .map_err(|e| e.to_string()),
                        AnimateOp::PlayState {
                            state,
                            layer,
                            seconds,
                            offset,
                        } => controller::state_index(&play.def, &state, layer).map(|(l, s)| {
                            play.run
                                .cross_fade(l, s, seconds, offset, &mut play.pending);
                        }),
                        AnimateOp::PlayOneShot {
                            motion,
                            slot,
                            blend_in,
                            blend_out,
                            speed,
                        } => play
                            .slot_index(&slot)
                            .map(|l| play.play_one_shot(l, motion, blend_in, blend_out, speed)),
                        AnimateOp::StopOneShot { slot, blend_out } => play
                            .slot_index(&slot)
                            .map(|l| play.stop_one_shot(l, blend_out)),
                        AnimateOp::SetIkTarget {
                            goal,
                            position,
                            weight,
                        } => play
                            .ik_index(&goal)
                            .map(|i| play.ik.targets[i] = Some((position, weight))),
                        AnimateOp::ClearIkTarget { goal } => {
                            play.ik_index(&goal).map(|i| play.ik.targets[i] = None)
                        }
                        _ => unreachable!("a controller op"),
                    },
                }
            };
            return match result {
                Ok(()) => true,
                Err(why) => self.reject(
                    index,
                    lane,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::ANIMATOR,
                    bad(&why),
                ),
            };
        }
        let committed = self.ecs.copy::<Animator>(entity).expect("checked animator");
        let target_clip = committed.clip;
        let animator = before;
        let Ok(mut run) = self.ecs.get_mut::<AnimatorRun>(entity) else {
            return false;
        };
        match op {
            AnimateOp::Play { .. } => {
                run.time = 0.0;
                run.fresh = true;
                run.seek = None;
                run.fade = None;
            }
            AnimateOp::Crossfade { seconds, .. } => {
                // The clip playing so far becomes the outgoing side, with
                // the targets it had resolved; nothing to fade from when
                // it never resolved (or the fade is instant).
                // A blend space fades out as its dominant clip at that
                // clip's own time.
                let fade_speed = if animator.playing {
                    animator.speed
                } else {
                    0.0
                };
                let resolved = seconds > 0.0
                    && run.resolved.as_ref() == Some(&animator.clip)
                    && target_clip != animator.clip;
                let outgoing = if !resolved {
                    None
                } else if let Some(data) = self.anim.clips.get(&animator.clip) {
                    Some(Fade {
                        clip: data.clone(),
                        clip_ref: animator.clip.clone(),
                        time: run.time,
                        speed: fade_speed,
                        looping: animator.looping,
                        targets: std::mem::take(&mut run.targets),
                        remaining: seconds,
                        total: seconds,
                    })
                } else if !run.blend.is_empty() {
                    let (i, j, t) = run.blend_segment;
                    let k = if t < 0.5 { i } else { j };
                    let scale = if run.blend_duration > 0.0 {
                        run.blend[k].clip.duration.max(0.0) / run.blend_duration
                    } else {
                        0.0
                    };
                    let time = run.time * scale;
                    let slot = &mut run.blend[k];
                    Some(Fade {
                        clip: slot.clip.clone(),
                        clip_ref: slot.clip_ref.clone(),
                        time,
                        speed: fade_speed,
                        looping: animator.looping,
                        targets: std::mem::take(&mut slot.targets),
                        remaining: seconds,
                        total: seconds,
                    })
                } else {
                    None
                };
                run.time = 0.0;
                run.fresh = true;
                run.seek = None;
                run.fade = outgoing;
                run.resolved = None;
            }
            AnimateOp::Stop => {
                run.time = 0.0;
                run.fresh = false;
                run.seek = None;
                run.fade = None;
            }
            AnimateOp::Pause
            | AnimateOp::Resume
            | AnimateOp::SetSpeed(_)
            | AnimateOp::SetWeight(_) => {
                unreachable!("configuration publishes through the gameplay journal")
            }
            AnimateOp::SetTime(t) => {
                run.fresh = true;
                run.seek = Some(t);
            }
            AnimateOp::SetParameter { name, value } => run.set_parameter(&name, value),
            AnimateOp::SetControllerParameter { .. }
            | AnimateOp::PlayState { .. }
            | AnimateOp::PlayOneShot { .. }
            | AnimateOp::StopOneShot { .. }
            | AnimateOp::SetIkTarget { .. }
            | AnimateOp::ClearIkTarget { .. } => {}
        }
        drop(run);
        true
    }

    /// Every `Animator` advances its clock and writes its clip's pose over
    /// its subtree (a crossfade blends the outgoing clip's pose out);
    /// every `Skin` keeps its joint lookups current. Returns animators +
    /// skins ticked.
    ///
    /// Animators apply ancestor-first (depth, then entity order), so one
    /// under another is a layer over it: it sees the base pose in the
    /// transforms it blends over at its `weight` (or adds its delta to).
    #[cfg(not(feature = "animation"))]
    pub(super) fn run_animation(&mut self, _tick: Tick) -> u32 {
        0
    }

    #[cfg(feature = "animation")]
    pub(super) fn run_animation(&mut self, tick: Tick) -> u32 {
        self.attach_state::<Animator, AnimatorRun>();
        self.attach_state::<SkinComponent, SkinRun>();
        let dt = self.config.fixed_dt;
        let mut ticks = std::mem::take(&mut self.anim.animators);
        ticks.clear();
        let mut count = 0;

        let mut animators = std::mem::take(&mut self.anim.order);
        animators.clear();
        animators.extend(
            self.ecs
                .hecs()
                .query::<&Handle>()
                .with::<&Animator>()
                .without::<&scene::Inactive>()
                .iter()
                .map(|h| (depth_of(&self.ecs, h.0), h.0)),
        );
        animators.sort_unstable();
        let mut pose = std::mem::take(&mut self.anim.pose);
        let mut fade_pose = std::mem::take(&mut self.anim.fade_pose);
        let mut layer_pose = std::mem::take(&mut self.anim.layer_pose);
        let mut crossed: Vec<usize> = Vec::new();
        let mut transforms = Vec::new();
        for &(_, entity) in &animators {
            count += 1;
            let Ok(mut animator) = self.ecs.copy::<Animator>(entity) else {
                continue;
            };
            let previous_animator = animator.clone();
            let Ok(mut run) = self.ecs.get_mut::<AnimatorRun>(entity) else {
                continue;
            };
            let mut out = AnimatorTick {
                entity,
                ended: None,
                unavailable: None,
                markers: Vec::new(),
                root: None,
                states: Vec::new(),
                one_shots_ended: Vec::new(),
            };
            pose.clear();
            run.root_delta = Vec3::ZERO;
            run.root_yaw = 0.0;
            // ADR-0032: an animator with a controller runs its state
            // machine; its own clip is not used.
            let controlled = animator.controller.clone().filter(|c| !c.is_nil());
            if let Some(c) = &controlled {
                let run = &mut *run;
                match controller::play_for(&self.anim.controllers, c, &mut run.controller) {
                    Ok(play) => {
                        let tables = controller::Tables {
                            ecs: &self.ecs,
                            clips: &self.anim.clips,
                            blends: &self.anim.blends,
                        };
                        controller::tick(
                            &tables,
                            entity,
                            &animator,
                            play,
                            dt,
                            &mut pose,
                            &mut fade_pose,
                            &mut layer_pose,
                            &mut out,
                        );
                    }
                    Err(missing) => out.unavailable = Some(missing),
                }
                // Reported once per missing asset, like a clip.
                if let Some(m) = out.unavailable.take() {
                    if run.reported.as_ref() != Some(&m) {
                        run.reported = Some(m.clone());
                        out.unavailable = Some(m);
                    }
                }
                if let Some((delta, _, yaw)) = out.root {
                    run.root_delta = delta;
                    run.root_yaw = yaw;
                }
                run.fade = None;
            } else {
                run.controller = None;
            }
            let clip_ref = animator.clip.clone();
            let playing = if controlled.is_some() || clip_ref.is_nil() {
                None
            } else if let Some(c) = self.anim.clips.get(&clip_ref) {
                Some(Playing::Clip(c.clone()))
            } else if let Some(b) = self.anim.blends.get(&clip_ref) {
                Some(Playing::Blend(b.clone()))
            } else {
                if run.reported.as_ref() != Some(&clip_ref) {
                    run.reported = Some(clip_ref.clone());
                    out.unavailable = Some(clip_ref.clone());
                }
                None
            };
            // A clip (or space) written onto the component starts from
            // its beginning like a `Play` would.
            let switched = |run: &mut AnimatorRun| {
                if run.resolved.is_some() && run.resolved.as_ref() != Some(&clip_ref) {
                    run.fresh = true;
                    run.seek = None;
                }
            };
            // What plays this tick and over how long a clock.
            let mut duration = 0.0;
            let mut ready = false;
            match &playing {
                Some(Playing::Clip(clip)) => {
                    // Targets: cached while every one still lives under its name.
                    let names: Vec<&str> =
                        clip.channels.iter().map(|c| c.target.as_str()).collect();
                    if run.resolved.as_ref() != Some(&clip_ref)
                        || !targets_valid(&self.ecs, &run.targets, &names)
                    {
                        switched(&mut run);
                        run.blend.clear();
                        run.targets = resolve_names(&self.ecs, entity, &names);
                        run.resolved = Some(clip_ref.clone());
                        let targets = std::mem::take(&mut run.targets);
                        let mut reference = std::mem::take(&mut run.reference);
                        reference.clear();
                        clip.sample(0.0, &targets, &mut reference);
                        run.reference = reference;
                        run.root = RootChannels::resolve(&self.ecs, entity, clip, &targets);
                        run.targets = targets;
                    }
                    duration = clip.duration.max(0.0);
                    ready = true;
                }
                Some(Playing::Blend(space)) => {
                    // Every point's clip resolved under the animator; a
                    // point whose clip is not registered is reported
                    // like a missing clip and the space waits for it.
                    if run.resolved.as_ref() != Some(&clip_ref)
                        || run.blend.len() != space.points.len()
                        || run.blend.iter().any(|s| !s.valid(&self.ecs))
                    {
                        switched(&mut run);
                        let mut slots = Vec::with_capacity(space.points.len());
                        let mut missing = None;
                        for p in &space.points {
                            match self.anim.clips.get(&p.clip) {
                                Some(c) => slots.push(BlendSlot::resolve(
                                    &self.ecs,
                                    entity,
                                    p.clip.clone(),
                                    c,
                                )),
                                None => {
                                    missing = Some(p.clip.clone());
                                    break;
                                }
                            }
                        }
                        run.blend.clear();
                        run.targets.clear();
                        run.root = None;
                        run.blend_duration = 0.0;
                        match missing {
                            Some(m) => {
                                run.resolved = None;
                                if run.reported.as_ref() != Some(&m) {
                                    run.reported = Some(m.clone());
                                    out.unavailable = Some(m);
                                }
                            }
                            None => {
                                let mut reference = std::mem::take(&mut run.reference);
                                reference.clear();
                                if let Some(first) = slots.first() {
                                    first.clip.sample(0.0, &first.targets, &mut reference);
                                }
                                run.reference = reference;
                                run.blend = slots;
                                run.resolved = Some(clip_ref.clone());
                            }
                        }
                    }
                    if !run.blend.is_empty() {
                        let value = run
                            .parameter(&space.parameter)
                            .unwrap_or(space.points[0].at);
                        let (i, j, t) = space.segment(value);
                        let d = (run.blend[i].clip.duration.max(0.0) * (1.0 - t)
                            + run.blend[j].clip.duration.max(0.0) * t)
                            .max(0.0);
                        // The clock keeps its phase when the parameter
                        // changes the blend's length.
                        if !run.fresh && run.blend_duration > 0.0 && d != run.blend_duration {
                            run.time = run.time * d / run.blend_duration;
                        }
                        run.blend_duration = d;
                        run.blend_segment = (i, j, t);
                        duration = d;
                        ready = true;
                    }
                }
                None => {}
            }
            if ready {
                let prev = run.time;
                let mut advanced = false;
                let mut next_raw = run.time;
                if run.fresh {
                    run.fresh = false;
                    run.time = match run.seek.take() {
                        Some(t) => t.min(duration),
                        None if animator.speed < 0.0 => duration,
                        None => 0.0,
                    };
                    // A fresh non-looping clip of no length ends at once.
                    if !animator.looping && duration == 0.0 && animator.playing {
                        out.ended = Some(clip_ref.clone());
                    }
                } else if animator.playing {
                    let was_at_end = at_end(run.time, animator.speed, duration);
                    next_raw = run.time + animator.speed * dt;
                    if animator.looping {
                        run.time = if duration > 0.0 {
                            next_raw.rem_euclid(duration)
                        } else {
                            0.0
                        };
                    } else {
                        run.time = next_raw.clamp(0.0, duration);
                        next_raw = run.time;
                        if !was_at_end && at_end(run.time, animator.speed, duration) {
                            out.ended = Some(clip_ref.clone());
                        }
                    }
                    advanced = next_raw != prev;
                }
                let looping = animator.looping;
                let root_motion = animator.root_motion;
                match &playing {
                    Some(Playing::Clip(clip)) => {
                        clip.sample(run.time, &run.targets, &mut pose);
                        if advanced {
                            // Markers: every crossing between the two clocks.
                            crossed.clear();
                            clip.markers_crossed(prev, next_raw, looping, &mut crossed);
                            for &m in &crossed {
                                out.markers
                                    .push((clip_ref.clone(), clip.markers[m].name.clone()));
                            }
                        }
                        if root_motion {
                            if let Some(root) = &run.root {
                                pin_root(&mut pose, clip, root, run.time);
                                if advanced {
                                    if let Some((delta, axis, yaw)) = root_delta(
                                        &self.ecs, entity, clip, root, prev, next_raw, looping,
                                    ) {
                                        run.root_delta = delta;
                                        run.root_yaw = yaw;
                                        out.root = Some((delta, axis, yaw));
                                    }
                                }
                            }
                        }
                    }
                    Some(Playing::Blend(_)) => {
                        // Phase sync: the clock's fraction of the blended
                        // duration is each clip's fraction of its own.
                        let (i, j, t) = run.blend_segment;
                        let d = run.blend_duration;
                        let phase = if d > 0.0 { run.time / d } else { 0.0 };
                        let scale = |k: usize| {
                            if d > 0.0 {
                                run.blend[k].clip.duration.max(0.0) / d
                            } else {
                                0.0
                            }
                        };
                        let (si, sj) = (scale(i), scale(j));
                        let two = j != i && t > 0.0;
                        {
                            let a = &run.blend[i];
                            a.clip
                                .sample(phase * a.clip.duration.max(0.0), &a.targets, &mut pose);
                            if root_motion {
                                if let Some(root) = &a.root {
                                    pin_root(&mut pose, &a.clip, root, phase * a.clip.duration);
                                }
                            }
                        }
                        if two {
                            let b = &run.blend[j];
                            fade_pose.clear();
                            b.clip.sample(
                                phase * b.clip.duration.max(0.0),
                                &b.targets,
                                &mut fade_pose,
                            );
                            if root_motion {
                                if let Some(root) = &b.root {
                                    pin_root(
                                        &mut fade_pose,
                                        &b.clip,
                                        root,
                                        phase * b.clip.duration,
                                    );
                                }
                            }
                            pose.blend(&fade_pose, t);
                        }
                        if advanced {
                            // Markers come from the dominant clip, on its
                            // own clock.
                            let k = if t < 0.5 { i } else { j };
                            let sk = scale(k);
                            let dominant = &run.blend[k];
                            crossed.clear();
                            dominant.clip.markers_crossed(
                                prev * sk,
                                next_raw * sk,
                                looping,
                                &mut crossed,
                            );
                            for &m in &crossed {
                                out.markers.push((
                                    dominant.clip_ref.clone(),
                                    dominant.clip.markers[m].name.clone(),
                                ));
                            }
                            if root_motion {
                                // The two clips' travel, blended like the pose.
                                let motion = |k: usize, sk: f32| {
                                    let s = &run.blend[k];
                                    s.root.as_ref().and_then(|root| {
                                        root_delta(
                                            &self.ecs,
                                            entity,
                                            &s.clip,
                                            root,
                                            prev * sk,
                                            next_raw * sk,
                                            looping,
                                        )
                                    })
                                };
                                let a = motion(i, si);
                                let b = if two { motion(j, sj) } else { None };
                                let blended = match (a, b) {
                                    (Some((da, axis, ya)), Some((db, _, yb))) => {
                                        Some((da.lerp(db, t), axis, ya + (yb - ya) * t))
                                    }
                                    (Some(a), None) if !two => Some(a),
                                    (Some((da, axis, ya)), None) => {
                                        Some((da * (1.0 - t), axis, ya * (1.0 - t)))
                                    }
                                    (None, Some((db, axis, yb))) => Some((db * t, axis, yb * t)),
                                    (None, None) => None,
                                };
                                if let Some((delta, axis, yaw)) = blended {
                                    run.root_delta = delta;
                                    run.root_yaw = yaw;
                                    out.root = Some((delta, axis, yaw));
                                }
                            }
                        }
                    }
                    None => {}
                }
            }
            if out.ended.is_some() {
                animator.playing = false;
            }
            // The outgoing clip keeps its clock and fades out.
            if let Some(fade) = &mut run.fade {
                fade.remaining -= dt;
                if fade.remaining <= 0.0 || playing.is_none() {
                    run.fade = None;
                } else {
                    let duration = fade.clip.duration.max(0.0);
                    let next = fade.time + fade.speed * dt;
                    fade.time = if fade.looping {
                        if duration > 0.0 {
                            next.rem_euclid(duration)
                        } else {
                            0.0
                        }
                    } else {
                        next.clamp(0.0, duration)
                    };
                    fade_pose.clear();
                    fade.clip.sample(fade.time, &fade.targets, &mut fade_pose);
                    let weight = 1.0 - fade.remaining / fade.total;
                    fade_pose.blend(&pose, weight);
                    std::mem::swap(&mut pose, &mut fade_pose);
                }
            }
            let weight = animator.weight.clamp(0.0, 1.0);
            let additive = animator.additive;
            if additive {
                // The layer's delta from its first frame over what the
                // transforms hold now (the base pose of this tick).
                fade_pose.clear();
                fade_pose.add_delta(&pose, &run.reference, weight, &mut |e| {
                    self.ecs.copy::<Transform>(e).ok()
                });
                std::mem::swap(&mut pose, &mut fade_pose);
            }
            drop(run);
            transforms.clear();
            if weight > 0.0 {
                for entry in &pose.entries {
                    let Ok(current) = self.ecs.copy::<Transform>(entry.entity) else {
                        continue;
                    };
                    let target = entry.apply_to(current);
                    let next = if weight >= 1.0 || additive {
                        target
                    } else {
                        Transform {
                            position: current.position.lerp(target.position, weight),
                            rotation: current.rotation.slerp(target.rotation, weight).normalize(),
                            scale: current.scale.lerp(target.scale, weight),
                        }
                    };
                    if next != current {
                        transforms.push((entry.entity, next));
                    }
                }
            }
            // Playback clocks/controller transitions and their events belong to
            // completed runtime evaluation. A rejected authored pose does not
            // rewind that evaluation; only pose/config data is atomic here.
            // Root-motion dispatch is suppressed when pose publication fails.
            let published = if transforms.is_empty() && animator == previous_animator {
                true
            } else {
                match self.edit_native_data_at_tick(tick, |edit| {
                    for &(target, transform) in &transforms {
                        edit.set_transform(target, transform)?;
                    }
                    if animator != previous_animator {
                        edit.set_component(entity, ComponentValue::Animator(animator))?;
                    }
                    Ok(())
                }) {
                    Ok(_) => true,
                    Err(error) => {
                        self.reject(
                            u32::MAX,
                            Lane::Simulation,
                            DiagnosticCode::INVALID_PAYLOAD,
                            entity,
                            ComponentId::ANIMATOR,
                            format!("animation pose: {error:?}"),
                        );
                        out.root = None;
                        false
                    }
                }
            };
            if published && controlled.is_some() {
                self.run_ik(entity, dt);
            }
            ticks.push(out);
        }
        pose.clear();
        fade_pose.clear();
        layer_pose.clear();
        self.anim.pose = pose;
        self.anim.fade_pose = fade_pose;
        self.anim.layer_pose = layer_pose;
        self.anim.order = animators;

        for t in ticks.drain(..) {
            if let Some(clip) = t.unavailable {
                self.diagnostics.push(Diagnostic {
                    command_index: u32::MAX,
                    lane: Lane::Simulation,
                    code: DiagnosticCode::CLIP_UNAVAILABLE,
                    entity: t.entity,
                    component: ComponentId::ANIMATOR,
                    message: format!(
                        "animator: {what} {clip} is not registered with the world (not in the loaded scene's closure?)",
                        what = if self
                            .ecs
                            .get::<Animator>(t.entity)
                            .is_ok_and(|a| a.controller.as_ref() == Some(&clip))
                        {
                            "controller"
                        } else {
                            "clip"
                        }
                    ),
                });
            }
            for (clip, name) in t.markers {
                self.events.push(Event {
                    tick,
                    payload: EventPayload::AnimationMarker {
                        entity: t.entity,
                        clip,
                        name,
                    },
                });
            }
            for (layer, entered, state, from) in t.states {
                self.events.push(Event {
                    tick,
                    payload: if entered {
                        EventPayload::AnimatorStateEntered {
                            entity: t.entity,
                            layer,
                            state,
                            from,
                        }
                    } else {
                        EventPayload::AnimatorStateExited {
                            entity: t.entity,
                            layer,
                            state,
                        }
                    },
                });
            }
            for clip in t.one_shots_ended {
                self.events.push(Event {
                    tick,
                    payload: EventPayload::AnimationEnded {
                        entity: t.entity,
                        clip,
                    },
                });
            }
            if let Some(clip) = t.ended {
                self.events.push(Event {
                    tick,
                    payload: EventPayload::AnimationEnded {
                        entity: t.entity,
                        clip,
                    },
                });
            }
            if let Some((delta, axis, yaw)) = t.root {
                self.apply_root_motion(t.entity, delta, axis, yaw, dt);
            }
        }
        self.anim.animators = ticks;

        count += self.run_skins();
        count
    }

    /// Root motion of one animator (M3-19c): a `CharacterController` on
    /// the entity walks by it this tick (the delta as a world velocity on
    /// top of its `Move`; the controller faces its motion, so the yaw is
    /// reported, not applied), else the entity's local transform moves
    /// and turns by it.
    fn apply_root_motion(&mut self, entity: Entity, delta: Vec3, axis: Vec3, yaw: f32, dt: f32) {
        if self.ecs.has::<CharacterController>(entity) {
            if dt <= 0.0 {
                return;
            }
            let parent_rotation = self
                .ecs
                .get::<Parent>(entity)
                .ok()
                .and_then(|p| scene::world_rotation(&self.ecs, p.0).ok())
                .unwrap_or(Quat::IDENTITY);
            let velocity = parent_rotation * delta / dt;
            if !velocity.is_finite() {
                self.reject(
                    u32::MAX,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::ANIMATOR,
                    "animation root motion produced non-finite character velocity",
                );
                return;
            }
            self.physics.add_root_motion(entity, velocity);
            return;
        }
        let Ok(current) = self.ecs.copy::<Transform>(entity) else {
            return;
        };
        let rotation = if yaw != 0.0 && axis != Vec3::ZERO {
            (Quat::from_axis_angle(axis, yaw) * current.rotation).normalize()
        } else {
            current.rotation
        };
        let next = Transform {
            position: current.position + delta,
            rotation,
            scale: current.scale,
        };
        if next != current {
            if let Err(error) =
                self.edit_native_data_at_tick(self.tick(), |edit| edit.set_transform(entity, next))
            {
                self.reject(
                    u32::MAX,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::ANIMATOR,
                    format!("animation root motion: {error:?}"),
                );
            }
        }
    }

    /// Every `Skin` component's joints resolved to entities under its
    /// skeleton root, cached while they hold.
    fn run_skins(&mut self) -> u32 {
        let mut skinned = std::mem::take(&mut self.scratch.entities);
        skinned.clear();
        skinned.extend(
            self.ecs
                .hecs()
                .query::<&Handle>()
                .with::<&SkinComponent>()
                .without::<&scene::Inactive>()
                .iter()
                .map(|h| h.0),
        );
        skinned.sort_unstable();
        let mut unavailable: Vec<(Entity, AssetRef)> = Vec::new();
        let mut count = 0;
        for &entity in &skinned {
            count += 1;
            let Ok(component) = self.ecs.get::<SkinComponent>(entity) else {
                continue;
            };
            let Ok(mut run) = self.ecs.get_mut::<SkinRun>(entity) else {
                continue;
            };
            let skin_ref = component.skin.clone();
            let Some(skin) = self.anim.skins.get(&skin_ref) else {
                if !skin_ref.is_nil() && run.reported.as_ref() != Some(&skin_ref) {
                    run.reported = Some(skin_ref.clone());
                    unavailable.push((entity, skin_ref));
                }
                run.resolved = None;
                run.joints.clear();
                continue;
            };
            let names: Vec<&str> = skin.joints.iter().map(|j| j.name.as_str()).collect();
            // The skeleton root is the referenced object (ADR-0035), the
            // tree root when None; a destroyed one leaves the bind pose.
            let expected = match component.skeleton.get() {
                None => Some(tree_root(&self.ecs, entity)),
                Some(r) => self.ecs.has::<Transform>(r).then_some(r),
            };
            let root_ok = run.root.is_some() && run.root == expected;
            if run.resolved.as_ref() != Some(&skin_ref)
                || !root_ok
                || !targets_valid(&self.ecs, &run.joints, &names)
            {
                let root = expected;
                run.root = root;
                run.joints = match root {
                    Some(r) => resolve_names(&self.ecs, r, &names),
                    None => vec![None; names.len()],
                };
                run.resolved = Some(skin_ref);
            }
        }
        self.scratch.entities = skinned;
        for (entity, skin) in unavailable {
            self.diagnostics.push(Diagnostic {
                command_index: u32::MAX,
                lane: Lane::Simulation,
                code: DiagnosticCode::SKIN_UNAVAILABLE,
                entity,
                component: ComponentId::SKIN,
                message: format!(
                    "skin: {skin} is not registered with the world (not in the loaded scene's closure?); the mesh draws in its bind pose"
                ),
            });
        }
        count
    }

    /// The joints a skinned mesh entity resolved (`None` without a
    /// registered skin): the skin and one entity per joint.
    pub(super) fn skin_joints(&self, entity: Entity) -> Option<(Arc<Skin>, Vec<Option<Entity>>)> {
        let component = self.ecs.get::<SkinComponent>(entity).ok()?;
        let run = self.ecs.get::<SkinRun>(entity).ok()?;
        if run.resolved.as_ref() != Some(&component.skin) {
            return None;
        }
        let skin = self.anim.skins.get(&component.skin)?.clone();
        Some((skin, run.joints.clone()))
    }

    /// `FYNKEL_QUERY_ANIMATOR`: the animator's playback state, `None`
    /// without an `Animator`.
    pub fn animator_state(&self, entity: Entity) -> Option<AnimatorStateRecord> {
        let animator = self.ecs.get::<Animator>(entity).ok()?;
        let blend = self.anim.blends.get(&animator.clip);
        let (time, fade_remaining, parameter, root_delta, root_yaw, blend_duration) =
            match self.ecs.get::<AnimatorRun>(entity) {
                Ok(run) => (
                    run.time,
                    run.fade
                        .as_ref()
                        .map(|f| f.remaining.max(0.0))
                        .unwrap_or(0.0),
                    // The parameter the playing blend space reads; the
                    // last one set for a clip.
                    match blend {
                        Some(space) => run.parameter(&space.parameter).unwrap_or(0.0),
                        None => run.parameters.last().map(|(_, v)| *v).unwrap_or(0.0),
                    },
                    run.root_delta,
                    run.root_yaw,
                    if run.blend.is_empty() {
                        None
                    } else {
                        Some(run.blend_duration)
                    },
                ),
                Err(_) => (0.0, 0.0, 0.0, Vec3::ZERO, 0.0, None),
            };
        Some(AnimatorStateRecord {
            entity,
            time,
            // A blend space's length is the blended one of its two
            // active clips.
            duration: self
                .anim
                .clips
                .get(&animator.clip)
                .map(|c| c.duration)
                .or(blend_duration)
                .unwrap_or(0.0),
            speed: animator.speed,
            fade_remaining,
            playing: animator.playing,
            looping: animator.looping,
            clip: animator.clip.clone(),
            weight: animator.weight,
            parameter,
            root_delta,
            root_yaw,
        })
    }

    /// The matrix palette of a skinned mesh for the frame: per joint,
    /// mesh space → posed joint → back to mesh space, from the joints'
    /// world matrices as `world_of` resolves them (identity for a joint
    /// that did not resolve: its bind pose).
    pub(super) fn skin_palette(
        mesh_world: Mat4,
        skin: &Skin,
        joints: &[Option<Entity>],
        world_of: &mut dyn FnMut(Entity) -> Option<Mat4>,
        out: &mut Vec<Mat4>,
    ) {
        let inverse_mesh = mesh_world.inverse();
        for (joint, entity) in skin.joints.iter().zip(joints) {
            let m = entity
                .and_then(&mut *world_of)
                .map(|w| inverse_mesh * w * joint.inverse_bind)
                .unwrap_or(Mat4::IDENTITY);
            out.push(m);
        }
    }
}

/// Registered clips, blend spaces and skins by reference.
pub(super) type ClipTable = HashMap<AssetRef, Arc<Clip>>;
pub(super) type BlendTable = HashMap<AssetRef, Arc<BlendSpace>>;
pub(super) type SkinTable = HashMap<AssetRef, Arc<Skin>>;

//! The animator and skin systems (M3-19, `docs/animation.md`): clip
//! playback onto the animators' subtrees in the §5.1 pre-tick slot —
//! before the catalogue's movers and physics, so a collider on a posed
//! joint is where the pose put it this tick — and the skins' joint
//! lookups, whose result the frame's extract turns into a matrix palette.
//!
//! Nothing here allocates per tick once a target list is resolved: a
//! clip's targets are cached on the animator (`AnimatorRun`) and a skin's
//! joints on the mesh entity (`SkinRun`), re-resolved only when the clip
//! changes or a cached entity dies or is renamed.

use std::collections::HashMap;
use std::sync::Arc;

use fynkel_contracts::{AssetRef, ComponentId, DiagnosticCode, Entity, Tick};
use fynkel_ecs::internal::Handle;
use fynkel_ecs::World as Ecs;
use fynkel_scene::{self as scene, Children, Parent, Transform};
use glam::Mat4;

use super::{Diagnostic, Name, World};
use crate::animation::{Clip, Skin};
use crate::command::{AnimateOp, Lane};
use crate::components::{Animator, Skin as SkinComponent};
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
        }
    }
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
fn find_named(ecs: &Ecs, root: Entity, name: &str) -> Option<Entity> {
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
    unavailable: Option<AssetRef>,
}

impl World {
    /// Makes a clip available to `Animator`s that name `clip`
    /// (`docs/animation.md`; the loaders do this for every clip a loaded
    /// scene's animators name and the bridge for every `Play`).
    pub fn register_clip(&mut self, clip: AssetRef, data: Clip) {
        self.clips.insert(clip, Arc::new(data));
    }

    pub fn has_clip(&self, clip: &AssetRef) -> bool {
        self.clips.contains_key(clip)
    }

    pub fn clip(&self, clip: &AssetRef) -> Option<&Arc<Clip>> {
        self.clips.get(clip)
    }

    /// Makes a skin available to `Skin` components that name it.
    pub fn register_skin(&mut self, skin: AssetRef, data: Skin) {
        self.skins.insert(skin, Arc::new(data));
    }

    pub fn has_skin(&self, skin: &AssetRef) -> bool {
        self.skins.contains_key(skin)
    }

    pub fn skin(&self, skin: &AssetRef) -> Option<&Arc<Skin>> {
        self.skins.get(skin)
    }

    /// The clips and skins the world knows, for reports.
    pub fn registered_clips(&self) -> impl Iterator<Item = &AssetRef> {
        self.clips.keys()
    }

    pub fn registered_skins(&self) -> impl Iterator<Item = &AssetRef> {
        self.skins.keys()
    }

    /// `Command::Animate` at the tick boundary: edits the animator and its
    /// run state; the system samples the result this tick.
    pub(super) fn apply_animate(&mut self, index: u32, entity: Entity, op: AnimateOp) -> bool {
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
        match &op {
            AnimateOp::Play { speed, .. } | AnimateOp::SetSpeed(speed) if !speed.is_finite() => {
                return self.reject(
                    index,
                    lane,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::ANIMATOR,
                    bad("speed must be finite"),
                );
            }
            AnimateOp::Crossfade { speed, seconds, .. } => {
                if !speed.is_finite() {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::INVALID_PAYLOAD,
                        entity,
                        ComponentId::ANIMATOR,
                        bad("speed must be finite"),
                    );
                }
                if !seconds.is_finite() || *seconds < 0.0 {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::INVALID_PAYLOAD,
                        entity,
                        ComponentId::ANIMATOR,
                        bad("crossfade seconds must be finite and >= 0"),
                    );
                }
            }
            AnimateOp::SetTime(t) if !t.is_finite() || *t < 0.0 => {
                return self.reject(
                    index,
                    lane,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::ANIMATOR,
                    bad("time must be finite and >= 0"),
                );
            }
            _ => {}
        }
        if !self.ecs.has::<AnimatorRun>(entity) {
            let _ = self.ecs.insert(entity, AnimatorRun::default());
        }
        let Ok(mut animator) = self.ecs.get_mut::<Animator>(entity) else {
            return false;
        };
        let Ok(mut run) = self.ecs.get_mut::<AnimatorRun>(entity) else {
            return false;
        };
        match op {
            AnimateOp::Play {
                clip,
                looping,
                speed,
            } => {
                animator.clip = clip;
                animator.looping = looping;
                animator.speed = speed;
                animator.playing = true;
                run.time = 0.0;
                run.fresh = true;
                run.seek = None;
                run.fade = None;
            }
            AnimateOp::Crossfade {
                clip,
                looping,
                speed,
                seconds,
            } => {
                // The clip playing so far becomes the outgoing side, with
                // the targets it had resolved; nothing to fade from when
                // it never resolved (or the fade is instant).
                let outgoing = match (&run.resolved, self.clips.get(&animator.clip)) {
                    (Some(resolved), Some(data))
                        if seconds > 0.0 && *resolved == animator.clip && clip != animator.clip =>
                    {
                        Some(Fade {
                            clip: data.clone(),
                            clip_ref: animator.clip.clone(),
                            time: run.time,
                            speed: if animator.playing {
                                animator.speed
                            } else {
                                0.0
                            },
                            looping: animator.looping,
                            targets: std::mem::take(&mut run.targets),
                            remaining: seconds,
                            total: seconds,
                        })
                    }
                    _ => None,
                };
                animator.clip = clip;
                animator.looping = looping;
                animator.speed = speed;
                animator.playing = true;
                run.time = 0.0;
                run.fresh = true;
                run.seek = None;
                run.fade = outgoing;
                run.resolved = None;
            }
            AnimateOp::Stop => {
                animator.playing = false;
                run.time = 0.0;
                run.fresh = false;
                run.seek = None;
                run.fade = None;
            }
            AnimateOp::Pause => animator.playing = false,
            AnimateOp::Resume => animator.playing = true,
            AnimateOp::SetSpeed(speed) => animator.speed = speed,
            AnimateOp::SetTime(t) => {
                run.fresh = true;
                run.seek = Some(t);
            }
        }
        true
    }

    /// Every `Animator` advances its clock and writes its clip's pose over
    /// its subtree (a crossfade blends the outgoing clip's pose out);
    /// every `Skin` keeps its joint lookups current. Returns animators +
    /// skins ticked.
    pub(super) fn run_animation(&mut self, tick: Tick) -> u32 {
        self.attach_state::<Animator, AnimatorRun>();
        self.attach_state::<SkinComponent, SkinRun>();
        let dt = self.config.fixed_dt;
        let mut ticks = std::mem::take(&mut self.animator_scratch);
        ticks.clear();
        let mut count = 0;

        let mut animators = std::mem::take(&mut self.entity_scratch);
        animators.clear();
        animators.extend(
            self.ecs
                .hecs()
                .query::<&Handle>()
                .with::<&Animator>()
                .iter()
                .map(|h| h.0),
        );
        animators.sort_unstable();
        let mut pose = std::mem::take(&mut self.pose_scratch);
        let mut fade_pose = std::mem::take(&mut self.fade_pose_scratch);
        for &entity in &animators {
            count += 1;
            let Ok(mut animator) = self.ecs.get_mut::<Animator>(entity) else {
                continue;
            };
            let Ok(mut run) = self.ecs.get_mut::<AnimatorRun>(entity) else {
                continue;
            };
            let mut out = AnimatorTick {
                entity,
                ended: None,
                unavailable: None,
            };
            pose.clear();
            let clip_ref = animator.clip.clone();
            let clip = if clip_ref.is_nil() {
                None
            } else {
                match self.clips.get(&clip_ref) {
                    Some(c) => Some(c.clone()),
                    None => {
                        if run.reported.as_ref() != Some(&clip_ref) {
                            run.reported = Some(clip_ref.clone());
                            out.unavailable = Some(clip_ref.clone());
                        }
                        None
                    }
                }
            };
            if let Some(clip) = &clip {
                // Targets: cached while every one still lives under its name.
                let names: Vec<&str> = clip.channels.iter().map(|c| c.target.as_str()).collect();
                if run.resolved.as_ref() != Some(&clip_ref)
                    || !targets_valid(&self.ecs, &run.targets, &names)
                {
                    run.targets = resolve_names(&self.ecs, entity, &names);
                    run.resolved = Some(clip_ref.clone());
                }
                let duration = clip.duration.max(0.0);
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
                    let next = run.time + animator.speed * dt;
                    if animator.looping {
                        run.time = if duration > 0.0 {
                            next.rem_euclid(duration)
                        } else {
                            0.0
                        };
                    } else {
                        run.time = next.clamp(0.0, duration);
                        if !was_at_end && at_end(run.time, animator.speed, duration) {
                            out.ended = Some(clip_ref.clone());
                        }
                    }
                }
                clip.sample(run.time, &run.targets, &mut pose);
            }
            if out.ended.is_some() {
                animator.playing = false;
            }
            // The outgoing clip keeps its clock and fades out.
            if let Some(fade) = &mut run.fade {
                fade.remaining -= dt;
                if fade.remaining <= 0.0 || clip.is_none() {
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
            drop(animator);
            drop(run);
            for entry in &pose.entries {
                let Ok(current) = self.ecs.copy::<Transform>(entry.entity) else {
                    continue;
                };
                let next = entry.apply_to(current);
                if next != current {
                    let _ = scene::set_local(&self.ecs, entry.entity, next);
                }
            }
            ticks.push(out);
        }
        pose.clear();
        fade_pose.clear();
        self.pose_scratch = pose;
        self.fade_pose_scratch = fade_pose;
        self.entity_scratch = animators;

        for t in ticks.drain(..) {
            if let Some(clip) = t.unavailable {
                self.diagnostics.push(Diagnostic {
                    command_index: u32::MAX,
                    lane: Lane::Simulation,
                    code: DiagnosticCode::CLIP_UNAVAILABLE,
                    entity: t.entity,
                    component: ComponentId::ANIMATOR,
                    message: format!(
                        "animator: clip {clip} is not registered with the world (not in the loaded scene's closure?)"
                    ),
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
        }
        self.animator_scratch = ticks;

        count += self.run_skins();
        count
    }

    /// Every `Skin` component's joints resolved to entities under its
    /// skeleton root, cached while they hold.
    fn run_skins(&mut self) -> u32 {
        let mut skinned = std::mem::take(&mut self.entity_scratch);
        skinned.clear();
        skinned.extend(
            self.ecs
                .hecs()
                .query::<&Handle>()
                .with::<&SkinComponent>()
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
            let Some(skin) = self.skins.get(&skin_ref) else {
                if !skin_ref.is_nil() && run.reported.as_ref() != Some(&skin_ref) {
                    run.reported = Some(skin_ref.clone());
                    unavailable.push((entity, skin_ref));
                }
                run.resolved = None;
                run.joints.clear();
                continue;
            };
            let names: Vec<&str> = skin.joints.iter().map(|j| j.name.as_str()).collect();
            let root_ok = run.root.is_some_and(|r| {
                self.ecs.has::<Transform>(r) && {
                    component.skeleton.is_empty()
                        || self
                            .ecs
                            .get::<Name>(r)
                            .is_ok_and(|n| n.value == component.skeleton)
                }
            });
            if run.resolved.as_ref() != Some(&skin_ref)
                || !root_ok
                || !targets_valid(&self.ecs, &run.joints, &names)
            {
                let top = tree_root(&self.ecs, entity);
                let root = if component.skeleton.is_empty() {
                    Some(top)
                } else {
                    find_named(&self.ecs, top, &component.skeleton)
                };
                run.root = root;
                run.joints = match root {
                    Some(r) => resolve_names(&self.ecs, r, &names),
                    None => vec![None; names.len()],
                };
                run.resolved = Some(skin_ref);
            }
        }
        self.entity_scratch = skinned;
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
        let skin = self.skins.get(&component.skin)?.clone();
        Some((skin, run.joints.clone()))
    }

    /// `FYNKEL_QUERY_ANIMATOR`: the animator's playback state, `None`
    /// without an `Animator`.
    pub fn animator_state(&self, entity: Entity) -> Option<AnimatorStateRecord> {
        let animator = self.ecs.get::<Animator>(entity).ok()?;
        let (time, fade_remaining) = match self.ecs.get::<AnimatorRun>(entity) {
            Ok(run) => (
                run.time,
                run.fade
                    .as_ref()
                    .map(|f| f.remaining.max(0.0))
                    .unwrap_or(0.0),
            ),
            Err(_) => (0.0, 0.0),
        };
        Some(AnimatorStateRecord {
            entity,
            time,
            duration: self
                .clips
                .get(&animator.clip)
                .map(|c| c.duration)
                .unwrap_or(0.0),
            speed: animator.speed,
            fade_remaining,
            playing: animator.playing,
            looping: animator.looping,
            clip: animator.clip.clone(),
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

/// Registered clips and skins by reference.
pub(super) type ClipTable = HashMap<AssetRef, Arc<Clip>>;
pub(super) type SkinTable = HashMap<AssetRef, Arc<Skin>>;

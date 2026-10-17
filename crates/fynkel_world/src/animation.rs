//! Playing a model's clips (`docs/stage-9.md` phase 1): an [`Animator`]
//! on each spawned model that has clips, and [`World::animate`], which
//! moves every animator's nodes once a frame.
//!
//! An animator plays one clip, or several while a crossfade runs, each at
//! its own time and weight. A frame samples each playing clip's channels,
//! mixes them by weight, and writes the nodes' transforms: only the
//! properties some playing clip moves, so a node no clip touches keeps
//! what the game gave it. While clips mix, a clip that doesn't move a
//! property a fading one does gives that node's rest value for its share,
//! so a walk's swinging arm comes back to rest as the idle takes over.

use std::sync::Arc;

use fynkel_load::{ClipData, Property};
use glam::{Quat, Vec3};

use crate::ObjectId;
use crate::components::Transform;
use crate::world::{Result, World, WorldError};

/// One clip playing on an animator.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Play {
    clip: usize,
    /// Seconds into the clip, 0 to its length.
    time: f32,
    weight: f32,
    /// Weight a second: positive while it fades in, negative out.
    fade: f32,
    looping: bool,
    /// A clip that doesn't loop and reached its end, reported once.
    ended: bool,
}

/// What an animator's clip did in a frame, for the game's events: its
/// time went from `from` to `to`, passing its end `loops` times on the
/// way (a looping clip), or reaching it (`ended`, one that doesn't loop).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationEvent {
    pub root: ObjectId,
    pub clip: u32,
    pub from: f32,
    pub to: f32,
    pub loops: u32,
    pub ended: bool,
}

/// A spawned model's clips and what they're playing. Made by
/// [`World::spawn_model`] for a model with clips, keyed by its root.
#[derive(Debug)]
pub(crate) struct Animator {
    clips: Vec<Arc<ClipData>>,
    /// Each model node's object, index for index with the model's nodes.
    nodes: Vec<hecs::Entity>,
    /// Each node's transform in the file: what a clip that doesn't move a
    /// property gives while clips mix.
    rest: Vec<Transform>,
    playing: Vec<Play>,
    speed: f32,
    /// Whether the game listens for this animator's events.
    reporting: bool,
    /// A frame's mix, by node and property ([`slot`]), kept between frames
    /// so a frame allocates nothing.
    mix: Vec<Mix>,
    touched: Vec<u32>,
}

/// A node's property as a frame mixes it.
#[derive(Clone, Copy, Debug, Default)]
struct Mix {
    sum: [f32; 4],
    weight: f32,
    /// Some playing clip moves it.
    seen: bool,
}

/// The mix slot for a node's property.
fn slot(node: usize, property: Property) -> usize {
    node * 3
        + match property {
            Property::Translation => 0,
            Property::Rotation => 1,
            Property::Scale => 2,
        }
}

fn rest_value(rest: &Transform, property: Property) -> [f32; 4] {
    match property {
        Property::Translation => rest.position.extend(0.0).to_array(),
        Property::Rotation => rest.rotation.to_array(),
        Property::Scale => rest.scale.extend(0.0).to_array(),
    }
}

impl Animator {
    pub(crate) fn new(clips: Vec<Arc<ClipData>>, nodes: Vec<hecs::Entity>, rest: Vec<Transform>) -> Self {
        let slots = nodes.len() * 3;
        Self {
            clips,
            nodes,
            rest,
            playing: Vec::new(),
            speed: 1.0,
            reporting: false,
            mix: vec![Mix::default(); slots],
            touched: Vec::new(),
        }
    }

    /// The clip playing at full weight or fading in: what the game last
    /// asked for.
    fn current(&self) -> Option<&Play> {
        self.playing.last()
    }

    /// Advances every playing clip by `dt` seconds (times the speed) and
    /// its fade; reports to `events`.
    fn advance(&mut self, root: ObjectId, dt: f32, events: &mut Vec<AnimationEvent>) {
        let step = dt * self.speed;
        for play in &mut self.playing {
            let length = self.clips[play.clip].length;
            let from = play.time;
            let mut loops = 0;
            let mut ended = false;
            if play.looping && length > 0.0 {
                let to = play.time + step;
                loops = (to / length).floor().max(0.0) as u32;
                play.time = to.rem_euclid(length);
            } else {
                play.time = (play.time + step).clamp(0.0, length);
                if play.time >= length && !play.ended && step > 0.0 {
                    play.ended = true;
                    ended = true;
                }
            }
            play.weight = (play.weight + play.fade * dt).clamp(0.0, 1.0);
            if (play.fade > 0.0 && play.weight >= 1.0) || (play.fade < 0.0 && play.weight <= 0.0) {
                play.fade = 0.0;
            }
            if self.reporting && (play.time != from || loops > 0 || ended) {
                events.push(AnimationEvent { root, clip: play.clip as u32, from, to: play.time, loops, ended });
            }
        }
    }

    /// Drops the clips that faded out, after their last pose.
    fn tidy(&mut self) {
        self.playing.retain(|p| p.weight > 0.0 || p.fade > 0.0);
    }

    /// Samples the playing clips and writes the mix into the nodes. A clip
    /// that just faded out still counts, at weight 0: what it moved goes
    /// back to the others' pose, or to rest.
    fn pose(&mut self, ecs: &mut hecs::World) {
        let total: f32 = self.playing.iter().map(|p| p.weight).sum();
        if total <= 0.0 {
            return;
        }
        for play in &self.playing {
            for channel in &self.clips[play.clip].channels {
                let at = slot(channel.node, channel.property);
                let mix = &mut self.mix[at];
                if !mix.seen {
                    mix.seen = true;
                    self.touched.push(at as u32);
                }
                if play.weight > 0.0 {
                    add(&mut mix.sum, channel.sample(play.time), play.weight, channel.property);
                    mix.weight += play.weight;
                }
            }
        }
        for &at in &self.touched {
            let at = at as usize;
            let (node, property) = (at / 3, [Property::Translation, Property::Rotation, Property::Scale][at % 3]);
            let Mix { mut sum, weight, .. } = std::mem::take(&mut self.mix[at]);
            // The share of the clips that don't move it: the rest pose.
            if weight < total {
                add(&mut sum, rest_value(&self.rest[node], property), total - weight, property);
            }
            let Ok(transform) = ecs.query_one_mut::<&mut Transform>(self.nodes[node]) else {
                // The game destroyed this node: nothing to move.
                continue;
            };
            let [x, y, z, w] = sum;
            match property {
                Property::Translation => transform.position = Vec3::new(x, y, z) / total,
                Property::Scale => transform.scale = Vec3::new(x, y, z) / total,
                Property::Rotation => transform.rotation = Quat::from_xyzw(x, y, z, w).normalize(),
            }
        }
        self.touched.clear();
    }
}

/// Adds `value` times `weight` to `sum`; a rotation the same way round as
/// what's there, so two near rotations never cancel.
fn add(sum: &mut [f32; 4], value: [f32; 4], weight: f32, property: Property) {
    let flip = property == Property::Rotation && sum.iter().zip(&value).map(|(a, b)| a * b).sum::<f32>() < 0.0;
    let weight = if flip { -weight } else { weight };
    for (s, v) in sum.iter_mut().zip(value) {
        *s += v * weight;
    }
}

/// What an animator is playing, for the game to read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimatorState {
    /// The clip last played or faded to, if any is playing.
    pub clip: Option<u32>,
    pub time: f32,
    pub length: f32,
    pub speed: f32,
}

impl World {
    /// Makes `root`'s animator, for a model just spawned.
    pub(crate) fn add_animator(&mut self, root: ObjectId, animator: Animator) {
        self.animators.insert(root, animator);
    }

    fn animator(&mut self, root: ObjectId) -> Result<&mut Animator> {
        self.entity(root)?;
        self.animators.get_mut(&root).ok_or(WorldError::NoComponent { object: root, component: "Animator" })
    }

    /// Whether `root` is a model copy with clips.
    pub fn has_animator(&self, root: ObjectId) -> bool {
        self.animators.contains_key(&root)
    }

    /// The names of `root`'s clips, in the file's order, and their lengths
    /// in seconds.
    pub fn clips(&self, root: ObjectId) -> Result<Vec<(&str, f32)>> {
        self.entity(root)?;
        let animator =
            self.animators.get(&root).ok_or(WorldError::NoComponent { object: root, component: "Animator" })?;
        Ok(animator.clips.iter().map(|c| (c.name.as_str(), c.length)).collect())
    }

    /// Plays clip `clip` of `root`'s from its start. With `fade` 0, at once,
    /// in place of everything playing; otherwise it fades in over `fade`
    /// seconds as everything else fades out. A clip already fading in or
    /// playing keeps its time.
    pub fn play(&mut self, root: ObjectId, clip: u32, fade: f32, looping: bool) -> Result<()> {
        let animator = self.animator(root)?;
        let clip = clip as usize;
        if clip >= animator.clips.len() {
            return Err(WorldError::NoClip { object: root, clip: clip as u32, count: animator.clips.len() as u32 });
        }
        let fade = if fade.is_finite() { fade.max(0.0) } else { 0.0 };
        let fresh = Play { clip, time: 0.0, weight: 1.0, fade: 0.0, looping, ended: false };
        if fade == 0.0 {
            animator.playing.clear();
            animator.playing.push(fresh);
            return Ok(());
        }
        // The one asked for again keeps its time; the rest fade out
        // together, reaching 0 when it reaches 1.
        let kept = animator.playing.iter().position(|p| p.clip == clip);
        let mut play = match kept {
            Some(i) => animator.playing.remove(i),
            None => Play { weight: 0.0, ..fresh },
        };
        play.looping = looping;
        play.fade = (1.0 - play.weight) / fade;
        for other in &mut animator.playing {
            other.fade = -other.weight / fade;
        }
        animator.playing.push(play);
        Ok(())
    }

    /// Stops every clip: the nodes keep the pose they're in.
    pub fn stop(&mut self, root: ObjectId) -> Result<()> {
        self.animator(root)?.playing.clear();
        Ok(())
    }

    /// How fast `root`'s clips play: 1 as made, 0 paused, negative
    /// backwards.
    pub fn set_animation_speed(&mut self, root: ObjectId, speed: f32) -> Result<()> {
        if !speed.is_finite() {
            return Err(WorldError::Field(crate::FieldError::NotFinite { ty: "Animator", field: "speed" }));
        }
        self.animator(root)?.speed = speed;
        Ok(())
    }

    /// Moves the current clip to `time` seconds, kept within its length.
    pub fn set_animation_time(&mut self, root: ObjectId, time: f32) -> Result<()> {
        if !time.is_finite() {
            return Err(WorldError::Field(crate::FieldError::NotFinite { ty: "Animator", field: "time" }));
        }
        let animator = self.animator(root)?;
        let clips = &animator.clips;
        if let Some(play) = animator.playing.last_mut() {
            let length = clips[play.clip].length;
            play.time = time.clamp(0.0, length);
            play.ended = !play.looping && play.time >= length;
        }
        Ok(())
    }

    /// Whether `root`'s events are reported by [`World::animate`].
    pub fn set_animation_reporting(&mut self, root: ObjectId, on: bool) -> Result<()> {
        self.animator(root)?.reporting = on;
        Ok(())
    }

    pub fn animator_state(&self, root: ObjectId) -> Result<AnimatorState> {
        self.entity(root)?;
        let animator =
            self.animators.get(&root).ok_or(WorldError::NoComponent { object: root, component: "Animator" })?;
        let current = animator.current();
        Ok(AnimatorState {
            clip: current.map(|p| p.clip as u32),
            time: current.map_or(0.0, |p| p.time),
            length: current.map_or(0.0, |p| animator.clips[p.clip].length),
            speed: animator.speed,
        })
    }

    /// Whether `root` plays clip `clip` (fading in, at full weight, or
    /// fading out), and at what weight.
    pub fn clip_weight(&self, root: ObjectId, clip: u32) -> Result<f32> {
        self.entity(root)?;
        let animator =
            self.animators.get(&root).ok_or(WorldError::NoComponent { object: root, component: "Animator" })?;
        Ok(animator.playing.iter().filter(|p| p.clip == clip as usize).map(|p| p.weight).sum())
    }

    /// The number of animators: models spawned with clips, still here.
    pub fn animator_count(&self) -> usize {
        self.animators.len()
    }

    /// Advances every animator by `dt` seconds and poses its model: once a
    /// frame, before the frame is drawn. Animators of models that aren't
    /// shown keep time but don't pose. What the reporting ones did is
    /// added to `events`.
    pub fn animate(&mut self, dt: f32, events: &mut Vec<AnimationEvent>) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut animators = std::mem::take(&mut self.animators);
        for (&root, animator) in &mut animators {
            if animator.playing.is_empty() {
                continue;
            }
            animator.advance(root, dt, events);
            if self.active_in_hierarchy(root).unwrap_or(false) {
                animator.pose(&mut self.ecs);
            }
            animator.tidy();
        }
        self.animators = animators;
    }
}

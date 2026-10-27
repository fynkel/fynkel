//! Playing a model's clips (`docs/stage-9.md` phases 1 and 3): an
//! [`Animator`] on each spawned model that has clips, and
//! [`World::animate`], which moves every animator's nodes once a frame.
//!
//! An animator has layers. The base layer (0) moves the whole model;
//! each layer above plays over it, on the whole model or on one joint and
//! everything below it (its mask), in place of what's underneath or added
//! to it, at its own weight: a wave on the arm while the legs walk, a
//! breath added to any pose.
//!
//! A layer plays a motion: one clip, or a 1D blend of clips mixed by one
//! number (idle, walk and run by speed), whose clips keep the same pace so
//! feet land together. Several motions play while a crossfade runs, each
//! at its own time and weight. A frame samples each playing clip's
//! channels, mixes them by weight, and writes the nodes' transforms: only
//! the properties some playing clip moves, so a node no clip touches
//! keeps what the game gave it.
//!
//! Every fade starts from the pose shown: a layer's motions that haven't
//! faded in yet leave their share to what's underneath (for the base, the
//! pose the model was in when it began), so a game can change its mind
//! every frame and the model never jumps. What's underneath a property no
//! lower layer moves is kept from frame to frame while the layers go on
//! writing it, unless the game sets it, so a layer never mixes with its
//! own last pose. While the base layer's motions mix, a clip that
//! doesn't move a property a fading one does gives that node's rest value
//! for its share, so a walk's swinging arm comes back to rest as the idle
//! takes over.

use std::sync::Arc;

use fynkel_load::{ClipData, Property};
use glam::{Quat, Vec3};

use crate::ObjectId;
use crate::components::Transform;
use crate::world::{Result, World, WorldError};

/// The most layers an animator has, the base one included.
pub const MAX_LAYERS: u32 = 8;

/// The most blends one animator keeps.
pub const MAX_BLENDS: u32 = 64;

/// What a layer plays: a model's clip, or one of the animator's blends
/// ([`World::define_blend`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Clip(u32),
    Blend(u32),
}

/// Clips mixed by one number: each at a threshold, sorted, and the
/// number it's at. Between two thresholds, the two clips around it.
#[derive(Clone, Debug)]
struct Blend {
    clips: Vec<(usize, f32)>,
    at: f32,
}

impl Blend {
    /// The clips `at` mixes and their weights: one alone at or past an
    /// end, else the two around it.
    fn weights(&self) -> [(usize, f32); 2] {
        let clips = &self.clips;
        let after = clips.partition_point(|&(_, t)| t <= self.at);
        if after == 0 {
            return [(clips[0].0, 1.0), (clips[0].0, 0.0)];
        }
        if after == clips.len() {
            let last = clips[clips.len() - 1].0;
            return [(last, 1.0), (last, 0.0)];
        }
        let ((a, ta), (b, tb)) = (clips[after - 1], clips[after]);
        let t = if tb > ta { (self.at - ta) / (tb - ta) } else { 1.0 };
        [(a, 1.0 - t), (b, t)]
    }

    /// Seconds a round takes: its clips' lengths, by weight.
    fn length(&self, clips: &[Arc<ClipData>]) -> f32 {
        self.weights().iter().map(|&(c, w)| clips[c].length * w).sum()
    }
}

/// One motion playing on a layer.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Play {
    motion: Motion,
    /// Seconds into a clip, 0 to its length; for a blend, how far round,
    /// 0 to 1.
    time: f32,
    weight: f32,
    /// Weight a second: positive while it fades in, negative out.
    fade: f32,
    looping: bool,
    /// A clip that doesn't loop and reached its end, reported once.
    ended: bool,
}

/// What an animator's clip did in a frame, for the game's events: the
/// span its time went through, from `from` on to `to`, passing the clip's
/// end `loops` times on the way (a looping clip), or reaching it (`ended`,
/// one that doesn't loop). Played backwards, it's the same span, earliest
/// first: `to` is where it started, and `loops` how often it passed the
/// start. A blend's clips report each, while they're in its mix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationEvent {
    pub root: ObjectId,
    pub clip: u32,
    pub from: f32,
    pub to: f32,
    pub loops: u32,
    pub ended: bool,
}

/// One of an animator's layers.
#[derive(Clone, Debug)]
struct Layer {
    playing: Vec<Play>,
    /// The nodes it moves, or `None` for all.
    mask: Option<Vec<bool>>,
    /// Added to what's underneath, from each clip's first pose, rather
    /// than in place of it.
    additive: bool,
    /// How much of it shows, 0 to 1.
    weight: f32,
}

impl Layer {
    fn new() -> Self {
        Self { playing: Vec::new(), mask: None, additive: false, weight: 1.0 }
    }
}

/// A spawned model's clips and what they're playing. Made by
/// [`World::spawn_model`] for a model with clips, keyed by its root.
#[derive(Debug)]
pub(crate) struct Animator {
    clips: Vec<Arc<ClipData>>,
    /// Each model node's object, index for index with the model's nodes,
    /// with its name and its parent's index.
    nodes: Vec<hecs::Entity>,
    names: Vec<String>,
    parents: Vec<Option<usize>>,
    /// Each node's transform in the file: what a clip that doesn't move a
    /// property gives while the base layer's clips mix.
    rest: Vec<Transform>,
    layers: Vec<Layer>,
    blends: Vec<Blend>,
    speed: f32,
    /// Whether the game listens for this animator's events.
    reporting: bool,
    /// A layer's mix and the frame's pose, by node and property
    /// ([`slot`]), kept between frames so a frame allocates nothing.
    mix: Vec<Mix>,
    touched: Vec<u32>,
    out: Vec<Option<[f32; 4]>>,
    written: Vec<u32>,
    /// By slot: what's under every layer, the value written and the frame
    /// it was ([`Animator::under`]). Frames count [`Animator::pose`]s.
    floor: Vec<[f32; 4]>,
    last: Vec<[f32; 4]>,
    when: Vec<u32>,
    frame: u32,
}

/// A node's property as a layer mixes it.
#[derive(Clone, Copy, Debug, Default)]
struct Mix {
    sum: [f32; 4],
    weight: f32,
    /// Some playing clip moves it.
    seen: bool,
}

const PROPERTIES: [Property; 3] = [Property::Translation, Property::Rotation, Property::Scale];

/// The mix slot for a node's property.
fn slot(node: usize, property: Property) -> usize {
    node * 3
        + match property {
            Property::Translation => 0,
            Property::Rotation => 1,
            Property::Scale => 2,
        }
}

fn value_of(t: &Transform, property: Property) -> [f32; 4] {
    match property {
        Property::Translation => t.position.extend(0.0).to_array(),
        Property::Rotation => t.rotation.to_array(),
        Property::Scale => t.scale.extend(0.0).to_array(),
    }
}

/// What adds nothing: no move, no turn, scale 1.
fn no_change(property: Property) -> [f32; 4] {
    match property {
        Property::Translation => [0.0; 4],
        Property::Rotation => [0.0, 0.0, 0.0, 1.0],
        Property::Scale => [1.0, 1.0, 1.0, 0.0],
    }
}

/// How `value` differs from `reference`: the move, the turn after it, the
/// scale by which.
fn difference(value: [f32; 4], reference: [f32; 4], property: Property) -> [f32; 4] {
    match property {
        Property::Translation => std::array::from_fn(|i| value[i] - reference[i]),
        Property::Rotation => (Quat::from_array(reference).inverse() * Quat::from_array(value)).to_array(),
        Property::Scale => {
            std::array::from_fn(|i| if i < 3 && reference[i] != 0.0 { value[i] / reference[i] } else { 1.0 })
        }
    }
}

/// From `a` towards `b` by `t`; a rotation the short way, normalised.
fn towards(a: [f32; 4], b: [f32; 4], t: f32, property: Property) -> [f32; 4] {
    let flip = property == Property::Rotation && a.iter().zip(&b).map(|(x, y)| x * y).sum::<f32>() < 0.0;
    let mixed: [f32; 4] = std::array::from_fn(|i| a[i] + ((if flip { -b[i] } else { b[i] }) - a[i]) * t);
    if property == Property::Rotation { Quat::from_array(mixed).normalize().to_array() } else { mixed }
}

/// `under` with `change` (a [`difference`]) added at strength `t`.
fn added(under: [f32; 4], change: [f32; 4], t: f32, property: Property) -> [f32; 4] {
    let change = towards(no_change(property), change, t, property);
    match property {
        Property::Translation => std::array::from_fn(|i| under[i] + change[i]),
        Property::Rotation => (Quat::from_array(under) * Quat::from_array(change)).normalize().to_array(),
        Property::Scale => std::array::from_fn(|i| under[i] * change[i]),
    }
}

impl Animator {
    pub(crate) fn new(
        clips: Vec<Arc<ClipData>>,
        nodes: Vec<hecs::Entity>,
        names: Vec<String>,
        parents: Vec<Option<usize>>,
        rest: Vec<Transform>,
    ) -> Self {
        let slots = nodes.len() * 3;
        Self {
            clips,
            nodes,
            names,
            parents,
            rest,
            layers: vec![Layer::new()],
            blends: Vec::new(),
            speed: 1.0,
            reporting: false,
            mix: vec![Mix::default(); slots],
            touched: Vec::new(),
            out: vec![None; slots],
            written: Vec::new(),
            floor: vec![[0.0; 4]; slots],
            last: vec![[0.0; 4]; slots],
            when: vec![u32::MAX; slots],
            frame: 0,
        }
    }

    fn playing(&self) -> bool {
        self.layers.iter().any(|l| !l.playing.is_empty())
    }

    /// Each clip a play samples, its weight within the play and its time.
    fn parts(&self, play: &Play) -> [(usize, f32, f32); 2] {
        match play.motion {
            Motion::Clip(c) => [(c as usize, 1.0, play.time), (c as usize, 0.0, 0.0)],
            Motion::Blend(b) => {
                let [(a, wa), (b, wb)] = self.blends[b as usize].weights();
                [(a, wa, play.time * self.clips[a].length), (b, wb, play.time * self.clips[b].length)]
            }
        }
    }

    /// Advances every playing motion and its fade by `dt` seconds times
    /// the speed (forwards, for a fade); reports to `events`.
    fn advance(&mut self, root: ObjectId, dt: f32, events: &mut Vec<AnimationEvent>) {
        let step = dt * self.speed;
        let fading = dt * self.speed.abs();
        for l in 0..self.layers.len() {
            for p in 0..self.layers[l].playing.len() {
                let mut play = self.layers[l].playing[p];
                let from = play.time;
                let (mut loops, mut ended) = (0, false);
                let length = match play.motion {
                    Motion::Clip(c) => self.clips[c as usize].length,
                    Motion::Blend(b) => self.blends[b as usize].length(&self.clips),
                };
                // A blend's time goes round from 0 to 1.
                let (round, step) = match play.motion {
                    Motion::Clip(_) => (length, step),
                    Motion::Blend(_) => (1.0, if length > 0.0 { step / length } else { 0.0 }),
                };
                if play.looping && length > 0.0 {
                    let to = play.time + step;
                    // Forwards past the end, or backwards past the start.
                    loops = (to / round).floor().abs() as u32;
                    play.time = to.rem_euclid(round);
                } else {
                    play.time = (play.time + step).clamp(0.0, round);
                    if play.time >= round && !play.ended && step > 0.0 {
                        play.ended = true;
                        ended = true;
                    }
                }
                play.weight = (play.weight + play.fade * fading).clamp(0.0, 1.0);
                if (play.fade > 0.0 && play.weight >= 1.0) || (play.fade < 0.0 && play.weight <= 0.0) {
                    play.fade = 0.0;
                }
                self.layers[l].playing[p] = play;
                if self.reporting && (play.time != from || loops > 0 || ended) {
                    // The span gone through, earliest first.
                    let (from, to) = if step < 0.0 { (play.time, from) } else { (from, play.time) };
                    match play.motion {
                        Motion::Clip(c) => events.push(AnimationEvent { root, clip: c, from, to, loops, ended }),
                        Motion::Blend(b) => {
                            for (c, w) in self.blends[b as usize].weights() {
                                if w > 0.0 {
                                    let length = self.clips[c].length;
                                    let (from, to) = (from * length, to * length);
                                    events.push(AnimationEvent { root, clip: c as u32, from, to, loops, ended });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Drops the motions that faded out, after their last pose.
    fn tidy(&mut self) {
        for layer in &mut self.layers {
            layer.playing.retain(|p| p.weight > 0.0 || p.fade > 0.0);
        }
    }

    /// Forgets what's under every layer: the pose the nodes are in is the
    /// one to start from.
    fn hold(&mut self) {
        self.frame = self.frame.wrapping_add(2);
    }

    /// What's under a layer moving a node's property: what a lower layer
    /// gave it this frame; else, if the last frame wrote it and the game
    /// hasn't set it since, what was under the layers then; else what it
    /// is now.
    fn under(&mut self, ecs: &hecs::World, at: usize) -> [f32; 4] {
        if let Some(value) = self.out[at] {
            return value;
        }
        let (node, property) = (at / 3, PROPERTIES[at % 3]);
        let now = match ecs.get::<&Transform>(self.nodes[node]) {
            Ok(t) => value_of(&t, property),
            Err(_) => value_of(&self.rest[node], property),
        };
        let kept = self.when[at] == self.frame.wrapping_sub(1) && self.last[at] == now;
        if !kept {
            self.floor[at] = now;
        }
        self.floor[at]
    }

    /// Samples each layer's motions and writes the pose into the nodes. A
    /// motion that just faded out still counts, at weight 0: what it moved
    /// goes back to the others' pose, or to rest.
    fn pose(&mut self, ecs: &mut hecs::World) {
        self.frame = self.frame.wrapping_add(1);
        for l in 0..self.layers.len() {
            if self.layers[l].playing.is_empty() {
                continue;
            }
            let total: f32 = self.layers[l].playing.iter().map(|p| p.weight).sum();
            let layer = &self.layers[l];
            let (additive, base) = (layer.additive, l == 0);
            // A layer's motions not yet faded in leave their share to the
            // pose underneath.
            let strength = total.min(1.0) * if base { 1.0 } else { layer.weight };
            for play in &layer.playing {
                for (clip, share, time) in self.parts(play) {
                    if share <= 0.0 {
                        continue;
                    }
                    let weight = share * play.weight;
                    for channel in &self.clips[clip].channels {
                        if layer.mask.as_ref().is_some_and(|m| !m[channel.node]) {
                            continue;
                        }
                        let at = slot(channel.node, channel.property);
                        let mix = &mut self.mix[at];
                        if !mix.seen {
                            mix.seen = true;
                            self.touched.push(at as u32);
                        }
                        if weight > 0.0 {
                            let mut value = channel.sample(time);
                            if additive {
                                value = difference(value, channel.sample(0.0), channel.property);
                            }
                            add(&mut mix.sum, value, weight, channel.property);
                            mix.weight += weight;
                        }
                    }
                }
            }
            for i in 0..self.touched.len() {
                let at = self.touched[i] as usize;
                let (node, property) = (at / 3, PROPERTIES[at % 3]);
                let Mix { mut sum, weight, .. } = std::mem::take(&mut self.mix[at]);
                let under = self.under(ecs, at);
                // All of it just faded out: what's underneath, as it was.
                if total <= 0.0 {
                    if self.out[at].replace(under).is_none() {
                        self.written.push(at as u32);
                    }
                    continue;
                }
                // The share of the motions that don't move it: rest on the
                // base layer, what's underneath on the others.
                if weight < total {
                    let fill = match (additive, base) {
                        (true, _) => no_change(property),
                        (false, true) => value_of(&self.rest[node], property),
                        (false, false) => under,
                    };
                    add(&mut sum, fill, total - weight, property);
                }
                let mixed = if property == Property::Rotation {
                    Quat::from_array(sum).normalize().to_array()
                } else {
                    sum.map(|v| v / total)
                };
                let value = if additive {
                    added(under, mixed, strength, property)
                } else {
                    towards(under, mixed, strength, property)
                };
                if self.out[at].replace(value).is_none() {
                    self.written.push(at as u32);
                }
                // A base fully shown is what's left when it stops: what
                // a fade out of it starts from.
                if base && strength >= 1.0 - 1e-4 {
                    self.floor[at] = value;
                }
            }
            self.touched.clear();
        }
        for &at in &self.written {
            let at = at as usize;
            let (node, property) = (at / 3, PROPERTIES[at % 3]);
            let value = self.out[at].take().expect("written");
            (self.last[at], self.when[at]) = (value, self.frame);
            let Ok(transform) = ecs.query_one_mut::<&mut Transform>(self.nodes[node]) else {
                // The game destroyed this node: nothing to move.
                continue;
            };
            let [x, y, z, w] = value;
            match property {
                Property::Translation => transform.position = Vec3::new(x, y, z),
                Property::Scale => transform.scale = Vec3::new(x, y, z),
                Property::Rotation => transform.rotation = Quat::from_xyzw(x, y, z, w),
            }
        }
        self.written.clear();
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

/// What a layer of an animator is playing, for the game to read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimatorState {
    /// The motion last played or faded to, if any is playing.
    pub motion: Option<Motion>,
    /// Seconds into it. For a blend, into a round of it.
    pub time: f32,
    /// Its length in seconds; for a blend, a round's at its number now.
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

    fn animator_ref(&self, root: ObjectId) -> Result<&Animator> {
        self.entity(root)?;
        self.animators.get(&root).ok_or(WorldError::NoComponent { object: root, component: "Animator" })
    }

    /// `root`'s layer `layer`, made (whole, in place, at weight 1) if it
    /// isn't yet.
    fn animator_layer(&mut self, root: ObjectId, layer: u32) -> Result<&mut Animator> {
        if layer >= MAX_LAYERS {
            return Err(WorldError::Animator("an animator's layers are 0 to 7"));
        }
        let animator = self.animator(root)?;
        while animator.layers.len() <= layer as usize {
            animator.layers.push(Layer::new());
        }
        Ok(animator)
    }

    /// Whether `root` is a model copy with clips.
    pub fn has_animator(&self, root: ObjectId) -> bool {
        self.animators.contains_key(&root)
    }

    /// The names of `root`'s clips, in the file's order, and their lengths
    /// in seconds.
    pub fn clips(&self, root: ObjectId) -> Result<Vec<(&str, f32)>> {
        let animator = self.animator_ref(root)?;
        Ok(animator.clips.iter().map(|c| (c.name.as_str(), c.length)).collect())
    }

    /// Plays clip `clip` on `root`'s base layer: [`play_on`](Self::play_on)
    /// layer 0.
    pub fn play(&mut self, root: ObjectId, clip: u32, fade: f32, looping: bool) -> Result<()> {
        self.play_on(root, 0, Motion::Clip(clip), fade, looping)
    }

    /// Plays `motion` on `root`'s layer `layer` from its start. With `fade`
    /// 0, at once, in place of everything the layer plays; otherwise it
    /// fades in over `fade` seconds as everything else on the layer fades
    /// out. Fading to the current motion (the one last played or faded to)
    /// changes nothing; to one fading out, it fades back in from where it
    /// is. A blend always loops.
    pub fn play_on(&mut self, root: ObjectId, layer: u32, motion: Motion, fade: f32, looping: bool) -> Result<()> {
        let animator = self.animator_layer(root, layer)?;
        match motion {
            Motion::Clip(clip) if clip as usize >= animator.clips.len() => {
                return Err(WorldError::NoClip { object: root, clip, count: animator.clips.len() as u32 });
            }
            Motion::Blend(blend) if blend as usize >= animator.blends.len() => {
                return Err(WorldError::Animator("the animator has no such blend"));
            }
            _ => {}
        }
        let looping = looping || matches!(motion, Motion::Blend(_));
        let playing = &mut animator.layers[layer as usize].playing;
        let fade = if fade.is_finite() { fade.max(0.0) } else { 0.0 };
        let fresh = Play { motion, time: 0.0, weight: 1.0, fade: 0.0, looping, ended: false };
        if fade == 0.0 {
            playing.clear();
            playing.push(fresh);
            return Ok(());
        }
        // The current motion asked for again: its fade, if one runs, goes
        // on as it was, so a game can ask each frame.
        if let Some(current) = playing.last_mut()
            && current.motion == motion
        {
            current.looping = looping;
            return Ok(());
        }
        // One fading out keeps its time; the rest fade out together,
        // reaching 0 when it reaches 1.
        let kept = playing.iter().position(|p| p.motion == motion);
        let mut play = match kept {
            Some(i) => playing.remove(i),
            None => Play { weight: 0.0, ..fresh },
        };
        play.looping = looping;
        play.fade = (1.0 - play.weight) / fade;
        for other in playing.iter_mut() {
            other.fade = -other.weight / fade;
        }
        playing.push(play);
        Ok(())
    }

    /// Makes a blend of `root`'s clips, each at a threshold: played with
    /// [`play_on`](Self::play_on), it mixes the two clips around the
    /// number [`set_blend`](Self::set_blend) gives it, keeping their pace
    /// together. Returns the blend's number.
    pub fn define_blend(&mut self, root: ObjectId, clips: &[(u32, f32)]) -> Result<u32> {
        let animator = self.animator(root)?;
        if clips.is_empty() {
            return Err(WorldError::Animator("a blend needs a clip"));
        }
        if animator.blends.len() as u32 >= MAX_BLENDS {
            return Err(WorldError::Animator("an animator keeps at most 64 blends"));
        }
        let count = animator.clips.len() as u32;
        let mut sorted = Vec::with_capacity(clips.len());
        for &(clip, threshold) in clips {
            if clip >= count {
                return Err(WorldError::NoClip { object: root, clip, count });
            }
            if !threshold.is_finite() {
                return Err(WorldError::Animator("a blend's thresholds must be finite"));
            }
            sorted.push((clip as usize, threshold));
        }
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let at = sorted[0].1;
        animator.blends.push(Blend { clips: sorted, at });
        Ok(animator.blends.len() as u32 - 1)
    }

    /// Sets the number blend `blend` of `root`'s mixes its clips by.
    pub fn set_blend(&mut self, root: ObjectId, blend: u32, at: f32) -> Result<()> {
        if !at.is_finite() {
            return Err(WorldError::Field(crate::FieldError::NotFinite { ty: "Animator", field: "blend" }));
        }
        let animator = self.animator(root)?;
        let Some(blend) = animator.blends.get_mut(blend as usize) else {
            return Err(WorldError::Animator("the animator has no such blend"));
        };
        blend.at = at;
        Ok(())
    }

    /// Sets `root`'s layer `layer`: the joint it moves from (`mask`, with
    /// everything below it; `None` for the whole model), whether it adds
    /// to what's underneath, and how much of it shows. The base layer (0)
    /// moves the whole model in place, at weight 1.
    pub fn set_animation_layer(
        &mut self,
        root: ObjectId,
        layer: u32,
        mask: Option<&str>,
        additive: bool,
        weight: f32,
    ) -> Result<()> {
        if !weight.is_finite() {
            return Err(WorldError::Field(crate::FieldError::NotFinite { ty: "Animator", field: "weight" }));
        }
        if layer == 0 && (mask.is_some() || additive || weight != 1.0) {
            return Err(WorldError::Animator(
                "the base layer moves the whole model, in place, at weight 1; use a layer above it",
            ));
        }
        let animator = self.animator_layer(root, layer)?;
        let mask = match mask {
            None => None,
            Some(name) => {
                let Some(joint) = animator.names.iter().position(|n| n == name) else {
                    return Err(WorldError::Animator("the model has no part by that name"));
                };
                let below = |mut node: usize| loop {
                    if node == joint {
                        return true;
                    }
                    match animator.parents[node] {
                        Some(parent) => node = parent,
                        None => return false,
                    }
                };
                Some((0..animator.nodes.len()).map(below).collect())
            }
        };
        let l = &mut animator.layers[layer as usize];
        l.mask = mask;
        l.additive = additive;
        l.weight = weight.clamp(0.0, 1.0);
        Ok(())
    }

    /// Sets how much of `root`'s layer `layer` shows, 0 to 1.
    pub fn set_animation_layer_weight(&mut self, root: ObjectId, layer: u32, weight: f32) -> Result<()> {
        if !weight.is_finite() {
            return Err(WorldError::Field(crate::FieldError::NotFinite { ty: "Animator", field: "weight" }));
        }
        if layer == 0 && weight != 1.0 {
            return Err(WorldError::Animator("the base layer shows at weight 1; use a layer above it"));
        }
        self.animator_layer(root, layer)?.layers[layer as usize].weight = weight.clamp(0.0, 1.0);
        Ok(())
    }

    /// Stops every clip on `root`'s base layer at once: the nodes keep the
    /// pose they're in.
    pub fn stop(&mut self, root: ObjectId) -> Result<()> {
        self.stop_on(root, 0, 0.0)
    }

    /// Stops what `root`'s layer `layer` plays: at once with `fade` 0, the
    /// nodes keeping the pose they're in, or fading out over `fade` seconds
    /// to what's underneath (for the base layer, the pose it's in).
    pub fn stop_on(&mut self, root: ObjectId, layer: u32, fade: f32) -> Result<()> {
        let animator = self.animator_layer(root, layer)?;
        let playing = &mut animator.layers[layer as usize].playing;
        let fade = if fade.is_finite() { fade.max(0.0) } else { 0.0 };
        if fade == 0.0 {
            playing.clear();
            animator.hold();
        } else {
            for play in playing.iter_mut() {
                play.fade = -play.weight / fade;
            }
        }
        Ok(())
    }

    /// How fast `root`'s clips play, on every layer: 1 as made, 0 paused,
    /// negative backwards.
    pub fn set_animation_speed(&mut self, root: ObjectId, speed: f32) -> Result<()> {
        if !speed.is_finite() {
            return Err(WorldError::Field(crate::FieldError::NotFinite { ty: "Animator", field: "speed" }));
        }
        self.animator(root)?.speed = speed;
        Ok(())
    }

    /// Moves the base layer's current motion to `time` seconds.
    pub fn set_animation_time(&mut self, root: ObjectId, time: f32) -> Result<()> {
        self.set_animation_time_on(root, 0, time)
    }

    /// Moves layer `layer`'s current motion to `time` seconds, kept
    /// within its length (a blend's, a round's at its number now).
    pub fn set_animation_time_on(&mut self, root: ObjectId, layer: u32, time: f32) -> Result<()> {
        if !time.is_finite() {
            return Err(WorldError::Field(crate::FieldError::NotFinite { ty: "Animator", field: "time" }));
        }
        let animator = self.animator_layer(root, layer)?;
        let Animator { clips, blends, layers, .. } = animator;
        if let Some(play) = layers[layer as usize].playing.last_mut() {
            let (length, round) = match play.motion {
                Motion::Clip(c) => (clips[c as usize].length, clips[c as usize].length),
                Motion::Blend(b) => (blends[b as usize].length(clips), 1.0),
            };
            play.time = if length > 0.0 { (time / length * round).clamp(0.0, round) } else { 0.0 };
            play.ended = !play.looping && play.time >= round;
        }
        Ok(())
    }

    /// Whether `root`'s events are reported by [`World::animate`].
    pub fn set_animation_reporting(&mut self, root: ObjectId, on: bool) -> Result<()> {
        self.animator(root)?.reporting = on;
        Ok(())
    }

    /// What `root`'s base layer plays.
    pub fn animator_state(&self, root: ObjectId) -> Result<AnimatorState> {
        self.animator_state_on(root, 0)
    }

    /// What `root`'s layer `layer` plays.
    pub fn animator_state_on(&self, root: ObjectId, layer: u32) -> Result<AnimatorState> {
        let animator = self.animator_ref(root)?;
        let current = animator.layers.get(layer as usize).and_then(|l| l.playing.last());
        let (time, length) = match current.map(|p| (p, p.motion)) {
            None => (0.0, 0.0),
            Some((p, Motion::Clip(c))) => (p.time, animator.clips[c as usize].length),
            Some((p, Motion::Blend(b))) => {
                let length = animator.blends[b as usize].length(&animator.clips);
                (p.time * length, length)
            }
        };
        Ok(AnimatorState { motion: current.map(|p| p.motion), time, length, speed: animator.speed })
    }

    /// How much of the base layer's mix clip `clip` is.
    pub fn clip_weight(&self, root: ObjectId, clip: u32) -> Result<f32> {
        self.clip_weight_on(root, 0, clip)
    }

    /// How much of layer `layer`'s mix clip `clip` is: 0 not playing, 1
    /// alone, between while it fades or shares a blend.
    pub fn clip_weight_on(&self, root: ObjectId, layer: u32, clip: u32) -> Result<f32> {
        let animator = self.animator_ref(root)?;
        let Some(layer) = animator.layers.get(layer as usize) else { return Ok(0.0) };
        Ok(layer
            .playing
            .iter()
            .flat_map(|p| animator.parts(p).map(|(c, share, _)| (c, share * p.weight)))
            .filter(|&(c, _)| c == clip as usize)
            .map(|(_, w)| w)
            .sum())
    }

    /// The parts of `root` some clip moves, each once, in the model's node
    /// order: what a frame's [`animate`](Self::animate) may change. Parts
    /// the game destroyed are left out.
    pub fn animated_parts(&self, root: ObjectId) -> Result<Vec<ObjectId>> {
        let animator = self.animator_ref(root)?;
        let mut moved = vec![false; animator.nodes.len()];
        for clip in &animator.clips {
            for channel in &clip.channels {
                moved[channel.node] = true;
            }
        }
        Ok(moved
            .iter()
            .zip(&animator.nodes)
            .filter(|(moved, _)| **moved)
            .filter_map(|(_, &entity)| self.ecs.get::<&ObjectId>(entity).ok().map(|id| *id))
            .collect())
    }

    /// The roots of the animators the last [`animate`](Self::animate)
    /// posed: those playing and shown. Only their parts may have moved.
    pub fn posed(&self) -> &[ObjectId] {
        &self.posed
    }

    /// The number of animators: models spawned with clips, still here.
    pub fn animator_count(&self) -> usize {
        self.animators.len()
    }

    /// Advances every animator by `dt` seconds and poses its model: once a
    /// frame, before the frame is drawn. Animators of models that aren't
    /// shown keep time but don't pose. What the reporting ones did is
    /// added to `events`; those that posed are [`posed`](Self::posed).
    pub fn animate(&mut self, dt: f32, events: &mut Vec<AnimationEvent>) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut animators = std::mem::take(&mut self.animators);
        self.posed.clear();
        for (&root, animator) in &mut animators {
            if !animator.playing() {
                continue;
            }
            animator.advance(root, dt, events);
            if self.active_in_hierarchy(root).unwrap_or(false) {
                animator.pose(&mut self.ecs);
                self.posed.push(root);
            }
            animator.tidy();
        }
        self.animators = animators;
    }
}

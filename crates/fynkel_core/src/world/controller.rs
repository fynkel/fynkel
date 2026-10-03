//! The Animator Controller in the tick (ADR-0032 § 6): an `Animator`
//! whose `controller` names a registered [`ControllerDef`] runs its state
//! machine (`fynkel_animation::controller`) instead of its `clip`, and
//! this module turns the machine's states and clocks into a pose — each
//! layer's current state (blended with the one fading out), the layers
//! over each other (override at the layer's weight, or additive), the
//! base layer's root motion and each layer's dominant motion's markers.
//!
//! Motions resolve like the animator's own clip does: by name under the
//! animator (under the layer's `root` node when it has one — the mask by
//! placement moved into the asset), cached per state while every target
//! still lives under its name. A state's motion is a tree of clips
//! (ADR-0033 § 1): a clip is one leaf, a blend space a 1D tree of its
//! points, a blend tree itself; every leaf shares the state's normalized
//! time, and the state's length is the leaves' weighted by the tree.
//!
//! Every layer also has a **slot** (ADR-0033 § 2): a one-shot played into
//! it from code covers the layer's machine — blending in, playing once,
//! blending out — while the machine keeps running underneath. A layer
//! without states shows only its slot: the one-shot's weight scales the
//! layer's then.

#![cfg_attr(not(feature = "animation"), allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::sync::Arc;

use fynkel_contracts::{AssetRef, Entity};
use fynkel_ecs::World as Ecs;
use fynkel_scene::{Parent, Transform};
use glam::Vec3;

use super::animation::{
    find_binding, pin_root, root_delta, AnimatorTick, BlendSlot, BlendTable, ClipTable,
};

use crate::animation::blend_tree::{direct_weights, weights, TreeDef, TreeKind};
use crate::animation::controller::{
    masked_in, Blending, ControllerDef, ControllerRun, MotionDef, ParameterKind, StateEvent, Value,
};
use crate::animation::{Clip, Pose};
use crate::components::Animator;
use crate::schema::{AnimatorLayerStateRecord, AnimatorParameterKind, AnimatorParameterRecord};

/// Registered Animator Controllers by reference.
pub(super) type ControllerTable = HashMap<AssetRef, Arc<ControllerDef>>;

/// One animator's run of its controller (on `AnimatorRun::controller`).
#[derive(Clone, Debug)]
pub struct ControllerPlay {
    /// The controller this run is for.
    pub controller: AssetRef,
    pub def: Arc<ControllerDef>,
    pub run: ControllerRun,
    /// Events of `PlayState` commands, raised with the next tick's.
    pub pending: Vec<StateEvent>,
    /// Resolved motions, per layer and state.
    motions: Vec<Vec<Option<Motion>>>,
    /// Each target's transform when a motion first resolved it: what a
    /// layer over a node the layers below do not pose starts from (its
    /// transform now would be last tick's pose, compounding).
    rest: HashMap<Entity, Transform>,
    binding_validation: Option<Vec<String>>,
    /// Scratch kept for its capacity: each state's length this tick
    /// (flattened, `offsets[layer] + state`), the layers' clocks before
    /// the step, the incoming side of a crossfade.
    lengths: Vec<f32>,
    offsets: Vec<usize>,
    before: Vec<Before>,
    incoming: Pose,
    /// Each layer's slot (ADR-0033 § 2).
    pub slots: Vec<SlotRun>,
    /// Scratch: a one-shot's pose.
    shot_pose: Pose,
    /// Per layer: the machine's pose last tick (under its slot), the
    /// pose an interruption froze (ADR-0033 § 4) and the run's
    /// interruption count it was frozen at.
    shown: Vec<Pose>,
    frozen: Vec<Pose>,
    interruptions: Vec<u32>,
    /// Per layer with a mask: whether it poses each node met so far.
    masks: Vec<HashMap<Entity, bool>>,
    /// The IK goals' targets from code and what they carry over ticks
    /// (ADR-0033 § 5).
    pub ik: super::ik::IkRun,
}

/// A layer's slot: the one-shot playing, and one blending out (stopped,
/// or replaced by the one playing).
#[derive(Clone, Debug, Default)]
pub struct SlotRun {
    pub current: Option<OneShot>,
    pub fading: Option<OneShot>,
}

/// A one-shot (ADR-0033 § 2).
#[derive(Clone, Debug)]
pub struct OneShot {
    /// The clip or blend space.
    pub motion: AssetRef,
    /// Seconds played (its own, times its speed).
    pub elapsed: f32,
    pub speed: f32,
    pub blend_in: f32,
    pub blend_out: f32,
    /// Stopped (or replaced): seconds since, over how long it blends
    /// out, from which weight.
    stop: Option<(f32, f32, f32)>,
    /// Played this tick: its first tick samples the start.
    fresh: bool,
    /// The motion resolved under the layer's root.
    resolved: Option<Motion>,
    /// Its length when last resolved, seconds.
    pub length: f32,
}

impl OneShot {
    /// How much of it shows now.
    pub fn weight(&self) -> f32 {
        if let Some((since, over, from)) = self.stop {
            return if over <= 0.0 {
                0.0
            } else {
                from * (1.0 - since / over).clamp(0.0, 1.0)
            };
        }
        let win = if self.blend_in > 0.0 {
            (self.elapsed / self.blend_in).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let wout = if self.blend_out > 0.0 && self.length > 0.0 {
            ((self.length - self.elapsed) / self.blend_out).clamp(0.0, 1.0)
        } else {
            1.0
        };
        win * wout
    }

    /// Its normalized time, 0 → 1.
    pub fn normalized(&self) -> f32 {
        if self.length > 0.0 {
            (self.elapsed / self.length).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    fn stopped(mut self, blend_out: f32) -> OneShot {
        let from = self.weight();
        self.stop = Some((0.0, blend_out, from));
        self
    }
}

impl ControllerPlay {
    pub(super) fn collect_missing_bindings(&self, out: &mut Vec<(AssetRef, String)>) {
        out.extend(
            self.binding_validation
                .iter()
                .flatten()
                .map(|key| (self.controller.clone(), key.clone())),
        );
        for motion in self.motions.iter().flatten().flatten() {
            for slot in &motion.slots {
                slot.collect_missing_bindings(out);
            }
        }
        for shot in self
            .slots
            .iter()
            .flat_map(|s| s.current.iter().chain(s.fading.iter()))
        {
            if let Some(motion) = &shot.resolved {
                for slot in &motion.slots {
                    slot.collect_missing_bindings(out);
                }
            }
        }
    }

    pub(super) fn invalidate_bindings(&mut self) {
        self.binding_validation = None;
        for layer in &mut self.motions {
            layer.fill(None);
        }
        for slot in &mut self.slots {
            for shot in slot.current.iter_mut().chain(slot.fading.iter_mut()) {
                shot.resolved = None;
            }
        }
        for mask in &mut self.masks {
            mask.clear();
        }
        self.ik.invalidate_bindings();
        for pose in self.shown.iter_mut().chain(self.frozen.iter_mut()) {
            pose.clear();
        }
    }

    pub fn new(controller: AssetRef, def: Arc<ControllerDef>) -> ControllerPlay {
        let mut offsets = Vec::with_capacity(def.layers.len());
        let mut n = 0;
        for layer in &def.layers {
            offsets.push(n);
            n += layer.states.len();
        }
        ControllerPlay {
            controller,
            run: ControllerRun::new(&def),
            motions: def
                .layers
                .iter()
                .map(|l| vec![None; l.states.len()])
                .collect(),
            rest: HashMap::new(),
            binding_validation: None,
            lengths: vec![0.0; n],
            offsets,
            before: Vec::with_capacity(def.layers.len()),
            incoming: Pose::default(),
            slots: vec![SlotRun::default(); def.layers.len()],
            shot_pose: Pose::default(),
            shown: vec![Pose::default(); def.layers.len()],
            frozen: vec![Pose::default(); def.layers.len()],
            interruptions: vec![0; def.layers.len()],
            masks: vec![HashMap::new(); def.layers.len()],
            ik: super::ik::IkRun::new(def.ik.len()),
            def,
            pending: Vec::new(),
        }
    }
}

/// A parameter a tree reads: the controller's, or a fixed value (a
/// blend space whose parameter the controller does not have stays at
/// its first point).
#[derive(Clone, Copy, Debug)]
enum Param {
    Index(usize),
    Fixed(f32),
}

impl Param {
    fn value(self, run: &ControllerRun) -> f32 {
        match self {
            Param::Index(i) => run.values.get(i).map_or(0.0, |v| v.as_f32()),
            Param::Fixed(v) => v,
        }
    }
}

/// A resolved motion's shape: a clip (an index into the motion's slots)
/// or a tree of them.
#[derive(Clone, Debug)]
enum Node {
    Leaf(usize),
    Tree {
        kind: TreeKind,
        params: [Param; 2],
        positions: Vec<[f32; 2]>,
        /// Each child with its time scale and (a direct tree's) weight
        /// parameter.
        children: Vec<(Node, f32, Param)>,
    },
}

impl Node {
    /// Calls `f(slot, weight, scale)` for every leaf with a weight, the
    /// weights multiplied down the tree, `scale` the time scales'.
    fn visit(&self, run: &ControllerRun, w: f32, scale: f32, f: &mut dyn FnMut(usize, f32, f32)) {
        match self {
            Node::Leaf(i) => f(*i, w, scale),
            Node::Tree {
                kind,
                params,
                positions,
                children,
            } => {
                let n = children.len().min(TreeDef::MAX_CHILDREN);
                let mut ws = [0.0f32; TreeDef::MAX_CHILDREN];
                if *kind == TreeKind::Direct {
                    let mut values = [0.0f32; TreeDef::MAX_CHILDREN];
                    for (v, (_, _, p)) in values.iter_mut().zip(children) {
                        *v = p.value(run);
                    }
                    direct_weights(&values[..n], &mut ws[..n]);
                } else {
                    let input = [params[0].value(run), params[1].value(run)];
                    weights(*kind, &positions[..n], input, &mut ws[..n]);
                }
                for (k, (child, time_scale, _)) in children.iter().take(n).enumerate() {
                    if ws[k] > 0.0 {
                        child.visit(run, w * ws[k], scale * time_scale, f);
                    }
                }
            }
        }
    }
}

/// A state's motion as a layer of one animator resolved it.
#[derive(Clone, Debug)]
struct Motion {
    root: Node,
    slots: Vec<BlendSlot>,
    /// The first clip's first frame over its targets: an additive
    /// layer's zero.
    reference: Pose,
}

impl Motion {
    fn valid(&self, ecs: &Ecs) -> bool {
        self.slots.iter().all(|s| s.valid(ecs))
    }

    /// Every clip playing now: `f(slot, weight, time scale)`.
    fn leaves(&self, run: &ControllerRun, f: &mut dyn FnMut(usize, f32, f32)) {
        self.root.visit(run, 1.0, 1.0, f);
    }

    /// The motion's length now, seconds: its clips' lengths (over their
    /// time scales) weighted as they play.
    fn length(&self, run: &ControllerRun) -> f32 {
        let mut length = 0.0;
        self.leaves(run, &mut |i, w, scale| {
            length += w * self.slots[i].clip.duration.max(0.0) / scale;
        });
        length
    }

    /// The heaviest clip now (markers come from it).
    fn dominant(&self, run: &ControllerRun) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        self.leaves(run, &mut |i, w, _| {
            if best.is_none_or(|(_, b)| w > b) {
                best = Some((i, w));
            }
        });
        best.map(|(i, _)| i)
    }
}

/// A clip's unwrapped seconds at the state's normalized time `n`.
fn seconds(clip: &Clip, n: f32, looping: bool) -> f32 {
    let d = clip.duration.max(0.0);
    if looping {
        n * d
    } else {
        n.clamp(0.0, 1.0) * d
    }
}

/// The unwrapped seconds of a clip at normalized `n` → the time to sample.
fn wrap(clip: &Clip, seconds: f32, looping: bool) -> f32 {
    let d = clip.duration.max(0.0);
    if looping && d > 0.0 {
        seconds.rem_euclid(d)
    } else {
        seconds.clamp(0.0, d)
    }
}

/// What one layer's clocks were before the step.
#[derive(Clone, Copy, Debug)]
struct Before {
    state: Option<usize>,
    time: f32,
    fade: Option<(usize, f32)>,
}

/// Everything the tick reads besides the animator.
pub(super) struct Tables<'a> {
    pub ecs: &'a Ecs,
    pub clips: &'a ClipTable,
    pub blends: &'a BlendTable,
}

/// The `ControllerPlay` for the animator's controller, made (or remade,
/// when the animator names another controller or the registered one was
/// replaced) as needed; `Err` names what is missing.
pub(super) fn play_for<'p>(
    controllers: &ControllerTable,
    controller: &AssetRef,
    play: &'p mut Option<ControllerPlay>,
) -> Result<&'p mut ControllerPlay, AssetRef> {
    let Some(def) = controllers.get(controller) else {
        *play = None;
        return Err(controller.clone());
    };
    let current = play
        .as_ref()
        .is_some_and(|p| p.controller == *controller && Arc::ptr_eq(&p.def, def));
    if !current {
        *play = Some(ControllerPlay::new(controller.clone(), def.clone()));
    }
    Ok(play.as_mut().expect("just set"))
}

/// Resolves (or re-resolves) the motion of `state` in `layer`; left
/// `None` for no motion, one with a clip or space that is not registered
/// (reported through `missing`), or a layer whose root node is not
/// there.
fn resolve(
    t: &Tables,
    entity: Entity,
    play: &mut ControllerPlay,
    layer: usize,
    state: usize,
    missing: &mut Option<AssetRef>,
) {
    if play.motions[layer][state]
        .as_ref()
        .is_some_and(|m| m.valid(t.ecs))
    {
        return;
    }
    play.motions[layer][state] = None;
    let def = play.def.clone();
    let Some(motion) = def.layers[layer].states[state].motion.as_ref() else {
        return;
    };
    let root = match &def.layers[layer].root {
        None => entity,
        Some(name) => match find_binding(t.ecs, entity, name) {
            Some(e) => e,
            None => return,
        },
    };
    let mut slots = Vec::new();
    let Some(node) = build(t, root, &def, motion, &mut slots, missing) else {
        return;
    };
    let Some(first) = slots.first() else {
        return;
    };
    let mut reference = Pose::default();
    first.clip.sample(0.0, &first.targets, &mut reference);
    for e in slots.iter().flat_map(|s| s.targets.iter().flatten()) {
        if let std::collections::hash_map::Entry::Vacant(v) = play.rest.entry(*e) {
            if let Ok(tr) = t.ecs.copy::<Transform>(*e) {
                v.insert(tr);
            }
        }
    }
    play.motions[layer][state] = Some(Motion {
        root: node,
        slots,
        reference,
    });
}

/// A one-shot's motion resolved under the layer's root (again when a
/// target went); `None` when it cannot be (reported through `missing`).
fn resolve_shot(
    t: &Tables,
    entity: Entity,
    def: &ControllerDef,
    layer: usize,
    shot: &mut OneShot,
    run: &ControllerRun,
    missing: &mut Option<AssetRef>,
) {
    if shot.resolved.as_ref().is_some_and(|m| m.valid(t.ecs)) {
        shot.length = shot.resolved.as_ref().map_or(0.0, |m| m.length(run));
        return;
    }
    shot.resolved = None;
    let root = match &def.layers[layer].root {
        None => entity,
        Some(name) => match find_binding(t.ecs, entity, name) {
            Some(e) => e,
            None => return,
        },
    };
    let mut slots = Vec::new();
    let motion = MotionDef::Asset(shot.motion.clone());
    let Some(node) = build(t, root, def, &motion, &mut slots, missing) else {
        return;
    };
    let Some(first) = slots.first() else {
        return;
    };
    let mut reference = Pose::default();
    first.clip.sample(0.0, &first.targets, &mut reference);
    let m = Motion {
        root: node,
        slots,
        reference,
    };
    shot.length = m.length(run);
    shot.resolved = Some(m);
}

/// A one-shot's pose at its time into `into` (cleared first).
fn sample_shot(shot: &OneShot, run: &ControllerRun, into: &mut Pose, scratch: &mut Pose) -> bool {
    let Some(m) = shot.resolved.as_ref() else {
        return false;
    };
    into.clear();
    let n = shot.normalized();
    let mut acc = 0.0f32;
    m.leaves(run, &mut |i, w, _| {
        let slot = &m.slots[i];
        let time = wrap(&slot.clip, seconds(&slot.clip, n, false), false);
        if acc == 0.0 {
            slot.clip.sample(time, &slot.targets, into);
        } else {
            scratch.clear();
            slot.clip.sample(time, &slot.targets, scratch);
            into.blend(scratch, w / (acc + w));
        }
        acc += w;
    });
    true
}

/// `pose` blended towards `other` by `weight`, what `pose` does not set
/// starting from rest (so a node only `other` poses is not snapped).
fn blend_over(
    t: &Tables,
    rest: &HashMap<Entity, Transform>,
    pose: &mut Pose,
    other: &Pose,
    weight: f32,
) {
    for e in &other.entries {
        if pose.get(e.entity).is_none() {
            if let Some(tr) = rest_of(t, rest, e.entity) {
                pose.entries.push(crate::animation::PoseEntry {
                    entity: e.entity,
                    translation: Some(tr.position),
                    rotation: Some(tr.rotation),
                    scale: Some(tr.scale),
                });
            }
        }
    }
    pose.blend(other, weight);
}

/// A motion's node, its clips resolved under `root` into `slots`.
fn build(
    t: &Tables,
    root: Entity,
    def: &ControllerDef,
    motion: &MotionDef,
    slots: &mut Vec<BlendSlot>,
    missing: &mut Option<AssetRef>,
) -> Option<Node> {
    let clip = |r: &AssetRef, slots: &mut Vec<BlendSlot>, missing: &mut Option<AssetRef>| {
        let Some(c) = t.clips.get(r) else {
            *missing = Some(r.clone());
            return None;
        };
        slots.push(BlendSlot::resolve(t.ecs, root, r.clone(), c));
        Some(Node::Leaf(slots.len() - 1))
    };
    match motion {
        MotionDef::Asset(r) => {
            if t.clips.contains_key(r) {
                return clip(r, slots, missing);
            }
            let Some(space) = t.blends.get(r) else {
                *missing = Some(r.clone());
                return None;
            };
            // A blend space is a 1D tree of its points.
            let first = space.points.first().map_or(0.0, |p| p.at);
            let param = def
                .parameter_index(&space.parameter)
                .filter(|&i| def.parameters[i].kind == ParameterKind::Float)
                .map_or(Param::Fixed(first), Param::Index);
            let mut children = Vec::with_capacity(space.points.len());
            for p in &space.points {
                children.push((clip(&p.clip, slots, missing)?, 1.0, Param::Fixed(0.0)));
            }
            if children.is_empty() {
                return None;
            }
            Some(Node::Tree {
                kind: TreeKind::OneD,
                params: [param, Param::Fixed(0.0)],
                positions: space.points.iter().map(|p| [p.at, 0.0]).collect(),
                children,
            })
        }
        MotionDef::Tree(tree) => {
            let mut params = [Param::Fixed(0.0); 2];
            for (k, &p) in tree.parameters.iter().take(2).enumerate() {
                params[k] = Param::Index(p);
            }
            let mut children = Vec::with_capacity(tree.children.len());
            for c in &tree.children {
                let node = build(t, root, def, &c.motion, slots, missing)?;
                children.push((
                    node,
                    c.time_scale,
                    c.parameter.map_or(Param::Fixed(0.0), Param::Index),
                ));
            }
            Some(Node::Tree {
                kind: tree.kind,
                params,
                positions: tree.children.iter().map(|c| c.position).collect(),
                children,
            })
        }
    }
}

/// One tick of an animator that runs a controller: the machine steps
/// (unless the animator is paused), the layers' poses land in `pose`,
/// events, markers and root motion in `out`. `scratch` and `layer_pose`
/// are the caller's reusable poses.
#[allow(clippy::too_many_arguments)]
pub(super) fn tick(
    t: &Tables,
    entity: Entity,
    animator: &Animator,
    play: &mut ControllerPlay,
    dt: f32,
    pose: &mut Pose,
    scratch: &mut Pose,
    layer_pose: &mut Pose,
    out: &mut AnimatorTick,
) {
    if play.binding_validation.is_none() {
        let missing = play
            .def
            .layers
            .iter()
            .flat_map(|layer| layer.root.iter().chain(layer.mask.iter().map(|m| &m.node)))
            .filter(|key| find_binding(t.ecs, entity, key).is_none())
            .cloned()
            .collect();
        play.binding_validation = Some(missing);
        play.rest.retain(|node, _| {
            let mut at = *node;
            while t.ecs.contains(at) {
                if at == entity {
                    return true;
                }
                match t.ecs.get::<Parent>(at) {
                    Ok(p) => at = p.0,
                    Err(_) => break,
                }
            }
            false
        });
    }
    let def = play.def.clone();
    play.before.clear();
    play.before.extend(play.run.layers.iter().map(|l| Before {
        state: l.state,
        time: l.time,
        fade: l.crossfade.map(|f| (f.from, f.from_time)),
    }));
    let mut events = std::mem::take(&mut play.pending);
    // Every state's motion resolved before the step, for its length.
    let mut missing = None;
    for (l, layer) in def.layers.iter().enumerate() {
        for (s, st) in layer.states.iter().enumerate() {
            if st.motion.is_some() {
                resolve(t, entity, play, l, s, &mut missing);
            }
        }
    }
    if let Some(m) = missing {
        if out.unavailable.is_none() {
            out.unavailable = Some(m);
        }
    }
    let advanced = animator.playing;
    if advanced {
        for l in 0..def.layers.len() {
            for s in 0..def.layers[l].states.len() {
                let i = play.offsets[l] + s;
                play.lengths[i] = play.length(l, s);
            }
        }
        let (lengths, offsets) = (&play.lengths, &play.offsets);
        let mut length = |l: usize, s: usize| lengths[offsets[l] + s];
        play.run
            .step(&def, dt * animator.speed, &mut length, &mut events);
    }

    // The slots: each one-shot resolved and its clock advanced; one that
    // reached its end is gone (and ended), one blended out is dropped.
    for l in 0..def.layers.len() {
        let mut slot = std::mem::take(&mut play.slots[l]);
        for shot in [&mut slot.current, &mut slot.fading].into_iter().flatten() {
            let mut missing = None;
            resolve_shot(t, entity, &def, l, shot, &play.run, &mut missing);
            if let Some(m) = missing {
                if out.unavailable.is_none() {
                    out.unavailable = Some(m);
                }
            }
            let prev = shot.elapsed;
            if shot.fresh {
                shot.fresh = false;
            } else if advanced {
                let step = dt * animator.speed * shot.speed;
                shot.elapsed = (shot.elapsed + step).min(shot.length.max(0.0));
                if let Some((since, _, _)) = &mut shot.stop {
                    *since += dt * animator.speed;
                }
            }
            // Its markers, while it plays (not while it blends out).
            if shot.stop.is_none() && shot.elapsed > prev {
                shot_markers(shot, &play.run, prev, out);
            }
        }
        if let Some(shot) = &slot.current {
            if shot.resolved.is_some() && shot.elapsed >= shot.length {
                out.one_shots_ended.push(shot.motion.clone());
                slot.current = None;
            }
        }
        if slot
            .fading
            .as_ref()
            .is_some_and(|f| f.weight() <= 0.0 || f.elapsed >= f.length)
        {
            slot.fading = None;
        }
        play.slots[l] = slot;
    }

    // The layers, base first.
    pose.clear();
    for (l, layer) in def.layers.iter().enumerate() {
        let lr = play.run.layers[l].clone();
        let shots = [
            play.slots[l].fading.as_ref(),
            play.slots[l].current.as_ref(),
        ];
        let Some(current) = lr.state else {
            // A layer without states (ADR-0033 § 2): only its slot shows,
            // at the one-shots' weight times the layer's.
            layer_pose.clear();
            let mut shown = 0.0f32;
            for shot in shots.into_iter().flatten() {
                let w = shot.weight();
                if w <= 0.0 || !sample_shot(shot, &play.run, &mut play.shot_pose, scratch) {
                    continue;
                }
                if shown == 0.0 {
                    layer_pose.clear();
                    layer_pose
                        .entries
                        .extend_from_slice(&play.shot_pose.entries);
                } else {
                    layer_pose.blend(&play.shot_pose, w);
                }
                shown = w + shown * (1.0 - w);
            }
            let weight = layer.weight.clamp(0.0, 1.0) * shown;
            if weight <= 0.0 {
                continue;
            }
            mask_pose(t, entity, &def, play, l, layer_pose);
            let reference = play.slots[l]
                .current
                .as_ref()
                .or(play.slots[l].fading.as_ref())
                .and_then(|s| s.resolved.as_ref())
                .map(|m| &m.reference);
            match (layer.blending, reference) {
                (Blending::Additive, Some(r)) if l > 0 => {
                    pose.add_delta(layer_pose, r, weight, &mut |e| rest_of(t, &play.rest, e));
                }
                _ => blend_over(t, &play.rest, pose, layer_pose, weight),
            }
            continue;
        };
        let target: &mut Pose = if l == 0 { &mut *pose } else { &mut *layer_pose };
        target.clear();
        let base_root = l == 0 && animator.playing;
        let root_on = |s: usize| layer.states[s].root_motion.unwrap_or(animator.root_motion);
        // The incoming (current) state.
        let mut sample = |state: usize, n: f32, into: &mut Pose| -> bool {
            let st = &layer.states[state];
            let Some(m) = play.motions[l][state].as_ref() else {
                return false;
            };
            let pin = l == 0 && root_on(state);
            // The leaves blended in one by one, each by its share of
            // what is in so far.
            let mut acc = 0.0f32;
            m.leaves(&play.run, &mut |i, w, _| {
                let slot = &m.slots[i];
                let time = wrap(&slot.clip, seconds(&slot.clip, n, st.looping), st.looping);
                let dst: &mut Pose = if acc == 0.0 {
                    &mut *into
                } else {
                    &mut *scratch
                };
                if acc != 0.0 {
                    dst.clear();
                }
                slot.clip.sample(time, &slot.targets, dst);
                if pin {
                    if let Some(root) = &slot.root {
                        pin_root(dst, &slot.clip, root, time);
                    }
                }
                if acc != 0.0 {
                    into.blend(scratch, w / (acc + w));
                }
                acc += w;
            });
            true
        };
        // An interruption this tick: what showed last tick is held.
        if lr.interruptions != play.interruptions[l] {
            play.interruptions[l] = lr.interruptions;
            let frozen = &mut play.frozen[l];
            frozen.clear();
            frozen.entries.extend_from_slice(&play.shown[l].entries);
        }
        match lr.crossfade {
            Some(fade) if fade.frozen => {
                // The held pose under, the incoming blended in.
                let mut incoming = std::mem::take(&mut play.incoming);
                incoming.clear();
                target.entries.extend_from_slice(&play.frozen[l].entries);
                sample(current, lr.time, &mut incoming);
                target.blend(&incoming, fade.weight());
                play.incoming = incoming;
            }
            Some(fade) => {
                // The outgoing state under, the incoming blended in.
                let mut incoming = std::mem::take(&mut play.incoming);
                incoming.clear();
                sample(fade.from, fade.from_time, target);
                sample(current, lr.time, &mut incoming);
                target.blend(&incoming, fade.weight());
                play.incoming = incoming;
            }
            None => {
                sample(current, lr.time, target);
            }
        }
        let shown = &mut play.shown[l];
        shown.clear();
        shown.entries.extend_from_slice(&target.entries);
        // The slot covers the machine (ADR-0033 § 2).
        for shot in shots.into_iter().flatten() {
            let w = shot.weight();
            if w > 0.0 && sample_shot(shot, &play.run, &mut play.shot_pose, scratch) {
                blend_over(t, &play.rest, target, &play.shot_pose, w);
            }
        }
        // The mask (ADR-0033 § 5): the nodes it leaves out keep what the
        // layers below gave them.
        mask_pose(t, entity, &def, play, l, target);

        // Markers and root motion: what advanced this tick.
        let b = play.before[l];
        let w_in = lr.crossfade.map_or(1.0, |f| f.weight());
        // The incoming state advanced when it was current before the step
        // and was not re-entered.
        let entered_now = events
            .iter()
            .any(|e| matches!(e, StateEvent::Entered { layer, .. } if *layer == l));
        let in_span = (advanced && !entered_now && b.state == Some(current))
            .then_some((current, b.time, lr.time));
        // The outgoing state advanced: it was the fade's before, or it was
        // current and a transition out of it fired this tick.
        let out_span = lr.crossfade.and_then(|f| {
            let prev = match b.fade {
                Some((s, time)) if s == f.from => Some(time),
                _ if b.state == Some(f.from) => Some(b.time),
                _ => None,
            }?;
            advanced.then_some((f.from, prev, f.from_time))
        });
        let dominant = if w_in >= 0.5 { in_span } else { out_span };
        if let Some((s, prev, next)) = dominant {
            markers(&def, play, l, s, prev, next, out);
        }
        if base_root {
            let mut delta = Vec3::ZERO;
            let mut yaw = 0.0;
            let mut axis = None;
            for (span, w) in [(in_span, w_in), (out_span, 1.0 - w_in)] {
                let Some((s, prev, next)) = span else {
                    continue;
                };
                if w <= 0.0 || !root_on(s) {
                    continue;
                }
                if let Some((d, a, y)) = root_of(t, entity, &def, play, l, s, prev, next) {
                    delta += d * w;
                    yaw += y * w;
                    axis.get_or_insert(a);
                }
            }
            if let Some(a) = axis {
                out.root = Some((delta, a, yaw));
            }
        }

        // Layers over the base.
        if l > 0 {
            let weight = layer.weight.clamp(0.0, 1.0);
            if weight <= 0.0 {
                continue;
            }
            match layer.blending {
                Blending::Override => {
                    // What the layers below do not set starts from rest.
                    for e in &layer_pose.entries {
                        if pose.get(e.entity).is_none() {
                            if let Some(tr) = rest_of(t, &play.rest, e.entity) {
                                pose.entries.push(crate::animation::PoseEntry {
                                    entity: e.entity,
                                    translation: Some(tr.position),
                                    rotation: Some(tr.rotation),
                                    scale: Some(tr.scale),
                                });
                            }
                        }
                    }
                    pose.blend(layer_pose, weight);
                }
                Blending::Additive => {
                    let reference = play.motions[l][current].as_ref();
                    if let Some(m) = reference {
                        pose.add_delta(layer_pose, &m.reference, weight, &mut |e| {
                            rest_of(t, &play.rest, e)
                        });
                    }
                }
            }
        }
    }
    layer_pose.clear();
    scratch.clear();

    for e in events.drain(..) {
        let name = |l: usize, s: usize| def.layers[l].states[s].name.clone();
        out.states.push(match e {
            StateEvent::Entered { layer, state, from } => (
                layer as u32,
                true,
                name(layer, state),
                from.map(|f| name(layer, f)).unwrap_or_default(),
            ),
            StateEvent::Exited { layer, state } => {
                (layer as u32, false, name(layer, state), String::new())
            }
        });
    }
    play.pending = events;
}

/// Drops from `pose` what layer `l`'s mask does not pose (ADR-0033 § 5):
/// a node's names from it up to the animator, decided once per node.
fn mask_pose(
    t: &Tables,
    entity: Entity,
    def: &ControllerDef,
    play: &mut ControllerPlay,
    l: usize,
    pose: &mut Pose,
) {
    let mask = &def.layers[l].mask;
    if mask.is_empty() {
        return;
    }
    let cache = &mut play.masks[l];
    pose.entries.retain(|e| {
        *cache.entry(e.entity).or_insert_with(|| {
            let mut names = Vec::new();
            let mut at = e.entity;
            loop {
                if let Ok(n) = t.ecs.get::<crate::AnimationBinding>(at) {
                    names.push(n.key.clone());
                }
                if at == entity {
                    break;
                }
                match t.ecs.get::<Parent>(at) {
                    Ok(p) => at = p.0,
                    Err(_) => break,
                }
            }
            masked_in(mask, names.iter().map(String::as_str))
        })
    });
}

/// A target's rest transform (its transform now if it has none).
fn rest_of(t: &Tables, rest: &HashMap<Entity, Transform>, e: Entity) -> Option<Transform> {
    rest.get(&e)
        .copied()
        .or_else(|| t.ecs.copy::<Transform>(e).ok())
}

/// The markers a state's dominant clip crossed between two normalized
/// times.
fn markers(
    def: &ControllerDef,
    play: &ControllerPlay,
    l: usize,
    s: usize,
    prev: f32,
    next: f32,
    out: &mut AnimatorTick,
) {
    let st = &def.layers[l].states[s];
    let Some(m) = play.motions[l][s].as_ref() else {
        return;
    };
    let Some(k) = m.dominant(&play.run) else {
        return;
    };
    let slot = &m.slots[k];
    let a = seconds(&slot.clip, prev, st.looping);
    let b = seconds(&slot.clip, next, st.looping);
    let mut crossed = Vec::new();
    slot.clip.markers_crossed(a, b, st.looping, &mut crossed);
    for i in crossed {
        out.markers
            .push((slot.clip_ref.clone(), slot.clip.markers[i].name.clone()));
    }
}

/// The markers a one-shot's heaviest clip crossed since `prev` seconds.
fn shot_markers(shot: &OneShot, run: &ControllerRun, prev: f32, out: &mut AnimatorTick) {
    let Some(m) = shot.resolved.as_ref() else {
        return;
    };
    let Some(k) = m.dominant(run) else {
        return;
    };
    let slot = &m.slots[k];
    let (a, b) = if shot.length > 0.0 {
        (prev / shot.length, shot.elapsed / shot.length)
    } else {
        return;
    };
    let mut crossed = Vec::new();
    slot.clip.markers_crossed(
        seconds(&slot.clip, a, false),
        seconds(&slot.clip, b, false),
        false,
        &mut crossed,
    );
    for i in crossed {
        out.markers
            .push((slot.clip_ref.clone(), slot.clip.markers[i].name.clone()));
    }
}

/// A state's root motion between two normalized times: its clips'
/// deltas, blended like its pose.
#[allow(clippy::too_many_arguments)]
fn root_of(
    t: &Tables,
    entity: Entity,
    def: &ControllerDef,
    play: &ControllerPlay,
    l: usize,
    s: usize,
    prev: f32,
    next: f32,
) -> Option<(Vec3, Vec3, f32)> {
    let st = &def.layers[l].states[s];
    let m = play.motions[l][s].as_ref()?;
    let mut result: Option<(Vec3, Vec3, f32)> = None;
    m.leaves(&play.run, &mut |i, w, _| {
        let slot = &m.slots[i];
        let Some(root) = &slot.root else { return };
        let from = seconds(&slot.clip, prev, st.looping);
        let to = seconds(&slot.clip, next, st.looping);
        let Some((d, axis, y)) = root_delta(t.ecs, entity, &slot.clip, root, from, to, st.looping)
        else {
            return;
        };
        result = Some(match result {
            None => (d * w, axis, y * w),
            Some((rd, ra, ry)) => (rd + d * w, ra, ry + y * w),
        });
    });
    result
}

/// `FYNKEL_QUERY_ANIMATOR_CONTROLLER`: the layers' states and the
/// parameters of the controller the animator runs.
pub(super) fn state(
    entity: Entity,
    play: &ControllerPlay,
) -> (Vec<AnimatorLayerStateRecord>, Vec<AnimatorParameterRecord>) {
    let motions_length = |l: usize, s: usize| play.length(l, s);
    let def = &play.def;
    let layers = def
        .layers
        .iter()
        .enumerate()
        .map(|(l, layer)| {
            let lr = &play.run.layers[l];
            let name =
                |s: Option<usize>| s.map(|s| layer.states[s].name.clone()).unwrap_or_default();
            let current = play.slots[l].current.as_ref();
            let shot = current.map(|s| s.motion.clone()).unwrap_or_default();
            let shot_time = current.map_or(0.0, |s| s.normalized());
            let shot_weight = current.map_or(0.0, |s| s.weight());
            match lr.crossfade {
                // Unity: the current state is the one fading out, the
                // next the one fading in.
                Some(f) => AnimatorLayerStateRecord {
                    entity,
                    layer: l as u32,
                    slot: layer.name.clone(),
                    one_shot: shot.clone(),
                    one_shot_time: shot_time,
                    one_shot_weight: shot_weight,
                    state: name(Some(f.from)),
                    normalized_time: f.from_time,
                    length: motions_length(l, f.from),
                    in_transition: true,
                    next: name(lr.state),
                    next_normalized_time: lr.time,
                    transition_progress: f.weight(),
                },
                None => AnimatorLayerStateRecord {
                    entity,
                    layer: l as u32,
                    slot: layer.name.clone(),
                    one_shot: shot,
                    one_shot_time: shot_time,
                    one_shot_weight: shot_weight,
                    state: name(lr.state),
                    normalized_time: lr.time,
                    length: lr.state.map_or(0.0, |s| motions_length(l, s)),
                    in_transition: false,
                    next: String::new(),
                    next_normalized_time: 0.0,
                    transition_progress: 0.0,
                },
            }
        })
        .collect();
    let parameters = def
        .parameters
        .iter()
        .zip(&play.run.values)
        .map(|(p, v)| {
            use crate::animation::controller::ParameterKind as K;
            AnimatorParameterRecord {
                entity,
                name: p.name.clone(),
                kind: match p.kind {
                    K::Float => AnimatorParameterKind::Float,
                    K::Int => AnimatorParameterKind::Int,
                    K::Bool => AnimatorParameterKind::Bool,
                    K::Trigger => AnimatorParameterKind::Trigger,
                },
                value: v.as_f32(),
                integer: match v {
                    Value::Int(i) => *i,
                    _ => 0,
                },
                flag: matches!(v, Value::Bool(true)),
            }
        })
        .collect();
    (layers, parameters)
}

impl ControllerPlay {
    /// An IK goal by name (ADR-0033 § 5).
    pub fn ik_index(&self, goal: &str) -> Result<usize, String> {
        ik_index(&self.def, goal)
    }
    pub fn slot_index(&self, slot: &str) -> Result<usize, String> {
        slot_index(&self.def, slot)
    }

    /// Plays `motion` once in layer `l`'s slot; the one playing there
    /// blends out over the new one's blend in.
    pub fn play_one_shot(
        &mut self,
        l: usize,
        motion: AssetRef,
        blend_in: f32,
        blend_out: f32,
        speed: f32,
    ) {
        let slot = &mut self.slots[l];
        if let Some(old) = slot.current.take() {
            slot.fading = Some(old.stopped(blend_in));
        }
        slot.current = Some(OneShot {
            motion,
            elapsed: 0.0,
            speed,
            blend_in,
            blend_out,
            stop: None,
            fresh: true,
            resolved: None,
            length: 0.0,
        });
    }

    /// Blends layer `l`'s one-shot out over `blend_out` seconds.
    pub fn stop_one_shot(&mut self, l: usize, blend_out: f32) {
        let slot = &mut self.slots[l];
        if let Some(old) = slot.current.take() {
            slot.fading = Some(old.stopped(blend_out));
        }
    }

    /// A state's motion length now, seconds; 0 without a resolved motion.
    pub(super) fn length(&self, l: usize, s: usize) -> f32 {
        self.motions[l][s]
            .as_ref()
            .map_or(0.0, |m| m.length(&self.run))
    }
}

pub(super) fn ik_index(def: &ControllerDef, goal: &str) -> Result<usize, String> {
    def.ik_index(goal).ok_or_else(|| {
        let names: Vec<&str> = def.ik.iter().map(|g| g.name.as_str()).collect();
        if names.is_empty() {
            format!("no IK goal `{goal}` (the controller has none)")
        } else {
            format!("no IK goal `{goal}` (the goals: {})", names.join(", "))
        }
    })
}
pub(super) fn slot_index(def: &ControllerDef, slot: &str) -> Result<usize, String> {
    if slot.is_empty() && !def.layers.is_empty() {
        return Ok(0);
    }
    def.layers
        .iter()
        .position(|l| l.name == slot)
        .ok_or_else(|| {
            let names: Vec<&str> = def.layers.iter().map(|l| l.name.as_str()).collect();
            format!("no slot `{slot}` (a slot is a layer: {})", names.join(", "))
        })
}
pub(super) fn state_index(
    def: &ControllerDef,
    state: &str,
    layer: Option<u32>,
) -> Result<(usize, usize), String> {
    let found = match layer {
        Some(l) => def
            .layers
            .get(l as usize)
            .and_then(|ly| ly.state_index(state).map(|s| (l as usize, s))),
        None => def.find_state(state),
    };
    found.ok_or_else(|| {
        let names: Vec<&str> = def
            .layers
            .iter()
            .flat_map(|l| l.states.iter().map(|s| s.name.as_str()))
            .collect();
        format!(
            "no state `{state}`{} (the states: {})",
            layer.map(|l| format!(" in layer {l}")).unwrap_or_default(),
            names.join(", ")
        )
    })
}
pub(super) fn validate_transport(def: &ControllerDef, op: &crate::AnimateOp) -> Result<(), String> {
    match op {
        crate::AnimateOp::SetControllerParameter { name, value } => {
            crate::animation::controller::ControllerRun::validate_value(def, name, *value)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        crate::AnimateOp::PlayState { state, layer, .. } => {
            state_index(def, state, *layer).map(|_| ())
        }
        crate::AnimateOp::PlayOneShot { slot, .. } | crate::AnimateOp::StopOneShot { slot, .. } => {
            slot_index(def, slot).map(|_| ())
        }
        crate::AnimateOp::SetIkTarget { goal, .. } | crate::AnimateOp::ClearIkTarget { goal } => {
            ik_index(def, goal).map(|_| ())
        }
        _ => Ok(()),
    }
}

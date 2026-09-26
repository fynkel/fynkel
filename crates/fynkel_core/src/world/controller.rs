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

#![cfg_attr(not(feature = "animation"), allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::sync::Arc;

use fynkel_contracts::{AssetRef, Entity};
use fynkel_ecs::World as Ecs;
use fynkel_scene::Transform;
use glam::Vec3;

use super::animation::{
    find_named, pin_root, root_delta, AnimatorTick, BlendSlot, BlendTable, ClipTable,
};
use crate::animation::blend_tree::{direct_weights, weights, TreeDef, TreeKind};
use crate::animation::controller::{
    Blending, ControllerDef, ControllerRun, MotionDef, ParameterKind, StateEvent, Value,
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
    /// Scratch kept for its capacity: each state's length this tick
    /// (flattened, `offsets[layer] + state`), the layers' clocks before
    /// the step, the incoming side of a crossfade.
    lengths: Vec<f32>,
    offsets: Vec<usize>,
    before: Vec<Before>,
    incoming: Pose,
}

impl ControllerPlay {
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
            lengths: vec![0.0; n],
            offsets,
            before: Vec::with_capacity(def.layers.len()),
            incoming: Pose::default(),
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
        Some(name) => match find_named(t.ecs, entity, name) {
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

    // The layers, base first.
    pose.clear();
    for (l, layer) in def.layers.iter().enumerate() {
        let lr = play.run.layers[l].clone();
        let Some(current) = lr.state else {
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
        match lr.crossfade {
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
            match lr.crossfade {
                // Unity: the current state is the one fading out, the
                // next the one fading in.
                Some(f) => AnimatorLayerStateRecord {
                    entity,
                    layer: l as u32,
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
    /// A state's motion length now, seconds; 0 without a resolved motion.
    pub(super) fn length(&self, l: usize, s: usize) -> f32 {
        self.motions[l][s]
            .as_ref()
            .map_or(0.0, |m| m.length(&self.run))
    }
}

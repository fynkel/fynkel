//! The Animator Controller's logic (ADR-0032 § 3, § 4, § 6): typed
//! parameters, conditions over them, the choice of transition and the
//! states' clocks. Pure — no entities, no poses: the world
//! (`fynkel_core`'s `run_animation`) asks it each tick which states play
//! at which fraction of their motion and samples those.
//!
//! A [`ControllerDef`] is the shared, read-only machine (one per
//! `.controller.json`), names already resolved to indices; a
//! [`ControllerRun`] is one animator's copy of the parameters and one
//! [`LayerRun`] per layer. The rules each tick, per layer:
//!
//! 1. The clocks advance: the state's (and, during a crossfade, the
//!    outgoing state's) normalized time by `speed * dt / length`, so a
//!    blend space whose length changes with its parameter keeps its
//!    phase; the crossfade by `dt`. A crossfade that ends exits the
//!    outgoing state.
//! 2. Unless a crossfade runs, the transitions are checked — Any
//!    State's first, then the current state's, each in file order — and
//!    the first whose conditions all hold (and whose exit time has come)
//!    fires: it uses up the triggers it tests, the next state starts at
//!    `offset` and is entered at once; a zero-length transition exits
//!    the outgoing state in the same tick. During a crossfade only the
//!    transition that started it may allow more (ADR-0033 § 4,
//!    [`Interruption`]): the incoming state's transitions (`next`), and
//!    Any State's too (`any`). An interruption exits the outgoing state
//!    and fades from the pose of that moment, frozen
//!    ([`Crossfade::frozen`]; the world keeps the pose).
//!
//! A sub-state machine (ADR-0033 § 4) is flattened before it gets here:
//! its states are the layer's, named by path (`Airborne/Fall`), and a
//! transition from the machine is one per state inside it, listed before
//! the machine's own so that the outer scope is checked first.
//! [`LayerDef::machines`] maps the machine's path to the state a jump
//! into it enters.
//!
//! An exit time `e` has come, for a looping state and `e < 1`, in each
//! tick whose clock crosses `e`, `1 + e`, `2 + e`, … (Unity: the check
//! repeats every loop); otherwise once the clock has reached `e`.

pub use crate::blend_tree::{ChildDef, MotionDef, TreeDef, TreeKind};

/// Unity's name for Any State in a transition's `from`.
pub const ANY_STATE: &str = "any";

/// What a parameter holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterKind {
    Float,
    Int,
    Bool,
    /// A bool that the transition it fires sets back to false.
    Trigger,
}

impl ParameterKind {
    pub fn name(self) -> &'static str {
        match self {
            ParameterKind::Float => "float",
            ParameterKind::Int => "int",
            ParameterKind::Bool => "bool",
            ParameterKind::Trigger => "trigger",
        }
    }
}

/// A parameter's value; a trigger is a `Bool`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Float(f32),
    Int(i32),
    Bool(bool),
}

impl Value {
    /// The value as a float: what a blend space and a speed parameter read.
    pub fn as_f32(self) -> f32 {
        match self {
            Value::Float(v) => v,
            Value::Int(v) => v as f32,
            Value::Bool(b) => b as u8 as f32,
        }
    }

    fn fits(self, kind: ParameterKind) -> bool {
        matches!(
            (self, kind),
            (Value::Float(_), ParameterKind::Float)
                | (Value::Int(_), ParameterKind::Int)
                | (Value::Bool(_), ParameterKind::Bool | ParameterKind::Trigger)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterDef {
    pub name: String,
    pub kind: ParameterKind,
    /// The starting value; of the parameter's kind (a trigger: false).
    pub default: Value,
}

/// One test of a parameter (ADR-0032 § 3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Test {
    /// A float or an int above the value.
    Greater(f32),
    /// A float or an int below the value.
    Less(f32),
    /// An int equal to the value.
    Equals(i32),
    NotEquals(i32),
    /// A bool.
    Is(bool),
    /// A trigger that is set.
    Set,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Condition {
    /// Index into [`ControllerDef::parameters`].
    pub parameter: usize,
    pub test: Test,
}

impl Condition {
    fn holds(&self, values: &[Value]) -> bool {
        match (self.test, values[self.parameter]) {
            (Test::Greater(x), v @ (Value::Float(_) | Value::Int(_))) => v.as_f32() > x,
            (Test::Less(x), v @ (Value::Float(_) | Value::Int(_))) => v.as_f32() < x,
            (Test::Equals(x), Value::Int(v)) => v == x,
            (Test::NotEquals(x), Value::Int(v)) => v != x,
            (Test::Is(x), Value::Bool(v)) => v == x,
            (Test::Set, Value::Bool(v)) => v,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateDef {
    pub name: String,
    /// A clip, a blend space or a blend tree (ADR-0033); `None` holds
    /// the pose.
    pub motion: Option<MotionDef>,
    pub speed: f32,
    /// Index of a float parameter the speed is multiplied by.
    pub speed_parameter: Option<usize>,
    pub looping: bool,
    /// `None`: the Animator's `root_motion`.
    pub root_motion: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionDef {
    /// The state it leaves; `None` is Any State.
    pub from: Option<usize>,
    pub to: usize,
    pub conditions: Vec<Condition>,
    pub exit_time: Option<f32>,
    /// The crossfade, in seconds.
    pub duration: f32,
    /// Where the next state starts, as a fraction of its motion.
    pub offset: f32,
    pub can_transition_to_self: bool,
    /// What may cut its crossfade short (ADR-0033 § 4).
    pub interruptible_by: Interruption,
}

/// Which transitions may fire while a transition's crossfade runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Interruption {
    /// None: the crossfade always ends first.
    #[default]
    None,
    /// The transitions of the state it goes to.
    Next,
    /// Any State's, then the next state's.
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Blending {
    #[default]
    Override,
    Additive,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerDef {
    pub name: String,
    pub weight: f32,
    pub blending: Blending,
    /// The node the layer poses from; `None` poses the whole character.
    pub root: Option<String>,
    /// Index into `states`; `None` only when there are none.
    pub default_state: Option<usize>,
    pub states: Vec<StateDef>,
    pub transitions: Vec<TransitionDef>,
    /// Each sub-state machine's path (`Airborne`) and the state it enters
    /// (its default, followed down).
    pub machines: Vec<(String, usize)>,
}

impl LayerDef {
    /// A state by name or path; a sub-state machine's path gives the
    /// state it enters.
    pub fn state_index(&self, name: &str) -> Option<usize> {
        self.states.iter().position(|s| s.name == name).or_else(|| {
            self.machines
                .iter()
                .find_map(|(path, s)| (path == name).then_some(*s))
        })
    }
}

/// A controller as the runtime runs it: names resolved to indices.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ControllerDef {
    pub name: String,
    pub parameters: Vec<ParameterDef>,
    pub layers: Vec<LayerDef>,
}

impl ControllerDef {
    /// The limits the schema holds.
    pub const MAX_PARAMETERS: usize = 64;
    pub const MAX_LAYERS: usize = 16;

    pub fn parameter_index(&self, name: &str) -> Option<usize> {
        self.parameters.iter().position(|p| p.name == name)
    }

    /// The first layer (from the base) with a state of that name.
    pub fn find_state(&self, name: &str) -> Option<(usize, usize)> {
        self.layers
            .iter()
            .enumerate()
            .find_map(|(l, layer)| layer.state_index(name).map(|s| (l, s)))
    }

    /// Every index in range, every value finite and of its kind — what
    /// the runtime relies on (the cook's validation said so already; this
    /// guards a hand-built def).
    pub fn validate(&self) -> Result<(), String> {
        let n = &self.name;
        if self.layers.is_empty() || self.layers.len() > Self::MAX_LAYERS {
            return Err(format!("{n}: 1 to {} layers", Self::MAX_LAYERS));
        }
        if self.parameters.len() > Self::MAX_PARAMETERS {
            return Err(format!("{n}: at most {} parameters", Self::MAX_PARAMETERS));
        }
        for p in &self.parameters {
            let finite = match p.default {
                Value::Float(v) => v.is_finite(),
                _ => true,
            };
            if !p.default.fits(p.kind) || !finite {
                return Err(format!(
                    "{n}: parameter {}: the default is not a {}",
                    p.name,
                    p.kind.name()
                ));
            }
        }
        let float = |i: usize| self.parameters.get(i).map(|p| p.kind);
        for layer in &self.layers {
            let l = &layer.name;
            let states = layer.states.len();
            if !(0.0..=1.0).contains(&layer.weight) {
                return Err(format!("{n} layer {l}: the weight is not within [0, 1]"));
            }
            match layer.default_state {
                Some(s) if s >= states => {
                    return Err(format!("{n} layer {l}: the default state is out of range"))
                }
                None if states > 0 => {
                    return Err(format!(
                        "{n} layer {l}: the layer has states but no default"
                    ))
                }
                _ => {}
            }
            for s in &layer.states {
                if let Some(MotionDef::Tree(tree)) = &s.motion {
                    let is_float = |i: usize| float(i) == Some(ParameterKind::Float);
                    tree.validate(&is_float)
                        .map_err(|e| format!("{n} state {}: {e}", s.name))?;
                }
                if !s.speed.is_finite() {
                    return Err(format!("{n} state {}: the speed is not finite", s.name));
                }
                if let Some(p) = s.speed_parameter {
                    if float(p) != Some(ParameterKind::Float) {
                        return Err(format!(
                            "{n} state {}: the speed parameter is not a float",
                            s.name
                        ));
                    }
                }
            }
            if layer.machines.iter().any(|(_, s)| *s >= states) {
                return Err(format!(
                    "{n} layer {l}: a sub-state machine's state is out of range"
                ));
            }
            for (i, t) in layer.transitions.iter().enumerate() {
                let bad = |what: &str| Err(format!("{n} layer {l} transition {i}: {what}"));
                if t.from.is_some_and(|f| f >= states) || t.to >= states {
                    return bad("a state is out of range");
                }
                if !t.duration.is_finite() || t.duration < 0.0 {
                    return bad("the duration must be finite and >= 0");
                }
                if !t.offset.is_finite() || !(0.0..=1.0).contains(&t.offset) {
                    return bad("the offset must be within [0, 1]");
                }
                if t.exit_time.is_some_and(|e| !e.is_finite() || e < 0.0) {
                    return bad("the exit time must be finite and >= 0");
                }
                for c in &t.conditions {
                    let fits = match (c.test, float(c.parameter)) {
                        (_, None) => false,
                        (
                            Test::Greater(x) | Test::Less(x),
                            Some(ParameterKind::Float | ParameterKind::Int),
                        ) => x.is_finite(),
                        (Test::Equals(_) | Test::NotEquals(_), Some(ParameterKind::Int)) => true,
                        (Test::Is(_), Some(ParameterKind::Bool)) => true,
                        (Test::Set, Some(ParameterKind::Trigger)) => true,
                        _ => false,
                    };
                    if !fits {
                        return bad("a condition does not fit its parameter");
                    }
                }
            }
        }
        Ok(())
    }
}

/// A crossfade in progress: the outgoing state keeps its clock while
/// its weight falls to zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossfade {
    pub from: usize,
    /// The outgoing state's normalized time.
    pub from_time: f32,
    pub elapsed: f32,
    pub duration: f32,
    /// What may interrupt it (the transition's; `None` for code's
    /// `play`/`crossFade`).
    pub interruptible_by: Interruption,
    /// It interrupted another crossfade: what fades out is the pose of
    /// that moment, held (the world keeps it), not `from` sampled.
    pub frozen: bool,
}

impl Crossfade {
    /// How much of the incoming state shows, 0 → 1.
    pub fn weight(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        }
    }
}

/// One layer of one animator.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LayerRun {
    /// The current state (the incoming one during a crossfade); `None`
    /// before the first tick, or for a layer without states.
    pub state: Option<usize>,
    /// The state's normalized time, unwrapped: 1.5 = halfway through the
    /// second loop (Unity's `normalizedTime`).
    pub time: f32,
    pub crossfade: Option<Crossfade>,
    /// The state has just been entered and its clock not yet advanced:
    /// this tick samples it at its start.
    pub fresh: bool,
    /// How many interruptions so far: when it changes, the world freezes
    /// the pose it showed last.
    pub interruptions: u32,
}

/// What happened in a layer this tick, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateEvent {
    /// `state` became the layer's current state, leaving `from` (none on
    /// the first tick).
    Entered {
        layer: usize,
        state: usize,
        from: Option<usize>,
    },
    /// `state` stopped playing: its crossfade out ended.
    Exited { layer: usize, state: usize },
}

/// A parameter name or type mismatch when code sets or reads one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParameterError {
    /// No parameter of that name; the names there are.
    Unknown { name: String, names: Vec<String> },
    /// The parameter holds another kind.
    Kind { name: String, kind: ParameterKind },
}

impl std::fmt::Display for ParameterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParameterError::Unknown { name, names } if names.is_empty() => {
                write!(f, "no parameter `{name}` (the controller has none)")
            }
            ParameterError::Unknown { name, names } => {
                write!(
                    f,
                    "no parameter `{name}` (the parameters: {})",
                    names.join(", ")
                )
            }
            ParameterError::Kind { name, kind } => {
                write!(f, "parameter `{name}` is a {}", kind.name())
            }
        }
    }
}

/// One animator's run of a controller: its own parameter values and a
/// clock per layer.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ControllerRun {
    pub values: Vec<Value>,
    pub layers: Vec<LayerRun>,
}

impl ControllerRun {
    /// Parameters at their defaults; every layer enters its default state
    /// on the first [`step`](Self::step).
    pub fn new(def: &ControllerDef) -> ControllerRun {
        ControllerRun {
            values: def.parameters.iter().map(|p| p.default).collect(),
            layers: vec![LayerRun::default(); def.layers.len()],
        }
    }

    fn index(def: &ControllerDef, name: &str) -> Result<usize, ParameterError> {
        def.parameter_index(name)
            .ok_or_else(|| ParameterError::Unknown {
                name: name.to_string(),
                names: def.parameters.iter().map(|p| p.name.clone()).collect(),
            })
    }

    /// Sets a parameter by name; `value` must be of its kind (a trigger
    /// takes a bool: `true` sets it, `false` resets it). A float must be
    /// finite.
    pub fn set(
        &mut self,
        def: &ControllerDef,
        name: &str,
        value: Value,
    ) -> Result<(), ParameterError> {
        let i = Self::index(def, name)?;
        let kind = def.parameters[i].kind;
        let finite = !matches!(value, Value::Float(v) if !v.is_finite());
        if !value.fits(kind) || !finite {
            return Err(ParameterError::Kind {
                name: name.to_string(),
                kind,
            });
        }
        self.values[i] = value;
        Ok(())
    }

    /// A parameter by name, and its kind.
    pub fn get(
        &self,
        def: &ControllerDef,
        name: &str,
    ) -> Result<(ParameterKind, Value), ParameterError> {
        let i = Self::index(def, name)?;
        Ok((def.parameters[i].kind, self.values[i]))
    }

    /// A float parameter by name (what a blend space reads): an int or a
    /// bool as a number; `None` when there is no such parameter.
    pub fn float(&self, def: &ControllerDef, name: &str) -> Option<f32> {
        def.parameter_index(name).map(|i| self.values[i].as_f32())
    }

    /// Jumps to a state (Unity's `Play`): at once, at `offset` of its
    /// motion, without conditions; the state left (and one fading out)
    /// is exited.
    pub fn play(&mut self, layer: usize, state: usize, offset: f32, events: &mut Vec<StateEvent>) {
        self.cross_fade(layer, state, 0.0, offset, events);
    }

    /// Fades to a state over `seconds` (Unity's `CrossFade`), without
    /// conditions. A crossfade already running is cut short: its
    /// outgoing state exits and the current one becomes the outgoing
    /// side.
    pub fn cross_fade(
        &mut self,
        layer: usize,
        state: usize,
        seconds: f32,
        offset: f32,
        events: &mut Vec<StateEvent>,
    ) {
        let run = &mut self.layers[layer];
        if let Some(fade) = run.crossfade.take() {
            events.push(StateEvent::Exited {
                layer,
                state: fade.from,
            });
        }
        let from = run.state;
        match from {
            Some(f) if seconds > 0.0 => {
                run.crossfade = Some(Crossfade {
                    from: f,
                    from_time: run.time,
                    elapsed: 0.0,
                    duration: seconds,
                    interruptible_by: Interruption::None,
                    frozen: false,
                });
            }
            Some(f) => events.push(StateEvent::Exited { layer, state: f }),
            None => {}
        }
        run.state = Some(state);
        run.time = offset;
        run.fresh = true;
        events.push(StateEvent::Entered { layer, state, from });
    }

    /// The effective speed of a state: its `speed` times its speed
    /// parameter.
    pub fn state_speed(&self, state: &StateDef) -> f32 {
        let p = state
            .speed_parameter
            .map(|i| self.values[i].as_f32())
            .unwrap_or(1.0);
        state.speed * p
    }

    /// One tick of every layer. `dt` is the tick's seconds times the
    /// Animator's speed; `length(layer, state)` is the state's motion's
    /// length in seconds now (a blend space's at its parameter; 0 for no
    /// motion, which counts as one second, as Unity's empty state).
    /// Events go to `events` in order.
    pub fn step(
        &mut self,
        def: &ControllerDef,
        dt: f32,
        length: &mut dyn FnMut(usize, usize) -> f32,
        events: &mut Vec<StateEvent>,
    ) {
        for l in 0..def.layers.len() {
            self.step_layer(def, l, dt, length, events);
        }
    }

    fn step_layer(
        &mut self,
        def: &ControllerDef,
        l: usize,
        dt: f32,
        length: &mut dyn FnMut(usize, usize) -> f32,
        events: &mut Vec<StateEvent>,
    ) {
        let layer = &def.layers[l];
        let Some(current) = self.layers[l].state else {
            // The first tick: the default state, sampled at its start
            // (the next tick advances it).
            if let Some(d) = layer.default_state {
                let run = &mut self.layers[l];
                run.state = Some(d);
                run.time = 0.0;
                run.fresh = false;
                events.push(StateEvent::Entered {
                    layer: l,
                    state: d,
                    from: None,
                });
            }
            return;
        };
        let rate = |this: &Self, s: usize, length: &mut dyn FnMut(usize, usize) -> f32| {
            let seconds = length(l, s);
            let seconds = if seconds > 0.0 { seconds } else { 1.0 };
            this.state_speed(&layer.states[s]) * dt / seconds
        };

        // 1. The clocks.
        let prev = self.layers[l].time;
        if self.layers[l].fresh {
            self.layers[l].fresh = false;
        } else {
            let r = rate(self, current, length);
            self.layers[l].time += r;
        }
        let now = self.layers[l].time;
        // A crossfade that ends this tick still holds the check back.
        let fading = self.layers[l].crossfade.map(|f| f.interruptible_by);
        if let Some(mut fade) = self.layers[l].crossfade {
            fade.elapsed += dt.abs();
            if !fade.frozen {
                fade.from_time += rate(self, fade.from, length);
            }
            if fade.elapsed >= fade.duration {
                self.layers[l].crossfade = None;
                events.push(StateEvent::Exited {
                    layer: l,
                    state: fade.from,
                });
            } else {
                self.layers[l].crossfade = Some(fade);
            }
        }
        let interruptible = fading.unwrap_or(Interruption::Any);
        if interruptible == Interruption::None {
            return;
        }

        // 2. At most one transition: Any State's, then the current state's
        // (during a crossfade: what it may be interrupted by).
        let state = &layer.states[current];
        let exit_reached = |e: f32| {
            if state.looping && e < 1.0 {
                if e <= 0.0 {
                    return true;
                }
                // A crossing of e + k for some whole k this tick.
                let k = (now - e).floor();
                prev < e + k && e + k <= now && k >= 0.0
            } else {
                now >= e
            }
        };
        let any = layer
            .transitions
            .iter()
            .filter(|t| t.from.is_none() && interruptible == Interruption::Any);
        let own = layer.transitions.iter().filter(|t| t.from == Some(current));
        let fired = any.chain(own).find(|t| {
            if t.from.is_none() && t.to == current && !t.can_transition_to_self {
                return false;
            }
            if t.exit_time.is_some_and(|e| !exit_reached(e)) {
                return false;
            }
            t.conditions.iter().all(|c| c.holds(&self.values))
        });
        let Some(t) = fired else {
            return;
        };
        for c in &t.conditions {
            if c.test == Test::Set {
                self.values[c.parameter] = Value::Bool(false);
            }
        }
        let (to, duration, offset) = (t.to, t.duration, t.offset);
        let interrupting = self.layers[l].crossfade.is_some();
        self.cross_fade(l, to, duration, offset, events);
        let run = &mut self.layers[l];
        if let Some(fade) = &mut run.crossfade {
            fade.interruptible_by = t.interruptible_by;
            fade.frozen = interrupting;
        }
        if interrupting {
            run.interruptions = run.interruptions.wrapping_add(1);
        }
        // Sampled at `offset` this tick; the next one advances it.
        run.fresh = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::AssetRef;

    fn clip(n: u8) -> Option<AssetRef> {
        let mut bytes = [0u8; 16];
        bytes[15] = n;
        Some(AssetRef::with_sub(
            fynkel_contracts::AssetId::from_bytes(bytes),
            "anim/x",
        ))
    }

    fn state(name: &str, looping: bool) -> StateDef {
        StateDef {
            name: name.into(),
            motion: clip(1).map(Into::into),
            speed: 1.0,
            speed_parameter: None,
            looping,
            root_motion: None,
        }
    }

    fn transition(from: Option<usize>, to: usize, conditions: Vec<Condition>) -> TransitionDef {
        TransitionDef {
            from,
            to,
            conditions,
            exit_time: None,
            duration: 0.25,
            offset: 0.0,
            can_transition_to_self: false,
            interruptible_by: Interruption::None,
        }
    }

    const SPEED: usize = 0;
    const GROUNDED: usize = 1;
    const JUMP: usize = 2;
    const WEAPON: usize = 3;

    const LOCOMOTION: usize = 0;
    const JUMPING: usize = 1;
    const LAND: usize = 2;

    /// The ADR's player: Locomotion → Jump on the trigger, Jump → Land
    /// when grounded after 25 % of the jump, Land → Locomotion at its end.
    fn player() -> ControllerDef {
        let p = |name: &str, kind, default| ParameterDef {
            name: name.into(),
            kind,
            default,
        };
        ControllerDef {
            name: "Player".into(),
            parameters: vec![
                p("speed", ParameterKind::Float, Value::Float(1.0)),
                p("grounded", ParameterKind::Bool, Value::Bool(true)),
                p("jump", ParameterKind::Trigger, Value::Bool(false)),
                p("weapon", ParameterKind::Int, Value::Int(0)),
            ],
            layers: vec![LayerDef {
                name: "Base Layer".into(),
                weight: 1.0,
                blending: Blending::Override,
                root: None,
                default_state: Some(LOCOMOTION),
                states: vec![
                    StateDef {
                        speed_parameter: Some(SPEED),
                        ..state("Locomotion", true)
                    },
                    state("Jump", false),
                    state("Land", false),
                ],
                transitions: vec![
                    TransitionDef {
                        duration: 0.1,
                        ..transition(
                            Some(LOCOMOTION),
                            JUMPING,
                            vec![Condition {
                                parameter: JUMP,
                                test: Test::Set,
                            }],
                        )
                    },
                    TransitionDef {
                        exit_time: Some(0.25),
                        ..transition(
                            Some(JUMPING),
                            LAND,
                            vec![Condition {
                                parameter: GROUNDED,
                                test: Test::Is(true),
                            }],
                        )
                    },
                    TransitionDef {
                        exit_time: Some(1.0),
                        duration: 0.0,
                        ..transition(Some(LAND), LOCOMOTION, vec![])
                    },
                ],
                machines: vec![],
            }],
        }
    }

    const DT: f32 = 0.1;

    /// Every state one second long.
    fn tick(def: &ControllerDef, run: &mut ControllerRun) -> Vec<StateEvent> {
        let mut events = Vec::new();
        run.step(def, DT, &mut |_, _| 1.0, &mut events);
        events
    }

    fn entered(state: usize, from: Option<usize>) -> StateEvent {
        StateEvent::Entered {
            layer: 0,
            state,
            from,
        }
    }

    fn exited(state: usize) -> StateEvent {
        StateEvent::Exited { layer: 0, state }
    }

    #[test]
    fn the_player_def_is_valid() {
        player().validate().unwrap();
    }

    #[test]
    fn the_first_tick_enters_the_default_state_at_its_start() {
        let def = player();
        let mut run = ControllerRun::new(&def);
        assert_eq!(run.values[SPEED], Value::Float(1.0));
        assert_eq!(tick(&def, &mut run), [entered(LOCOMOTION, None)]);
        assert_eq!(
            (run.layers[0].state, run.layers[0].time),
            (Some(LOCOMOTION), 0.0)
        );
        tick(&def, &mut run);
        assert!((run.layers[0].time - 0.1).abs() < 1e-6);
        // A `play` between ticks: the next tick samples the start.
        run.play(0, LAND, 0.0, &mut Vec::new());
        tick(&def, &mut run);
        assert_eq!(run.layers[0].time, 0.0);
        tick(&def, &mut run);
        assert!((run.layers[0].time - 0.1).abs() < 1e-6);
    }

    #[test]
    fn a_whole_jump_follows_the_rules() {
        let def = player();
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        for _ in 0..3 {
            assert_eq!(tick(&def, &mut run), []);
        }
        // The trigger fires the jump and is used up.
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        run.set(&def, "grounded", Value::Bool(false)).unwrap();
        assert_eq!(tick(&def, &mut run), [entered(JUMPING, Some(LOCOMOTION))]);
        assert_eq!(run.values[JUMP], Value::Bool(false));
        let fade = run.layers[0].crossfade.unwrap();
        assert_eq!(
            (fade.from, fade.duration, fade.weight()),
            (LOCOMOTION, 0.1, 0.0)
        );
        // The 0.1 s crossfade ends the next tick: Locomotion exits.
        assert_eq!(tick(&def, &mut run), [exited(LOCOMOTION)]);
        assert!(run.layers[0].crossfade.is_none());
        // Grounded again before the exit time: nothing until 25 %.
        run.set(&def, "grounded", Value::Bool(true)).unwrap();
        assert_eq!(tick(&def, &mut run), []);
        assert!((run.layers[0].time - 0.2).abs() < 1e-6);
        assert_eq!(tick(&def, &mut run), [entered(LAND, Some(JUMPING))]);
        // Land, then back to Locomotion with no crossfade at its end.
        let mut all = Vec::new();
        for _ in 0..20 {
            all.extend(tick(&def, &mut run));
        }
        assert_eq!(
            all,
            [
                exited(JUMPING),
                exited(LAND),
                entered(LOCOMOTION, Some(LAND))
            ]
        );
        assert_eq!(run.layers[0].state, Some(LOCOMOTION));
    }

    #[test]
    fn the_same_inputs_give_the_same_states() {
        let def = player();
        let script = |run: &mut ControllerRun| {
            let mut log = Vec::new();
            for i in 0..60 {
                if i % 17 == 5 {
                    run.set(&def, "jump", Value::Bool(true)).unwrap();
                }
                run.set(&def, "grounded", Value::Bool(i % 17 > 9 || i % 17 < 5))
                    .unwrap();
                log.extend(tick(&def, run));
                log.push(StateEvent::Exited {
                    layer: 99,
                    state: (run.layers[0].time * 1000.0) as usize,
                });
            }
            log
        };
        let a = script(&mut ControllerRun::new(&def));
        let b = script(&mut ControllerRun::new(&def));
        assert_eq!(a, b);
        assert!(
            a.iter()
                .filter(|e| matches!(e, StateEvent::Entered { state: JUMPING, .. }))
                .count()
                >= 3
        );
    }

    #[test]
    fn a_trigger_waits_until_something_uses_it() {
        let def = player();
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        // In Land nothing tests `jump`: it stays set, then fires on return.
        run.play(0, LAND, 0.0, &mut Vec::new());
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        let mut all = Vec::new();
        for _ in 0..14 {
            all.extend(tick(&def, &mut run));
        }
        assert_eq!(
            all[..3],
            [
                exited(LAND),
                entered(LOCOMOTION, Some(LAND)),
                entered(JUMPING, Some(LOCOMOTION))
            ]
        );
        assert_eq!(run.values[JUMP], Value::Bool(false));
        // resetTrigger clears it.
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        run.set(&def, "jump", Value::Bool(false)).unwrap();
        assert_eq!(run.values[JUMP], Value::Bool(false));
    }

    #[test]
    fn any_state_comes_first_and_not_into_itself() {
        let mut def = player();
        // Any State → Jump on the trigger, listed after Locomotion's own.
        def.layers[0].transitions.push(TransitionDef {
            duration: 0.0,
            ..transition(
                None,
                JUMPING,
                vec![Condition {
                    parameter: JUMP,
                    test: Test::Set,
                }],
            )
        });
        def.validate().unwrap();
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        tick(&def, &mut run);
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        // Any State's zero-length transition wins over Locomotion's 0.1 s.
        assert_eq!(
            tick(&def, &mut run),
            [exited(LOCOMOTION), entered(JUMPING, Some(LOCOMOTION))]
        );
        // In Jump, the trigger again: Any State does not fire into Jump.
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        assert_eq!(tick(&def, &mut run), []);
        assert_eq!(run.values[JUMP], Value::Bool(true));
        // Unless it may.
        def.layers[0].transitions[3].can_transition_to_self = true;
        assert_eq!(
            tick(&def, &mut run),
            [exited(JUMPING), entered(JUMPING, Some(JUMPING))]
        );
        assert_eq!(run.layers[0].time, 0.0, "it restarts");
    }

    #[test]
    fn no_transition_interrupts_a_crossfade() {
        let mut def = player();
        def.layers[0].transitions[0].duration = 0.5;
        // Jump → Land with no exit time and no condition but grounded.
        def.layers[0].transitions[1].exit_time = None;
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        assert_eq!(tick(&def, &mut run), [entered(JUMPING, Some(LOCOMOTION))]);
        // grounded holds, but the 0.5 s crossfade runs for five ticks.
        let mut all = Vec::new();
        for _ in 0..4 {
            all.extend(tick(&def, &mut run));
        }
        assert_eq!(all, []);
        let w = run.layers[0].crossfade.unwrap().weight();
        assert!((w - 0.8).abs() < 1e-5, "{w}");
        // The fifth ends it; the check waits for the next tick.
        assert_eq!(tick(&def, &mut run), [exited(LOCOMOTION)]);
        assert_eq!(tick(&def, &mut run), [entered(LAND, Some(JUMPING))]);
    }

    #[test]
    fn an_interruptible_crossfade_gives_way_to_the_next_states_transitions() {
        let mut def = player();
        def.layers[0].transitions[0].duration = 0.5;
        def.layers[0].transitions[0].interruptible_by = Interruption::Next;
        def.layers[0].transitions[1].exit_time = None;
        // Any State → Locomotion on weapon == 1: not for `next`.
        def.layers[0].transitions.push(TransitionDef {
            can_transition_to_self: false,
            ..transition(
                None,
                LOCOMOTION,
                vec![Condition {
                    parameter: WEAPON,
                    test: Test::Equals(1),
                }],
            )
        });
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        run.set(&def, "grounded", Value::Bool(false)).unwrap();
        assert_eq!(tick(&def, &mut run), [entered(JUMPING, Some(LOCOMOTION))]);
        run.set(&def, "weapon", Value::Int(1)).unwrap();
        assert_eq!(tick(&def, &mut run), [], "Any State waits");
        run.set(&def, "weapon", Value::Int(0)).unwrap();
        run.set(&def, "grounded", Value::Bool(true)).unwrap();
        // Jump → Land cuts the fade: Locomotion exits, the pose freezes.
        assert_eq!(
            tick(&def, &mut run),
            [exited(LOCOMOTION), entered(LAND, Some(JUMPING))]
        );
        let fade = run.layers[0].crossfade.unwrap();
        assert_eq!((fade.from, fade.frozen), (JUMPING, true));
        assert_eq!(fade.interruptible_by, Interruption::None);
        assert_eq!(run.layers[0].interruptions, 1);
        let t = fade.from_time;
        tick(&def, &mut run);
        assert_eq!(run.layers[0].crossfade.unwrap().from_time, t, "held");
    }

    #[test]
    fn any_lets_any_state_interrupt_too() {
        let mut def = player();
        def.layers[0].transitions[0].duration = 0.5;
        def.layers[0].transitions[0].interruptible_by = Interruption::Any;
        def.layers[0].transitions.push(transition(
            None,
            LAND,
            vec![Condition {
                parameter: WEAPON,
                test: Test::Equals(1),
            }],
        ));
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        run.set(&def, "jump", Value::Bool(true)).unwrap();
        run.set(&def, "grounded", Value::Bool(false)).unwrap();
        tick(&def, &mut run);
        run.set(&def, "weapon", Value::Int(1)).unwrap();
        assert_eq!(
            tick(&def, &mut run),
            [exited(LOCOMOTION), entered(LAND, Some(JUMPING))]
        );
        assert!(run.layers[0].crossfade.unwrap().frozen);
    }

    #[test]
    fn a_machine_path_names_the_state_it_enters() {
        let mut def = player();
        def.layers[0].states[JUMPING].name = "Airborne/Jump".into();
        def.layers[0].machines.push(("Airborne".into(), JUMPING));
        def.validate().unwrap();
        assert_eq!(def.find_state("Airborne"), Some((0, JUMPING)));
        assert_eq!(def.find_state("Airborne/Jump"), Some((0, JUMPING)));
        def.layers[0].machines[0].1 = 9;
        assert!(def.validate().is_err());
    }

    #[test]
    fn exit_time_repeats_each_loop_below_one_and_holds_from_one() {
        let mut def = player();
        // Locomotion → Land at 0.5 of each loop, while weapon == 2.
        def.layers[0].transitions.push(TransitionDef {
            exit_time: Some(0.5),
            duration: 0.0,
            ..transition(
                Some(LOCOMOTION),
                LAND,
                vec![Condition {
                    parameter: WEAPON,
                    test: Test::Equals(2),
                }],
            )
        });
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        tick(&def, &mut run);
        // The clock passes 0.5 at tick 6 (0.1 each), weapon still 0; then
        // weapon becomes 2 at 0.8: it waits for 1.5.
        let mut fired_at = None;
        for i in 0..20 {
            if run.layers[0].time >= 0.75 {
                run.set(&def, "weapon", Value::Int(2)).unwrap();
            }
            let before = run.layers[0].time;
            if !tick(&def, &mut run).is_empty() {
                fired_at = Some((i, before));
                break;
            }
        }
        let (_, before) = fired_at.unwrap();
        assert!(
            before < 1.5 && before + DT >= 1.5 - 1e-4,
            "fired as the clock went from {before}"
        );

        // Exit time 1 on a looping state holds from the first lap's end.
        let mut def = player();
        def.layers[0].transitions.push(TransitionDef {
            exit_time: Some(1.0),
            duration: 0.0,
            ..transition(
                Some(LOCOMOTION),
                LAND,
                vec![Condition {
                    parameter: WEAPON,
                    test: Test::Greater(0.0),
                }],
            )
        });
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        for _ in 0..16 {
            tick(&def, &mut run);
        }
        assert_eq!(run.layers[0].state, Some(LOCOMOTION));
        run.set(&def, "weapon", Value::Int(1)).unwrap();
        assert_eq!(
            tick(&def, &mut run),
            [exited(LOCOMOTION), entered(LAND, Some(LOCOMOTION))]
        );
    }

    #[test]
    fn a_speed_parameter_and_a_motion_length_set_the_clock() {
        let def = player();
        let mut run = ControllerRun::new(&def);
        let mut events = Vec::new();
        run.step(&def, DT, &mut |_, _| 2.0, &mut events);
        run.step(&def, DT, &mut |_, _| 2.0, &mut events);
        assert!((run.layers[0].time - 0.05).abs() < 1e-6);
        run.set(&def, "speed", Value::Float(0.5)).unwrap();
        run.step(&def, DT, &mut |_, _| 2.0, &mut events);
        // + 0.5 × 0.1 s over a 2 s motion.
        assert!((run.layers[0].time - 0.075).abs() < 1e-6);
        // No motion counts as a second.
        run.step(&def, DT, &mut |_, _| 0.0, &mut events);
        assert!((run.layers[0].time - 0.125).abs() < 1e-6);
    }

    #[test]
    fn conditions_test_their_kinds() {
        let values = [
            Value::Float(0.5),
            Value::Bool(true),
            Value::Bool(false),
            Value::Int(2),
        ];
        let c = |parameter, test| Condition { parameter, test }.holds(&values);
        assert!(c(SPEED, Test::Greater(0.1)) && !c(SPEED, Test::Greater(0.5)));
        assert!(c(SPEED, Test::Less(0.6)) && !c(SPEED, Test::Less(0.5)));
        assert!(
            c(WEAPON, Test::Equals(2))
                && c(WEAPON, Test::NotEquals(3))
                && c(WEAPON, Test::Greater(1.0))
        );
        assert!(c(GROUNDED, Test::Is(true)) && !c(GROUNDED, Test::Is(false)));
        assert!(!c(JUMP, Test::Set));
        // A test of another kind never holds.
        assert!(
            !c(SPEED, Test::Is(true))
                && !c(GROUNDED, Test::Greater(0.0))
                && !c(SPEED, Test::Equals(0))
        );
    }

    #[test]
    fn parameters_are_set_by_name_and_kind() {
        let def = player();
        let mut run = ControllerRun::new(&def);
        run.set(&def, "speed", Value::Float(2.0)).unwrap();
        assert_eq!(
            run.get(&def, "speed").unwrap(),
            (ParameterKind::Float, Value::Float(2.0))
        );
        assert_eq!(run.float(&def, "weapon"), Some(0.0));
        let e = run.set(&def, "speed", Value::Bool(true)).unwrap_err();
        assert_eq!(e.to_string(), "parameter `speed` is a float");
        let e = run.set(&def, "speed", Value::Float(f32::NAN)).unwrap_err();
        assert_eq!(
            e,
            ParameterError::Kind {
                name: "speed".into(),
                kind: ParameterKind::Float
            }
        );
        let e = run.set(&def, "run", Value::Float(1.0)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "no parameter `run` (the parameters: speed, grounded, jump, weapon)"
        );
        assert!(run.set(&def, "jump", Value::Bool(true)).is_ok());
        assert!(run.set(&def, "weapon", Value::Float(1.0)).is_err());
    }

    #[test]
    fn play_and_cross_fade_jump_without_conditions() {
        let def = player();
        let mut run = ControllerRun::new(&def);
        tick(&def, &mut run);
        let mut events = Vec::new();
        run.cross_fade(0, LAND, 0.2, 0.5, &mut events);
        assert_eq!(events, [entered(LAND, Some(LOCOMOTION))]);
        assert_eq!(run.layers[0].time, 0.5);
        // A second crossfade cuts the first: its outgoing side exits.
        events.clear();
        run.cross_fade(0, JUMPING, 0.2, 0.0, &mut events);
        assert_eq!(events, [exited(LOCOMOTION), entered(JUMPING, Some(LAND))]);
        assert_eq!(run.layers[0].crossfade.unwrap().from, LAND);
        events.clear();
        run.play(0, LOCOMOTION, 0.0, &mut events);
        assert_eq!(
            events,
            [
                exited(LAND),
                exited(JUMPING),
                entered(LOCOMOTION, Some(JUMPING))
            ]
        );
        assert!(run.layers[0].crossfade.is_none());
        assert_eq!(def.find_state("Land"), Some((0, LAND)));
    }

    #[test]
    fn validate_refuses_what_the_runtime_cannot_run() {
        let bad = |f: &dyn Fn(&mut ControllerDef)| {
            let mut def = player();
            f(&mut def);
            def.validate().unwrap_err()
        };
        bad(&|d| d.layers.clear());
        bad(&|d| d.layers[0].default_state = None);
        bad(&|d| d.layers[0].default_state = Some(9));
        bad(&|d| d.layers[0].transitions[0].to = 9);
        bad(&|d| d.layers[0].transitions[0].conditions[0].test = Test::Is(true));
        bad(&|d| d.layers[0].transitions[0].conditions[0].parameter = 9);
        bad(&|d| d.layers[0].transitions[0].duration = -1.0);
        bad(&|d| d.layers[0].states[0].speed_parameter = Some(GROUNDED));
        bad(&|d| d.parameters[0].default = Value::Int(1));
        bad(&|d| d.layers[0].weight = 2.0);
        // A layer with no states needs no default.
        let mut def = player();
        def.layers.push(LayerDef {
            name: "Empty".into(),
            weight: 1.0,
            blending: Blending::Additive,
            root: Some("spine".into()),
            default_state: None,
            states: vec![],
            transitions: vec![],
            machines: vec![],
        });
        def.validate().unwrap();
        let mut run = ControllerRun::new(&def);
        assert_eq!(tick(&def, &mut run), [entered(LOCOMOTION, None)]);
        assert_eq!(run.layers[1].state, None);
    }
}

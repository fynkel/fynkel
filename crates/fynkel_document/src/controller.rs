//! Animator Controller documents (ADR-0032): Unity's Animator Controller
//! as one `.controller.json` — parameters, and per layer a state machine
//! of states (each playing a clip or a blend space) and transitions on
//! conditions over the parameters. The document is data only: the
//! runtime (`fynkel_animation`, `fynkel_core`) runs it every tick; the
//! cook checks it and writes it as it is.
//!
//! Like [`crate::blend`], the tree stays the source of truth and the view
//! is lenient: a broken document still describes itself, every problem a
//! diagnostic with a pointer.
//!
//! Semantic rules (`FYN-D11xx`): a field that does not read — a
//! malformed assetRef, a number that is not finite, a default of another
//! type — is `D1100`; a name that is not unique or does not resolve is
//! `D1101`; a condition that does not fit its parameter's type, a speed
//! parameter that is not a float, an Any State transition that would
//! fire every tick is `D1102`; a motion that is not a clip or a blend
//! space is `D1103`; a blend tree (ADR-0033) of the wrong shape is
//! `D1104`. Unknown ids are the asset database's `A0004`/`A0005`.

use std::collections::HashSet;

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use uuid::Uuid;

pub use fynkel_core::animation::blend_tree::TreeKind;

use crate::asset::{AssetDatabase, AssetKind};
use crate::json::Node;
use crate::rig::{is_model, RIG_CLIP_PREFIX};
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// What a transition's `from` names for Any State.
pub const ANY_STATE: &str = "any";

/// A crossfade's length when the transition gives none (Unity's 0.25 s).
pub const DEFAULT_DURATION: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterType {
    Float,
    Int,
    Bool,
    Trigger,
}

impl ParameterType {
    pub fn name(self) -> &'static str {
        match self {
            ParameterType::Float => "float",
            ParameterType::Int => "int",
            ParameterType::Bool => "bool",
            ParameterType::Trigger => "trigger",
        }
    }

    fn parse(s: &str) -> Option<ParameterType> {
        Some(match s {
            "float" => ParameterType::Float,
            "int" => ParameterType::Int,
            "bool" => ParameterType::Bool,
            "trigger" => ParameterType::Trigger,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub kind: ParameterType,
    /// The starting value as a number: a bool or trigger is 0 or 1.
    pub default: f64,
    /// `/parameters/<i>`.
    pub pointer: String,
}

/// What a condition asks of its parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Test {
    Greater(f64),
    Less(f64),
    Equals(i64),
    NotEquals(i64),
    Is(bool),
    /// A trigger's: holds while it is set.
    Set,
}

impl Test {
    fn fits(self, kind: ParameterType) -> bool {
        matches!(
            (self, kind),
            (
                Test::Greater(_) | Test::Less(_),
                ParameterType::Float | ParameterType::Int
            ) | (Test::Equals(_) | Test::NotEquals(_), ParameterType::Int)
                | (Test::Is(_), ParameterType::Bool)
                | (Test::Set, ParameterType::Trigger)
        )
    }

    fn word(self) -> &'static str {
        match self {
            Test::Greater(_) => "greater",
            Test::Less(_) => "less",
            Test::Equals(_) => "equals",
            Test::NotEquals(_) => "notEquals",
            Test::Is(_) => "is",
            Test::Set => "no test",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub parameter: String,
    pub test: Test,
    pub pointer: String,
}

/// What a state or a blend tree's child plays.
#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    /// A clip or a blend space document.
    Asset(AssetRef),
    Tree(BlendTree),
}

impl Motion {
    pub fn as_asset(&self) -> Option<&AssetRef> {
        match self {
            Motion::Asset(a) => Some(a),
            Motion::Tree(_) => None,
        }
    }

    /// Every asset it plays with the pointer of its `motion`, depth first.
    pub fn assets<'a>(&'a self, pointer: &str, out: &mut Vec<(&'a AssetRef, String)>) {
        match self {
            Motion::Asset(a) => out.push((a, pointer.to_string())),
            Motion::Tree(t) => {
                for c in &t.children {
                    if let Some(m) = &c.motion {
                        m.assets(&format!("{}/motion", c.pointer), out);
                    }
                }
            }
        }
    }
}

/// A blend tree (ADR-0033 § 1): `{ "blendTree": { type, parameters,
/// children } }` in a state's (or a child's) `motion`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendTree {
    /// None: the type did not read (`D1100`).
    pub kind: Option<TreeKind>,
    pub parameters: Vec<String>,
    pub children: Vec<Child>,
    /// `…/motion/blendTree`.
    pub pointer: String,
}

impl BlendTree {
    /// The motions at the bottom, counting nested trees'.
    pub fn leaf_count(&self) -> usize {
        self.children
            .iter()
            .map(|c| match &c.motion {
                Some(Motion::Tree(t)) => t.leaf_count(),
                _ => 1,
            })
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Child {
    pub motion: Option<Motion>,
    /// A 1D tree's.
    pub threshold: Option<f64>,
    /// A 2D tree's.
    pub position: Option<[f64; 2]>,
    pub time_scale: f64,
    /// A direct tree's: the float parameter that is its weight.
    pub parameter: Option<String>,
    /// `…/blendTree/children/<i>`.
    pub pointer: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub name: String,
    /// None: an empty state (it holds the pose).
    pub motion: Option<Motion>,
    pub speed: f32,
    pub speed_parameter: Option<String>,
    pub looping: bool,
    /// None: the Animator's `root_motion`.
    pub root_motion: Option<bool>,
    /// Some: a sub-state machine (ADR-0033 § 4), which has states
    /// instead of a motion.
    pub machine: Option<Machine>,
    /// `/layers/<l>/states/<i>` (`…/states/<i>/states/<j>` inside a
    /// sub-state machine).
    pub pointer: String,
}

/// A sub-state machine's own states and transitions (ADR-0033 § 4). A
/// transition into it enters its default state; one from it is checked
/// from every state inside it. Its names are its own: `from` and `to`
/// name its states, and code names them by path (`Airborne/Fall`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Machine {
    pub default_state: Option<String>,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
}

/// How deep sub-state machines may nest (a layer's states are depth 0).
pub const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// A state's name, or [`ANY_STATE`].
    pub from: String,
    pub to: String,
    pub conditions: Vec<Condition>,
    pub exit_time: Option<f32>,
    pub duration: f32,
    pub offset: f32,
    pub can_transition_to_self: bool,
    /// What may cut its crossfade short (ADR-0033 § 4).
    pub interruptible_by: Interruption,
    /// `/layers/<l>/transitions/<i>`.
    pub pointer: String,
}

/// A transition's `interruptibleBy`: which transitions may fire while its
/// crossfade runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Interruption {
    /// None: its crossfade always ends first (the default).
    #[default]
    None,
    /// The transitions of the state it goes to.
    Next,
    /// Any State's and the next state's.
    Any,
}

impl Interruption {
    /// The Inspector's words.
    pub fn name(self) -> &'static str {
        match self {
            Interruption::None => "none",
            Interruption::Next => "next",
            Interruption::Any => "any",
        }
    }
}

impl Transition {
    pub fn from_any(&self) -> bool {
        self.from == ANY_STATE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blending {
    Override,
    Additive,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub weight: f32,
    pub blending: Blending,
    /// The node it poses from; None: the whole character.
    pub root: Option<String>,
    /// Which nodes it poses (ADR-0033 § 5); empty: every node.
    pub mask: Vec<MaskEntry>,
    pub default_state: Option<String>,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    /// `/layers/<l>`.
    pub pointer: String,
}

impl Layer {
    pub fn state(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }

    /// The layer and every sub-state machine in it, outer first: what
    /// its names are checked in.
    pub fn scopes(&self) -> Vec<Scope<'_>> {
        fn walk<'a>(path: String, states: &'a [State], out: &mut Vec<Scope<'a>>) {
            for s in states {
                if let Some(m) = &s.machine {
                    let p = if path.is_empty() {
                        s.name.clone()
                    } else {
                        format!("{path}/{}", s.name)
                    };
                    out.push(Scope {
                        path: p.clone(),
                        default_state: m.default_state.as_deref(),
                        states: &m.states,
                        transitions: &m.transitions,
                        pointer: &s.pointer,
                    });
                    walk(p, &m.states, out);
                }
            }
        }
        let mut out = vec![Scope {
            path: String::new(),
            default_state: self.default_state.as_deref(),
            states: &self.states,
            transitions: &self.transitions,
            pointer: &self.pointer,
        }];
        walk(String::new(), &self.states, &mut out);
        out
    }

    /// Every state that plays (not a sub-state machine), by path
    /// (`Airborne/Fall`), in file order.
    pub fn leaves(&self) -> Vec<(String, &State)> {
        fn walk<'a>(path: &str, states: &'a [State], out: &mut Vec<(String, &'a State)>) {
            for s in states {
                let p = if path.is_empty() {
                    s.name.clone()
                } else {
                    format!("{path}/{}", s.name)
                };
                match &s.machine {
                    Some(m) => walk(&p, &m.states, out),
                    None => out.push((p, s)),
                }
            }
        }
        let mut out = Vec::new();
        walk("", &self.states, &mut out);
        out
    }
}

/// One node of a layer's mask.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskEntry {
    pub node: String,
    pub include: bool,
    pub subtree: bool,
    /// `/layers/<l>/mask/<i>`.
    pub pointer: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IkType {
    TwoBone,
    LookAt,
    FootPlacement,
}

impl IkType {
    pub fn parse(s: &str) -> Option<IkType> {
        match s {
            "twoBone" => Some(IkType::TwoBone),
            "lookAt" => Some(IkType::LookAt),
            "footPlacement" => Some(IkType::FootPlacement),
            _ => None,
        }
    }
}

/// An IK goal's weight: a number, or a float parameter's name.
#[derive(Clone, Debug, PartialEq)]
pub enum IkWeight {
    Value(f32),
    Parameter(String),
}

/// One IK goal (ADR-0033 § 5), as written: the fields of its type are
/// set, the others `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ik {
    pub name: String,
    /// None: not one of the three (reported).
    pub kind: Option<IkType>,
    pub weight: IkWeight,
    pub target: Option<String>,
    pub tip: Option<String>,
    pub middle: Option<String>,
    pub root: Option<String>,
    pub pole: Option<String>,
    pub head: Option<String>,
    pub spine: Vec<String>,
    /// Degrees.
    pub limit: f32,
    pub body: f32,
    pub hips: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
    pub reach: f32,
    /// `/ik/<i>`.
    pub pointer: String,
}

/// A layer or a sub-state machine: states whose names `from`, `to` and
/// `defaultState` resolve among.
#[derive(Clone, Debug)]
pub struct Scope<'a> {
    /// `""` for the layer, else the machine's path (`Airborne`).
    pub path: String,
    pub default_state: Option<&'a str>,
    pub states: &'a [State],
    pub transitions: &'a [Transition],
    /// The layer's or the machine state's.
    pub pointer: &'a str,
}

impl Scope<'_> {
    pub fn state(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }

    /// `name`'s path from the layer.
    pub fn path_of(&self, name: &str) -> String {
        if self.path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{name}", self.path)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Controller {
    pub document: Document,
    pub parameters: Vec<Parameter>,
    pub layers: Vec<Layer>,
    pub ik: Vec<Ik>,
    read_diagnostics: Vec<Diagnostic>,
}

/// Reads fields of one object, collecting what does not read.
struct Reader<'a> {
    root: &'a Node,
    diags: &'a mut Vec<Diagnostic>,
}

impl Reader<'_> {
    fn bad(&mut self, message: String, pointer: &str) {
        self.diags.push(anchor(
            self.root,
            Diagnostic::error(Code::CONTROLLER_MALFORMED_ENTRY, message),
            pointer,
        ));
    }

    fn string(&mut self, node: &Node, key: &str, pointer: &str) -> Option<String> {
        let v = node.get(key)?;
        match v.as_str() {
            Some(s) if !s.is_empty() => Some(s.to_string()),
            _ => {
                self.bad(
                    format!("{key} must be a non-empty string"),
                    &format!("{pointer}/{key}"),
                );
                None
            }
        }
    }

    fn number(&mut self, node: &Node, key: &str, pointer: &str) -> Option<f64> {
        let v = node.get(key)?;
        match v.as_f64() {
            Some(n) if n.is_finite() => Some(n),
            _ => {
                self.bad(
                    format!("{key} must be a finite number"),
                    &format!("{pointer}/{key}"),
                );
                None
            }
        }
    }

    fn boolean(&mut self, node: &Node, key: &str, pointer: &str) -> Option<bool> {
        let v = node.get(key)?;
        match v.as_bool() {
            Some(b) => Some(b),
            None => {
                self.bad(
                    format!("{key} must be true or false"),
                    &format!("{pointer}/{key}"),
                );
                None
            }
        }
    }

    /// `node`'s `motion`: an assetRef or `{ "blendTree": … }`; absent is
    /// None, and so is one that does not read (reported).
    fn motion(&mut self, node: &Node, pointer: &str) -> Option<Motion> {
        let value = node.get("motion")?;
        let mp = format!("{pointer}/motion");
        if let Some(tree) = value.get("blendTree") {
            let tp = format!("{mp}/blendTree");
            if tree.as_object().is_none() {
                self.bad("blendTree must be an object".into(), &tp);
                return None;
            }
            let kind = match tree
                .get("type")
                .map(|t| t.as_str().and_then(TreeKind::parse))
            {
                Some(Some(k)) => Some(k),
                Some(None) => {
                    self.bad(
                        "type must be 1d, simpleDirectional, freeformDirectional, freeformCartesian or direct".into(),
                        &format!("{tp}/type"),
                    );
                    None
                }
                None => {
                    self.bad("a blend tree needs a type".into(), &tp);
                    None
                }
            };
            let mut parameters = Vec::new();
            match tree.get("parameters").map(|v| v.as_array()) {
                None => {}
                Some(None) => self.bad(
                    "parameters must be an array of names".into(),
                    &format!("{tp}/parameters"),
                ),
                Some(Some(list)) => {
                    for (i, v) in list.iter().enumerate() {
                        match v.as_str() {
                            Some(n) if !n.is_empty() => parameters.push(n.to_string()),
                            _ => self.bad(
                                "a parameter is a name".into(),
                                &format!("{tp}/parameters/{i}"),
                            ),
                        }
                    }
                }
            }
            let mut children = Vec::new();
            for (c, cp) in self.objects(tree, "children", &tp) {
                let position = match c.get("position") {
                    None => None,
                    Some(v) => match v
                        .as_array()
                        .map(|a| a.iter().map(Node::as_f64).collect::<Vec<_>>())
                    {
                        Some(xs)
                            if xs.len() == 2
                                && xs.iter().all(|x| x.is_some_and(f64::is_finite)) =>
                        {
                            Some([xs[0].unwrap_or(0.0), xs[1].unwrap_or(0.0)])
                        }
                        _ => {
                            self.bad(
                                "position must be two finite numbers [x, y]".into(),
                                &format!("{cp}/position"),
                            );
                            None
                        }
                    },
                };
                children.push(Child {
                    motion: self.motion(c, &cp),
                    threshold: self.number(c, "threshold", &cp),
                    position,
                    time_scale: self.number(c, "timeScale", &cp).unwrap_or(1.0),
                    parameter: self.string(c, "parameter", &cp),
                    pointer: cp,
                });
            }
            return Some(Motion::Tree(BlendTree {
                kind,
                parameters,
                children,
                pointer: tp,
            }));
        }
        match crate::codec::asset_ref_field(node.as_object()?, "motion") {
            Ok(a) => Some(Motion::Asset(a)),
            Err(e) => {
                self.bad(
                    format!("motion: {}", e.message),
                    &format!("{pointer}/motion{}", e.pointer),
                );
                None
            }
        }
    }

    /// The `states` of a layer or of a sub-state machine; `depth` is 0
    /// for a layer's.
    fn states(&mut self, node: &Node, pointer: &str, depth: usize) -> Vec<State> {
        let mut states = Vec::new();
        for (s, sp) in self.objects(node, "states", pointer) {
            let Some(name) = self.string(s, "name", &sp) else {
                continue;
            };
            let motion = self.motion(s, &sp);
            let machine = ["states", "transitions", "defaultState"]
                .iter()
                .any(|k| s.get(k).is_some())
                .then(|| {
                    if motion.is_some() {
                        self.bad(
                            "a sub-state machine has states, not a motion".into(),
                            &format!("{sp}/motion"),
                        );
                    }
                    if depth + 1 >= MAX_DEPTH {
                        self.bad(
                            format!("sub-state machines nest at most {MAX_DEPTH} deep"),
                            &sp,
                        );
                        return Machine::default();
                    }
                    Machine {
                        default_state: self.string(s, "defaultState", &sp),
                        states: self.states(s, &sp, depth + 1),
                        transitions: self.transitions(s, &sp),
                    }
                });
            states.push(State {
                name,
                motion,
                speed: self.number(s, "speed", &sp).unwrap_or(1.0) as f32,
                speed_parameter: self.string(s, "speedParameter", &sp),
                looping: self.boolean(s, "loop", &sp).unwrap_or(true),
                root_motion: self.boolean(s, "rootMotion", &sp),
                machine,
                pointer: sp,
            });
        }
        states
    }

    /// The `transitions` of a layer or of a sub-state machine.
    fn transitions(&mut self, node: &Node, pointer: &str) -> Vec<Transition> {
        let mut transitions = Vec::new();
        for (t, tp) in self.objects(node, "transitions", pointer) {
            let from = self.string(t, "from", &tp);
            let to = self.string(t, "to", &tp);
            let mut conditions = Vec::new();
            for (c, cp) in self.objects(t, "conditions", &tp) {
                let Some(parameter) = self.string(c, "parameter", &cp) else {
                    continue;
                };
                let mut tests = Vec::new();
                if let Some(v) = self.number(c, "greater", &cp) {
                    tests.push(Test::Greater(v));
                }
                if let Some(v) = self.number(c, "less", &cp) {
                    tests.push(Test::Less(v));
                }
                for (key, not) in [("equals", false), ("notEquals", true)] {
                    if let Some(v) = self.number(c, key, &cp) {
                        if v.fract() != 0.0 {
                            self.bad(
                                format!("{key} takes a whole number"),
                                &format!("{cp}/{key}"),
                            );
                        }
                        let v = v as i64;
                        tests.push(if not {
                            Test::NotEquals(v)
                        } else {
                            Test::Equals(v)
                        });
                    }
                }
                if let Some(v) = self.boolean(c, "is", &cp) {
                    tests.push(Test::Is(v));
                }
                let test = match tests.as_slice() {
                    [] => Test::Set,
                    [one] => *one,
                    _ => {
                        self.bad("a condition has one test".into(), &cp);
                        tests[0]
                    }
                };
                conditions.push(Condition {
                    parameter,
                    test,
                    pointer: cp,
                });
            }
            let exit_time = self.number(t, "exitTime", &tp).map(|v| v as f32);
            let duration = self
                .number(t, "duration", &tp)
                .map_or(DEFAULT_DURATION, |v| v as f32);
            let offset = self.number(t, "offset", &tp).unwrap_or(0.0) as f32;
            let to_self = self.boolean(t, "canTransitionToSelf", &tp).unwrap_or(false);
            let interruptible_by = match t.get("interruptibleBy").map(Node::as_str) {
                None | Some(Some("none")) => Interruption::None,
                Some(Some("next")) => Interruption::Next,
                Some(Some("any")) => Interruption::Any,
                Some(_) => {
                    self.bad(
                        "interruptibleBy must be none, next or any".into(),
                        &format!("{tp}/interruptibleBy"),
                    );
                    Interruption::None
                }
            };
            if let (Some(from), Some(to)) = (from, to) {
                transitions.push(Transition {
                    from,
                    to,
                    conditions,
                    exit_time,
                    duration,
                    offset,
                    can_transition_to_self: to_self,
                    interruptible_by,
                    pointer: tp,
                });
            }
        }
        transitions
    }

    /// The objects of the array `key` with their pointers; a non-object
    /// item is reported and left out.
    fn objects<'n>(&mut self, node: &'n Node, key: &str, pointer: &str) -> Vec<(&'n Node, String)> {
        let Some(v) = node.get(key) else {
            return Vec::new();
        };
        let Some(list) = v.as_array() else {
            self.bad(
                format!("{key} must be an array"),
                &format!("{pointer}/{key}"),
            );
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, item) in list.iter().enumerate() {
            let p = format!("{pointer}/{key}/{i}");
            if item.as_object().is_some() {
                out.push((item, p));
            } else {
                self.bad(format!("{key}[{i}] must be an object"), &p);
            }
        }
        out
    }
}

impl Controller {
    pub fn from_document(document: Document) -> Controller {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Controller {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!(
                        "expected an Animator Controller, got {}",
                        document.kind.name()
                    ),
                )
                .with_pointer(""),
            );
        }
        let root = document.root.clone();
        let mut r = Reader {
            root: &root,
            diags: &mut diags,
        };

        let mut parameters = Vec::new();
        for (node, pointer) in r.objects(&root, "parameters", "") {
            let name = r.string(node, "name", &pointer);
            let kind = match node
                .get("type")
                .map(|t| t.as_str().and_then(ParameterType::parse))
            {
                Some(Some(k)) => Some(k),
                Some(None) => {
                    r.bad(
                        "type must be float, int, bool or trigger".into(),
                        &format!("{pointer}/type"),
                    );
                    None
                }
                None => None,
            };
            let default = match (node.get("default"), kind) {
                (None, _) | (_, None) => 0.0,
                (Some(d), Some(ParameterType::Bool)) => match d.as_bool() {
                    Some(b) => f64::from(u8::from(b)),
                    None => {
                        r.bad(
                            "a bool's default is true or false".into(),
                            &format!("{pointer}/default"),
                        );
                        0.0
                    }
                },
                (Some(_), Some(ParameterType::Trigger)) => {
                    r.bad(
                        "a trigger has no default: it starts unset".into(),
                        &format!("{pointer}/default"),
                    );
                    0.0
                }
                (Some(d), Some(k)) => match d.as_f64().filter(|n| n.is_finite()) {
                    Some(n) if k == ParameterType::Int && n.fract() != 0.0 => {
                        r.bad(
                            "an int's default is a whole number".into(),
                            &format!("{pointer}/default"),
                        );
                        n.trunc()
                    }
                    Some(n) => n,
                    None => {
                        r.bad(
                            format!("a {}'s default is a number", k.name()),
                            &format!("{pointer}/default"),
                        );
                        0.0
                    }
                },
            };
            if let (Some(name), Some(kind)) = (name, kind) {
                parameters.push(Parameter {
                    name,
                    kind,
                    default,
                    pointer,
                });
            }
        }

        let mut layers = Vec::new();
        for (node, lp) in r.objects(&root, "layers", "") {
            let name = r.string(node, "name", &lp).unwrap_or_default();
            let weight = r.number(node, "weight", &lp).unwrap_or(1.0) as f32;
            let blending = match node.get("blending").map(Node::as_str) {
                None | Some(Some("override")) => Blending::Override,
                Some(Some("additive")) => Blending::Additive,
                Some(_) => {
                    r.bad(
                        "blending must be override or additive".into(),
                        &format!("{lp}/blending"),
                    );
                    Blending::Override
                }
            };
            let root_node = r.string(node, "root", &lp);
            let mut mask = Vec::new();
            for (e, ep) in r.objects(node, "mask", &lp) {
                let entry = r.string(e, "node", &ep);
                let include = r.boolean(e, "include", &ep);
                let subtree = r.boolean(e, "subtree", &ep).unwrap_or(true);
                if let (Some(node), Some(include)) = (entry, include) {
                    mask.push(MaskEntry {
                        node,
                        include,
                        subtree,
                        pointer: ep,
                    });
                }
            }
            let default_state = r.string(node, "defaultState", &lp);

            let states = r.states(node, &lp, 0);
            let transitions = r.transitions(node, &lp);

            layers.push(Layer {
                name,
                weight,
                blending,
                root: root_node,
                mask,
                default_state,
                states,
                transitions,
                pointer: lp,
            });
        }

        let mut ik = Vec::new();
        for (node, ip) in r.objects(&root, "ik", "") {
            let name = r.string(node, "name", &ip).unwrap_or_default();
            let kind = match node.get("type").map(|t| t.as_str().and_then(IkType::parse)) {
                Some(Some(k)) => Some(k),
                Some(None) => {
                    r.bad(
                        "type must be twoBone, lookAt or footPlacement".into(),
                        &format!("{ip}/type"),
                    );
                    None
                }
                None => None,
            };
            let weight = match node.get("weight") {
                None => IkWeight::Value(1.0),
                Some(w) => match (w.as_f64(), w.as_str()) {
                    (Some(v), _) if (0.0..=1.0).contains(&v) => IkWeight::Value(v as f32),
                    (_, Some(p)) if !p.is_empty() => IkWeight::Parameter(p.to_string()),
                    _ => {
                        r.bad(
                            "weight must be a number within [0, 1] or a float parameter's name"
                                .into(),
                            &format!("{ip}/weight"),
                        );
                        IkWeight::Value(1.0)
                    }
                },
            };
            let mut spine = Vec::new();
            match node.get("spine").map(|v| v.as_array()) {
                None => {}
                Some(None) => r.bad(
                    "spine must be an array of node names".into(),
                    &format!("{ip}/spine"),
                ),
                Some(Some(list)) => {
                    for (i, v) in list.iter().enumerate() {
                        match v.as_str() {
                            Some(n) if !n.is_empty() => spine.push(n.to_string()),
                            _ => r.bad("a node is a name".into(), &format!("{ip}/spine/{i}")),
                        }
                    }
                }
            }
            ik.push(Ik {
                name,
                kind,
                weight,
                target: r.string(node, "target", &ip),
                tip: r.string(node, "tip", &ip),
                middle: r.string(node, "middle", &ip),
                root: r.string(node, "root", &ip),
                pole: r.string(node, "pole", &ip),
                head: r.string(node, "head", &ip),
                spine,
                limit: r.number(node, "limit", &ip).unwrap_or(70.0) as f32,
                body: r.number(node, "body", &ip).unwrap_or(0.3) as f32,
                hips: r.string(node, "hips", &ip),
                left: r.string(node, "left", &ip),
                right: r.string(node, "right", &ip),
                reach: r.number(node, "reach", &ip).unwrap_or(0.5) as f32,
                pointer: ip,
            });
        }

        Controller {
            document,
            parameters,
            layers,
            ik,
            read_diagnostics: diags,
        }
    }

    pub fn id(&self) -> Uuid {
        self.document.envelope.id
    }

    pub fn name(&self) -> &str {
        &self.document.envelope.name
    }

    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    /// "state `Walk`" or "a blend tree child of state `Walk`" for a
    /// motion's pointer.
    fn who(&self, pointer: &str) -> String {
        let leaves: Vec<(String, &State)> = self.layers.iter().flat_map(Layer::leaves).collect();
        let state = leaves
            .iter()
            .find(|(_, s)| pointer.starts_with(&format!("{}/", s.pointer)));
        match state {
            Some((path, _)) if pointer.contains("/blendTree/") => {
                format!("a blend tree child of state `{path}`")
            }
            Some((path, _)) => format!("state `{path}`"),
            None => "a motion".to_string(),
        }
    }

    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// Every motion the states name, once each (what the cook makes the
    /// controller depend on and the runtime registers).
    pub fn motions(&self) -> Vec<AssetRef> {
        let mut out: Vec<AssetRef> = Vec::new();
        for (m, _) in self.motion_refs() {
            if !m.is_nil() && !out.contains(m) {
                out.push(m.clone());
            }
        }
        out
    }

    /// Every asset a state or a blend tree's child plays, with the
    /// pointer of its `motion`.
    pub fn motion_refs(&self) -> Vec<(&AssetRef, String)> {
        let mut out = Vec::new();
        for (_, s) in self.layers.iter().flat_map(Layer::leaves) {
            if let Some(m) = &s.motion {
                m.assets(&format!("{}/motion", s.pointer), &mut out);
            }
        }
        out
    }

    /// Envelope + JSON Schema, the read problems and the naming and type
    /// rules (`D1101`, `D1102`); the rule that needs the asset database is
    /// [`Controller::validate_references`].
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for d in &self.read_diagnostics {
            // JSON Schema already said it (wrong type / missing field).
            if d.code == Code::CONTROLLER_MALFORMED_ENTRY
                && d.pointer
                    .as_ref()
                    .is_some_and(|p| schema_pointers.contains(p))
            {
                continue;
            }
            out.push(d.clone());
        }
        let root = &self.document.root;
        let names = |message: String, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::CONTROLLER_NAMES, message),
                pointer,
            )
        };
        let mut seen = HashSet::new();
        for p in &self.parameters {
            if !seen.insert(p.name.as_str()) {
                out.push(names(
                    format!("two parameters are named `{}`", p.name),
                    &format!("{}/name", p.pointer),
                ));
            }
        }
        let mut seen_layers = HashSet::new();
        for l in &self.layers {
            if !l.name.is_empty() && !seen_layers.insert(l.name.as_str()) {
                out.push(names(
                    format!("two layers are named `{}`", l.name),
                    &format!("{}/name", l.pointer),
                ));
            }
            for scope in l.scopes() {
                self.validate_scope(l, &scope, &mut out);
            }
        }
        self.validate_ik(&mut out);
        finish(out, &self.document)
    }

    /// The IK goals' names (`D1101`) and weight parameters (`D1101`,
    /// `D1102`); the nodes each type needs are the schema's.
    fn validate_ik(&self, out: &mut Vec<Diagnostic>) {
        let root = &self.document.root;
        let mut seen = HashSet::new();
        for g in &self.ik {
            if !g.name.is_empty() && !seen.insert(g.name.as_str()) {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::CONTROLLER_NAMES,
                        format!("two IK goals are named `{}`", g.name),
                    ),
                    &format!("{}/name", g.pointer),
                ));
            }
            if let IkWeight::Parameter(p) = &g.weight {
                let at = format!("{}/weight", g.pointer);
                match self.parameter(p) {
                    None => out.push(
                        anchor(
                            root,
                            Diagnostic::error(
                                Code::CONTROLLER_NAMES,
                                format!("weight `{p}` is not a parameter"),
                            ),
                            &at,
                        )
                        .with_suggestion("a number within [0, 1], or a float parameter's name"),
                    ),
                    Some(q) if q.kind != ParameterType::Float => out.push(anchor(
                        root,
                        Diagnostic::error(
                            Code::CONTROLLER_CONDITION,
                            format!("weight `{p}` is a {}, not a float", q.kind.name()),
                        ),
                        &at,
                    )),
                    Some(_) => {}
                }
            }
        }
    }

    /// One scope's names (`D1101`) and types (`D1102`): a layer's own
    /// states and transitions, or a sub-state machine's.
    fn validate_scope(&self, l: &Layer, scope: &Scope<'_>, out: &mut Vec<Diagnostic>) {
        let root = &self.document.root;
        let names = |message: String, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::CONTROLLER_NAMES, message),
                pointer,
            )
        };
        let typed = |message: String, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::CONTROLLER_CONDITION, message),
                pointer,
            )
        };
        let parameter_names = || {
            self.parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let within = if scope.path.is_empty() {
            format!("layer `{}`", l.name)
        } else {
            format!("sub-state machine `{}`", scope.path)
        };

        let mut seen_states = HashSet::new();
        for s in scope.states {
            if s.name == ANY_STATE {
                out.push(
                    names(
                        format!("a state may not be named `{ANY_STATE}` (it means Any State)"),
                        &format!("{}/name", s.pointer),
                    )
                    .with_suggestion("rename the state"),
                );
            } else if s.name.contains('/') {
                out.push(
                    names(
                        format!(
                            "a state's name may not hold `/` (`{}`): it separates a path",
                            s.name
                        ),
                        &format!("{}/name", s.pointer),
                    )
                    .with_suggestion("rename the state"),
                );
            } else if !seen_states.insert(s.name.as_str()) {
                let message = if scope.path.is_empty() {
                    format!("two states of layer `{}` are named `{}`", l.name, s.name)
                } else {
                    format!("two states of {within} are named `{}`", s.name)
                };
                out.push(names(message, &format!("{}/name", s.pointer)));
            }
            if let Some(m) = &s.machine {
                if m.states.is_empty() {
                    out.push(
                        names(
                            format!(
                                "sub-state machine `{}` has no states",
                                scope.path_of(&s.name)
                            ),
                            &s.pointer,
                        )
                        .with_suggestion("give it states, or make it a state with a motion"),
                    );
                }
            }
            if let Some(Motion::Tree(tree)) = &s.motion {
                self.validate_tree(tree, 1, out);
            }
            if let Some(sp) = &s.speed_parameter {
                match self.parameter(sp) {
                    None => out.push(
                        names(
                            format!("speedParameter `{sp}` is not a parameter"),
                            &format!("{}/speedParameter", s.pointer),
                        )
                        .with_suggestion(format!("the parameters: {}", parameter_names())),
                    ),
                    Some(p) if p.kind != ParameterType::Float => out.push(typed(
                        format!("speedParameter `{sp}` is a {}, not a float", p.kind.name()),
                        &format!("{}/speedParameter", s.pointer),
                    )),
                    Some(_) => {}
                }
            }
        }
        let state_names = || {
            scope
                .states
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match scope.default_state {
            Some(d) if scope.state(d).is_none() => out.push(
                names(
                    format!("defaultState `{d}` is not a state of {within}"),
                    &format!("{}/defaultState", scope.pointer),
                )
                .with_suggestion(format!("the states: {}", state_names())),
            ),
            None if !scope.states.is_empty() => out.push(
                names(
                    format!("{within} has states but no defaultState"),
                    scope.pointer,
                )
                .with_suggestion(format!(
                    "name the state it starts in: one of {}",
                    state_names()
                )),
            ),
            _ => {}
        }
        for t in scope.transitions {
            if t.from_any() && !scope.path.is_empty() {
                out.push(
                    names(
                        format!("Any State's transitions belong to the layer, not to {within}"),
                        &format!("{}/from", t.pointer),
                    )
                    .with_suggestion(format!(
                        "move it to layer `{}`'s transitions, or from `{}` there: that is checked from every state inside it",
                        l.name, scope.path
                    )),
                );
            } else if !t.from_any() && scope.state(&t.from).is_none() {
                out.push(
                    names(
                        format!("from `{}` is not a state of {within}", t.from),
                        &format!("{}/from", t.pointer),
                    )
                    .with_suggestion(format!("a state ({}) or `{ANY_STATE}`", state_names())),
                );
            }
            if scope.state(&t.to).is_none() {
                out.push(
                    names(
                        format!("to `{}` is not a state of {within}", t.to),
                        &format!("{}/to", t.pointer),
                    )
                    .with_suggestion(format!("the states: {}", state_names())),
                );
            }
            for c in &t.conditions {
                match self.parameter(&c.parameter) {
                    None => out.push(
                        names(
                            format!("`{}` is not a parameter", c.parameter),
                            &format!("{}/parameter", c.pointer),
                        )
                        .with_suggestion(format!("the parameters: {}", parameter_names())),
                    ),
                    Some(p) if !c.test.fits(p.kind) => out.push(
                        typed(
                            format!(
                                "`{}` is a {}: a condition on it cannot use {}",
                                p.name,
                                p.kind.name(),
                                c.test.word()
                            ),
                            &c.pointer,
                        )
                        .with_suggestion(match p.kind {
                            ParameterType::Float => "a float takes `greater` or `less`",
                            ParameterType::Int => {
                                "an int takes `greater`, `less`, `equals` or `notEquals`"
                            }
                            ParameterType::Bool => "a bool takes `is: true` or `is: false`",
                            ParameterType::Trigger => {
                                "a trigger takes no test: `{ \"parameter\": \"jump\" }`"
                            }
                        }),
                    ),
                    Some(_) => {}
                }
            }
            if t.from_any() && t.conditions.is_empty() && t.exit_time.is_none() {
                out.push(
                    anchor(
                        root,
                        Diagnostic::warning(
                            Code::CONTROLLER_CONDITION,
                            format!("an Any State transition to `{}` with no condition and no exit time fires every tick", t.to),
                        ),
                        &t.pointer,
                    )
                    .with_suggestion("give it a condition"),
                );
            }
        }
    }

    /// A blend tree's shape (`D1104`) and names (`D1101`/`D1102`),
    /// nested trees included; `depth` is 1 for a state's own tree.
    fn validate_tree(&self, tree: &BlendTree, depth: usize, out: &mut Vec<Diagnostic>) {
        use fynkel_core::animation::blend_tree::TreeDef;
        let root = &self.document.root;
        let shape = |message: String, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::CONTROLLER_BLEND_TREE, message),
                pointer,
            )
        };
        let parameter_names = || {
            self.parameters
                .iter()
                .filter(|p| p.kind == ParameterType::Float)
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        // A float parameter, or the diagnostic for the one named.
        let float = |name: &str, pointer: &str| -> Option<Diagnostic> {
            match self.parameter(name) {
                None => Some(
                    anchor(
                        root,
                        Diagnostic::error(
                            Code::CONTROLLER_NAMES,
                            format!("`{name}` is not a parameter"),
                        ),
                        pointer,
                    )
                    .with_suggestion(format!("the float parameters: {}", parameter_names())),
                ),
                Some(p) if p.kind != ParameterType::Float => Some(anchor(
                    root,
                    Diagnostic::error(
                        Code::CONTROLLER_CONDITION,
                        format!(
                            "`{name}` is a {}: a blend tree reads float parameters",
                            p.kind.name()
                        ),
                    ),
                    pointer,
                )),
                Some(_) => None,
            }
        };
        if depth > TreeDef::MAX_DEPTH {
            out.push(shape(
                format!("blend trees nest at most {} deep", TreeDef::MAX_DEPTH),
                &tree.pointer,
            ));
            return;
        }
        if depth == 1 && tree.leaf_count() > TreeDef::MAX_LEAVES {
            out.push(shape(
                format!(
                    "the tree plays {} motions; at most {}",
                    tree.leaf_count(),
                    TreeDef::MAX_LEAVES
                ),
                &tree.pointer,
            ));
        }
        let Some(kind) = tree.kind else {
            return;
        };
        let want = kind.parameter_count();
        if tree.parameters.len() != want {
            out.push(
                shape(
                    format!(
                        "a {} tree reads {want} parameter(s); it names {}",
                        kind.name(),
                        tree.parameters.len()
                    ),
                    &format!("{}/parameters", tree.pointer),
                )
                .with_suggestion(match want {
                    0 => "a direct tree names a parameter on each child instead".to_string(),
                    1 => format!("one float parameter: {}", parameter_names()),
                    _ => format!("two float parameters (x, y): {}", parameter_names()),
                }),
            );
        }
        for (i, name) in tree.parameters.iter().enumerate() {
            if let Some(d) = float(name, &format!("{}/parameters/{i}", tree.pointer)) {
                out.push(d);
            }
        }
        if tree.children.is_empty() || tree.children.len() > TreeDef::MAX_CHILDREN {
            out.push(shape(
                format!(
                    "a blend tree has 1 to {} children; this one has {}",
                    TreeDef::MAX_CHILDREN,
                    tree.children.len()
                ),
                &format!("{}/children", tree.pointer),
            ));
        }
        let mut seen: Vec<([f64; 2], usize)> = Vec::new();
        for (i, c) in tree.children.iter().enumerate() {
            let missing = |what: &str| {
                shape(
                    format!("child {i} of a {} tree needs {what}", kind.name()),
                    &c.pointer,
                )
            };
            let at = match kind {
                TreeKind::OneD => match c.threshold {
                    Some(t) => Some([t, 0.0]),
                    None => {
                        out.push(missing("a threshold"));
                        None
                    }
                },
                TreeKind::Direct => {
                    match &c.parameter {
                        None => out.push(missing("a parameter (its weight)")),
                        Some(name) => {
                            if let Some(d) = float(name, &format!("{}/parameter", c.pointer)) {
                                out.push(d);
                            }
                        }
                    }
                    None
                }
                _ => {
                    if c.position.is_none() {
                        out.push(missing("a position [x, y]"));
                    }
                    c.position
                }
            };
            if let Some(at) = at {
                if let Some((_, j)) = seen.iter().find(|(p, _)| *p == at) {
                    out.push(shape(
                        format!("children {j} and {i} are at the same position"),
                        &c.pointer,
                    ));
                } else {
                    seen.push((at, i));
                }
            }
            if c.time_scale.is_nan() || c.time_scale <= 0.0 {
                out.push(shape(
                    "timeScale must be above 0".into(),
                    &format!("{}/timeScale", c.pointer),
                ));
            }
            match &c.motion {
                None => out.push(missing("a motion")),
                Some(Motion::Tree(t)) => self.validate_tree(t, depth + 1, out),
                Some(Motion::Asset(_)) => {}
            }
        }
    }

    /// The rule against the asset database: a state's motion is an
    /// animation clip — a rig document's `anim/<name>` or a model
    /// source's `anim/<n>` — or a blend space (`D1103`). A reference the
    /// database cannot resolve is left to
    /// [`AssetDatabase::check_references`] (`A0004`/`A0005`).
    pub fn validate_references(&self, db: &AssetDatabase) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        for (motion, pointer) in self.motion_refs() {
            if motion.is_nil() {
                continue;
            }
            let Ok(resolved) = db.resolve(motion) else {
                continue;
            };
            let clip_sub = resolved
                .sub
                .as_ref()
                .is_some_and(|(sub, _)| sub.starts_with(RIG_CLIP_PREFIX));
            let carrier = is_model(&resolved.entry.kind)
                || resolved.entry.kind == AssetKind::Document(DocumentKind::Rig);
            let blend = resolved.entry.kind == AssetKind::Document(DocumentKind::Blend)
                && resolved.sub.is_none();
            if !(blend || carrier && clip_sub) {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::CONTROLLER_MOTION,
                        format!(
                            "{} plays {}{}, which is not an animation clip (anim/<n>) or a blend space",
                            self.who(&pointer),
                            resolved.entry.path,
                            match &resolved.sub {
                                Some((sub, _)) => format!("#{sub}"),
                                None => String::new(),
                            }
                        ),
                    )
                    .with_id(motion.to_string())
                    .with_suggestion("pick a model's anim/<n>, a rig's clip or a .blend.json"),
                    &pointer,
                ));
            }
        }
        finish(out, &self.document)
    }
}

fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    let mut at = pointer.to_string();
    loop {
        if let Some(s) = root.pointer_span(&at) {
            return d.at(s.start.line, s.start.column);
        }
        match at.rfind('/') {
            Some(i) => at.truncate(i),
            None => return d,
        }
    }
}

fn finish(mut out: Vec<Diagnostic>, document: &Document) -> Vec<Diagnostic> {
    for d in &mut out {
        if d.document.is_none() {
            d.document = document.path.clone();
        }
    }
    out
}

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
//! space is `D1103`. Unknown ids are the asset database's `A0004`/`A0005`.

use std::collections::HashSet;

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use uuid::Uuid;

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

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub name: String,
    /// None: an empty state (it holds the pose).
    pub motion: Option<AssetRef>,
    pub speed: f32,
    pub speed_parameter: Option<String>,
    pub looping: bool,
    /// None: the Animator's `root_motion`.
    pub root_motion: Option<bool>,
    /// `/layers/<l>/states/<i>`.
    pub pointer: String,
}

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
    /// `/layers/<l>/transitions/<i>`.
    pub pointer: String,
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
}

#[derive(Clone, Debug)]
pub struct Controller {
    pub document: Document,
    pub parameters: Vec<Parameter>,
    pub layers: Vec<Layer>,
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
            let default_state = r.string(node, "defaultState", &lp);

            let mut states = Vec::new();
            for (s, sp) in r.objects(node, "states", &lp) {
                let Some(name) = r.string(s, "name", &sp) else {
                    continue;
                };
                let motion = match s.as_object().filter(|_| s.get("motion").is_some()) {
                    None => None,
                    Some(members) => match crate::codec::asset_ref_field(members, "motion") {
                        Ok(a) => Some(a),
                        Err(e) => {
                            r.bad(
                                format!("motion: {}", e.message),
                                &format!("{sp}/motion{}", e.pointer),
                            );
                            None
                        }
                    },
                };
                states.push(State {
                    name,
                    motion,
                    speed: r.number(s, "speed", &sp).unwrap_or(1.0) as f32,
                    speed_parameter: r.string(s, "speedParameter", &sp),
                    looping: r.boolean(s, "loop", &sp).unwrap_or(true),
                    root_motion: r.boolean(s, "rootMotion", &sp),
                    pointer: sp,
                });
            }

            let mut transitions = Vec::new();
            for (t, tp) in r.objects(node, "transitions", &lp) {
                let from = r.string(t, "from", &tp);
                let to = r.string(t, "to", &tp);
                let mut conditions = Vec::new();
                for (c, cp) in r.objects(t, "conditions", &tp) {
                    let Some(parameter) = r.string(c, "parameter", &cp) else {
                        continue;
                    };
                    let mut tests = Vec::new();
                    if let Some(v) = r.number(c, "greater", &cp) {
                        tests.push(Test::Greater(v));
                    }
                    if let Some(v) = r.number(c, "less", &cp) {
                        tests.push(Test::Less(v));
                    }
                    for (key, not) in [("equals", false), ("notEquals", true)] {
                        if let Some(v) = r.number(c, key, &cp) {
                            if v.fract() != 0.0 {
                                r.bad(
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
                    if let Some(v) = r.boolean(c, "is", &cp) {
                        tests.push(Test::Is(v));
                    }
                    let test = match tests.as_slice() {
                        [] => Test::Set,
                        [one] => *one,
                        _ => {
                            r.bad("a condition has one test".into(), &cp);
                            tests[0]
                        }
                    };
                    conditions.push(Condition {
                        parameter,
                        test,
                        pointer: cp,
                    });
                }
                let exit_time = r.number(t, "exitTime", &tp).map(|v| v as f32);
                let duration = r
                    .number(t, "duration", &tp)
                    .map_or(DEFAULT_DURATION, |v| v as f32);
                let offset = r.number(t, "offset", &tp).unwrap_or(0.0) as f32;
                let to_self = r.boolean(t, "canTransitionToSelf", &tp).unwrap_or(false);
                if let (Some(from), Some(to)) = (from, to) {
                    transitions.push(Transition {
                        from,
                        to,
                        conditions,
                        exit_time,
                        duration,
                        offset,
                        can_transition_to_self: to_self,
                        pointer: tp,
                    });
                }
            }

            layers.push(Layer {
                name,
                weight,
                blending,
                root: root_node,
                default_state,
                states,
                transitions,
                pointer: lp,
            });
        }

        Controller {
            document,
            parameters,
            layers,
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

    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// Every motion the states name, once each (what the cook makes the
    /// controller depend on and the runtime registers).
    pub fn motions(&self) -> Vec<AssetRef> {
        let mut out: Vec<AssetRef> = Vec::new();
        for s in self.layers.iter().flat_map(|l| &l.states) {
            if let Some(m) = &s.motion {
                if !m.is_nil() && !out.contains(m) {
                    out.push(m.clone());
                }
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
        let typed = |message: String, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::CONTROLLER_CONDITION, message),
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
        let parameter_names = || {
            self.parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        let mut seen_layers = HashSet::new();
        for l in &self.layers {
            if !l.name.is_empty() && !seen_layers.insert(l.name.as_str()) {
                out.push(names(
                    format!("two layers are named `{}`", l.name),
                    &format!("{}/name", l.pointer),
                ));
            }
            let mut seen_states = HashSet::new();
            for s in &l.states {
                if s.name == ANY_STATE {
                    out.push(
                        names(
                            format!("a state may not be named `{ANY_STATE}` (it means Any State)"),
                            &format!("{}/name", s.pointer),
                        )
                        .with_suggestion("rename the state"),
                    );
                } else if !seen_states.insert(s.name.as_str()) {
                    out.push(names(
                        format!("two states of layer `{}` are named `{}`", l.name, s.name),
                        &format!("{}/name", s.pointer),
                    ));
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
                l.states
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            match &l.default_state {
                Some(d) if l.state(d).is_none() => out.push(
                    names(
                        format!("defaultState `{d}` is not a state of layer `{}`", l.name),
                        &format!("{}/defaultState", l.pointer),
                    )
                    .with_suggestion(format!("the states: {}", state_names())),
                ),
                None if !l.states.is_empty() => out.push(
                    names(
                        format!("layer `{}` has states but no defaultState", l.name),
                        &l.pointer,
                    )
                    .with_suggestion(format!(
                        "name the state it starts in: one of {}",
                        state_names()
                    )),
                ),
                _ => {}
            }
            for t in &l.transitions {
                if !t.from_any() && l.state(&t.from).is_none() {
                    out.push(
                        names(
                            format!("from `{}` is not a state of layer `{}`", t.from, l.name),
                            &format!("{}/from", t.pointer),
                        )
                        .with_suggestion(format!("a state ({}) or `{ANY_STATE}`", state_names())),
                    );
                }
                if l.state(&t.to).is_none() {
                    out.push(
                        names(
                            format!("to `{}` is not a state of layer `{}`", t.to, l.name),
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
        finish(out, &self.document)
    }

    /// The rule against the asset database: a state's motion is an
    /// animation clip — a rig document's `anim/<name>` or a model
    /// source's `anim/<n>` — or a blend space (`D1103`). A reference the
    /// database cannot resolve is left to
    /// [`AssetDatabase::check_references`] (`A0004`/`A0005`).
    pub fn validate_references(&self, db: &AssetDatabase) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        for s in self.layers.iter().flat_map(|l| &l.states) {
            let Some(motion) = s.motion.as_ref().filter(|m| !m.is_nil()) else {
                continue;
            };
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
                            "state `{}` plays {}{}, which is not an animation clip (anim/<n>) or a blend space",
                            s.name,
                            resolved.entry.path,
                            match &resolved.sub {
                                Some((sub, _)) => format!("#{sub}"),
                                None => String::new(),
                            }
                        ),
                    )
                    .with_id(motion.to_string())
                    .with_suggestion("pick a model's anim/<n>, a rig's clip or a .blend.json"),
                    &format!("{}/motion", s.pointer),
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

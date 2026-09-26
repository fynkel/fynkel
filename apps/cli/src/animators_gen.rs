//! `lib/generated/animators.g.dart` (ADR-0033 § 3): typed access to every
//! Animator Controller of a project, so game code writes
//! `PlayerAnimator(animation).speed = v` and a renamed parameter or state
//! is a compile error instead of a throw at run time. Written by
//! `fynkel codegen --project <dir>` (and `--assets-only`, which the editor
//! runs after a controller is saved) beside `assets.g.dart`.
//!
//! For `player.controller.json` it writes:
//!
//! - `PlayerAnimator`, an extension type over `AnimationControl`: a getter
//!   and setter per float, int or bool parameter, a method per trigger
//!   (and `reset…`), the base layer's `state` and `nextState`, typed
//!   `play`/`crossfade`, and `playOneShot`/`stopOneShot`/
//!   `isPlayingOneShot` taking a `PlayerSlot`;
//! - `PlayerState`, the base layer's states as an enum (another layer with
//!   states gets `Player<Layer>State` and a `<layer>State` getter);
//! - `PlayerSlot`, the layers' slots.
//!
//! The class is named after the file (`player.controller.json` →
//! `Player`); two controllers with the same file name get a number. A
//! controller that does not read is left out (`fynkel validate` reports
//! it). Without controllers there is no file, unless one was written
//! before (then it is emptied, so imports keep compiling).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use fynkel_document::asset::AssetKind;
use fynkel_document::DocumentKind;
use serde_json::Value;

use crate::assets_gen::{assets_dir, camel, dart_string, ident, scan, stem, upper_first, KEYWORDS};

pub const DART_OUT: &str = "lib/generated/animators.g.dart";

/// One controller as the file says.
struct Controller {
    id: String,
    path: String,
    /// `Player`.
    name: String,
    parameters: Vec<(String, String)>,
    /// `(layer name, its states)`.
    layers: Vec<(String, Vec<String>)>,
}

/// The Dart source for the project at `project`, or `None` when there is
/// nothing to write: not a project's package, or no controllers and no
/// file from before.
pub fn generate(project: &Path) -> Result<Option<String>, String> {
    let Some(dir) = assets_dir(project)? else {
        return Ok(None);
    };
    let mut entries: Vec<_> = scan(&dir)
        .into_iter()
        .filter(|e| e.kind == AssetKind::Document(DocumentKind::Controller))
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut taken = BTreeSet::new();
    let mut controllers = Vec::new();
    for e in &entries {
        let Ok(text) = std::fs::read_to_string(dir.join(&e.path)) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let base = upper_first(&camel(&stem(e).0));
        let base = if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
            format!("Controller{base}")
        } else {
            base
        };
        let mut name = base.clone();
        let mut n = 2;
        while !taken.insert(name.clone()) {
            name = format!("{base}{n}");
            n += 1;
        }
        let names = |v: Option<&Value>, key: &str| -> Vec<Value> {
            v.and_then(|v| v.get(key))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let text_of = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
        controllers.push(Controller {
            id: e.id.to_string(),
            path: e.path.clone(),
            name,
            parameters: names(Some(&doc), "parameters")
                .iter()
                .filter_map(|p| Some((text_of(p, "name")?, text_of(p, "type")?)))
                .collect(),
            layers: names(Some(&doc), "layers")
                .iter()
                .filter_map(|l| {
                    let mut states = Vec::new();
                    leaves(l, "", &mut states);
                    Some((text_of(l, "name")?, states))
                })
                .collect(),
        });
    }
    if controllers.is_empty() && !project.join(DART_OUT).is_file() {
        return Ok(None);
    }
    Ok(Some(render(&controllers)))
}

/// The states of a layer or a sub-state machine that play, by path
/// (`Airborne/Fall`, ADR-0033 § 4): a sub-state machine is not one.
fn leaves(scope: &Value, prefix: &str, out: &mut Vec<String>) {
    let states = scope.get("states").and_then(Value::as_array);
    for s in states.into_iter().flatten() {
        let Some(name) = s.get("name").and_then(Value::as_str) else {
            continue;
        };
        let path = format!("{prefix}{name}");
        if s.get("states").is_some() {
            leaves(s, &format!("{path}/"), out);
        } else {
            out.push(path);
        }
    }
}

/// Names every `AnimationControl` wrapper already uses, and what an enum
/// value or a Dart object cannot be called.
const FIXED: &[&str] = &[
    "animation",
    "controller",
    "state",
    "nextState",
    "play",
    "crossfade",
    "playOneShot",
    "stopOneShot",
    "isPlayingOneShot",
];

const ENUM_TAKEN: &[&str] = &["index", "name", "values", "stateName", "slotName"];

/// `names` as distinct identifiers, none in `taken` (a clash gets
/// `suffix`, then a number).
fn distinct(names: &[String], taken: &[&str], suffix: &str) -> Vec<String> {
    let mut used: BTreeSet<String> = taken.iter().map(|s| s.to_string()).collect();
    used.extend(KEYWORDS.iter().map(|s| s.to_string()));
    names
        .iter()
        .map(|n| {
            let mut id = ident(n);
            if used.contains(&id) {
                id = format!("{}{suffix}", id.trim_end_matches('$'));
            }
            let base = id.clone();
            let mut k = 2;
            while used.contains(&id) {
                id = format!("{base}{k}");
                k += 1;
            }
            used.insert(id.clone());
            id
        })
        .collect()
}

fn render(controllers: &[Controller]) -> String {
    let mut o = String::new();
    let _ = writeln!(
        o,
        "// GENERATED by `fynkel codegen` from the project's Animator Controllers"
    );
    let _ = writeln!(o, "// (ADR-0033 § 3). Do not edit.");
    let _ = writeln!(o, "//");
    let _ = writeln!(o, "// ignore_for_file: type=lint");
    if controllers.is_empty() {
        return o;
    }
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "import 'package:fynkel/fynkel.dart' show AnimationControl, AssetRef;"
    );
    for c in controllers {
        o.push('\n');
        controller(c, &mut o);
    }
    o
}

fn controller(c: &Controller, o: &mut String) {
    let n = &c.name;
    let path = &c.path;
    let with_states: Vec<usize> = (0..c.layers.len())
        .filter(|&i| !c.layers[i].1.is_empty())
        .collect();
    let layer_names: Vec<String> = c.layers.iter().map(|l| l.0.clone()).collect();

    // The enums: each layer's states, then the slots.
    for &i in &with_states {
        let (layer, states) = &c.layers[i];
        let e = state_enum(n, i, layer);
        let _ = writeln!(
            o,
            "/// The states of `{path}`'s {} (`{layer}`).",
            if i == 0 { "base layer" } else { "layer" }
        );
        let _ = writeln!(o, "enum {e} {{");
        for (id, s) in distinct(states, ENUM_TAKEN, "State").iter().zip(states) {
            let _ = writeln!(o, "  {id}({}),", dart_string(s));
        }
        let _ = writeln!(o, "  ;\n");
        let _ = writeln!(o, "  const {e}(this.stateName);\n");
        let _ = writeln!(o, "  /// The state's name in the controller.");
        let _ = writeln!(o, "  final String stateName;\n");
        let _ = writeln!(o, "  static {e}? _of(String? name) => switch (name) {{");
        for (id, s) in distinct(states, ENUM_TAKEN, "State").iter().zip(states) {
            let _ = writeln!(o, "        {} => {e}.{id},", dart_string(s));
        }
        let _ = writeln!(o, "        _ => null,");
        let _ = writeln!(o, "      }};");
        let _ = writeln!(o, "}}\n");
    }
    let _ = writeln!(
        o,
        "/// The slots of `{path}` (ADR-0033 § 2): one per layer, named after it."
    );
    let _ = writeln!(o, "enum {n}Slot {{");
    for (id, l) in distinct(&layer_names, ENUM_TAKEN, "Slot")
        .iter()
        .zip(&layer_names)
    {
        let _ = writeln!(o, "  {id}({}),", dart_string(l));
    }
    let _ = writeln!(o, "  ;\n");
    let _ = writeln!(o, "  const {n}Slot(this.slotName);\n");
    let _ = writeln!(
        o,
        "  /// The layer's name: what `playOneShot` takes as `slot`."
    );
    let _ = writeln!(o, "  final String slotName;");
    let _ = writeln!(o, "}}\n");

    // The wrapper.
    let base = if with_states.first() == Some(&0) {
        Some(state_enum(n, 0, &c.layers[0].0))
    } else {
        None
    };
    let _ = writeln!(
        o,
        "/// Typed access to an `Animator` running `{path}`: `{n}Animator(animation)`."
    );
    let _ = writeln!(
        o,
        "/// A renamed parameter or state is then a compile error, not a throw."
    );
    let _ = writeln!(
        o,
        "extension type const {n}Animator(AnimationControl animation) {{"
    );
    let _ = writeln!(o, "  /// The controller itself.");
    let _ = writeln!(
        o,
        "  static const controller = AssetRef({});",
        dart_string(&c.id)
    );
    let mut fixed: Vec<&str> = FIXED.to_vec();
    let getters: Vec<String> = with_states
        .iter()
        .filter(|&&i| i > 0)
        .map(|&i| format!("{}State", ident(&c.layers[i].0).trim_end_matches('$')))
        .collect();
    fixed.extend(getters.iter().map(String::as_str));
    let resets: Vec<String> = c
        .parameters
        .iter()
        .filter(|(_, t)| t == "trigger")
        .map(|(p, _)| format!("reset{}", upper_first(&camel(p))))
        .collect();
    fixed.extend(resets.iter().map(String::as_str));
    let names: Vec<String> = c.parameters.iter().map(|p| p.0.clone()).collect();
    for (id, (p, t)) in distinct(&names, &fixed, "Parameter")
        .iter()
        .zip(&c.parameters)
    {
        let q = dart_string(p);
        let _ = writeln!(o);
        match t.as_str() {
            "float" => {
                let _ = writeln!(o, "  /// The float `{p}`.");
                let _ = writeln!(o, "  double get {id} => animation.getFloat({q});");
                let _ = writeln!(
                    o,
                    "  set {id}(double value) => animation.setFloat({q}, value);"
                );
            }
            "int" => {
                let _ = writeln!(o, "  /// The int `{p}`.");
                let _ = writeln!(o, "  int get {id} => animation.getInteger({q});");
                let _ = writeln!(
                    o,
                    "  set {id}(int value) => animation.setInteger({q}, value);"
                );
            }
            "bool" => {
                let _ = writeln!(o, "  /// The bool `{p}`.");
                let _ = writeln!(o, "  bool get {id} => animation.getBool({q});");
                let _ = writeln!(
                    o,
                    "  set {id}(bool value) => animation.setBool({q}, value);"
                );
            }
            "trigger" => {
                let _ = writeln!(
                    o,
                    "  /// Sets the trigger `{p}`; the transition that uses it clears it."
                );
                let _ = writeln!(o, "  void {id}() => animation.setTrigger({q});");
                let reset = format!("reset{}", upper_first(&camel(p)));
                let _ = writeln!(o, "  void {reset}() => animation.resetTrigger({q});");
            }
            _ => {}
        }
    }
    if let Some(e) = &base {
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "  /// The base layer's state now (null before the first tick)."
        );
        let _ = writeln!(
            o,
            "  {e}? get state => {e}._of(animation.stateOf()?.name);\n"
        );
        let _ = writeln!(
            o,
            "  /// The state the base layer is blending into, or null."
        );
        let _ = writeln!(
            o,
            "  {e}? get nextState => {e}._of(animation.stateOf()?.next);\n"
        );
        let _ = writeln!(o, "  /// Goes to [state] at once, without conditions.");
        let _ = writeln!(
            o,
            "  void play({e} state) => animation.play(state.stateName);\n"
        );
        let _ = writeln!(o, "  /// Blends into [state] over [seconds].");
        let _ = writeln!(
            o,
            "  void crossfade({e} state, double seconds, {{double offset = 0}}) =>"
        );
        let _ = writeln!(
            o,
            "      animation.crossfade(state.stateName, seconds, offset: offset);"
        );
    }
    for (&i, getter) in with_states.iter().filter(|&&i| i > 0).zip(&getters) {
        let e = state_enum(n, i, &c.layers[i].0);
        let _ = writeln!(o);
        let _ = writeln!(o, "  /// The layer `{}`'s state now.", c.layers[i].0);
        let _ = writeln!(
            o,
            "  {e}? get {getter} => {e}._of(animation.stateOf(layer: {i})?.name);"
        );
    }
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "  /// Plays [motion] once in [slot] over its layer's states (ADR-0033 § 2)."
    );
    let _ = writeln!(o, "  void playOneShot(");
    let _ = writeln!(o, "    AssetRef motion, {{");
    let _ = writeln!(o, "    required {n}Slot slot,");
    let _ = writeln!(o, "    double blendIn = 0.1,");
    let _ = writeln!(o, "    double blendOut = 0.2,");
    let _ = writeln!(o, "    double speed = 1,");
    let _ = writeln!(o, "  }}) =>");
    let _ = writeln!(
        o,
        "      animation.playOneShot(motion, slot: slot.slotName, blendIn: blendIn, blendOut: blendOut, speed: speed);\n"
    );
    let _ = writeln!(o, "  /// Blends [slot]'s one-shot out.");
    let _ = writeln!(
        o,
        "  void stopOneShot({n}Slot slot, {{double blendOut = 0.2}}) =>"
    );
    let _ = writeln!(
        o,
        "      animation.stopOneShot(slot: slot.slotName, blendOut: blendOut);\n"
    );
    let _ = writeln!(o, "  /// Whether a one-shot plays in [slot].");
    let _ = writeln!(
        o,
        "  bool isPlayingOneShot({n}Slot slot) => animation.isPlayingOneShot(slot: slot.slotName);"
    );
    let _ = writeln!(o, "}}");
}

/// `PlayerState` for the base layer, `PlayerUpperBodyState` for another.
fn state_enum(controller: &str, layer: usize, name: &str) -> String {
    if layer == 0 {
        format!("{controller}State")
    } else {
        format!("{controller}{}State", upper_first(&camel(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> Controller {
        Controller {
            id: "dc6664dd-aa4e-5906-80c9-0d095d403646".into(),
            path: "models/player.controller.json".into(),
            name: "Player".into(),
            parameters: vec![
                ("speed".into(), "float".into()),
                ("weapon".into(), "int".into()),
                ("grounded".into(), "bool".into()),
                ("jump".into(), "trigger".into()),
                ("state".into(), "bool".into()),
            ],
            layers: vec![
                (
                    "Base Layer".into(),
                    vec!["Locomotion".into(), "Jump".into(), "name".into()],
                ),
                ("UpperBody".into(), vec![]),
                ("Arms".into(), vec!["Aim".into()]),
            ],
        }
    }

    #[test]
    fn a_sub_state_machines_states_are_named_by_path() {
        let layer: Value = serde_json::from_str(
            r#"{"name":"Base Layer","states":[{"name":"Locomotion"},
                {"name":"Airborne","states":[{"name":"Jump"},{"name":"Fall"}]}]}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        leaves(&layer, "", &mut out);
        assert_eq!(out, ["Locomotion", "Airborne/Jump", "Airborne/Fall"]);
        assert_eq!(
            distinct(&out, ENUM_TAKEN, "State"),
            ["locomotion", "airborneJump", "airborneFall"]
        );
    }

    #[test]
    fn a_controller_becomes_an_extension_type_and_enums() {
        let text = render(&[player()]);
        for want in [
            "enum PlayerState {\n  locomotion('Locomotion'),\n  jump('Jump'),\n  nameState('name'),\n  ;",
            "enum PlayerArmsState {\n  aim('Aim'),",
            "enum PlayerSlot {\n  baseLayer('Base Layer'),\n  upperBody('UpperBody'),\n  arms('Arms'),",
            "extension type const PlayerAnimator(AnimationControl animation) {",
            "static const controller = AssetRef('dc6664dd-aa4e-5906-80c9-0d095d403646');",
            "double get speed => animation.getFloat('speed');",
            "set weapon(int value) => animation.setInteger('weapon', value);",
            "set grounded(bool value) => animation.setBool('grounded', value);",
            "void jump() => animation.setTrigger('jump');",
            "void resetJump() => animation.resetTrigger('jump');",
            // A parameter named like a member keeps a distinct name.
            "bool get stateParameter => animation.getBool('state');",
            "PlayerState? get state => PlayerState._of(animation.stateOf()?.name);",
            "PlayerArmsState? get armsState => PlayerArmsState._of(animation.stateOf(layer: 2)?.name);",
            "void play(PlayerState state) => animation.play(state.stateName);",
            "required PlayerSlot slot,",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
    }

    #[test]
    fn no_controllers_is_a_header_only() {
        let text = render(&[]);
        assert!(!text.contains("import"));
        assert!(text.starts_with("// GENERATED"));
    }
}

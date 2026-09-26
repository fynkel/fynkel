//! Animator Controllers (ADR-0032): `.controller.json` read into
//! parameters, layers, states and transitions, and checked — names
//! unique and resolving, conditions fitting their parameter's type.

use std::path::Path;

use fynkel_document::controller::{Blending, ParameterType, Test, ANY_STATE, DEFAULT_DURATION};
use fynkel_document::{Controller, Document, DocumentKind, SchemaSet};

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn doc(body: &str) -> Controller {
    let text = format!(
        r#"{{"$schema":"https://fynkel.dev/schemas/controller.schema.json","formatVersion":1,"id":"c0000000-0000-4000-8000-000000000801","name":"Player"{body}}}"#
    );
    Controller::from_document(
        Document::parse(
            DocumentKind::Controller,
            Some("player.controller.json"),
            &text,
        )
        .unwrap(),
    )
}

fn codes(c: &Controller) -> Vec<(String, String)> {
    c.validate(&schemas())
        .iter()
        .map(|d| (d.code.0.to_string(), d.pointer.clone().unwrap_or_default()))
        .collect()
}

const CLIP: &str = r#"{"asset":"c0000000-0000-4000-8000-000000000005","sub":"anim/bob"}"#;

/// The ADR's example: locomotion, a jump on a trigger, back on `grounded`.
fn player(transitions: &str) -> String {
    format!(
        r#","parameters":[
            {{"name":"speed","type":"float","default":0.5}},
            {{"name":"weapon","type":"int","default":2}},
            {{"name":"grounded","type":"bool","default":true}},
            {{"name":"jump","type":"trigger"}}],
          "layers":[{{"name":"Base Layer","defaultState":"Locomotion",
            "states":[
              {{"name":"Locomotion","motion":{CLIP},"speedParameter":"speed"}},
              {{"name":"Jump","motion":{CLIP},"loop":false,"speed":1.2,"rootMotion":true}},
              {{"name":"Hold"}}],
            "transitions":[{transitions}]}}]"#
    )
}

const JUMP: &str = r#"{"from":"Locomotion","to":"Jump","duration":0.1,"conditions":[{"parameter":"jump"}]},
    {"from":"Jump","to":"Locomotion","exitTime":0.3,"conditions":[{"parameter":"grounded","is":true}]}"#;

#[test]
fn the_adr_example_reads_and_validates() {
    let c = doc(&player(JUMP));
    assert_eq!(codes(&c), []);
    assert!(
        c.read_diagnostics().is_empty(),
        "{:?}",
        c.read_diagnostics()
    );

    let p: Vec<_> = c
        .parameters
        .iter()
        .map(|p| (p.name.as_str(), p.kind, p.default))
        .collect();
    assert_eq!(
        p,
        [
            ("speed", ParameterType::Float, 0.5),
            ("weapon", ParameterType::Int, 2.0),
            ("grounded", ParameterType::Bool, 1.0),
            ("jump", ParameterType::Trigger, 0.0),
        ]
    );

    let base = &c.layers[0];
    assert_eq!(
        (
            base.name.as_str(),
            base.weight,
            base.blending,
            base.root.as_deref()
        ),
        ("Base Layer", 1.0, Blending::Override, None)
    );
    assert_eq!(base.default_state.as_deref(), Some("Locomotion"));
    let jump = base.state("Jump").unwrap();
    assert_eq!(
        (jump.looping, jump.speed, jump.root_motion),
        (false, 1.2, Some(true))
    );
    assert_eq!(
        jump.motion.as_ref().unwrap().as_asset().unwrap().sub,
        "anim/bob"
    );
    let loco = base.state("Locomotion").unwrap();
    assert_eq!(
        (
            loco.looping,
            loco.speed_parameter.as_deref(),
            loco.root_motion
        ),
        (true, Some("speed"), None)
    );
    assert!(
        base.state("Hold").unwrap().motion.is_none(),
        "an empty state holds the pose"
    );

    let t = &base.transitions;
    assert_eq!(t[0].conditions[0].test, Test::Set);
    assert_eq!(
        (t[0].duration, t[0].exit_time, t[0].offset),
        (0.1, None, 0.0)
    );
    assert_eq!(t[1].conditions[0].test, Test::Is(true));
    assert_eq!(
        (t[1].duration, t[1].exit_time),
        (DEFAULT_DURATION, Some(0.3))
    );
    assert_eq!(t[1].pointer, "/layers/0/transitions/1");

    assert_eq!(c.motions().len(), 1, "one motion, named twice");
}

#[test]
fn a_new_controller_is_valid() {
    let d = Document::new(DocumentKind::Controller, "Player");
    let c = Controller::from_document(d);
    assert!(
        c.validate(&schemas()).is_empty(),
        "{:?}",
        c.validate(&schemas())
    );
    assert_eq!(c.layers.len(), 1);
    assert_eq!(c.layers[0].name, "Base Layer");
    assert!(c.parameters.is_empty());
}

#[test]
fn names_resolve_and_are_unique() {
    // Unknown states, a parameter nobody declared, `any` as a target.
    let c = doc(&player(
        r#"{"from":"Swim","to":"Jump"},
           {"from":"Locomotion","to":"Fly","conditions":[{"parameter":"jump"}]},
           {"from":"Locomotion","to":"Jump","conditions":[{"parameter":"crouch","is":true}]},
           {"from":"Locomotion","to":"any","conditions":[{"parameter":"jump"}]}"#,
    ));
    assert_eq!(
        codes(&c),
        [
            ("FYN-D1101".into(), "/layers/0/transitions/0/from".into()),
            ("FYN-D1101".into(), "/layers/0/transitions/1/to".into()),
            (
                "FYN-D1101".into(),
                "/layers/0/transitions/2/conditions/0/parameter".into()
            ),
            ("FYN-D1101".into(), "/layers/0/transitions/3/to".into()),
        ]
    );

    // Two states, two parameters, two layers with one name; a state named
    // `any`; a default that is not a state; states without a default.
    let c = doc(
        r#","parameters":[{"name":"a","type":"float"},{"name":"a","type":"bool"}],
           "layers":[
             {"name":"Base","defaultState":"Idle","states":[{"name":"Idle"},{"name":"Idle"},{"name":"any"}]},
             {"name":"Base","defaultState":"Nope","states":[{"name":"Wave"}]},
             {"name":"Arms","states":[{"name":"Point"}]}]"#,
    );
    assert_eq!(
        codes(&c),
        [
            ("FYN-D1101".into(), "/parameters/1/name".into()),
            ("FYN-D1101".into(), "/layers/0/states/1/name".into()),
            ("FYN-D1101".into(), "/layers/0/states/2/name".into()),
            ("FYN-D1101".into(), "/layers/1/name".into()),
            ("FYN-D1101".into(), "/layers/1/defaultState".into()),
            ("FYN-D1101".into(), "/layers/2".into()),
        ]
    );
    // A layer without states needs no default.
    assert_eq!(codes(&doc(r#","layers":[{"name":"Empty"}]"#)), []);
}

#[test]
fn conditions_fit_their_parameters() {
    let c = doc(&player(
        r#"{"from":"Locomotion","to":"Jump","conditions":[{"parameter":"speed","is":true}]},
           {"from":"Locomotion","to":"Jump","conditions":[{"parameter":"grounded","greater":0}]},
           {"from":"Locomotion","to":"Jump","conditions":[{"parameter":"jump","is":true}]},
           {"from":"Locomotion","to":"Jump","conditions":[{"parameter":"speed","equals":1}]},
           {"from":"Locomotion","to":"Jump","conditions":[{"parameter":"speed"}]},
           {"from":"Locomotion","to":"Jump","conditions":[
             {"parameter":"speed","greater":0.1},{"parameter":"speed","less":4},
             {"parameter":"weapon","equals":2},{"parameter":"weapon","notEquals":0},
             {"parameter":"weapon","greater":1},{"parameter":"grounded","is":false},{"parameter":"jump"}]}"#,
    ));
    assert_eq!(
        codes(&c),
        (0..5)
            .map(|i| (
                "FYN-D1102".to_string(),
                format!("/layers/0/transitions/{i}/conditions/0")
            ))
            .collect::<Vec<_>>()
    );
    let t = &c.layers[0].transitions[5].conditions;
    assert_eq!(
        t.iter().map(|c| c.test).collect::<Vec<_>>(),
        [
            Test::Greater(0.1),
            Test::Less(4.0),
            Test::Equals(2),
            Test::NotEquals(0),
            Test::Greater(1.0),
            Test::Is(false),
            Test::Set
        ]
    );
}

#[test]
fn a_speed_parameter_is_a_float_and_any_state_needs_a_reason() {
    let text = player(r#"{"from":"any","to":"Jump"}"#).replace(
        r#""speedParameter":"speed""#,
        r#""speedParameter":"weapon""#,
    );
    let c = doc(&text);
    let diags = c.validate(&schemas());
    let got: Vec<_> = diags
        .iter()
        .map(|d| (d.code.0, d.severity, d.pointer.clone().unwrap_or_default()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "FYN-D1102",
                fynkel_document::Severity::Error,
                "/layers/0/states/0/speedParameter".to_string()
            ),
            (
                "FYN-D1102",
                fynkel_document::Severity::Warning,
                "/layers/0/transitions/0".to_string()
            ),
        ]
    );
    assert!(c.layers[0].transitions[0].from_any());
    assert_eq!(ANY_STATE, "any");
    // With a condition it is fine.
    let c = doc(&player(
        r#"{"from":"any","to":"Jump","conditions":[{"parameter":"jump"}]}"#,
    ));
    assert_eq!(codes(&c), []);
}

#[test]
fn malformed_entries_are_the_schemas_or_the_readers() {
    // What the schema catches: a wrong type, an unknown field, two tests.
    let c = doc(&player(
        r#"{"from":"Locomotion","to":"Jump","duration":-1}"#,
    ));
    assert_eq!(
        codes(&c),
        [(
            "FYN-D0100".into(),
            "/layers/0/transitions/0/duration".into()
        )]
    );
    let c = doc(&player(
        r#"{"from":"Locomotion","to":"Jump","conditions":[{"parameter":"speed","greater":1,"less":2}]}"#,
    ));
    assert!(
        codes(&c).iter().any(|(code, _)| code == "FYN-D0100"),
        "{:?}",
        codes(&c)
    );
    let c = doc(r#","layers":[]"#);
    assert_eq!(codes(&c), [("FYN-D0100".into(), "/layers".into())]);

    // What only the reader sees: a default of another type.
    let c = doc(
        r#","parameters":[{"name":"on","type":"bool","default":1},{"name":"n","type":"int","default":1.5},{"name":"t","type":"trigger","default":true}],"layers":[{"name":"Base"}]"#,
    );
    assert_eq!(
        codes(&c),
        [
            ("FYN-D1100".into(), "/parameters/0/default".into()),
            ("FYN-D1100".into(), "/parameters/1/default".into()),
            ("FYN-D1100".into(), "/parameters/2/default".into()),
        ]
    );
}

/// A state playing a 2D tree (ADR-0033): idle at the centre, a walk
/// forward and a 1D tree of two clips to the right.
fn strafe(tree: &str) -> String {
    format!(
        r#","parameters":[
            {{"name":"moveX","type":"float"}},
            {{"name":"moveY","type":"float"}},
            {{"name":"jump","type":"trigger"}}],
          "layers":[{{"name":"Base Layer","defaultState":"Strafe",
            "states":[{{"name":"Strafe","motion":{{"blendTree":{tree}}}}}]}}]"#
    )
}

#[test]
fn a_blend_tree_reads_with_its_nested_trees() {
    use fynkel_document::controller::{Motion, TreeKind};
    let c = doc(&strafe(&format!(
        r#"{{"type":"freeformDirectional","parameters":["moveX","moveY"],"children":[
            {{"motion":{CLIP},"position":[0,0]}},
            {{"motion":{CLIP},"position":[0,2],"timeScale":1.5}},
            {{"motion":{{"blendTree":{{"type":"1d","parameters":["moveX"],"children":[
                {{"motion":{CLIP},"threshold":1}},
                {{"motion":{CLIP},"threshold":4}}]}}}},"position":[2,0]}}]}}"#
    )));
    assert_eq!(codes(&c), []);
    let Some(Motion::Tree(t)) = &c.layers[0].states[0].motion else {
        panic!("a tree");
    };
    assert_eq!(t.kind, Some(TreeKind::FreeformDirectional));
    assert_eq!(t.parameters, ["moveX", "moveY"]);
    assert_eq!(t.children[1].position, Some([0.0, 2.0]));
    assert_eq!(t.children[1].time_scale, 1.5);
    assert_eq!(t.leaf_count(), 4);
    let Some(Motion::Tree(inner)) = &t.children[2].motion else {
        panic!("a nested tree");
    };
    assert_eq!(inner.children[1].threshold, Some(4.0));
    // The motions a cook depends on: the one clip, once.
    assert_eq!(c.motions().len(), 1);
    assert_eq!(c.motion_refs().len(), 4);
    assert_eq!(
        c.motion_refs()[3].1,
        "/layers/0/states/0/motion/blendTree/children/2/motion/blendTree/children/1/motion"
    );
}

#[test]
fn a_blend_trees_shape_and_parameters_are_checked() {
    let at = |p: &str| format!("/layers/0/states/0/motion/blendTree{p}");
    let d1104 = |p: &str| ("FYN-D1104".to_string(), at(p));
    // One parameter for a 2D tree; a child without a position; two
    // children at one place.
    let c = doc(&strafe(&format!(
        r#"{{"type":"simpleDirectional","parameters":["moveX"],"children":[
            {{"motion":{CLIP},"position":[0,0]}},
            {{"motion":{CLIP}}},
            {{"motion":{CLIP},"position":[0,0]}}]}}"#
    )));
    assert_eq!(
        codes(&c),
        [
            d1104("/parameters"),
            d1104("/children/1"),
            d1104("/children/2"),
        ]
    );
    // A parameter that is not there (D1101) or not a float (D1102); a
    // direct child without its weight's parameter.
    let c = doc(&strafe(&format!(
        r#"{{"type":"1d","parameters":["speed"],"children":[{{"motion":{CLIP},"threshold":0}}]}}"#
    )));
    assert_eq!(codes(&c), [("FYN-D1101".to_string(), at("/parameters/0"))]);
    let c = doc(&strafe(&format!(
        r#"{{"type":"1d","parameters":["jump"],"children":[{{"motion":{CLIP},"threshold":0}}]}}"#
    )));
    assert_eq!(codes(&c), [("FYN-D1102".to_string(), at("/parameters/0"))]);
    let c = doc(&strafe(&format!(
        r#"{{"type":"direct","children":[{{"motion":{CLIP}}},{{"motion":{CLIP},"parameter":"moveX"}}]}}"#
    )));
    assert_eq!(codes(&c), [d1104("/children/0")]);
    // An unknown type is the schema's (and the reader's) problem.
    let c = doc(&strafe(&format!(
        r#"{{"type":"3d","children":[{{"motion":{CLIP}}}]}}"#
    )));
    assert!(
        codes(&c)
            .iter()
            .any(|(code, _)| code == "FYN-D0002" || code == "FYN-D1100"),
        "{:?}",
        codes(&c)
    );
    // Nesting five deep.
    let mut tree = format!(
        r#"{{"type":"1d","parameters":["moveX"],"children":[{{"motion":{CLIP},"threshold":0}}]}}"#
    );
    for _ in 0..4 {
        tree = format!(
            r#"{{"type":"1d","parameters":["moveX"],"children":[{{"motion":{{"blendTree":{tree}}},"threshold":0}}]}}"#
        );
    }
    let c = doc(&strafe(&tree));
    assert!(
        codes(&c).iter().any(|(code, p)| code == "FYN-D1104" && p.ends_with("/blendTree/children/0/motion/blendTree/children/0/motion/blendTree/children/0/motion/blendTree/children/0/motion/blendTree")),
        "{:?}",
        codes(&c)
    );
}

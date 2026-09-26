//! Animator Controllers (ADR-0032): `.controller.json` read into
//! parameters, layers, states and transitions, and checked — names
//! unique and resolving, conditions fitting their parameter's type.

use std::path::Path;

use fynkel_document::controller::{
    Blending, IkType, IkWeight, Interruption, ParameterType, Test, ANY_STATE, DEFAULT_DURATION,
};
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

/// The ADR's Airborne (ADR-0033 § 4): Jump → Fall → Land inside one
/// state, left from the machine on `grounded`.
fn airborne(inner: &str, outer: &str) -> String {
    format!(
        r#","parameters":[
            {{"name":"grounded","type":"bool","default":true}},
            {{"name":"jump","type":"trigger"}}],
          "layers":[{{"name":"Base Layer","defaultState":"Locomotion",
            "states":[
              {{"name":"Locomotion","motion":{CLIP}}},
              {{"name":"Airborne","defaultState":"Jump",
                "states":[
                  {{"name":"Jump","motion":{CLIP},"loop":false}},
                  {{"name":"Fall","motion":{CLIP}}},
                  {{"name":"Land","motion":{CLIP},"loop":false}}],
                "transitions":[{inner}]}}],
            "transitions":[{outer}]}}]"#
    )
}

const INNER: &str = r#"{"from":"Jump","to":"Fall","exitTime":0.9},
    {"from":"Fall","to":"Land","conditions":[{"parameter":"grounded","is":true}]}"#;
const OUTER: &str = r#"{"from":"Locomotion","to":"Airborne","conditions":[{"parameter":"jump"}]},
    {"from":"Airborne","to":"Locomotion","exitTime":1,"interruptibleBy":"next"}"#;

#[test]
fn a_sub_state_machine_reads_as_a_state_with_states() {
    let c = doc(&airborne(INNER, OUTER));
    assert_eq!(codes(&c), []);
    let base = &c.layers[0];
    let m = base.states[1].machine.as_ref().unwrap();
    assert_eq!(m.default_state.as_deref(), Some("Jump"));
    assert_eq!(m.states.len(), 3);
    assert_eq!(m.transitions[1].pointer, "/layers/0/states/1/transitions/1");
    assert_eq!(base.transitions[1].interruptible_by, Interruption::Next);
    assert_eq!(base.transitions[0].interruptible_by, Interruption::None);
    let leaves: Vec<_> = base.leaves().into_iter().map(|(p, _)| p).collect();
    assert_eq!(
        leaves,
        [
            "Locomotion",
            "Airborne/Jump",
            "Airborne/Fall",
            "Airborne/Land"
        ]
    );
    let scopes: Vec<_> = base.scopes().into_iter().map(|s| s.path).collect();
    assert_eq!(scopes, ["", "Airborne"]);
    assert_eq!(c.motion_refs().len(), 4);
}

#[test]
fn a_sub_state_machines_names_are_its_own() {
    // From/to name the machine's own states, not the layer's.
    let c = doc(&airborne(
        r#"{"from":"Jump","to":"Locomotion"},{"from":"any","to":"Fall","conditions":[{"parameter":"jump"}]}"#,
        r#"{"from":"Locomotion","to":"Fall"}"#,
    ));
    assert_eq!(
        codes(&c),
        [
            ("FYN-D1101".into(), "/layers/0/transitions/0/to".into()),
            (
                "FYN-D1101".into(),
                "/layers/0/states/1/transitions/0/to".into()
            ),
            (
                "FYN-D1101".into(),
                "/layers/0/states/1/transitions/1/from".into()
            ),
        ]
    );
    // A default, states, no motion, no `/`.
    let body = airborne(INNER, OUTER)
        .replace(r#""defaultState":"Jump","#, "")
        .replace(r#""name":"Fall""#, r#""name":"Fall/Down""#);
    let c = doc(&body);
    let got = codes(&c);
    assert!(
        got.contains(&("FYN-D1101".into(), "/layers/0/states/1".into())),
        "{got:?}"
    );
    assert!(
        got.contains(&(
            "FYN-D1101".into(),
            "/layers/0/states/1/states/1/name".into()
        )),
        "{got:?}"
    );
    let c = doc(&airborne(INNER, OUTER).replace(
        r#"{"name":"Airborne","#,
        &format!(r#"{{"name":"Airborne","motion":{CLIP},"#),
    ));
    assert!(!codes(&c).is_empty());
    let c = doc(&airborne(INNER, OUTER).replace(
        r#"{"from":"Airborne","to":"Locomotion","exitTime":1,"interruptibleBy":"next"}"#,
        r#"{"from":"Airborne","to":"Locomotion","exitTime":1,"interruptibleBy":"always"}"#,
    ));
    assert!(!codes(&c).is_empty());
}

#[test]
fn a_mask_and_ik_goals_read_and_validate() {
    let body = player(JUMP)
        .replace(
            r#""name":"Base Layer","#,
            r#""name":"Base Layer","mask":[{"node":"Spine","include":true},{"node":"Head","include":false,"subtree":false}],"#,
        )
        + r#","ik":[
            {"name":"leftHand","type":"twoBone","tip":"LeftHand","pole":"Elbow","weight":"speed"},
            {"name":"look","type":"lookAt","head":"Head","spine":["Spine1","Spine2"],"limit":60,"target":"Coin"},
            {"name":"feet","type":"footPlacement","hips":"Hips","left":"LeftFoot","right":"RightFoot"}]"#;
    let c = doc(&body);
    assert_eq!(codes(&c), []);
    let mask = &c.layers[0].mask;
    assert_eq!(mask.len(), 2);
    assert!(mask[0].include && mask[0].subtree);
    assert!(!mask[1].include && !mask[1].subtree);
    assert_eq!(mask[1].pointer, "/layers/0/mask/1");
    assert_eq!(c.ik.len(), 3);
    assert_eq!(c.ik[0].kind, Some(IkType::TwoBone));
    assert_eq!(c.ik[0].weight, IkWeight::Parameter("speed".into()));
    assert_eq!(c.ik[1].spine, ["Spine1", "Spine2"]);
    assert_eq!(c.ik[1].limit, 60.0);
    assert_eq!(c.ik[1].target.as_deref(), Some("Coin"));
    assert_eq!(c.ik[2].reach, 0.5);
    assert_eq!(c.ik[2].weight, IkWeight::Value(1.0));

    // Names unique, the weight a float parameter, each type's nodes.
    let body = player(JUMP)
        + r#","ik":[
            {"name":"a","type":"twoBone","tip":"LeftHand","weight":"grounded"},
            {"name":"a","type":"lookAt","head":"Head","weight":"nope"},
            {"name":"b","type":"footPlacement","hips":"Hips","left":"LeftFoot"}]"#;
    let got = codes(&doc(&body));
    for want in [
        ("FYN-D1102", "/ik/0/weight"),
        ("FYN-D1101", "/ik/1/name"),
        ("FYN-D1101", "/ik/1/weight"),
    ] {
        assert!(
            got.contains(&(want.0.into(), want.1.into())),
            "{want:?} in {got:?}"
        );
    }
    assert!(got.iter().any(|(_, p)| p.starts_with("/ik/2")), "{got:?}");
}

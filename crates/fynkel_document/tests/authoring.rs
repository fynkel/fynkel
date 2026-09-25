//! Authoring commands and transactions (§11.1, ADR-0016): every command,
//! byte-exact undo/redo, conflicts on a stale base revision, whole-transaction
//! rejection and rollback, and the JSON codec used by `fynkel scene apply`.

use std::path::{Path, PathBuf};

use fynkel_contracts::AssetRef;
use fynkel_document::authoring::diff;
use fynkel_document::{
    expand, Applied, Command, ComponentSpec, Document, DocumentKind, Editor, Node, Prefab,
    PrefabLibrary, Request, Revision, Scene, SchemaSet,
};
use serde_json::json;
use uuid::Uuid;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn open(rel: &str) -> (Editor, String) {
    let doc = Document::read(&fixtures().join(rel), None).unwrap();
    let text = doc.write().unwrap();
    (Editor::open(doc, &schemas()).unwrap(), text)
}

fn u(s: &str) -> Uuid {
    s.parse().unwrap()
}

const ROOT: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const KID1: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const KID2: &str = "cccccccc-0000-4000-8000-000000000003";
const TRANSFORM: &str = "11111111-0000-4000-8000-000000000001";

fn transform() -> ComponentSpec {
    ComponentSpec {
        id: None,
        type_id: "fynkel.Transform".into(),
        version: None,
        data: Some(Node::from_serde(&json!({
            "position": [0, 0, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1]
        }))),
    }
}

fn apply(editor: &mut Editor, commands: Vec<Command>) -> Applied {
    let r = editor.apply(Request::new(commands), &schemas());
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    r
}

fn text(editor: &Editor) -> String {
    editor.document().write().unwrap()
}

fn scene(editor: &Editor) -> Scene {
    editor.scene()
}

// --------------------------------------------------------------- basics

#[test]
fn revision_is_the_hash_of_the_canonical_text() {
    let (editor, original) = open("scene/hierarchy_ok.scene.json");
    assert_eq!(editor.revision(), &Revision::of_text(&original));
    assert_eq!(editor.revision().as_str().len(), 64);
    assert!(!editor.is_dirty());
    assert!(Revision::parse(editor.revision().as_str()).is_some());
    assert!(Revision::parse("abc").is_none());
}

#[test]
fn only_scenes_prefabs_and_materials_open() {
    let s = schemas();
    let doc = Document::read(&fixtures().join("project/fynkel.project.json"), None).unwrap();
    let err = Editor::open(doc, &s).err().unwrap();
    assert_eq!(err[0].code.0, "FYN-E0007");
    assert!(err[0].message.contains("data assets are"));
    // A material opens (M4-01) and is edited through document fields.
    let doc = Document::read(
        &fixtures().join("project/assets/materials/rusted.material.json"),
        None,
    )
    .unwrap();
    assert!(Editor::open(doc, &s).is_ok());

    let mut outdated = Document::new(DocumentKind::Scene, "old");
    outdated.envelope.format_version = 0;
    outdated.root.set("formatVersion", Node::integer(0));
    let err = Editor::open(outdated, &s).err().unwrap();
    assert_eq!(err[0].code.0, "FYN-E0007");
    assert!(err[0].message.contains("older"));

    let mut newer = Document::new(DocumentKind::Scene, "new");
    newer.envelope.format_version = 99;
    newer.root.set("formatVersion", Node::integer(99));
    let err = Editor::open(newer, &s).err().unwrap();
    assert_eq!(err[0].code.0, "FYN-E0007");
    assert!(err[0].message.contains("newer"));
}

#[test]
fn every_transaction_undoes_to_the_exact_original_text() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let s = schemas();
    let new_id = u("dddddddd-0000-4000-8000-000000000004");
    let steps: Vec<Vec<Command>> = vec![
        vec![Command::RenameObject {
            id: u(KID1),
            name: "First".into(),
        }],
        vec![Command::SetField {
            entity: u(ROOT),
            component: u(TRANSFORM),
            field: "position".into(),
            value: Node::from_serde(&json!([1, 2, 3])),
        }],
        vec![Command::SetField {
            entity: u(ROOT),
            component: u(TRANSFORM),
            field: "/position/1".into(),
            value: Node::from_serde(&json!(7.5)),
        }],
        vec![Command::CreateObject {
            id: Some(new_id),
            name: "Kid3".into(),
            parent: Some(u(ROOT)),
            index: Some(1),
            components: vec![transform()],
        }],
        vec![Command::ReparentObject {
            id: u(KID2),
            parent: Some(new_id),
            index: None,
        }],
        vec![Command::AddComponent {
            entity: u(KID1),
            component: ComponentSpec {
                id: Some(u("22222222-0000-4000-8000-000000000002")),
                type_id: "fynkel.PointLight".into(),
                version: Some(1),
                data: Some(Node::from_serde(
                    &json!({"color": [1, 1, 1], "intensity": 2, "range": 5, "culling_mask": 4294967295u32}),
                )),
            },
        }],
        vec![Command::RemoveComponent {
            entity: u(KID1),
            component: u(TRANSFORM),
        }],
        vec![Command::DeleteObject { id: new_id }],
        vec![Command::ReparentObject {
            id: u(KID1),
            parent: None,
            index: None,
        }],
    ];
    let mut texts = vec![original.clone()];
    let mut revisions = vec![editor.revision().clone()];
    for commands in steps {
        let before = editor.revision().clone();
        let r = editor.apply(
            Request {
                id: None,
                base_revision: Some(before.clone()),
                commands: commands.into_iter().map(Into::into).collect(),
            },
            &s,
        );
        assert!(r.ok, "{:#?}", r.all_diagnostics());
        assert_eq!(r.base_revision, before);
        assert_ne!(r.revision, before, "a command that changes nothing?");
        assert_eq!(&r.revision, editor.revision());
        assert!(r.mutations.iter().all(|m| !m.undo.is_empty()));
        texts.push(text(&editor));
        revisions.push(editor.revision().clone());
        // The document stays valid after each step.
        let diags = editor.validate(&s);
        assert!(!fynkel_document::has_errors(&diags), "{diags:#?}");
    }
    assert!(editor.is_dirty());
    assert_eq!(editor.history().len(), texts.len() - 1);

    // Undo all the way back, byte for byte, then redo all the way forward.
    for i in (1..texts.len()).rev() {
        let t = editor.undo().unwrap();
        assert_eq!(t.revision, revisions[i]);
        assert_eq!(text(&editor), texts[i - 1], "undo to step {}", i - 1);
        assert_eq!(editor.revision(), &revisions[i - 1]);
    }
    assert!(!editor.is_dirty());
    assert_eq!(editor.undo().unwrap_err().code.0, "FYN-E0009");
    for i in 1..texts.len() {
        editor.redo().unwrap();
        assert_eq!(text(&editor), texts[i], "redo to step {i}");
        assert_eq!(editor.revision(), &revisions[i]);
    }
    assert_eq!(editor.redo().unwrap_err().code.0, "FYN-E0009");
    // A new transaction clears the redo stack.
    editor.undo().unwrap();
    assert!(editor.can_redo());
    apply(
        &mut editor,
        vec![Command::RenameObject {
            id: u(ROOT),
            name: "R".into(),
        }],
    );
    assert!(!editor.can_redo());
}

#[test]
fn create_and_delete_maintain_the_hierarchy() {
    let (mut editor, _) = open("scene/hierarchy_ok.scene.json");
    let r = apply(
        &mut editor,
        vec![
            Command::CreateObject {
                id: None,
                name: "Kid3".into(),
                parent: Some(u(ROOT)),
                index: Some(0),
                components: vec![transform()],
            },
            Command::CreateObject {
                id: None,
                name: "Loose".into(),
                parent: None,
                index: None,
                components: vec![],
            },
        ],
    );
    assert_eq!(r.mutations.len(), 2);
    let kid3 = r.mutations[0].affected_ids[0];
    let loose = r.mutations[1].affected_ids[0];
    assert!(r.mutations[0].affected_ids.contains(&u(ROOT)));
    let sc = scene(&editor);
    let root = sc.entity(u(ROOT)).unwrap();
    assert_eq!(root.children[0], kid3);
    assert_eq!(root.children.len(), 3);
    assert_eq!(sc.entity(kid3).unwrap().parent, Some(u(ROOT)));
    assert_eq!(sc.entity(kid3).unwrap().components.len(), 1);
    assert_eq!(sc.entity(loose).unwrap().parent, None);
    assert_eq!(sc.roots().count(), 2);

    // Deleting Root removes its subtree and nothing else.
    let r = apply(&mut editor, vec![Command::DeleteObject { id: u(ROOT) }]);
    let sc = scene(&editor);
    assert_eq!(sc.entities.len(), 1);
    assert!(sc.entity(loose).is_some());
    assert_eq!(r.mutations[0].affected_ids.len(), 4);
    editor.undo().unwrap();
    assert_eq!(scene(&editor).entities.len(), 5);
}

#[test]
fn a_root_created_at_an_index_takes_that_place_among_the_roots() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let loose = |name: &str, index| Command::CreateObject {
        id: None,
        name: name.into(),
        parent: None,
        index,
        components: vec![],
    };
    // Roots: Root, then A and B appended.
    apply(&mut editor, vec![loose("A", None), loose("B", None)]);
    let names = |e: &Editor| -> Vec<String> { scene(e).roots().map(|r| r.name.clone()).collect() };
    assert_eq!(names(&editor), ["Root", "A", "B"]);
    // Before A; at the front; past the end is the end.
    let r = apply(
        &mut editor,
        vec![
            loose("C", Some(1)),
            loose("D", Some(0)),
            loose("E", Some(99)),
        ],
    );
    assert_eq!(names(&editor), ["D", "Root", "C", "A", "B", "E"]);
    // Replaced in place (Hierarchy → Project): deleted and re-created
    // under the same id at its index, in one transaction, it keeps its place.
    let c = r.mutations[0].affected_ids[0];
    apply(
        &mut editor,
        vec![
            Command::DeleteObject { id: c },
            Command::CreateObject {
                id: Some(c),
                name: "C".into(),
                parent: None,
                index: Some(2),
                components: vec![],
            },
        ],
    );
    assert_eq!(names(&editor), ["D", "Root", "C", "A", "B", "E"]);
    for _ in 0..3 {
        editor.undo().unwrap();
    }
    assert_eq!(text(&editor), original);
}

#[test]
fn a_root_moves_among_the_roots_and_a_root_copy_follows_its_original() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let loose = |name: &str| Command::CreateObject {
        id: None,
        name: name.into(),
        parent: None,
        index: None,
        components: vec![],
    };
    let r = apply(&mut editor, vec![loose("A"), loose("B")]);
    let names = |e: &Editor| -> Vec<String> { scene(e).roots().map(|r| r.name.clone()).collect() };
    assert_eq!(names(&editor), ["Root", "A", "B"]);
    let (a, b) = (
        r.mutations[0].affected_ids[0],
        r.mutations[1].affected_ids[0],
    );
    let to_root = |id, index| Command::ReparentObject {
        id,
        parent: None,
        index: Some(index),
    };
    // Up, down, and to the end (the index counts the roots without it).
    apply(&mut editor, vec![to_root(b, 0)]);
    assert_eq!(names(&editor), ["B", "Root", "A"]);
    apply(&mut editor, vec![to_root(b, 1)]);
    assert_eq!(names(&editor), ["Root", "B", "A"]);
    apply(&mut editor, vec![to_root(b, 2)]);
    assert_eq!(names(&editor), ["Root", "A", "B"]);
    // A child made a root at an index lands there.
    apply(&mut editor, vec![to_root(u(KID2), 1)]);
    assert_eq!(scene(&editor).roots().nth(1).map(|r| r.id), Some(u(KID2)));
    // A root's copy comes right after it, not at the end.
    apply(
        &mut editor,
        vec![Command::DuplicateObject {
            id: a,
            new_id: None,
            name: Some("A (1)".into()),
            index: None,
        }],
    );
    let after: Vec<String> = names(&editor);
    let at = after.iter().position(|n| n == "A").unwrap();
    assert_eq!(after[at + 1], "A (1)");
    for _ in 0..6 {
        editor.undo().unwrap();
    }
    assert_eq!(text(&editor), original);
}

#[test]
fn pasted_objects_get_fresh_ids_and_land_where_asked() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let doc: serde_json::Value = serde_json::from_str(&original).unwrap();
    // Copy Root's whole subtree, as the editor's Copy sends it.
    let entities: Vec<(String, fynkel_document::Node)> = doc["entities"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), fynkel_document::Node::from_serde(v)))
        .collect();
    let count = entities.len();
    let r = apply(
        &mut editor,
        vec![Command::PasteObjects {
            entities: entities.clone(),
            parent: None,
            index: Some(0),
        }],
    );
    let ids = &r.mutations[0].affected_ids;
    assert_eq!(ids.len(), count);
    let s = scene(&editor);
    let roots: Vec<_> = s.roots().map(|r| r.id).collect();
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0], ids[0], "the pasted root comes first, at index 0");
    assert_ne!(roots[0], u(ROOT));
    assert_eq!(s.roots().next().unwrap().name, "Root");
    // Under a parent: the copy's children keep their shape.
    apply(
        &mut editor,
        vec![Command::PasteObjects {
            entities,
            parent: Some(u(KID2)),
            index: None,
        }],
    );
    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
}

#[test]
fn reparent_refuses_cycles_and_unknown_entities() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let s = schemas();
    let r = editor.apply(
        Request::new(vec![Command::ReparentObject {
            id: u(ROOT),
            parent: Some(u(KID2)),
            index: None,
        }]),
        &s,
    );
    assert!(!r.ok);
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0006");
    assert_eq!(text(&editor), original);
    let r = editor.apply(
        Request::new(vec![Command::ReparentObject {
            id: u(ROOT),
            parent: Some(u(ROOT)),
            index: None,
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0006");
    let r = editor.apply(
        Request::new(vec![Command::RenameObject {
            id: Uuid::new_v4(),
            name: "x".into(),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0002");
    let r = editor.apply(
        Request::new(vec![Command::RemoveComponent {
            entity: u(ROOT),
            component: Uuid::new_v4(),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0003");
    let r = editor.apply(
        Request::new(vec![Command::CreateObject {
            id: Some(u(ROOT)),
            name: "dup".into(),
            parent: None,
            index: None,
            components: vec![],
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0010");
    assert_eq!(text(&editor), original);
    assert!(editor.history().is_empty());
}

#[test]
fn stale_base_revision_is_a_conflict_and_nothing_is_touched() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let s = schemas();
    let stale = Revision::of_text("something else");
    let r = editor.apply(
        Request {
            id: Some(u("99999999-0000-4000-8000-000000000009")),
            base_revision: Some(stale),
            commands: vec![Command::RenameObject {
                id: u(ROOT),
                name: "Changed".into(),
            }
            .into()],
        },
        &s,
    );
    assert!(!r.ok);
    assert_eq!(r.transaction_id, u("99999999-0000-4000-8000-000000000009"));
    assert_eq!(r.diagnostics[0].code.0, "FYN-E0001");
    assert!(r.mutations.is_empty());
    assert_eq!(text(&editor), original);
    assert_eq!(r.revision, r.base_revision);

    // Two writers: the second one's base is stale after the first commits.
    let base = editor.revision().clone();
    apply(
        &mut editor,
        vec![Command::RenameObject {
            id: u(ROOT),
            name: "A".into(),
        }],
    );
    let r = editor.apply(
        Request {
            id: None,
            base_revision: Some(base),
            commands: vec![Command::RenameObject {
                id: u(ROOT),
                name: "B".into(),
            }
            .into()],
        },
        &s,
    );
    assert_eq!(r.diagnostics[0].code.0, "FYN-E0001");
    assert_eq!(scene(&editor).entity(u(ROOT)).unwrap().name, "A");
}

#[test]
fn a_transaction_that_would_leave_errors_is_rejected_whole() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let s = schemas();
    // Command 1 is fine; command 2 breaks the Transform schema.
    let r = editor.apply(
        Request::new(vec![
            Command::RenameObject {
                id: u(ROOT),
                name: "Fine".into(),
            },
            Command::SetField {
                entity: u(ROOT),
                component: u(TRANSFORM),
                field: "position".into(),
                value: Node::from_serde(&json!("not a vector")),
            },
        ]),
        &s,
    );
    assert!(!r.ok);
    assert_eq!(r.mutations.len(), 2, "both commands ran before validation");
    let rejected = &r.diagnostics[0];
    assert_eq!(rejected.code.0, "FYN-E0005");
    assert!(!rejected.related.is_empty());
    assert!(rejected.related[0]
        .pointer
        .as_deref()
        .unwrap()
        .contains("/position"));
    assert_eq!(text(&editor), original, "rolled back to the byte");
    assert!(editor.history().is_empty());
    assert_eq!(editor.revision(), &Revision::of_text(&original));

    // A component the schema does not know is only a warning (FYN-D0205):
    // the transaction commits and the warning rides along.
    let r = editor.apply(
        Request::new(vec![Command::AddComponent {
            entity: u(ROOT),
            component: ComponentSpec {
                id: None,
                type_id: "acme.Unknown".into(),
                version: None,
                data: None,
            },
        }]),
        &s,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    assert_eq!(r.diagnostics[0].code.0, "FYN-D0205");
    assert_ne!(text(&editor), original);
}

#[test]
fn baseline_errors_are_tolerated_but_new_ones_are_not() {
    // The cycle fixture is already invalid; editing an unrelated entity works.
    let doc = Document::read(&fixtures().join("scene/cycle.scene.json"), None).unwrap();
    let s = schemas();
    let mut editor = Editor::open(doc, &s).unwrap();
    let id = editor.scene().entities[0].id;
    let r = editor.apply(
        Request::new(vec![Command::RenameObject {
            id,
            name: "renamed".into(),
        }]),
        &s,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
}

#[test]
fn set_unset_and_assign_fields() {
    let (mut editor, _) = open("scene/hierarchy_ok.scene.json");
    let mesh = AssetRef::with_sub(
        "c0000000-0000-4000-8000-000000000001".parse().unwrap(),
        "mesh/0",
    );
    let material = AssetRef::new("c0000000-0000-4000-8000-000000000020".parse().unwrap());
    let renderer = u("33333333-0000-4000-8000-000000000003");
    apply(
        &mut editor,
        vec![
            Command::AddComponent {
                entity: u(KID1),
                component: ComponentSpec {
                    id: Some(renderer),
                    type_id: "fynkel.MeshRenderer".into(),
                    version: None,
                    data: Some(Node::from_serde(&json!({
                        "mesh": {"asset": "c0000000-0000-4000-8000-000000000001", "sub": "mesh/1"},
                        "material": {"asset": "c0000000-0000-4000-8000-000000000020"},
                        "cast_shadows": true, "receive_shadows": true, "visible": true
                    }))),
                },
            },
            Command::AssignAsset {
                entity: u(KID1),
                component: renderer,
                field: "mesh".into(),
                asset: mesh.clone(),
            },
            Command::AssignAsset {
                entity: u(KID1),
                component: renderer,
                field: "material".into(),
                asset: material.clone(),
            },
            Command::SetField {
                entity: u(KID1),
                component: renderer,
                field: "visible".into(),
                value: Node::from_serde(&json!(false)),
            },
        ],
    );
    let sc = scene(&editor);
    let c = &sc.entity(u(KID1)).unwrap().components[1];
    assert_eq!(c.id, renderer);
    let data = c.data.to_serde();
    assert_eq!(
        data["mesh"],
        json!({"asset": "c0000000-0000-4000-8000-000000000001", "sub": "mesh/0"})
    );
    assert_eq!(
        data["material"],
        json!({"asset": "c0000000-0000-4000-8000-000000000020"})
    );
    assert_eq!(data["visible"], json!(false));

    // Unsetting a required field is rejected by validation; unsetting an
    // unknown field is an invalid command.
    let s = schemas();
    let r = editor.apply(
        Request::new(vec![Command::UnsetField {
            entity: u(KID1),
            component: renderer,
            field: "visible".into(),
        }]),
        &s,
    );
    assert_eq!(r.diagnostics[0].code.0, "FYN-E0005");
    let r = editor.apply(
        Request::new(vec![Command::UnsetField {
            entity: u(KID1),
            component: renderer,
            field: "nope".into(),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0004");
    // Set + unset an extra field inside one transaction is a no-op overall.
    let before = text(&editor);
    let r = apply(
        &mut editor,
        vec![
            Command::SetField {
                entity: u(KID1),
                component: u(TRANSFORM),
                field: "extra".into(),
                value: Node::from_serde(&json!(1)),
            },
            Command::UnsetField {
                entity: u(KID1),
                component: u(TRANSFORM),
                field: "extra".into(),
            },
        ],
    );
    assert_eq!(text(&editor), before);
    assert_eq!(r.revision, r.base_revision);
}

// --------------------------------------------------------------- prefabs

#[test]
fn prefab_documents_keep_their_one_root() {
    let (mut editor, original) = open("valid/turret.prefab.json");
    let s = schemas();
    let before = Prefab::from_document(editor.document().clone());
    let root = before.root;
    let refused = |r: &fynkel_document::authoring::Applied| {
        !r.ok
            && r.all_diagnostics()
                .iter()
                .flat_map(|d| std::iter::once(d).chain(d.related.iter()))
                .any(|d| d.code.0 == "FYN-D0400")
    };

    // A second parentless object is refused: a prefab has one root.
    let r = editor.apply(
        Request::new(vec![Command::CreateObject {
            id: None,
            name: "Extra root".into(),
            parent: None,
            index: None,
            components: vec![transform()],
        }]),
        &s,
    );
    assert!(refused(&r), "{:#?}", r.all_diagnostics());
    assert_eq!(text(&editor), original);

    // Under the root it is an ordinary child.
    let r = apply(
        &mut editor,
        vec![Command::CreateObject {
            id: None,
            name: "Extra".into(),
            parent: Some(root),
            index: Some(0),
            components: vec![transform()],
        }],
    );
    let new_id = r.mutations[0].affected_ids[0];
    let p = Prefab::from_document(editor.document().clone());
    assert_eq!(p.root, root);
    assert_eq!(p.entity(root).unwrap().children[0], new_id);
    let d = p.validate(&s);
    assert!(!fynkel_document::has_errors(&d), "{d:#?}");

    // Taking it out to the top, or deleting the root, is refused too.
    let r = editor.apply(
        Request::new(vec![Command::ReparentObject {
            id: new_id,
            parent: None,
            index: None,
        }]),
        &s,
    );
    assert!(refused(&r), "{:#?}", r.all_diagnostics());
    let r = editor.apply(Request::new(vec![Command::DeleteObject { id: root }]), &s);
    assert!(refused(&r), "{:#?}", r.all_diagnostics());
    apply(&mut editor, vec![Command::DeleteObject { id: new_id }]);
    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);

    // A prefab cannot instance itself (ADR-0022 amendment 2026-09-25:
    // nested instances are fine in a prefab, a cycle is not).
    let r = editor.apply(
        Request::new(vec![Command::InstantiatePrefab {
            id: None,
            name: "x".into(),
            parent: Some(root),
            index: None,
            prefab: AssetRef::new("10000000-0000-4000-8000-000000000001".parse().unwrap()),
            instance: None,
            components: vec![],
            overrides: vec![],
        }]),
        &s,
    );
    assert!(!r.ok);
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-D0402");
    assert_eq!(text(&editor), original);
}

const TURRET_A: &str = "20000000-0000-4000-8000-0000000000a1";
const LABEL: &str = "20000000-0000-4000-8000-0000000000a3";

#[test]
fn instantiate_and_override_prefabs() {
    let (mut editor, original) = open("prefab/instances_ok.scene.json");
    let s = schemas();
    let instance = u("20000000-0000-4000-8000-0000000000e9");
    let r = apply(
        &mut editor,
        vec![Command::InstantiatePrefab {
            id: None,
            name: "Turret C".into(),
            parent: None,
            index: None,
            prefab: AssetRef::new("10000000-0000-4000-8000-000000000001".parse().unwrap()),
            instance: Some(instance),
            components: vec![],
            overrides: vec![],
        }],
    );
    let host = r.mutations[0].affected_ids[0];
    assert!(r.mutations[0].affected_ids.contains(&instance));
    let sc = scene(&editor);
    let e = sc.entity(host).unwrap();
    let p = e.prefab.as_ref().unwrap();
    assert_eq!(p.instance, instance);
    assert!(p.overrides.is_empty());
    assert_eq!(sc.instances().count(), 3);

    // Set an override; setting the same target again replaces it.
    let set = |value: serde_json::Value| Command::SetPrefabOverride {
        entity: u(TURRET_A),
        r#override: Node::from_serde(&json!({
            "op": "setField",
            "entity": "10000000-0000-4000-8000-0000000000b1",
            "component": "10000000-0000-4000-8000-0000000000c2",
            "field": "cast_shadows",
            "value": value
        })),
    };
    apply(&mut editor, vec![set(json!(true))]);
    let ov = |editor: &Editor| {
        scene(editor)
            .entity(u(TURRET_A))
            .unwrap()
            .prefab
            .as_ref()
            .unwrap()
            .overrides
            .clone()
    };
    assert_eq!(ov(&editor).len(), 4, "replaced, not appended");
    match &ov(&editor)[0].op {
        fynkel_document::scene::OverrideOp::SetField { value, .. } => {
            assert_eq!(value.to_serde(), json!(true))
        }
        other => panic!("{other:?}"),
    }
    apply(
        &mut editor,
        vec![Command::SetPrefabOverride {
            entity: u(TURRET_A),
            r#override: Node::from_serde(&json!({
                "op": "removeObject",
                "entity": "10000000-0000-4000-8000-0000000000b2",
            })),
        }],
    );
    assert_eq!(ov(&editor).len(), 5);
    // Revert one, then all.
    apply(
        &mut editor,
        vec![Command::RevertPrefabOverride {
            entity: u(TURRET_A),
            index: Some(1),
        }],
    );
    assert_eq!(ov(&editor).len(), 4);
    assert_eq!(ov(&editor)[1].op.name(), "addObject");
    let r = editor.apply(
        Request::new(vec![Command::RevertPrefabOverride {
            entity: u(TURRET_A),
            index: Some(4),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0008");
    apply(
        &mut editor,
        vec![Command::RevertPrefabOverride {
            entity: u(TURRET_A),
            index: None,
        }],
    );
    assert!(ov(&editor).is_empty());
    // Not an instance / malformed op.
    let r = editor.apply(
        Request::new(vec![Command::RevertPrefabOverride {
            entity: u(LABEL),
            index: None,
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0008");
    let r = editor.apply(
        Request::new(vec![Command::SetPrefabOverride {
            entity: u(TURRET_A),
            r#override: Node::from_serde(&json!({"op": "teleport"})),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0004");

    while editor.can_undo() {
        editor.undo().unwrap();
    }
    assert_eq!(text(&editor), original);
}

const TURRET_B: &str = "20000000-0000-4000-8000-0000000000a2";

/// The prefabs the `prefab/` fixtures instance (four prefab documents).
fn library() -> PrefabLibrary {
    let (lib, diags) = PrefabLibrary::scan(&fixtures());
    assert!(diags.is_empty(), "{diags:#?}");
    lib
}

fn unpack(editor: &mut Editor, lib: &PrefabLibrary, entity: &str) -> Applied {
    let r = editor.apply_with_prefabs(
        Request::new(vec![Command::UnpackPrefab {
            entity: u(entity),
            completely: false,
        }]),
        &schemas(),
        lib,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    r
}

#[test]
fn set_object_active_writes_the_flag_next_to_the_name_and_undoes_exactly() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let s = schemas();
    let set = |active| Command::SetObjectActive {
        id: u(KID1),
        active,
    };
    // Active already: nothing to write.
    apply(&mut editor, vec![set(true)]);
    assert_eq!(text(&editor), original);

    apply(&mut editor, vec![set(false)]);
    let sc = scene(&editor);
    assert!(!sc.entity(u(KID1)).unwrap().active);
    assert!(sc.entity(u(ROOT)).unwrap().active);
    let keys: Vec<String> = editor
        .document()
        .root
        .pointer(&format!("/entities/{KID1}"))
        .and_then(Node::as_object)
        .unwrap()
        .iter()
        .map(|m| m.key.clone())
        .collect();
    assert_eq!(keys[..2], ["name".to_string(), "active".to_string()]);
    assert!(scene(&editor).validate(&s).is_empty());

    // Setting it again is a no-op; true removes the key.
    apply(&mut editor, vec![set(false)]);
    apply(&mut editor, vec![set(true)]);
    assert_eq!(text(&editor), original);
    assert!(editor.undo().is_ok());
    assert!(!scene(&editor).entity(u(KID1)).unwrap().active);
    while editor.undo().is_ok() {}
    assert_eq!(text(&editor), original);

    let r = editor.apply(
        Request::new(vec![Command::SetObjectActive {
            id: u("dddddddd-0000-4000-8000-000000000004"),
            active: false,
        }]),
        &s,
    );
    assert!(!r.ok);
    let err = Command::from_json(&json!({"command": "setObjectActive", "id": KID1, "active": 0}))
        .unwrap_err();
    assert!(
        err.message.contains("\"active\" must be a boolean"),
        "{err}"
    );
}

#[test]
fn set_active_overrides_expand_replace_and_validate() {
    let (mut editor, _) = open("prefab/instances_ok.scene.json");
    let lib = library();
    let s = schemas();
    let barrel = "10000000-0000-4000-8000-0000000000b2";
    let set = |value: serde_json::Value| Command::SetPrefabOverride {
        entity: u(TURRET_A),
        r#override: Node::from_serde(&json!({"op": "setActive", "entity": barrel, "value": value})),
    };
    let overrides = |editor: &Editor| {
        scene(editor)
            .entity(u(TURRET_A))
            .unwrap()
            .prefab
            .as_ref()
            .unwrap()
            .overrides
            .clone()
    };
    let before = overrides(&editor).len();
    apply(&mut editor, vec![set(json!(false))]);
    apply(&mut editor, vec![set(json!(false))]);
    assert_eq!(
        overrides(&editor).len(),
        before + 1,
        "replaced, not appended"
    );
    assert_eq!(
        overrides(&editor).last().unwrap().op,
        fynkel_document::scene::OverrideOp::SetActive {
            entity: u(barrel),
            value: false
        }
    );

    // The expansion carries the flag; the rest of the instance stays active.
    let x = expand(&editor.scene(), &lib, &s);
    assert!(!x.has_errors(), "{:#?}", x.diagnostics);
    let a = x
        .instances
        .iter()
        .find(|i| i.anchor == u(TURRET_A))
        .unwrap();
    let out = x.scene();
    assert!(!out.entity(a.entity(u(barrel)).unwrap()).unwrap().active);
    let inactive = out.entities.iter().filter(|e| !e.active).count();
    assert_eq!(inactive, 1);
    assert!(out.validate(&s).is_empty());

    // A non-boolean value does not parse; an unknown target does not expand.
    let r = editor.apply(Request::new(vec![set(json!("no"))]), &s);
    assert!(!r.ok);
    apply(
        &mut editor,
        vec![Command::SetPrefabOverride {
            entity: u(TURRET_A),
            r#override: Node::from_serde(&json!({
                "op": "setActive",
                "entity": "10000000-0000-4000-8000-0000000000ff",
                "value": false
            })),
        }],
    );
    let x = expand(&editor.scene(), &lib, &s);
    assert_eq!(x.diagnostics[0].code.0, "FYN-D0403");
}

#[test]
fn unpack_prefab_writes_the_expansion_in_place() {
    let (mut editor, original) = open("prefab/instances_ok.scene.json");
    let lib = library();
    let s = schemas();

    // The whole scene expanded the runtime's way is the reference.
    let expected = expand(&editor.scene(), &lib, &s);
    assert!(!expected.has_errors(), "{:#?}", expected.diagnostics);
    let expected_text = expected.document.write().unwrap();
    let a = expected
        .instances
        .iter()
        .find(|i| i.anchor == u(TURRET_A))
        .unwrap()
        .clone();

    // Unpack Turret A: the anchor is a plain entity, the derived entities
    // follow it in document order, everything else is untouched.
    let r = unpack(&mut editor, &lib, TURRET_A);
    let m = &r.mutations[0];
    assert_eq!(m.command, "unpackPrefab");
    assert_eq!(m.affected_ids[0], u(TURRET_A));
    // The instance is the prefab's root, so it is not among the derived.
    let derived: Vec<Uuid> = a
        .entities
        .iter()
        .map(|(_, sid)| *sid)
        .filter(|sid| *sid != u(TURRET_A))
        .collect();
    assert_eq!(&m.affected_ids[1..1 + derived.len()], &derived[..]);
    for (_, cid) in &a.components {
        assert!(m.affected_ids.contains(cid), "component {cid} is affected");
    }
    let sc = scene(&editor);
    let anchor = sc.entity(u(TURRET_A)).unwrap();
    assert!(anchor.prefab.is_none());
    assert_eq!(sc.instances().count(), 1, "Turret B is still an instance");
    let order: Vec<Uuid> = sc.entities.iter().map(|e| e.id).collect();
    let at = order.iter().position(|id| *id == u(TURRET_A)).unwrap();
    assert_eq!(&order[at + 1..at + 1 + derived.len()], &derived[..]);
    // The root's children lead the instance's children, each derived
    // entity's parent points back into the scene.
    let kids: Vec<Uuid> = derived
        .iter()
        .copied()
        .filter(|sid| sc.entity(*sid).unwrap().parent == Some(u(TURRET_A)))
        .collect();
    assert!(!kids.is_empty());
    assert_eq!(anchor.children[..kids.len()], kids[..]);
    for sid in &derived {
        let e = sc.entity(*sid).unwrap();
        assert!(
            sc.entity(e.parent.unwrap()).is_some(),
            "{sid} hangs off the scene"
        );
    }
    // The overrides were applied: Turret A's setField turned cast_shadows off.
    let b1 = a.entity(u("10000000-0000-4000-8000-0000000000b1")).unwrap();
    let renderer = sc
        .entity(b1)
        .unwrap()
        .components
        .iter()
        .find(|c| c.type_id == "fynkel.MeshRenderer")
        .unwrap();
    assert_eq!(
        renderer.data.get("cast_shadows").unwrap().to_serde(),
        json!(false)
    );
    assert!(
        sc.validate(&s).iter().all(|d| !d.is_error()),
        "{:#?}",
        sc.validate(&s)
    );

    // Unpacking the second instance too gives exactly the whole expansion.
    unpack(&mut editor, &lib, TURRET_B);
    assert_eq!(text(&editor), expected_text);
    assert_eq!(scene(&editor).instances().count(), 0);

    // Two transactions, two undo steps, back to the byte.
    editor.undo().unwrap();
    assert_eq!(scene(&editor).instances().count(), 1);
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
    editor.redo().unwrap();
    editor.redo().unwrap();
    assert_eq!(text(&editor), expected_text);
    editor.undo().unwrap();
    editor.undo().unwrap();

    // Not an instance.
    let r = editor.apply_with_prefabs(
        Request::new(vec![Command::UnpackPrefab {
            entity: u(LABEL),
            completely: false,
        }]),
        &s,
        &lib,
    );
    assert!(!r.ok);
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0008");
    // Unknown entity.
    let r = editor.apply_with_prefabs(
        Request::new(vec![Command::UnpackPrefab {
            entity: u(KID1),
            completely: false,
        }]),
        &s,
        &lib,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0002");
    // Without the library (plain `apply`) the prefab is unknown: refused
    // with the expansion's D0401 in `related`, nothing rewritten.
    let r = editor.apply(
        Request::new(vec![Command::UnpackPrefab {
            entity: u(TURRET_A),
            completely: false,
        }]),
        &s,
    );
    assert!(!r.ok);
    let d = &r.mutations[0].diagnostics[0];
    assert_eq!(d.code.0, "FYN-E0004");
    assert_eq!(d.related.len(), 1);
    assert_eq!(d.related[0].code.0, "FYN-D0401");
    assert_eq!(d.related[0].id.as_deref(), Some(TURRET_A));
    assert_eq!(text(&editor), original);
    assert!(!editor.is_dirty());
}

#[test]
fn unpack_prefab_of_an_imported_hierarchy() {
    // The fixture project's `Imported` instances `column.rig.json#prefab`
    // (a rig document whose hierarchy comes from `rig.glb`): the library
    // needs the importer, and the unpacked entities carry the derived
    // `node/<i>` ids with the Prop's material override applied.
    const IMPORTED: &str = "c0000000-0000-4000-8000-0000000004e8";
    let (mut editor, original) = open("project/assets/scenes/rig.scene.json");
    let s = schemas();
    let (db, _) = fynkel_document::AssetDatabase::scan(&fixtures().join("project/assets"), &s);
    let mut lib = PrefabLibrary::empty();
    let diags = lib.add_sources(
        &db,
        &PrefabLibrary::wanted_sources(&editor.scene()),
        &fynkel_import::ImportedPrefabs::default(),
    );
    assert!(diags.is_empty(), "{diags:#?}");
    let expected = expand(&editor.scene(), &lib, &s);
    assert!(!expected.has_errors(), "{:#?}", expected.diagnostics);

    unpack(&mut editor, &lib, IMPORTED);
    assert_eq!(text(&editor), expected.document.write().unwrap());
    let sc = scene(&editor);
    let names: Vec<&str> = sc.entities.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Camera", "Sun", "Rig", "Body", "hips", "spine", "head", "Prop", "Imported", "Body",
            "hips", "spine", "head", "Prop"
        ]
    );
    // `Imported` is the rig's root: the Prop hangs off it directly.
    let prop = sc.entities.last().unwrap();
    assert_eq!(prop.parent, Some(sc.entities[8].id));
    let renderer = prop
        .components
        .iter()
        .find(|c| c.type_id == "fynkel.MeshRenderer")
        .unwrap();
    assert_eq!(
        renderer.data.get("material").unwrap().to_serde()["asset"],
        json!("c0000000-0000-4000-8000-000000000020"),
        "the override is applied"
    );
    assert!(
        sc.validate(&s).iter().all(|d| !d.is_error()),
        "{:#?}",
        sc.validate(&s)
    );
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
}

// --------------------------------------------------------------- side effects

#[test]
fn save_and_create_material_are_side_effects_after_validation() {
    let dir = tempdir();
    let scene_path = dir.join("edited.scene.json");
    std::fs::copy(
        fixtures().join("scene/hierarchy_ok.scene.json"),
        &scene_path,
    )
    .unwrap();
    let doc = Document::read(&scene_path, None).unwrap();
    let s = schemas();
    let mut editor = Editor::open(doc, &s).unwrap();
    let material_path = dir.join("materials/new.material.json");
    let r = apply(
        &mut editor,
        vec![
            Command::RenameObject {
                id: u(ROOT),
                name: "Saved".into(),
            },
            Command::CreateMaterial {
                id: Some(u("44444444-0000-4000-8000-000000000004")),
                name: "New".into(),
                shader: None,
                path: material_path.clone(),
            },
            Command::SaveDocument { path: None },
        ],
    );
    assert!(r.mutations[1].undo.is_empty());
    assert!(r.mutations[2].undo.is_empty());
    assert_eq!(
        r.mutations[1].affected_ids,
        vec![u("44444444-0000-4000-8000-000000000004")]
    );
    assert!(!editor.is_dirty());
    let reread = Document::read(&scene_path, Some(DocumentKind::Scene)).unwrap();
    assert_eq!(reread.write().unwrap(), text(&editor));
    let material = Document::read(&material_path, Some(DocumentKind::Material)).unwrap();
    assert_eq!(material.envelope.name, "New");
    assert!(!fynkel_document::has_errors(&material.validate(&s)));
    // The saved material is a valid material for the built-in shader.
    let m = fynkel_document::Material::from_document(material);
    assert!(!fynkel_document::has_errors(
        &m.validate(&s, &fynkel_document::ShaderLibrary::builtin())
    ));

    // Never overwrites; a failed side effect rolls the transaction back.
    let before = text(&editor);
    let r = editor.apply(
        Request::new(vec![
            Command::RenameObject {
                id: u(ROOT),
                name: "Rolled back".into(),
            },
            Command::CreateMaterial {
                id: None,
                name: "Again".into(),
                shader: None,
                path: material_path.clone(),
            },
        ]),
        &s,
    );
    assert!(!r.ok);
    assert_eq!(r.mutations[1].diagnostics[0].code.0, "FYN-E0011");
    assert_eq!(text(&editor), before);
    // An unknown shader is refused up front.
    let r = editor.apply(
        Request::new(vec![Command::CreateMaterial {
            id: None,
            name: "Bad".into(),
            shader: Some("acme.magic@9".into()),
            path: dir.join("bad.material.json"),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0004");
    assert!(!dir.join("bad.material.json").exists());

    // Saving is not undone by undo (a saved file stays saved) but the
    // in-memory document is.
    editor.undo().unwrap();
    assert!(editor.is_dirty());
    assert_eq!(scene(&editor).entity(u(ROOT)).unwrap().name, "Root");
    let on_disk = Document::read(&scene_path, Some(DocumentKind::Scene)).unwrap();
    assert_eq!(
        Scene::from_document(on_disk).entity(u(ROOT)).unwrap().name,
        "Saved"
    );

    // A document without a path needs an explicit one.
    let mut fresh = Editor::open(Document::new(DocumentKind::Scene, "fresh"), &s).unwrap();
    let r = fresh.apply(Request::new(vec![Command::SaveDocument { path: None }]), &s);
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0011");
    let target = dir.join("fresh.scene.json");
    let r = fresh.apply(
        Request::new(vec![Command::SaveDocument {
            path: Some(target.clone()),
        }]),
        &s,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    assert!(target.exists());
    std::fs::remove_dir_all(&dir).ok();
}

// --------------------------------------------------------------- JSON codec

#[test]
fn commands_round_trip_through_json() {
    let mesh = AssetRef::with_sub(
        "c0000000-0000-4000-8000-000000000001".parse().unwrap(),
        "mesh/0",
    );
    let all = vec![
        Command::CreateObject {
            id: Some(u(KID1)),
            name: "n".into(),
            parent: Some(u(ROOT)),
            index: Some(2),
            components: vec![transform()],
        },
        Command::DeleteObject { id: u(ROOT) },
        Command::RenameObject {
            id: u(ROOT),
            name: "x".into(),
        },
        Command::SetObjectActive {
            id: u(ROOT),
            active: false,
        },
        Command::ReparentObject {
            id: u(ROOT),
            parent: None,
            index: None,
        },
        Command::AddComponent {
            entity: u(ROOT),
            component: ComponentSpec {
                id: None,
                type_id: "fynkel.Transform".into(),
                version: Some(1),
                data: None,
            },
        },
        Command::RemoveComponent {
            entity: u(ROOT),
            component: u(TRANSFORM),
        },
        Command::SetField {
            entity: u(ROOT),
            component: u(TRANSFORM),
            field: "/position/0".into(),
            value: Node::from_serde(&json!({"a": [1, 2.5, null, "s", true]})),
        },
        Command::UnsetField {
            entity: u(ROOT),
            component: u(TRANSFORM),
            field: "scale".into(),
        },
        Command::AssignAsset {
            entity: u(ROOT),
            component: u(TRANSFORM),
            field: "mesh".into(),
            asset: mesh.clone(),
        },
        Command::InstantiatePrefab {
            id: None,
            name: "p".into(),
            parent: Some(u(ROOT)),
            index: None,
            prefab: AssetRef::new(mesh.asset),
            instance: Some(u(KID2)),
            components: vec![],
            overrides: vec![Node::from_serde(
                &json!({"op": "setActive", "entity": KID1, "value": false}),
            )],
        },
        Command::SetPrefabOverride {
            entity: u(ROOT),
            r#override: Node::from_serde(&json!({"op": "removeObject", "entity": KID1})),
        },
        Command::RevertPrefabOverride {
            entity: u(ROOT),
            index: Some(1),
        },
        Command::UnpackPrefab {
            entity: u(ROOT),
            completely: false,
        },
        Command::UnpackPrefab {
            entity: u(ROOT),
            completely: true,
        },
        Command::ApplyPrefabOverride {
            r#override: Node::from_serde(&json!({"op": "removeObject", "entity": KID1})),
        },
        Command::DuplicateObject {
            id: u(ROOT),
            new_id: Some(u(KID2)),
            name: Some("Root copy".into()),
            index: Some(0),
        },
        Command::SetDocumentField {
            pointer: "/base_color".into(),
            value: Node::from_serde(&json!([1, 0.5, 0.25, 1])),
        },
        Command::UnsetDocumentField {
            pointer: "/metallic".into(),
        },
        Command::CreateMaterial {
            id: None,
            name: "m".into(),
            shader: Some("fynkel.pbr@1".into()),
            path: PathBuf::from("materials/m.material.json"),
        },
        Command::SaveDocument {
            path: Some(PathBuf::from("out.scene.json")),
        },
    ];
    for c in &all {
        let j = c.to_json();
        assert_eq!(j["command"], json!(c.name()));
        let back = Command::from_json(&j).unwrap_or_else(|d| panic!("{j}: {d}"));
        assert_eq!(&back, c);
    }
    let mut names: Vec<&str> = all.iter().map(Command::name).collect();
    names.dedup();
    assert_eq!(
        names,
        [
            "createObject",
            "deleteObject",
            "renameObject",
            "setObjectActive",
            "reparentObject",
            "addComponent",
            "removeComponent",
            "setField",
            "unsetField",
            "assignAsset",
            "instantiatePrefab",
            "setPrefabOverride",
            "revertPrefabOverride",
            "unpackPrefab",
            "applyPrefabOverride",
            "duplicateObject",
            "setDocumentField",
            "unsetDocumentField",
            "createMaterial",
            "saveDocument"
        ]
    );

    // Errors name the offending command and key.
    let err = Command::from_json(&json!({"command": "warp"})).unwrap_err();
    assert_eq!(err.code.0, "FYN-E0004");
    let err = Command::from_json(&json!({"command": "renameObject", "id": "nope"})).unwrap_err();
    assert!(err.message.contains("\"id\""));
    let err = Command::from_json(
        &json!({"command": "setField", "entity": ROOT, "component": TRANSFORM, "field": "f"}),
    )
    .unwrap_err();
    assert!(err.message.contains("value"));

    // Requests: array form and object form.
    let r = Request::from_json(&json!([{"command": "deleteObject", "id": ROOT}])).unwrap();
    assert_eq!(r.commands.len(), 1);
    assert!(r.base_revision.is_none());
    let rev = Revision::of_text("x");
    let r = Request::from_json(&json!({
        "transactionId": KID2,
        "baseRevision": rev.as_str(),
        "commands": [{"commandId": KID1, "command": "deleteObject", "id": ROOT}]
    }))
    .unwrap();
    assert_eq!(r.id, Some(u(KID2)));
    assert_eq!(r.base_revision, Some(rev));
    assert_eq!(r.commands[0].id, Some(u(KID1)));
    let errs = Request::from_json(
        &json!({"commands": [{"command": "nope"}, {"command": "deleteObject"}]}),
    )
    .unwrap_err();
    assert_eq!(errs.len(), 2);
    assert_eq!(errs[0].pointer.as_deref(), Some("/commands/0"));
    assert!(Request::from_json(&json!(3)).is_err());
    assert!(Request::from_json(&json!({"baseRevision": "short", "commands": []})).is_err());
}

#[test]
fn applied_json_has_the_mutation_shape() {
    let (mut editor, _) = open("scene/hierarchy_ok.scene.json");
    let r = apply(
        &mut editor,
        vec![Command::RenameObject {
            id: u(ROOT),
            name: "J".into(),
        }],
    );
    let j = r.json();
    assert_eq!(j["ok"], json!(true));
    assert_eq!(j["baseRevision"].as_str().unwrap().len(), 64);
    assert_ne!(j["revision"], j["baseRevision"]);
    let m = &j["mutations"][0];
    assert_eq!(m["command"], json!("renameObject"));
    assert_eq!(m["transactionId"], j["transactionId"]);
    assert_eq!(m["baseRevision"], j["baseRevision"]);
    assert_eq!(m["affectedIds"], json!([ROOT]));
    assert_eq!(m["diagnostics"], json!([]));
    assert_eq!(m["undo"][0]["op"], json!("replace"));
    assert_eq!(m["undo"][0]["value"], json!("Root"));
    assert!(m["commandId"].as_str().unwrap().len() == 36);
    // The diff helper used by --dry-run.
    let d = diff("a\nb\n", "a\nc\n", "x.json");
    assert!(d.contains("-b") && d.contains("+c") && d.contains("a/x.json"));
}

#[test]
fn inspect_json_carries_the_revision() {
    let (editor, original) = open("scene/hierarchy_ok.scene.json");
    let v = fynkel_document::scene::inspect_json(&editor.scene());
    assert_eq!(v["revision"], json!(Revision::of_text(&original).as_str()));
    let (p, original) = open("valid/turret.prefab.json");
    let v = fynkel_document::prefab::inspect_json(&Prefab::from_document(p.into_document()));
    assert_eq!(v["revision"], json!(Revision::of_text(&original).as_str()));
}

// --------------------------------------------------------------- M4-01

#[test]
fn materials_edit_through_document_fields() {
    let (mut editor, original) = open("project/assets/materials/rusted.material.json");
    let s = schemas();
    // A colour change: replace in place, byte-exact undo.
    let r = apply(
        &mut editor,
        vec![Command::SetDocumentField {
            pointer: "/base_color".into(),
            value: Node::from_serde(&json!([0.1, 0.2, 0.3, 1.0])),
        }],
    );
    assert!(r.mutations[0].affected_ids.is_empty());
    assert_eq!(
        editor
            .document()
            .root
            .pointer("/base_color/1")
            .and_then(Node::as_f64),
        Some(0.2)
    );
    assert!(editor.is_dirty());
    // Unset + set a scalar; a new member appends.
    apply(
        &mut editor,
        vec![
            Command::UnsetDocumentField {
                pointer: "/metallic".into(),
            },
            Command::SetDocumentField {
                pointer: "/roughness".into(),
                value: Node::from_serde(&json!(0.9)),
            },
            Command::SetDocumentField {
                pointer: "/double_sided".into(),
                value: Node::from_serde(&json!(true)),
            },
        ],
    );
    assert!(editor.document().root.pointer("/metallic").is_none());
    assert_eq!(
        editor
            .document()
            .root
            .pointer("/double_sided")
            .and_then(Node::as_bool),
        Some(true)
    );
    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
    assert!(!editor.is_dirty());
    editor.redo().unwrap();
    assert_eq!(
        editor
            .document()
            .root
            .pointer("/base_color/1")
            .and_then(Node::as_f64),
        Some(0.2)
    );

    // The envelope's identity and the entity tree are not fields.
    for pointer in ["/id", "/formatVersion", "/$schema", "", "base_color"] {
        let r = editor.apply(
            Request::new(vec![Command::SetDocumentField {
                pointer: pointer.into(),
                value: Node::from_serde(&json!(1)),
            }]),
            &s,
        );
        assert!(!r.ok, "{pointer}");
        assert_eq!(
            r.mutations[0].diagnostics[0].code.0, "FYN-E0012",
            "{pointer}"
        );
    }
    let r = editor.apply(
        Request::new(vec![Command::UnsetDocumentField {
            pointer: "/name".into(),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0012");
    let r = editor.apply(
        Request::new(vec![Command::UnsetDocumentField {
            pointer: "/nothing_here".into(),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0012");
    // Entity commands do not apply to a material.
    let r = editor.apply(
        Request::new(vec![Command::RenameObject {
            id: u(ROOT),
            name: "x".into(),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0007");
    // A value the material schema refuses rejects the transaction whole.
    let before = text(&editor);
    let r = editor.apply(
        Request::new(vec![Command::SetDocumentField {
            pointer: "/base_color".into(),
            value: Node::from_serde(&json!("red")),
        }]),
        &s,
    );
    assert!(!r.ok);
    assert_eq!(r.diagnostics[0].code.0, "FYN-E0005");
    assert_eq!(text(&editor), before);
    // The JSON form the ABI carries.
    let r = editor.apply(
        Request::from_json(
            &json!([{ "command": "setDocumentField", "pointer": "/name", "value": "Rusty" }]),
        )
        .unwrap(),
        &s,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    assert_eq!(
        editor.document().root.get("name").and_then(Node::as_str),
        Some("Rusty")
    );
}

#[test]
fn volume_profiles_edit_through_document_fields() {
    let s = schemas();
    let mut fresh = Document::new(DocumentKind::Volume, "Look").root.to_serde();
    fresh.as_object_mut().unwrap().remove("bloom");
    let doc = Document::parse(DocumentKind::Volume, None, &fresh.to_string()).unwrap();
    let original = doc.write().unwrap();
    let mut editor = Editor::open(doc, &s).unwrap();
    // Add Override: a block with one field; then a field of it; then off.
    apply(
        &mut editor,
        vec![Command::SetDocumentField {
            pointer: "/bloom".into(),
            value: Node::from_serde(&json!({ "intensity": 1.5 })),
        }],
    );
    apply(
        &mut editor,
        vec![
            Command::SetDocumentField {
                pointer: "/bloom/threshold".into(),
                value: Node::from_serde(&json!(2.0)),
            },
            Command::UnsetDocumentField {
                pointer: "/ambientOcclusion/radius".into(),
            },
        ],
    );
    let root = &editor.document().root;
    assert_eq!(
        root.pointer("/bloom/threshold").and_then(Node::as_f64),
        Some(2.0)
    );
    assert!(root.pointer("/ambientOcclusion/radius").is_none());
    // The schema's ranges refuse the transaction whole.
    let before = text(&editor);
    let r = editor.apply(
        Request::new(vec![Command::SetDocumentField {
            pointer: "/bloom/scatter".into(),
            value: Node::from_serde(&json!(4)),
        }]),
        &s,
    );
    assert!(!r.ok);
    assert_eq!(text(&editor), before);
    // No entities in a profile.
    let r = editor.apply(
        Request::new(vec![Command::RenameObject {
            id: u(ROOT),
            name: "x".into(),
        }]),
        &s,
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0007");
    assert!(r.mutations[0].diagnostics[0].message.contains("volume"));
    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
}

#[test]
fn document_fields_on_scenes_leave_the_entity_tree_to_entity_commands() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let s = schemas();
    apply(
        &mut editor,
        vec![Command::SetDocumentField {
            pointer: "/name".into(),
            value: Node::from_serde(&json!("Renamed scene")),
        }],
    );
    assert_eq!(
        editor.document().root.get("name").and_then(Node::as_str),
        Some("Renamed scene")
    );
    for pointer in ["/entities", &format!("/entities/{ROOT}/name"), "/root"] {
        let r = editor.apply(
            Request::new(vec![Command::SetDocumentField {
                pointer: pointer.to_string(),
                value: Node::from_serde(&json!({})),
            }]),
            &s,
        );
        assert_eq!(
            r.mutations[0].diagnostics[0].code.0, "FYN-E0012",
            "{pointer}"
        );
    }
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
}

#[test]
fn duplicate_object_copies_the_subtree_with_fresh_ids() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let copy_id = u("eeeeeeee-0000-4000-8000-000000000005");
    // The whole root: two children come along, every id is new, references
    // inside the copy point at the copy.
    let r = apply(
        &mut editor,
        vec![Command::DuplicateObject {
            id: u(ROOT),
            new_id: Some(copy_id),
            name: Some("Root (1)".into()),
            index: None,
        }],
    );
    let m = &r.mutations[0];
    assert_eq!(m.affected_ids[0], copy_id);
    let sc = scene(&editor);
    assert_eq!(sc.entities.len(), 6);
    let copy = sc.entity(copy_id).unwrap();
    assert_eq!(copy.name, "Root (1)");
    assert_eq!(copy.parent, None);
    assert_eq!(copy.children.len(), 2);
    for c in &copy.children {
        let child = sc.entity(*c).unwrap();
        assert_eq!(child.parent, Some(copy_id));
        assert!(child.id != u(KID1) && child.id != u(KID2));
        assert!(m.affected_ids.contains(c));
    }
    let names: Vec<&str> = copy
        .children
        .iter()
        .map(|c| sc.entity(*c).unwrap().name.as_str())
        .collect();
    assert_eq!(names, ["Kid1", "Kid2"]);
    // Component ids are fresh too, the data identical.
    let copy_comp = copy
        .components
        .iter()
        .find(|c| c.type_id == "fynkel.Transform")
        .unwrap();
    assert_ne!(copy_comp.id, u(TRANSFORM));
    assert!(m.affected_ids.contains(&copy_comp.id));
    assert_eq!(
        editor
            .document()
            .root
            .pointer(&format!(
                "/entities/{copy_id}/components/{}/data/scale",
                copy_comp.id
            ))
            .map(Node::to_serde),
        editor
            .document()
            .root
            .pointer(&format!(
                "/entities/{ROOT}/components/{TRANSFORM}/data/scale"
            ))
            .map(Node::to_serde),
    );
    // The original is untouched and still named Root.
    assert_eq!(sc.entity(u(ROOT)).unwrap().children, [u(KID1), u(KID2)]);
    assert!(editor.validate(&schemas()).iter().all(|d| !d.is_error()));
    // The copy has no spans; the canonical text still writes and re-reads.
    let again = Document::parse(DocumentKind::Scene, None, &text(&editor)).unwrap();
    assert_eq!(Scene::from_document(again).entities.len(), 6);

    // A child: the copy lands right after the original in the parent's list.
    let r = apply(
        &mut editor,
        vec![Command::DuplicateObject {
            id: u(KID1),
            new_id: None,
            name: None,
            index: None,
        }],
    );
    let kid_copy = r.mutations[0].affected_ids[0];
    let sc = scene(&editor);
    assert_eq!(
        sc.entity(u(ROOT)).unwrap().children,
        [u(KID1), kid_copy, u(KID2)]
    );
    assert_eq!(sc.entity(kid_copy).unwrap().name, "Kid1");
    assert!(r.mutations[0].affected_ids.contains(&u(ROOT)));
    // An explicit index wins.
    let r = apply(
        &mut editor,
        vec![Command::DuplicateObject {
            id: u(KID2),
            new_id: None,
            name: Some("First".into()),
            index: Some(0),
        }],
    );
    let first = r.mutations[0].affected_ids[0];
    assert_eq!(scene(&editor).entity(u(ROOT)).unwrap().children[0], first);
    // A taken id is refused; nothing changes.
    let before = text(&editor);
    let r = editor.apply(
        Request::new(vec![Command::DuplicateObject {
            id: u(KID2),
            new_id: Some(u(ROOT)),
            name: None,
            index: None,
        }]),
        &schemas(),
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0010");
    assert_eq!(text(&editor), before);
    let r = editor.apply(
        Request::new(vec![Command::DuplicateObject {
            id: u("99999999-0000-4000-8000-000000000009"),
            new_id: None,
            name: None,
            index: None,
        }]),
        &schemas(),
    );
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0002");
    // Three undos: the original bytes.
    editor.undo().unwrap();
    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
}

#[test]
fn duplicate_object_in_prefabs_and_of_prefab_instances() {
    // A prefab's root cannot be copied next to itself (one root); a child
    // is copied next to the original.
    let (mut editor, original) = open("valid/turret.prefab.json");
    let prefab = Prefab::from_document(editor.document().clone());
    let root = prefab.root;
    let r = editor.apply(
        Request::new(vec![Command::DuplicateObject {
            id: root,
            new_id: None,
            name: None,
            index: None,
        }]),
        &schemas(),
    );
    assert!(!r.ok);
    assert_eq!(text(&editor), original);
    let child = prefab.entity(root).unwrap().children[0];
    let r = apply(
        &mut editor,
        vec![Command::DuplicateObject {
            id: child,
            new_id: None,
            name: None,
            index: None,
        }],
    );
    let copy = r.mutations[0].affected_ids[0];
    let after = Prefab::from_document(editor.document().clone());
    let children = &after.entity(root).unwrap().children;
    assert_eq!(children[0], child);
    assert_eq!(children[1], copy);
    assert!(
        editor.validate(&schemas()).iter().all(|d| !d.is_error()),
        "{:#?}",
        editor.validate(&schemas())
    );
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);

    // A prefab instance: a fresh instance id, the overrides kept (they
    // address prefab-local ids), the expansion still valid.
    let (mut editor, original) = open("prefab/instances_ok.scene.json");
    let sc = scene(&editor);
    let a = sc.entities.iter().find(|e| e.name == "Turret A").unwrap();
    let a_instance = a.prefab.as_ref().unwrap().instance;
    let a_overrides = editor
        .document()
        .root
        .pointer(&format!("/entities/{}/prefab/overrides", a.id))
        .map(Node::to_serde)
        .unwrap();
    let r = apply(
        &mut editor,
        vec![Command::DuplicateObject {
            id: a.id,
            new_id: None,
            name: Some("Turret C".into()),
            index: None,
        }],
    );
    let copy = r.mutations[0].affected_ids[0];
    let sc = scene(&editor);
    let c = sc.entity(copy).unwrap();
    let c_instance = c.prefab.as_ref().unwrap().instance;
    assert_ne!(c_instance, a_instance);
    assert!(r.mutations[0].affected_ids.contains(&c_instance));
    assert_eq!(
        editor
            .document()
            .root
            .pointer(&format!("/entities/{copy}/prefab/overrides"))
            .map(Node::to_serde)
            .unwrap(),
        a_overrides
    );
    assert!(
        editor.validate(&schemas()).iter().all(|d| !d.is_error()),
        "{:#?}",
        editor.validate(&schemas())
    );
    editor.undo().unwrap();
    assert_eq!(text(&editor), original);
}

/// D7 (blueprint §19.2c, closed by M4-01): validation anchors one
/// diagnostic per entity through a `Locator`, so a document with
/// thousands of diagnostics costs O(n), not O(n²). 16 000 entities with
/// 8 000 unknown-component warnings took 726 ms at the M2 gate.
#[test]
fn large_documents_with_many_diagnostics_validate_in_linear_time() {
    fn synthetic(n: usize) -> String {
        let mut s = String::with_capacity(n * 400);
        s.push_str("{\n  \"$schema\": \"https://fynkel.dev/schemas/scene.schema.json\",\n  \"formatVersion\": 5,\n  \"id\": \"70d7ba8b-f7fb-4eba-920d-ec9e6b968330\",\n  \"name\": \"Big\",\n  \"entities\": {\n");
        for i in 0..n {
            let id = format!("aaaaaaaa-0000-4000-8000-{:012x}", i + 1);
            let comp = format!("11111111-0000-4000-8000-{:012x}", i + 1);
            let ty = if i % 2 == 0 {
                "fynkel.Transform"
            } else {
                "game.Unknown"
            };
            let data = if i % 2 == 0 {
                "{ \"position\": [0, 0, 0], \"rotation\": [0, 0, 0, 1], \"scale\": [1, 1, 1] }"
            } else {
                "{ \"x\": 1 }"
            };
            s.push_str(&format!(
                "    \"{id}\": {{ \"name\": \"E{i}\", \"parent\": null, \"children\": [], \"components\": {{ \"{comp}\": {{ \"type\": \"{ty}\", \"version\": 1, \"data\": {data} }} }} }}{}\n",
                if i + 1 < n { "," } else { "" }
            ));
        }
        s.push_str("  }\n}\n");
        s
    }
    let s = schemas();
    let time = |n: usize| {
        let text = synthetic(n);
        let start = std::time::Instant::now();
        let doc = Document::parse(DocumentKind::Scene, None, &text).unwrap();
        let read = start.elapsed();
        let start = std::time::Instant::now();
        let diags = Scene::from_document(doc).validate(&s);
        let validate = start.elapsed();
        let warnings = diags.iter().filter(|d| d.code.0 == "FYN-D0205").count();
        assert_eq!(warnings, n / 2);
        assert!(
            diags.iter().all(|d| d.line.is_some()),
            "every warning is anchored"
        );
        (read, validate)
    };
    // The best of three: the first run pays for page faults and the
    // allocator's growth, which is not what the bound is about.
    let best = |n: usize| {
        (0..3)
            .map(|_| time(n))
            .min_by_key(|(r, v)| *r + *v)
            .unwrap()
    };
    let (read_small, validate_small) = best(4_000);
    let (read, validate) = best(16_000);
    eprintln!(
        "16 000 entities / 8 000 warnings: read {read:?}, validate {validate:?} (4 000: read {read_small:?}, validate {validate_small:?})"
    );
    // Linear: 4× the entities cost at most ~6× (allocator noise), not 16×.
    assert!(
        validate < validate_small * 6,
        "validate {validate:?} vs {validate_small:?}"
    );
    assert!(read < read_small * 6, "read {read:?} vs {read_small:?}");
    if !cfg!(debug_assertions) {
        assert!(
            read + validate < std::time::Duration::from_millis(100),
            "{:?}",
            read + validate
        );
    }

    // apply + validate on a 10 000-entity scene stays linear as well.
    let apply_time = |n: usize| {
        let doc = Document::parse(DocumentKind::Scene, None, &synthetic(n)).unwrap();
        let mut editor = Editor::open(doc, &s).unwrap();
        let start = std::time::Instant::now();
        let r = editor.apply(
            Request::new(vec![Command::RenameObject {
                id: u(&format!("aaaaaaaa-0000-4000-8000-{:012x}", n)),
                name: "Last".into(),
            }]),
            &s,
        );
        assert!(r.ok, "{:#?}", r.all_diagnostics());
        start.elapsed()
    };
    let small = apply_time(2_500);
    let big = apply_time(10_000);
    eprintln!("apply+validate: 10 000 entities {big:?} (2 500: {small:?})");
    assert!(big < small * 6, "apply {big:?} vs {small:?}");
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fynkel-authoring-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn move_component_reorders_the_components_and_undoes_exactly() {
    let (mut editor, original) = open("scene/hierarchy_ok.scene.json");
    let light = ComponentSpec {
        id: Some(u("22222222-0000-4000-8000-000000000002")),
        type_id: "fynkel.PointLight".into(),
        version: None,
        data: Some(Node::from_serde(&json!({
            "color": [1, 1, 1], "intensity": 800, "range": 10, "culling_mask": 4294967295u32
        }))),
    };
    apply(
        &mut editor,
        vec![Command::AddComponent {
            entity: u(ROOT),
            component: light,
        }],
    );
    let added = text(&editor);
    let order = |editor: &Editor| -> Vec<String> {
        editor
            .document()
            .root
            .pointer(&format!("/entities/{ROOT}/components"))
            .and_then(Node::as_object)
            .unwrap()
            .iter()
            .map(|m| m.key.clone())
            .collect()
    };
    let before = order(&editor);
    assert_eq!(
        before.last().unwrap(),
        "22222222-0000-4000-8000-000000000002"
    );
    let mv = |index| Command::MoveComponent {
        entity: u(ROOT),
        component: u("22222222-0000-4000-8000-000000000002"),
        index,
    };

    // Move Up: to the first place; the data travels with it.
    let r = apply(&mut editor, vec![mv(0)]);
    assert_eq!(order(&editor)[0], "22222222-0000-4000-8000-000000000002");
    assert_eq!(order(&editor).len(), before.len());
    assert_eq!(
        r.mutations[0].affected_ids,
        vec![u(ROOT), u("22222222-0000-4000-8000-000000000002")]
    );
    // Past the end is last; the same place changes nothing.
    apply(&mut editor, vec![mv(99)]);
    assert_eq!(order(&editor), before);
    assert_eq!(text(&editor), added);
    let last = before.len() - 1;
    apply(&mut editor, vec![mv(last)]);
    assert_eq!(text(&editor), added);

    // Undo each step back to the byte, then the add.
    apply(&mut editor, vec![mv(0)]);
    editor.undo().unwrap();
    assert_eq!(text(&editor), added);
    while editor.undo().is_ok() {}
    assert_eq!(text(&editor), original);

    // The JSON form round-trips; an unknown component is refused.
    let json = mv(1).to_json();
    assert_eq!(json["command"], "moveComponent");
    assert_eq!(Command::from_json(&json).unwrap(), mv(1));
    let r = editor.apply(
        Request::new(vec![Command::MoveComponent {
            entity: u(ROOT),
            component: u("99999999-0000-4000-8000-000000000009"),
            index: 0,
        }]),
        &schemas(),
    );
    assert!(!r.ok);
}

// ------------------------------------------ variants, nesting, unpack (2026-09-25)

const TURRET: &str = "10000000-0000-4000-8000-000000000001";
const T_BASE: &str = "10000000-0000-4000-8000-0000000000b1";
const T_BARREL: &str = "10000000-0000-4000-8000-0000000000b2";
const T_RENDERER: &str = "10000000-0000-4000-8000-0000000000c2";
const T_BARREL_T: &str = "10000000-0000-4000-8000-0000000000c3";

fn full_transform(y: f64) -> serde_json::Value {
    json!({ "type": "fynkel.Transform", "version": 1,
        "data": { "position": [0, y, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } })
}

fn light() -> serde_json::Value {
    json!({ "type": "fynkel.PointLight", "version": 1,
        "data": { "color": [1, 0.5, 0.2], "intensity": 3.0, "range": 4.0, "culling_mask": 4294967295u32 } })
}

fn prefab_of(v: serde_json::Value) -> Prefab {
    Prefab::from_document(Document::parse(DocumentKind::Prefab, None, &v.to_string()).unwrap())
}

fn scene_of(v: serde_json::Value) -> Editor {
    Editor::open(
        Document::parse(DocumentKind::Scene, None, &v.to_string()).unwrap(),
        &schemas(),
    )
    .unwrap()
}

/// A scene holding one instance of `prefab` with `overrides`.
fn one_instance(
    anchor: &str,
    instance: &str,
    prefab: &str,
    overrides: serde_json::Value,
) -> Editor {
    scene_of(json!({
        "$schema": "https://fynkel.dev/schemas/scene.schema.json",
        "formatVersion": 5, "id": "60000000-0000-4000-8000-000000000000", "name": "S",
        "entities": { anchor: { "name": "Thing", "parent": null, "children": [], "components": {},
            "prefab": { "asset": { "asset": prefab }, "instance": instance, "overrides": overrides } } }
    }))
}

/// What a scene looks like once expanded, without ids: the tree of names,
/// active flags and components (type and data, by type).
fn shape(scene: &Scene, lib: &PrefabLibrary) -> serde_json::Value {
    let x = expand(scene, lib, &schemas());
    assert!(!x.has_errors(), "{:#?}", x.diagnostics);
    let out = x.scene();
    fn node(s: &Scene, e: &fynkel_document::scene::Entity) -> serde_json::Value {
        let mut components: Vec<(String, serde_json::Value)> = e
            .components
            .iter()
            .map(|c| (c.type_id.clone(), c.data.to_serde()))
            .collect();
        components.sort_by(|a, b| a.0.cmp(&b.0));
        json!({
            "name": e.name,
            "active": e.active,
            "components": components,
            "children": e.children.iter().map(|c| node(s, s.entity(*c).unwrap())).collect::<Vec<_>>(),
        })
    }
    json!(out
        .entities
        .iter()
        .filter(|e| e.parent.is_none())
        .map(|e| node(&out, e))
        .collect::<Vec<_>>())
}

fn instances_in(editor: &Editor) -> usize {
    editor.scene().instances().count()
}

/// Nested instances in a prefab document (ADR-0022 amendment 2026-09-25):
/// `instantiatePrefab` there takes an explicit id and instance and a full
/// override list, children go under it with `createObject`, the prefab
/// validates and flattens; a malformed override and an instance that
/// would contain the prefab itself are refused.
#[test]
fn prefabs_hold_nested_instances_with_ids_and_overrides() {
    let s = schemas();
    let lib = library();
    let holder = prefab_of(json!({
        "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
        "formatVersion": 5, "id": "60000000-0000-4000-8000-000000000001", "name": "Holder",
        "root": "60000000-0000-4000-8000-0000000000a1",
        "entities": { "60000000-0000-4000-8000-0000000000a1": { "name": "Holder", "parent": null,
            "children": [], "components": { "60000000-0000-4000-8000-0000000000c1": full_transform(0.0) } } }
    }));
    let root = holder.root;
    let mut editor = Editor::open(holder.document.clone(), &s).unwrap();
    let gun = u("60000000-0000-4000-8000-0000000000a2");
    let gun_instance = u("60000000-0000-4000-8000-0000000000e2");
    let scope = u("60000000-0000-4000-8000-0000000000a3");
    let moved = json!({ "op": "setField", "entity": T_BARREL, "component": T_BARREL_T,
                        "field": "position", "value": [0, 4, 0] });
    let instantiate = |overrides: Vec<Node>| Command::InstantiatePrefab {
        id: Some(gun),
        name: "Gun".into(),
        parent: Some(root),
        index: None,
        prefab: AssetRef::new(TURRET.parse().unwrap()),
        instance: Some(gun_instance),
        components: vec![],
        overrides,
    };
    let r = editor.apply_with_prefabs(
        Request::new(vec![
            instantiate(vec![Node::from_serde(&moved)]),
            Command::CreateObject {
                id: Some(scope),
                name: "Scope".into(),
                parent: Some(gun),
                index: None,
                components: vec![transform()],
            },
        ]),
        &s,
        &lib,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    assert!(r.mutations[0].affected_ids.contains(&gun_instance));
    let p = Prefab::from_document(editor.document().clone());
    assert!(p.validate(&s).is_empty(), "{:#?}", p.validate(&s));
    let inst = p.entity(gun).unwrap().prefab.as_ref().unwrap();
    assert_eq!(inst.instance, gun_instance);
    assert_eq!(inst.overrides.len(), 1);
    assert_eq!(p.entity(gun).unwrap().children, vec![scope]);

    // Flattened: Holder › Gun (the Turret's root) › Barrel (moved), Scope.
    let mut lib2 = lib.clone();
    lib2.add(p.clone());
    let (flat, _) = lib2.flattened(&s);
    let fh = flat.get(p.id()).unwrap();
    let names: Vec<&str> = fh.entities.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Holder", "Gun", "Barrel", "Scope"]);
    let barrel = fh
        .entity(fynkel_document::prefab::derive_id(
            gun_instance,
            u(T_BARREL),
        ))
        .unwrap();
    assert_eq!(
        barrel
            .component_of("fynkel.Transform")
            .unwrap()
            .data
            .get("position")
            .unwrap()
            .to_serde(),
        json!([0, 4, 0])
    );
    assert_eq!(fh.entity(gun).unwrap().children.last(), Some(&scope));

    // JSON: `overrides` round-trips.
    let c = instantiate(vec![Node::from_serde(&moved)]);
    assert_eq!(Command::from_json(&c.to_json()).unwrap(), c);
    // A malformed override is refused before anything is written.
    let before = text(&editor);
    let mut bad = instantiate(vec![Node::from_serde(&json!({ "op": "setField" }))]);
    if let Command::InstantiatePrefab { id, .. } = &mut bad {
        *id = None;
    }
    let r = editor.apply_with_prefabs(Request::new(vec![bad]), &s, &lib);
    assert!(!r.ok);
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-E0004");
    // A prefab that contains the Holder cannot go into it (a cycle).
    let outer = prefab_of(json!({
        "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
        "formatVersion": 5, "id": "60000000-0000-4000-8000-000000000002", "name": "Outer",
        "root": "60000000-0000-4000-8000-0000000000b1",
        "entities": { "60000000-0000-4000-8000-0000000000b1": { "name": "Outer", "parent": null,
            "children": [], "components": {},
            "prefab": { "asset": { "asset": "60000000-0000-4000-8000-000000000001" },
                        "instance": "60000000-0000-4000-8000-0000000000e9" } } }
    }));
    lib2.add(outer);
    let r = editor.apply_with_prefabs(
        Request::new(vec![Command::InstantiatePrefab {
            id: None,
            name: "Outer".into(),
            parent: Some(root),
            index: None,
            prefab: AssetRef::new("60000000-0000-4000-8000-000000000002".parse().unwrap()),
            instance: None,
            components: vec![],
            overrides: vec![],
        }]),
        &s,
        &lib2,
    );
    assert!(!r.ok);
    assert_eq!(r.mutations[0].diagnostics[0].code.0, "FYN-D0402");
    assert_eq!(text(&editor), before);
}

/// Apply to Prefab into a variant (ADR-0022 amendment 2026-09-25): each
/// override of a scene instance of the variant, sent as
/// `applyPrefabOverride` to the variant document, lands as an override of
/// the variant's root (translated to the base's ids) or as a plain child
/// of the root; the instance without its overrides then looks exactly as
/// it did with them. Removing what the variant added drops its override.
#[test]
fn apply_prefab_override_writes_into_a_variant() {
    use fynkel_document::prefab::{derive_id, new_variant};
    let s = schemas();
    let mut lib = library();
    let vi = u("61000000-0000-4000-8000-0000000000e1");
    let doc = new_variant(
        "Turret Variant",
        &AssetRef::new(TURRET.parse().unwrap()),
        "Base",
        Some(vi),
        vec![Node::from_serde(
            &json!({ "op": "setField", "entity": T_BASE,
            "component": T_RENDERER, "field": "cast_shadows", "value": false }),
        )],
    );
    let variant_id = doc.envelope.id.to_string();
    let mut variant = Editor::open(doc, &s).unwrap();
    let original = text(&variant);
    let vr = Prefab::from_document(variant.document().clone()).root;
    assert_eq!(
        Prefab::from_document(variant.document().clone())
            .variant_of()
            .unwrap()
            .prefab_id(),
        u(TURRET)
    );
    lib.add(Prefab::from_document(variant.document().clone()));

    // The scene's instance, overridden in the variant's (flat) ids.
    let barrel = derive_id(vi, u(T_BARREL)).to_string();
    let barrel_t = derive_id(vi, u(T_BARREL_T)).to_string();
    let renderer = derive_id(vi, u(T_RENDERER)).to_string();
    let light_id = "61000000-0000-4000-8000-0000000000c7";
    let muzzle = "61000000-0000-4000-8000-0000000000b7";
    let tag = "61000000-0000-4000-8000-0000000000b8";
    let tag_t = "61000000-0000-4000-8000-0000000000c8";
    let mut add_light = light();
    add_light["op"] = json!("addComponent");
    add_light["entity"] = json!(barrel);
    add_light["component"] = json!(light_id);
    let ops = json!([
        { "op": "setField", "entity": vr.to_string(), "component": renderer, "field": "visible", "value": false },
        { "op": "setField", "entity": barrel, "component": barrel_t, "field": "position", "value": [0, 7, 0] },
        add_light,
        { "op": "addObject", "entity": muzzle, "name": "Muzzle", "parent": barrel,
          "components": { "61000000-0000-4000-8000-0000000000c9": full_transform(1.0) } },
        { "op": "addObject", "entity": tag, "name": "Tag", "parent": vr.to_string(),
          "components": { tag_t: full_transform(3.0) } },
        { "op": "setActive", "entity": barrel, "value": false }
    ]);
    let anchor = "61000000-0000-4000-8000-0000000000a9";
    let instance = "61000000-0000-4000-8000-0000000000e9";
    let with = one_instance(anchor, instance, &variant_id, ops.clone());
    let before = shape(&with.scene(), &lib);

    for op in ops.as_array().unwrap() {
        let r = variant.apply_with_prefabs(
            Request::new(vec![Command::ApplyPrefabOverride {
                r#override: Node::from_serde(op),
            }]),
            &s,
            &lib,
        );
        assert!(r.ok, "{op}: {:#?}", r.all_diagnostics());
    }
    let p = Prefab::from_document(variant.document().clone());
    assert!(p.validate(&s).is_empty(), "{:#?}", p.validate(&s));
    let overrides = &p.variant_of().unwrap().overrides;
    let names: Vec<&str> = overrides.iter().map(|o| o.op.name()).collect();
    assert_eq!(
        names,
        [
            "setField",
            "setField",
            "setField",
            "addComponent",
            "addObject",
            "setActive"
        ],
        "in the base's ids, after the variant's own"
    );
    assert_eq!(overrides[1].op.entity(), u(T_BASE));
    assert_eq!(overrides[2].op.entity(), u(T_BARREL));
    // The object added under the root is the variant's own child, same id.
    assert_eq!(p.entity(u(tag)).unwrap().parent, Some(vr));

    // Applied: the instance without overrides looks the same.
    lib.add(p.clone());
    let without = one_instance(anchor, instance, &variant_id, json!([]));
    assert_eq!(shape(&without.scene(), &lib), before);

    // Removing what the variant added drops its override.
    let muzzle_now = derive_id(vi, u(muzzle)).to_string();
    let light_now = derive_id(vi, u(light_id)).to_string();
    for op in [
        json!({ "op": "removeObject", "entity": muzzle_now }),
        json!({ "op": "removeComponent", "entity": barrel, "component": light_now }),
        // A plain child of the variant: deleted.
        json!({ "op": "removeObject", "entity": tag }),
    ] {
        let r = variant.apply_with_prefabs(
            Request::new(vec![Command::ApplyPrefabOverride {
                r#override: Node::from_serde(&op),
            }]),
            &s,
            &lib,
        );
        assert!(r.ok, "{op}: {:#?}", r.all_diagnostics());
    }
    let p = Prefab::from_document(variant.document().clone());
    let names: Vec<&str> = p
        .variant_of()
        .unwrap()
        .overrides
        .iter()
        .map(|o| o.op.name())
        .collect();
    assert_eq!(names, ["setField", "setField", "setField", "setActive"]);
    assert!(p.entity(u(tag)).is_none());

    // An op on something the variant does not have is refused.
    let r = variant.apply_with_prefabs(
        Request::new(vec![Command::ApplyPrefabOverride {
            r#override: Node::from_serde(&json!({ "op": "removeObject",
                "entity": "61000000-0000-4000-8000-0000000000ff" })),
        }]),
        &s,
        &lib,
    );
    assert!(!r.ok);
    while variant.can_undo() {
        variant.undo().unwrap();
    }
    assert_eq!(text(&variant), original);
    let c = Command::ApplyPrefabOverride {
        r#override: Node::from_serde(&ops[0]),
    };
    assert_eq!(Command::from_json(&c.to_json()).unwrap(), c);
}

/// Unpack (one level) and Unpack Completely (ADR-0022 amendment
/// 2026-09-25): an instance of a prefab holding a nested instance becomes
/// plain entities with the nested instance kept (its overrides first, then
/// the outer instance's that land inside it, in its prefab's ids), or plain
/// entities only; an instance of a variant becomes an instance of the base.
/// The scene looks the same after each; one transaction each.
#[test]
fn unpack_one_level_keeps_nested_instances_and_completely_does_not() {
    use fynkel_document::prefab::derive_id;
    let s = schemas();
    let mut lib = library();
    const HOLDER: &str = "62000000-0000-4000-8000-000000000001";
    let h_root = "62000000-0000-4000-8000-0000000000a1";
    let h_t = "62000000-0000-4000-8000-0000000000c1";
    let gun = "62000000-0000-4000-8000-0000000000a2";
    let gi = u("62000000-0000-4000-8000-0000000000e2");
    let scope = "62000000-0000-4000-8000-0000000000a3";
    lib.add(prefab_of(json!({
        "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
        "formatVersion": 5, "id": HOLDER, "name": "Holder", "root": h_root,
        "entities": {
            h_root: { "name": "Holder", "parent": null, "children": [gun],
                      "components": { h_t: full_transform(0.0) } },
            gun: { "name": "Gun", "parent": h_root, "children": [scope], "components": {},
                   "prefab": { "asset": { "asset": TURRET }, "instance": gi.to_string(), "overrides": [
                       { "op": "setField", "entity": T_BARREL, "component": T_BARREL_T, "field": "position", "value": [0, 4, 0] }
                   ] } },
            scope: { "name": "Scope", "parent": gun, "children": [],
                     "components": { "62000000-0000-4000-8000-0000000000c3": full_transform(0.5) } }
        }
    })));
    let barrel = derive_id(gi, u(T_BARREL)).to_string();
    let ops = json!([
        { "op": "setField", "entity": h_root, "component": h_t, "field": "position", "value": [1, 0, 0] },
        { "op": "setField", "entity": barrel, "component": derive_id(gi, u(T_BARREL_T)).to_string(),
          "field": "position", "value": [0, 9, 0] },
        { "op": "addObject", "entity": "62000000-0000-4000-8000-0000000000b5", "name": "Lamp", "parent": barrel,
          "components": { "62000000-0000-4000-8000-0000000000c5": full_transform(0.1) } },
        { "op": "setField", "entity": gun, "component": derive_id(gi, u(T_RENDERER)).to_string(),
          "field": "cast_shadows", "value": false }
    ]);
    let anchor = "62000000-0000-4000-8000-0000000000aa";
    let j = u("62000000-0000-4000-8000-0000000000ea");
    let mut one = one_instance(anchor, &j.to_string(), HOLDER, ops.clone());
    let original = text(&one);
    let before = shape(&one.scene(), &lib);

    // One level: the Gun stays an instance, with the moved barrel, the
    // Lamp and the shadows in the Turret's ids.
    let r = one.apply_with_prefabs(
        Request::new(vec![Command::UnpackPrefab {
            entity: u(anchor),
            completely: false,
        }]),
        &s,
        &lib,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    assert_eq!(instances_in(&one), 1);
    let sc = one.scene();
    assert!(sc.entity(u(anchor)).unwrap().prefab.is_none());
    let new_gun = derive_id(j, u(gun));
    let kept = sc.entity(new_gun).unwrap().prefab.as_ref().unwrap();
    assert_eq!(kept.instance, derive_id(j, gi));
    let targets: Vec<(&str, Uuid)> = kept
        .overrides
        .iter()
        .map(|o| (o.op.name(), o.op.entity()))
        .collect();
    assert_eq!(
        targets,
        [
            ("setField", u(T_BARREL)),
            ("setField", u(T_BARREL)),
            ("addObject", u("62000000-0000-4000-8000-0000000000b5")),
            ("setField", u(T_BASE)),
        ]
    );
    assert_eq!(
        sc.entity(derive_id(j, u(scope))).unwrap().parent,
        Some(new_gun)
    );
    assert_eq!(shape(&sc, &lib), before);
    one.undo().unwrap();
    assert_eq!(text(&one), original);

    // Completely: plain entities only, the same scene.
    let r = one.apply_with_prefabs(
        Request::new(vec![Command::UnpackPrefab {
            entity: u(anchor),
            completely: true,
        }]),
        &s,
        &lib,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    assert_eq!(instances_in(&one), 0);
    assert_eq!(shape(&one.scene(), &lib), before);
    assert_eq!(one.scene().entities.len(), 5);

    // An instance of a variant, one level: an instance of the base with
    // the variant's overrides then its own; the variant's own child plain.
    const VARIANT: &str = "62000000-0000-4000-8000-000000000002";
    let v_root = "62000000-0000-4000-8000-0000000000d1";
    let vi = u("62000000-0000-4000-8000-0000000000e3");
    let flag = "62000000-0000-4000-8000-0000000000d2";
    lib.add(prefab_of(json!({
        "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
        "formatVersion": 5, "id": VARIANT, "name": "Turret Variant", "root": v_root,
        "entities": {
            v_root: { "name": "Base", "parent": null, "children": [flag], "components": {},
                "prefab": { "asset": { "asset": TURRET }, "instance": vi.to_string(), "overrides": [
                    { "op": "setField", "entity": T_BASE, "component": T_RENDERER, "field": "cast_shadows", "value": false }
                ] } },
            flag: { "name": "Flag", "parent": v_root, "children": [],
                    "components": { "62000000-0000-4000-8000-0000000000d3": full_transform(2.0) } }
        }
    })));
    let ops = json!([
        { "op": "setField", "entity": derive_id(vi, u(T_BARREL)).to_string(),
          "component": derive_id(vi, u(T_BARREL_T)).to_string(), "field": "position", "value": [0, 6, 0] }
    ]);
    let mut v = one_instance(anchor, &j.to_string(), VARIANT, ops);
    let before = shape(&v.scene(), &lib);
    let r = v.apply_with_prefabs(
        Request::new(vec![Command::UnpackPrefab {
            entity: u(anchor),
            completely: false,
        }]),
        &s,
        &lib,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    let sc = v.scene();
    let now = sc.entity(u(anchor)).unwrap().prefab.as_ref().unwrap();
    assert_eq!(now.prefab_id(), u(TURRET));
    assert_eq!(now.instance, derive_id(j, vi));
    assert_eq!(now.overrides.len(), 2);
    assert_eq!(
        sc.entity(u(anchor)).unwrap().children,
        vec![derive_id(j, u(flag))]
    );
    assert!(sc.validate(&s).is_empty(), "{:#?}", sc.validate(&s));
    assert_eq!(shape(&sc, &lib), before);

    // JSON: `completely` round-trips and defaults to one level.
    let c = Command::UnpackPrefab {
        entity: u(anchor),
        completely: true,
    };
    assert_eq!(Command::from_json(&c.to_json()).unwrap(), c);
    assert_eq!(
        Command::from_json(&json!({ "command": "unpackPrefab", "entity": anchor })).unwrap(),
        Command::UnpackPrefab {
            entity: u(anchor),
            completely: false
        }
    );
}

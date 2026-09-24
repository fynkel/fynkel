//! Physics materials (ADR-0025): `.physicsmaterial.json` read into
//! physics' `PhysicsMaterial` — absent fields take Unity's defaults — and
//! checked by the schema's ranges.

use std::path::Path;

use fynkel_document::{
    Code, Command, Document, DocumentKind, Editor, Node, PhysicsMaterialDocument, Request,
    SchemaSet,
};
use fynkel_physics::{CombineRule, PhysicsMaterial};

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn doc(body: &str) -> PhysicsMaterialDocument {
    let text = format!(
        r#"{{"$schema":"https://fynkel.dev/schemas/physics_material.schema.json","formatVersion":1,"id":"c0000000-0000-4000-8000-000000000801","name":"Ice"{body}}}"#
    );
    PhysicsMaterialDocument::from_document(
        Document::parse(
            DocumentKind::PhysicsMaterial,
            Some("ice.physicsmaterial.json"),
            &text,
        )
        .unwrap(),
    )
}

fn codes(v: &PhysicsMaterialDocument) -> Vec<(String, String)> {
    v.validate(&schemas())
        .iter()
        .map(|d| (d.code.0.to_string(), d.pointer.clone().unwrap_or_default()))
        .collect()
}

#[test]
fn fields_read_and_absent_ones_take_unitys_defaults() {
    let empty = doc("");
    assert_eq!(empty.material, PhysicsMaterial::default());
    assert_eq!(
        (empty.material.friction, empty.material.bounciness),
        (0.6, 0.0)
    );
    assert_eq!(codes(&empty), []);

    let ice = doc(
        r#","friction":0,"bounciness":0.25,"friction_combine":"minimum","bounce_combine":"maximum""#,
    );
    assert_eq!(
        ice.material,
        PhysicsMaterial {
            friction: 0.0,
            bounciness: 0.25,
            friction_combine: CombineRule::Minimum,
            bounce_combine: CombineRule::Maximum,
        }
    );
    assert_eq!(codes(&ice), []);
    assert_eq!(
        DocumentKind::from_path(Path::new("a/ice.physicsmaterial.json")),
        Some(DocumentKind::PhysicsMaterial)
    );
    assert_eq!(
        DocumentKind::parse_name("physics_material"),
        Some(DocumentKind::PhysicsMaterial)
    );
    // Not a `.material.json` (the render material's suffix).
    assert_eq!(
        DocumentKind::from_path(Path::new("a/ice.material.json")),
        Some(DocumentKind::Material)
    );
}

#[test]
fn the_schema_holds_the_ranges_and_the_view_reads_what_it_can() {
    let d = |p: &str| ("FYN-D0100".to_string(), p.to_string());
    assert_eq!(codes(&doc(r#","friction":-0.1"#)), [d("/friction")]);
    assert_eq!(codes(&doc(r#","bounciness":1.5"#)), [d("/bounciness")]);
    assert_eq!(
        codes(&doc(r#","friction_combine":"max""#)),
        [d("/friction_combine")]
    );
    assert_eq!(
        codes(&doc(r#","static_friction":0.5"#)),
        [d("/static_friction")]
    );
    // Out of range or of the wrong type: the schema says so once, and the
    // view keeps the default.
    let wrong = doc(r#","bounciness":"lots","bounce_combine":"sum""#);
    assert_eq!(wrong.material.bounciness, 0.0);
    assert_eq!(wrong.material.bounce_combine, CombineRule::Average);
    assert_eq!(wrong.read_diagnostics().len(), 2);
    assert!(wrong
        .read_diagnostics()
        .iter()
        .all(|x| x.code == Code::PHYSICS_MATERIAL_MALFORMED_ENTRY));
    assert_eq!(codes(&wrong), [d("/bounciness"), d("/bounce_combine")]);
    // A new document spells Unity's defaults out and validates.
    let fresh = Document::new(DocumentKind::PhysicsMaterial, "Bouncy");
    let text = fresh.write().unwrap();
    assert!(text.contains("\"friction\": 0.6"), "{text}");
    assert!(text.contains("\"bounce_combine\": \"average\""), "{text}");
    let fresh = PhysicsMaterialDocument::from_document(fresh);
    assert_eq!(fresh.material, PhysicsMaterial::default());
    assert_eq!(codes(&fresh), []);
}

#[test]
fn physics_materials_edit_through_document_fields() {
    let s = schemas();
    let doc = Document::new(DocumentKind::PhysicsMaterial, "Bouncy");
    let original = doc.write().unwrap();
    let mut editor = Editor::open(doc, &s).unwrap();
    let set = |pointer: &str, v: serde_json::Value| Command::SetDocumentField {
        pointer: pointer.into(),
        value: Node::from_serde(&v),
    };
    let r = editor.apply(
        Request::new(vec![
            set("/bounciness", serde_json::json!(1)),
            set("/bounce_combine", serde_json::json!("maximum")),
        ]),
        &s,
    );
    assert!(r.ok, "{:#?}", r.all_diagnostics());
    let view = PhysicsMaterialDocument::from_document(editor.document().clone());
    assert_eq!(view.material.bounciness, 1.0);
    assert_eq!(view.material.bounce_combine, CombineRule::Maximum);
    // The schema's ranges refuse the transaction whole.
    let r = editor.apply(
        Request::new(vec![set("/friction_combine", serde_json::json!("sum"))]),
        &s,
    );
    assert!(!r.ok);
    let r = editor.apply(
        Request::new(vec![set("/friction", serde_json::json!(-1))]),
        &s,
    );
    assert!(!r.ok);
    editor.undo().unwrap();
    assert_eq!(editor.document().write().unwrap(), original);
}

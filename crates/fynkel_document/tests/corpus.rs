//! Round-trip corpus (blueprint §19.2c item 2):
//! - `fixtures/valid/*`: parse, validate clean, and the canonical writer is
//!   a fixed point (the files are stored canonical).
//! - `fixtures/unknown/*`: unknown keys/lexemes survive a rewrite byte-for-
//!   byte; `*.expected.json` gives the rewritten form when hoisting applies.
//! - `fixtures/invalid/*`: `*.expected.json` lists the diagnostics (code,
//!   pointer, line, column) that must be reported, in order.

use std::path::{Path, PathBuf};

use fynkel_document::{Diagnostic, Document, DocumentKind, SchemaSet};

fn fixtures(sub: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(sub);
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| !p.to_string_lossy().ends_with(".expected.json"))
        .collect();
    v.sort();
    assert!(!v.is_empty(), "no fixtures in {}", dir.display());
    v
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn only_errors(list: &[Diagnostic]) -> Vec<&Diagnostic> {
    list.iter().filter(|d| d.is_error()).collect()
}

#[test]
fn valid_documents_parse_validate_and_are_canonical() {
    let schemas = schemas();
    for path in fixtures("valid") {
        let text = std::fs::read_to_string(&path).unwrap();
        let doc =
            Document::read(&path, None).unwrap_or_else(|d| panic!("{}: {d:?}", path.display()));
        let diags = doc.validate(&schemas);
        assert!(
            only_errors(&diags).is_empty(),
            "{}: {diags:#?}",
            path.display()
        );
        assert_eq!(
            doc.write().unwrap(),
            text,
            "{} is not canonical",
            path.display()
        );
        assert!(!doc.is_read_only());
    }
}

#[test]
fn unknown_data_round_trips_byte_for_byte() {
    for path in fixtures("unknown") {
        let text = std::fs::read_to_string(&path).unwrap();
        let expected_path = path.to_string_lossy().replace(".json", ".expected.json");
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|_| text.clone());
        let doc =
            Document::read(&path, None).unwrap_or_else(|d| panic!("{}: {d:?}", path.display()));
        let written = doc.write().unwrap();
        assert_eq!(written, expected, "{}", path.display());
        // And the rewritten form is a fixed point.
        let again = Document::parse(doc.kind, None, &written).unwrap();
        assert_eq!(again.write().unwrap(), written);
        assert_eq!(again.envelope, doc.envelope);
    }
}

#[test]
fn invalid_documents_report_exact_diagnostics() {
    let schemas = schemas();
    for path in fixtures("invalid") {
        let expected_path = path.to_string_lossy().replace(".json", ".expected.json");
        let expected: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(&expected_path).unwrap()).unwrap();
        let diags = match Document::read(&path, None) {
            Ok(doc) => doc.validate(&schemas),
            Err(d) => d,
        };
        let shown = format!(
            "{}:\n{}",
            path.display(),
            diags.iter().map(|d| format!("  {d}\n")).collect::<String>()
        );
        assert_eq!(diags.len(), expected.len(), "{shown}");
        for (d, e) in diags.iter().zip(&expected) {
            assert_eq!(d.code.as_str(), e["code"].as_str().unwrap(), "{shown}");
            if let Some(p) = e.get("pointer") {
                assert_eq!(d.pointer.as_deref(), p.as_str(), "{shown}");
            }
            if let Some(l) = e.get("line") {
                assert_eq!(d.line, l.as_u64().map(|v| v as u32), "{shown}");
            }
            if let Some(c) = e.get("column") {
                assert_eq!(d.column, c.as_u64().map(|v| v as u32), "{shown}");
            }
            assert_eq!(
                d.document.as_deref(),
                Some(path.to_string_lossy().as_ref()),
                "{shown}"
            );
        }
    }
}

#[test]
fn newer_documents_are_read_only() {
    let text =
        "{\"formatVersion\": 8, \"id\": \"70d7ba8b-f7fb-4eba-920d-ec9e6b968330\", \"name\": \"x\"}";
    let err = Document::parse(DocumentKind::Scene, None, text).unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-D0013");
}

#[test]
fn schema_violations_carry_pointer_and_position() {
    let schemas = SchemaSet::envelope_only();
    let text = "{\n  \"formatVersion\": 7,\n  \"id\": \"70d7ba8b-f7fb-4eba-920d-ec9e6b968330\",\n  \"name\": \"x\",\n  \"ref\": { \"asset\": \"nope\" }\n}\n";
    let doc = Document::parse(DocumentKind::Scene, Some("mem.scene.json"), text).unwrap();
    // No scene schema in the embedded set: warning, then envelope-only validation passes.
    let diags = doc.validate(&schemas);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code.as_str(), "FYN-D0101");
    assert_eq!(diags[0].document.as_deref(), Some("mem.scene.json"));

    // A schema that uses the envelope's $defs through $ref and forbids extras.
    let dir = std::env::temp_dir().join(format!("fynkel-doc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("scene.schema.json"),
        r##"{
          "$schema": "https://json-schema.org/draft/2020-12/schema",
          "$id": "https://fynkel.dev/schemas/scene.schema.json",
          "allOf": [{ "$ref": "https://fynkel.dev/schemas/envelope.schema.json" }],
          "properties": {
            "$schema": true, "formatVersion": true, "id": true, "name": true,
            "ref": { "$ref": "https://fynkel.dev/schemas/envelope.schema.json#/$defs/assetRef" }
          },
          "unevaluatedProperties": false
        }"##,
    )
    .unwrap();
    let schemas = SchemaSet::load(&dir).unwrap();
    let diags = doc.validate(&schemas);
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(diags[0].code.as_str(), "FYN-D0100");
    assert_eq!(diags[0].pointer.as_deref(), Some("/ref/asset"));
    assert_eq!((diags[0].line, diags[0].column), (Some(5), Some(12)));

    let text = text.replace("\"ref\"", "\"reff\"");
    let doc = Document::parse(DocumentKind::Scene, None, &text).unwrap();
    let diags = doc.validate(&schemas);
    assert!(
        diags.iter().any(|d| d.pointer.as_deref() == Some("/reff")),
        "{diags:#?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_documents_are_valid_and_canonical() {
    let schemas = SchemaSet::embedded();
    let doc = Document::new(DocumentKind::Material, "Fresh");
    assert!(only_errors(&doc.validate(&schemas)).is_empty());
    let text = doc.write().unwrap();
    let again = Document::parse(DocumentKind::Material, None, &text).unwrap();
    assert_eq!(again.envelope, doc.envelope);
    assert!(text.starts_with("{\n  \"$schema\": \"https://fynkel.dev/schemas/material.schema.json\",\n  \"formatVersion\": 2,\n  \"id\": \""));
}

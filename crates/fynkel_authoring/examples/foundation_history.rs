//! Inspector and transaction history in a generated 10k-asset project.
use fynkel_authoring::AuthoringSession;
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let project = PathBuf::from(std::env::args().nth(1).expect("project directory"));
    let mut session = AuthoringSession::new();
    let assets = session.open_project(&project).unwrap().assets.len();
    let path = project.join("assets/scenes/s0000.scene.json");
    let disk_before = std::fs::read(&path).unwrap();
    let handle = session.open_document(path.to_str().unwrap()).unwrap();
    let before = session
        .document(handle)
        .unwrap()
        .editor
        .document()
        .write()
        .unwrap();
    let doc = session.document_json(handle).unwrap();
    let ids: Vec<_> = doc["root"]["entities"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut apply = Vec::new();
    let mut inspect = Vec::new();
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for sample in 0..43 {
        let commands: Vec<_> = ids.iter().enumerate().map(|(i,id)| {
            json!({"command":"renameObject","id":id,"name":format!("edited {sample}/{i}")})
        }).collect();
        let start = Instant::now();
        let result = session.apply_json(handle, &json!(commands)).unwrap();
        let applied_us = start.elapsed().as_micros();
        assert_eq!(result["ok"], true);
        let after = session
            .document(handle)
            .unwrap()
            .editor
            .document()
            .write()
            .unwrap();
        let start = Instant::now();
        std::hint::black_box(session.inspect_json(handle).unwrap());
        std::hint::black_box(session.entities_json(handle, &ids).unwrap());
        let inspect_us = start.elapsed().as_micros();
        let start = Instant::now();
        session.undo(handle).unwrap();
        let undo_us = start.elapsed().as_micros();
        assert_eq!(
            session
                .document(handle)
                .unwrap()
                .editor
                .document()
                .write()
                .unwrap(),
            before
        );
        let start = Instant::now();
        session.redo(handle).unwrap();
        let redo_us = start.elapsed().as_micros();
        assert_eq!(
            session
                .document(handle)
                .unwrap()
                .editor
                .document()
                .write()
                .unwrap(),
            after
        );
        session.undo(handle).unwrap();
        if sample >= 3 {
            apply.push(applied_us);
            inspect.push(inspect_us);
            undo.push(undo_us);
            redo.push(redo_us);
        }
    }
    assert_eq!(
        std::fs::read(path).unwrap(),
        disk_before,
        "qualification never saves authored content"
    );
    for (workload, samples) in [
        ("apply", apply),
        ("inspect", inspect),
        ("undo", undo),
        ("redo", redo),
    ] {
        println!(
            "{}",
            json!({"workload":workload,"assets":assets,"scene_entities":ids.len(),"samples_us":samples,
            "scope":"native authoring transactions and inspector serialization in 10k-asset project; excludes Flutter UI layout"})
        );
    }
}

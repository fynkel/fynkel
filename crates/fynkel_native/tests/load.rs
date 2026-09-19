//! `fynkel_world_load_scene` / `_load_project` / `_load_report` over the
//! fixture project: a scene file and a project directory go into the world
//! through the ABI; bad input never crashes and leaves the world alone.

use std::path::{Path, PathBuf};
use std::ptr;

use fynkel_contracts::Status;
use fynkel_native::load::*;
use fynkel_native::*;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

struct Host {
    rt: *mut FynkelRuntime,
    w: *mut FynkelWorld,
}

impl Host {
    fn new() -> Self {
        let rt = fynkel_runtime_create();
        let w = unsafe { fynkel_world_create(rt, ptr::null()) };
        assert!(!w.is_null());
        Self { rt, w }
    }

    fn report(&self) -> serde_json::Value {
        let v = unsafe { fynkel_world_load_report(self.w) };
        if v.data.is_null() || v.length == 0 {
            return serde_json::Value::Null;
        }
        let bytes = unsafe { std::slice::from_raw_parts(v.data, v.length as usize) };
        serde_json::from_slice(bytes).unwrap()
    }

    fn live(&self) -> u32 {
        let mut s = FynkelWorldStats::default();
        assert_eq!(unsafe { fynkel_world_stats(self.w, &mut s) }, Status::Ok);
        s.live_entities
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            fynkel_world_destroy(self.w);
            fynkel_runtime_destroy(self.rt);
        }
    }
}

#[test]
fn project_and_scene_load_through_the_abi() {
    let host = Host::new();
    assert!(host.report().is_null(), "no report before any load");

    let dir = fixture().display().to_string();
    let s = unsafe { fynkel_world_load_project(host.w, dir.as_ptr(), dir.len() as u32) };
    assert_eq!(s, Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true);
    assert_eq!(r["project"]["name"], "Fixture Game");
    assert!(r["project"]["cache"]
        .as_str()
        .unwrap()
        .ends_with(".fynkel/cache"));
    assert_eq!(r["scene"]["name"], "Main");
    assert_eq!(r["scene"]["entities"], 4);
    assert_eq!(r["scene"]["rejectedComponents"], 0);
    assert_eq!(host.live(), 4);

    // The same scene again as a bare file: prefabs are found next to it.
    let path = fixture()
        .join("assets/scenes/main.scene.json")
        .display()
        .to_string();
    let s = unsafe { fynkel_world_load_scene(host.w, path.as_ptr(), path.len() as u32) };
    assert_eq!(
        s,
        Status::LoadFailed,
        "the scene's prefab is not next to it"
    );
    let r = host.report();
    assert_eq!(r["ok"], false);
    assert_eq!(r["diagnostics"][0]["code"], "FYN-D0401", "unknown prefab");
    assert_eq!(host.live(), 4, "a failed load leaves the world alone");
    // Loading again adds a second instance of the scene (ids are per load).
    let s = unsafe { fynkel_world_load_project(host.w, dir.as_ptr(), dir.len() as u32) };
    assert_eq!(s, Status::Ok);
    assert_eq!(host.live(), 8);

    // A prefab-free scene loads as a bare file into a fresh world.
    let fresh = Host::new();
    let path = fixture()
        .join("assets/prefabs/crate.prefab.json")
        .display()
        .to_string();
    let s = unsafe { fynkel_world_load_scene(fresh.w, path.as_ptr(), path.len() as u32) };
    assert_eq!(s, Status::LoadFailed, "a prefab is not a scene");
    assert_eq!(fresh.live(), 0);
    let path = fixture().join("assets/scenes/lone.scene.json");
    std::fs::write(
        &path,
        r#"{"$schema":"https://fynkel.dev/schemas/scene.schema.json","formatVersion":1,
            "id":"c0000000-0000-4000-8000-000000000031","name":"Lone","entities":{
            "c0000000-0000-4000-8000-0000000000e9":{"name":"Solo","parent":null,"children":[],"components":{}}}}"#,
    )
    .unwrap();
    let text = path.display().to_string();
    let s = unsafe { fynkel_world_load_scene(fresh.w, text.as_ptr(), text.len() as u32) };
    let _ = std::fs::remove_file(&path);
    assert_eq!(s, Status::Ok);
    assert_eq!(fresh.report()["project"], serde_json::Value::Null);
    assert_eq!(fresh.report()["scene"]["name"], "Lone");
    assert_eq!(fresh.live(), 1);
}

#[test]
fn bad_input_is_reported_not_fatal() {
    let host = Host::new();
    assert_eq!(
        unsafe { fynkel_world_load_scene(host.w, ptr::null(), 0) },
        Status::InvalidArgument
    );
    let bad = [0xffu8, 0xfe];
    assert_eq!(
        unsafe { fynkel_world_load_project(host.w, bad.as_ptr(), 2) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_world_load_project(ptr::null_mut(), bad.as_ptr(), 0) },
        Status::InvalidArgument
    );

    let missing = "/nonexistent/fynkel/project";
    let s = unsafe { fynkel_world_load_project(host.w, missing.as_ptr(), missing.len() as u32) };
    assert_eq!(s, Status::LoadFailed);
    let r = host.report();
    assert_eq!(r["diagnostics"][0]["code"], "FYN-P0001");
    assert_eq!(host.live(), 0);
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
}

//! `fynkel_world_load_project` / `_load_scene` / `_load_report` over the
//! fixture project's cooked cache: a host reads the cache only (M3-03) —
//! the startup scene by the index, any scene by id, residency retained on
//! the world; bad input never crashes and leaves the world alone.

use std::path::{Path, PathBuf};
use std::ptr;

use fynkel_contracts::Status;
use fynkel_import::{cook_project, CookOptions};
use fynkel_native::load::*;
use fynkel_native::*;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

/// The fixture project cooked into a cache of its own (removed on drop).
struct Cooked {
    dir: PathBuf,
}

impl Cooked {
    fn new(name: &str) -> Cooked {
        let dir = std::env::temp_dir().join(format!("fynkel-native-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let report = cook_project(&fixture(), Some(&dir), &CookOptions::default())
            .unwrap_or_else(|d| panic!("{d:?}"));
        assert!(!report.has_errors(), "{}", report.text());
        Cooked { dir }
    }

    fn text(&self) -> String {
        self.dir.display().to_string()
    }
}

impl Drop for Cooked {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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

    fn load_project(&self, cache: &str) -> Status {
        unsafe { fynkel_world_load_project(self.w, cache.as_ptr(), cache.len() as u32) }
    }

    fn load_scene(&self, cache: &str, id: &str) -> Status {
        unsafe {
            fynkel_world_load_scene(
                self.w,
                cache.as_ptr(),
                cache.len() as u32,
                id.as_ptr(),
                id.len() as u32,
            )
        }
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

const MAIN_SCENE: &str = "c0000000-0000-4000-8000-000000000030";

#[test]
fn project_and_scene_load_through_the_abi() {
    let cooked = Cooked::new("load");
    let cache = cooked.text();
    let host = Host::new();
    assert!(host.report().is_null(), "no report before any load");

    assert_eq!(host.load_project(&cache), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true);
    assert_eq!(r["cache"]["project"], "Fixture Game");
    assert_eq!(r["cache"]["dir"], cache);
    assert_eq!(r["cache"]["profile"], "dev");
    assert_eq!(r["cache"]["startupScene"], MAIN_SCENE);
    assert_eq!(r["scene"]["name"], "Main");
    assert_eq!(r["scene"]["id"], MAIN_SCENE);
    assert_eq!(r["scene"]["path"], "scenes/main.scene.json");
    assert_eq!(r["scene"]["key"].as_str().unwrap().len(), 64);
    // Camera, Robot, the "Crate A" instance and the crate it expands to —
    // expanded by the cook, not here.
    assert_eq!(r["scene"]["entities"], 4);
    assert_eq!(r["scene"]["rejectedComponents"], 0);
    // The scene, its prefab, the robot, the material and its texture.
    assert_eq!(r["scene"]["assets"], 5);
    assert_eq!(r["residency"]["retained"], 5);
    assert_eq!(r["residency"]["meshes"], 0, "nothing loaded until drawn");
    assert_eq!(host.live(), 4);
    // The Robot's `game.Spinner` is a Dart behaviour: known through the
    // cook's record of the project's registry, so no warning, and handed
    // to the SDK with its authored data and the spawned entity's handle.
    assert_eq!(r["diagnostics"].as_array().unwrap().len(), 0, "{r:#}");
    let behaviours = r["scene"]["behaviours"].as_array().unwrap();
    assert_eq!(behaviours.len(), 1);
    let b = &behaviours[0];
    assert_eq!(b["type"], "game.Spinner");
    assert_eq!(b["version"], 1);
    assert_eq!(b["known"], true);
    assert_eq!(b["data"]["speed"], 2.5);
    assert_eq!(b["authoringId"], "c0000000-0000-4000-8000-0000000000e1");
    assert_eq!(b["component"], "c0000000-0000-4000-8000-0000000000c2");
    assert!(b["entity"].as_u64().unwrap() != 0);

    // The same scene by id: a second instance (ids are per load), and the
    // residency count of its closure goes up, not the store's size.
    assert_eq!(host.load_scene(&cache, MAIN_SCENE), Status::Ok);
    assert_eq!(host.live(), 8);
    assert_eq!(host.report()["residency"]["retained"], 5);

    // Not a scene, not in the cache, not a UUID.
    let prefab = "c0000000-0000-4000-8000-000000000040";
    assert_eq!(host.load_scene(&cache, prefab), Status::LoadFailed);
    let r = host.report();
    assert_eq!(r["ok"], false);
    assert_eq!(r["diagnostics"][0]["code"], "FYN-R0006");
    assert_eq!(host.live(), 8, "a failed load leaves the world alone");
    let unknown = "c0000000-0000-4000-8000-0000000000ff";
    assert_eq!(host.load_scene(&cache, unknown), Status::LoadFailed);
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0003");
    assert_eq!(host.load_scene(&cache, "robot"), Status::InvalidArgument);
}

#[test]
fn the_cache_is_the_only_source() {
    // A cache cooked from the fixture, then the fixture's source tree is
    // not consulted at all: a copy of the cache alone loads.
    let cooked = Cooked::new("only");
    let moved = std::env::temp_dir().join(format!("fynkel-native-moved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&moved);
    copy_dir(&cooked.dir, &moved);
    drop(cooked);
    let host = Host::new();
    let cache = moved.display().to_string();
    assert_eq!(host.load_project(&cache), Status::Ok);
    assert_eq!(host.live(), 4);

    // A stale cache (another fingerprint) is refused, never re-read from
    // source; a missing one says to cook.
    let index = moved.join("index.json");
    let text = std::fs::read_to_string(&index).unwrap();
    let re = regex_replace(&text, "\"schemaFingerprint\": \"", "0xdeadbeefdeadbeef");
    std::fs::write(&index, re).unwrap();
    let fresh = Host::new();
    assert_eq!(fresh.load_project(&cache), Status::LoadFailed);
    assert_eq!(fresh.report()["diagnostics"][0]["code"], "FYN-R0002");
    let _ = std::fs::remove_dir_all(&moved);
    assert_eq!(fresh.load_project(&cache), Status::LoadFailed);
    let r = fresh.report();
    assert_eq!(r["diagnostics"][0]["code"], "FYN-R0001");
    assert!(r["diagnostics"][0]["suggestion"]
        .as_str()
        .unwrap()
        .contains("fynkel cook"));
    assert_eq!(fresh.live(), 0);
}

/// Replaces the quoted value after `key` with `value`.
fn regex_replace(text: &str, key: &str, value: &str) -> String {
    let start = text.find(key).unwrap() + key.len();
    let end = start + text[start..].find('"').unwrap();
    format!("{}{}{}", &text[..start], value, &text[end..])
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn bad_input_is_reported_not_fatal() {
    let host = Host::new();
    assert_eq!(
        unsafe { fynkel_world_load_scene(host.w, ptr::null(), 0, ptr::null(), 0) },
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

    let missing = "/nonexistent/fynkel/cache";
    assert_eq!(host.load_project(missing), Status::LoadFailed);
    let r = host.report();
    assert_eq!(r["diagnostics"][0]["code"], "FYN-R0001");
    assert_eq!(host.live(), 0);
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
}

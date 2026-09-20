//! `fynkel_world_load_project` / `_load_scene` / `_load_report` over the
//! fixture project's cooked cache: a host reads the cache only (M3-03) —
//! the startup scene by the index, any scene by id, residency retained on
//! the world; bad input never crashes and leaves the world alone.

use std::path::{Path, PathBuf};
use std::ptr;

use fynkel_contracts::{Entity, EventKind, Status};
use fynkel_core::schema::{EventHeader, SceneReadyEvent, SceneUnloadedEvent, ZoneEvent};
use fynkel_core::Name;
use fynkel_import::{cook_project, CookOptions};
use fynkel_native::buffer::{self, BufferKind};
use fynkel_native::load::*;
use fynkel_native::*;
use fynkel_scene::{self as scene, Transform};
use fynkel_wire::{get_u32, WireRecord};
use glam::Vec3;

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

    fn load_scene_with(&self, cache: &str, id: &str, flags: u32) -> Status {
        unsafe {
            fynkel_world_load_scene_with(
                self.w,
                cache.as_ptr(),
                cache.len() as u32,
                id.as_ptr(),
                id.len() as u32,
                flags,
            )
        }
    }

    fn unload_scene(&self, id: &str) -> Status {
        unsafe { fynkel_world_unload_scene(self.w, id.as_ptr(), id.len() as u32) }
    }

    fn bind_actions(&self, json: &str) -> Status {
        unsafe { fynkel_world_bind_actions(self.w, json.as_ptr(), json.len() as u32) }
    }

    fn step(&self) -> Status {
        let mut r = FynkelStepResult::default();
        unsafe { fynkel_world_step(self.w, &mut r) }
    }

    /// The drained events as `(kind, body, tail)` — the tail is the
    /// buffer's string table, shared by every body.
    fn events(&self) -> Vec<(u32, Vec<u8>, Vec<u8>)> {
        let v = unsafe { fynkel_world_drain_events(self.w) };
        if v.length == 0 {
            return Vec::new();
        }
        let b = unsafe { std::slice::from_raw_parts(v.data, v.length as usize) };
        let (h, records, tail) = buffer::validate(b, BufferKind::Events).unwrap();
        let mut out = Vec::new();
        let mut off = 0;
        for _ in 0..h.record_count {
            let kind = get_u32(records, off);
            let size = get_u32(records, off + 4) as usize;
            off += EventHeader::SIZE;
            out.push((kind, records[off..off + size].to_vec(), tail.to_vec()));
            off += size;
        }
        out
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
const PLAYGROUND: &str = "c0000000-0000-4000-8000-000000000031";
const RESULTS: &str = "c0000000-0000-4000-8000-000000000033";
const COIN: &str = "c0000000-0000-4000-8000-000000000042";

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
    // Camera, Robot, Sun, the "Crate A" instance and the crate it expands
    // to — expanded by the cook, not here — plus the physics Floor and
    // Crate B (M3-08).
    assert_eq!(r["scene"]["entities"], 7);
    assert_eq!(r["scene"]["rejectedComponents"], 0);
    // The scene, its prefab, the robot, the material and its three maps.
    assert_eq!(r["scene"]["assets"], 7);
    assert_eq!(r["residency"]["retained"], 7);
    assert_eq!(r["residency"]["meshes"], 0, "nothing loaded until drawn");
    assert_eq!(host.live(), 7);
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

    // The same scene by id: `load_scene` replaces (M3-12) — the first
    // instance is unloaded, the world holds one Main again, residency
    // is the scene's closure alone.
    assert_eq!(host.load_scene(&cache, MAIN_SCENE), Status::Ok);
    assert_eq!(host.live(), 7);
    let r = host.report();
    assert_eq!(r["mode"], "replace");
    assert_eq!(r["residency"]["retained"], 7);
    assert_eq!(r["scenes"].as_array().unwrap().len(), 1);
    assert_eq!(r["scenes"][0]["name"], "Main");
    assert_eq!(r["scenes"][0]["entities"], 7);

    // Not a scene, not in the cache, not a UUID.
    let prefab = "c0000000-0000-4000-8000-000000000040";
    assert_eq!(host.load_scene(&cache, prefab), Status::LoadFailed);
    let r = host.report();
    assert_eq!(r["ok"], false);
    assert_eq!(r["diagnostics"][0]["code"], "FYN-R0006");
    assert_eq!(host.live(), 7, "a failed load leaves the world alone");
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
    assert_eq!(host.live(), 7);

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

/// `fynkel_world_instantiate` (M3-08b): the cooked prefab becomes the
/// children of a new anchor — named, placed, parented as asked — its
/// closure retained on the world; bad ids and a missing project fail
/// with a report and leave the world alone.
#[test]
fn prefabs_instantiate_through_the_abi() {
    let cooked = Cooked::new("instantiate");
    let cache = cooked.text();
    let host = Host::new();
    const CRATE: &str = "c0000000-0000-4000-8000-000000000040";

    let desc = |id: &str, name: &str, parent: u64| FynkelInstantiateDesc {
        prefab_id_utf8: id.as_ptr(),
        prefab_id_len: id.len() as u32,
        name_utf8: name.as_ptr(),
        name_len: name.len() as u32,
        parent,
        position: [2.0, 0.0, -3.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };

    // Before any project: refused, reported, nothing spawned.
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc(CRATE, "", 0)) },
        Status::LoadFailed
    );
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0001");
    assert_eq!(host.live(), 0);

    assert_eq!(host.load_project(&cache), Status::Ok);
    assert_eq!(host.live(), 7);
    let retained = host.report()["residency"]["retained"].as_u64().unwrap();

    // The prefab's name by default, at the transform, a root.
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc(CRATE, "", 0)) },
        Status::Ok
    );
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert!(r["scene"].is_null());
    let i = &r["instance"];
    assert_eq!(i["prefab"], CRATE);
    assert_eq!(i["name"], "Crate");
    assert_eq!(i["path"], "prefabs/crate.prefab.json");
    assert_eq!(
        i["entities"], 1,
        "the prefab's entities; the anchor is extra"
    );
    assert_eq!(i["rejectedComponents"], 0);
    assert_eq!(i["behaviours"].as_array().unwrap().len(), 0);
    assert_eq!(host.live(), 9);
    let anchor = Entity::from_bits(i["anchor"].as_u64().unwrap());
    assert_eq!(
        r["residency"]["retained"].as_u64().unwrap(),
        retained,
        "the prefab's closure is already in the store"
    );
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        assert_eq!(ecs.copy::<Name>(anchor).unwrap().value, "Crate");
        assert_eq!(scene::parent_of(ecs, anchor), None);
        let t = ecs.copy::<Transform>(anchor).unwrap();
        assert_eq!(t.position, Vec3::new(2.0, 0.0, -3.0));
        let kids = scene::children_of(ecs, anchor);
        assert_eq!(kids.len(), 1);
        assert_eq!(ecs.copy::<Name>(kids[0]).unwrap().value, "Crate");
        assert_eq!(scene::parent_of(ecs, kids[0]), Some(anchor));
        assert_eq!(
            ecs.copy::<Transform>(kids[0]).unwrap().position,
            Vec3::new(0.0, 0.5, 0.0),
            "the prefab's own transform, local to the anchor"
        );
    }

    // Named and parented under the first anchor.
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc(CRATE, "Cargo", anchor.to_bits())) },
        Status::Ok
    );
    let r = host.report();
    assert_eq!(r["instance"]["name"], "Cargo");
    let second = Entity::from_bits(r["instance"]["anchor"].as_u64().unwrap());
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        assert_eq!(scene::parent_of(ecs, second), Some(anchor));
        assert_eq!(scene::children_of(ecs, anchor).len(), 2);
    }
    assert_eq!(host.live(), 11);

    // Not a prefab, unknown, a dead parent, malformed input.
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc(MAIN_SCENE, "", 0)) },
        Status::LoadFailed
    );
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0006");
    assert_eq!(
        unsafe {
            fynkel_world_instantiate(host.w, &desc("c0000000-0000-4000-8000-0000000000ff", "", 0))
        },
        Status::LoadFailed
    );
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0003");
    assert_eq!(
        unsafe {
            fynkel_world_instantiate(
                host.w,
                &desc(
                    CRATE,
                    "",
                    Entity::from_bits(0x0000_0001_0000_ffff).to_bits(),
                ),
            )
        },
        Status::LoadFailed
    );
    assert_eq!(host.report()["ok"], false);
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc("crate", "", 0)) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, ptr::null()) },
        Status::InvalidArgument
    );
    assert_eq!(
        host.live(),
        11,
        "a failed instantiate leaves the world alone"
    );
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
}

/// The input contract over the ABI: the cooked project's actions bind on
/// load, a captured event buffer drives them, the view reports them
/// (§5.1: a press across catch-up ticks is seen once).
#[test]
fn input_binds_on_load_captures_and_views_through_the_abi() {
    use fynkel_core::schema::{InputActionRecord, InputEventRecord};
    use fynkel_native::buffer::{BufferKind, Writer};
    use fynkel_wire::WireRecord;

    let cooked = Cooked::new("input");
    let host = Host::new();
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    let r = host.report();
    let actions = r["input"]["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 4, "{r:#}");
    assert_eq!(actions[0]["name"], "move");
    assert_eq!(actions[0]["type"], "axis2d");
    assert_eq!(actions[1]["name"], "jump");
    assert_eq!(r["input"]["bindingErrors"].as_array().unwrap().len(), 0);
    // The project's physics block reaches the world (M3-08).
    assert_eq!(r["physics"]["gravity"][1].as_f64().unwrap() as f32, -9.81);
    assert_eq!(
        r["physics"]["layers"],
        serde_json::json!(["default", "props"])
    );
    assert_eq!(r["physics"]["meshColliders"], 0);
    assert_eq!(r["physics"]["diagnostics"], serde_json::json!([]));

    let view = |w: *mut FynkelWorld| -> Vec<InputActionRecord> {
        let v = unsafe { fynkel_world_input_view(w) };
        let bytes = unsafe { std::slice::from_raw_parts(v.data, v.length as usize) };
        let (h, records, tail) =
            fynkel_native::buffer::validate(bytes, BufferKind::InputActions).unwrap();
        (0..h.record_count as usize)
            .map(|i| {
                let off = i * InputActionRecord::SIZE;
                InputActionRecord::decode(&records[off..off + InputActionRecord::SIZE], tail)
                    .unwrap()
            })
            .collect()
    };
    let capture = |w: *mut FynkelWorld, events: &[InputEventRecord]| -> Status {
        let mut wr = Writer::begin(Vec::new(), BufferKind::InputEvents);
        for e in events {
            wr.record(e);
        }
        let buf = wr.finish();
        unsafe { fynkel_world_capture_input(w, buf.as_ptr(), buf.len() as u32) }
    };
    let key = |control: &str, phase: u32| InputEventRecord {
        device: 0,
        control: control.to_string(),
        phase,
        x: if phase == 0 { 1.0 } else { 0.0 },
        y: 0.0,
    };

    // A 50 ms frame: three ticks. Space and W go down.
    let mut plan = FynkelFramePlan::default();
    assert_eq!(
        unsafe { fynkel_world_begin_frame(host.w, 0.05, &mut plan) },
        Status::Ok
    );
    assert_eq!(plan.steps, 3);
    assert_eq!(capture(host.w, &[key("Space", 0), key("W", 0)]), Status::Ok);
    let v = view(host.w);
    assert_eq!(v.len(), 4);
    let jump = &v[1];
    assert!(jump.held && jump.pressed && !jump.released && jump.frame_pressed);
    assert_eq!(jump.x, 1.0);
    let mv = &v[0];
    assert!(mv.held && mv.x == 0.0 && mv.y == 1.0, "{mv:?}");
    let mut sr = FynkelStepResult::default();
    assert_eq!(unsafe { fynkel_world_step(host.w, &mut sr) }, Status::Ok);
    let v = view(host.w);
    assert!(v[1].held && !v[1].pressed, "the second tick sees held only");
    assert!(
        v[1].frame_pressed,
        "the frame's raw edge stays for update()"
    );
    assert_eq!(unsafe { fynkel_world_step(host.w, &mut sr) }, Status::Ok);
    assert_eq!(unsafe { fynkel_world_step(host.w, &mut sr) }, Status::Ok);

    // Next frame: release. A gamepad stick as well (device 2, axis phase).
    assert_eq!(
        unsafe { fynkel_world_begin_frame(host.w, 1.0 / 60.0, &mut plan) },
        Status::Ok
    );
    let stick = InputEventRecord {
        device: 2,
        control: "leftStick".into(),
        phase: 2,
        x: -0.5,
        y: 0.25,
    };
    assert_eq!(
        capture(host.w, &[key("Space", 1), key("W", 1), stick]),
        Status::Ok
    );
    let v = view(host.w);
    assert!(!v[1].held && v[1].released && !v[1].pressed);
    assert!(
        (v[0].x + 0.5).abs() < 1e-6 && (v[0].y - 0.25).abs() < 1e-6,
        "{:?}",
        v[0]
    );

    // An empty capture is fine; a wrong buffer kind is a protocol error and
    // leaves the world usable; an unknown device is skipped.
    assert_eq!(
        unsafe { fynkel_world_capture_input(host.w, ptr::null(), 0) },
        Status::Ok
    );
    let wrong = Writer::begin(Vec::new(), BufferKind::Query).finish();
    assert_eq!(
        unsafe { fynkel_world_capture_input(host.w, wrong.as_ptr(), wrong.len() as u32) },
        Status::Protocol
    );
    let alien = InputEventRecord {
        device: 9,
        control: "X".into(),
        phase: 0,
        x: 1.0,
        y: 0.0,
    };
    assert_eq!(capture(host.w, &[alien]), Status::Ok);
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
    assert_eq!(host.live(), 7);
}

/// M3-12: scenes stack additively and unload one at a time, a replace
/// load clears everything (runtime prefabs included), a `TriggerZone`
/// on a runtime-spawned coin names its event through the wire, and the
/// user's rebinds sit over the project's actions across loads.
#[test]
fn scenes_stack_unload_replace_and_zones_fire_through_the_abi() {
    let cooked = Cooked::new("scenes");
    let cache = cooked.text();
    let host = Host::new();

    // Nothing to unload before a project.
    assert_eq!(host.unload_scene(MAIN_SCENE), Status::LoadFailed);
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0010");
    assert_eq!(host.bind_actions("{}"), Status::LoadFailed);

    assert_eq!(host.load_project(&cache), Status::Ok);
    assert_eq!(host.live(), 7);
    let _ = host.events();

    // Additive: Playground joins Main; the same id again is refused.
    assert_eq!(
        host.load_scene_with(&cache, PLAYGROUND, FYNKEL_LOAD_ADDITIVE),
        Status::Ok
    );
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["mode"], "additive");
    assert_eq!(r["scene"]["name"], "Playground");
    let scenes = r["scenes"].as_array().unwrap();
    assert_eq!(scenes.len(), 2);
    assert_eq!(scenes[0]["name"], "Main");
    assert_eq!(scenes[1]["name"], "Playground");
    assert_eq!(scenes[1]["entities"], 5);
    assert_eq!(host.live(), 12);
    let ev = host.events();
    let ready: Vec<_> = ev
        .iter()
        .filter(|e| e.0 == EventKind::SCENE_READY.0)
        .collect();
    assert_eq!(ready.len(), 1);
    let ready = SceneReadyEvent::decode(&ready[0].1, &ready[0].2).unwrap();
    assert_eq!(ready.scene.asset.to_string(), PLAYGROUND);
    assert_eq!(
        host.load_scene_with(&cache, PLAYGROUND, FYNKEL_LOAD_ADDITIVE),
        Status::LoadFailed
    );
    let r = host.report();
    assert_eq!(r["diagnostics"][0]["code"], "FYN-R0011");
    assert_eq!(host.live(), 12, "a refused load leaves the world alone");
    assert_eq!(
        host.load_scene_with(&cache, PLAYGROUND, 0x80),
        Status::InvalidArgument,
        "unknown flag bits"
    );

    // A coin spawned into Playground on top of the Player: its sensor
    // overlaps the character's capsule, so the first tick fires the
    // zone's named event on the general lane.
    let desc = FynkelInstantiateDesc {
        prefab_id_utf8: COIN.as_ptr(),
        prefab_id_len: COIN.len() as u32,
        name_utf8: ptr::null(),
        name_len: 0,
        parent: 0,
        position: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc) },
        Status::Ok
    );
    assert_eq!(host.live(), 14);
    let _ = host.events();
    assert_eq!(host.step(), Status::Ok);
    let ev = host.events();
    let zones: Vec<_> = ev
        .iter()
        .filter(|e| e.0 == EventKind::ZONE_ENTERED.0)
        .collect();
    assert_eq!(
        zones.len(),
        1,
        "{:?}",
        ev.iter().map(|e| e.0).collect::<Vec<_>>()
    );
    let z = ZoneEvent::decode(&zones[0].1, &zones[0].2).unwrap();
    assert_eq!(z.event, "collect");
    assert_ne!(z.zone, z.other);
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        assert_eq!(ecs.copy::<Name>(z.other).unwrap().value, "Player");
        assert_eq!(ecs.copy::<Name>(z.zone).unwrap().value, "Coin");
        assert!(
            ecs.get::<fynkel_core::TriggerZone>(z.zone).is_err(),
            "`once` removed the zone after its first entry"
        );
    }
    assert!(
        ev.iter().any(|e| e.0 == EventKind::TRIGGER_ENTER.0),
        "the physics-lane event is still there"
    );

    // Unload Main: its 7 go, the coin (a runtime spawn) and Playground
    // stay; unloading it again is FYN-R0010.
    assert_eq!(host.unload_scene(MAIN_SCENE), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["unloaded"]["name"], "Main");
    assert_eq!(r["unloaded"]["id"], MAIN_SCENE);
    assert_eq!(r["unloaded"]["entities"], 7);
    assert_eq!(r["scenes"].as_array().unwrap().len(), 1);
    assert_eq!(r["scenes"][0]["name"], "Playground");
    assert_eq!(host.live(), 7);
    let ev = host.events();
    assert_eq!(
        ev.iter().filter(|e| e.0 == EventKind::DESTROYED.0).count(),
        7
    );
    let gone = ev
        .iter()
        .find(|e| e.0 == EventKind::SCENE_UNLOADED.0)
        .unwrap();
    let gone = SceneUnloadedEvent::decode(&gone.1, &gone.2).unwrap();
    assert_eq!(gone.scene.asset.to_string(), MAIN_SCENE);
    assert_eq!(gone.entities, 7);
    assert_eq!(host.unload_scene(MAIN_SCENE), Status::LoadFailed);
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0010");
    assert_eq!(host.unload_scene("nope"), Status::InvalidArgument);

    // Rebinds sit over the project's actions and survive the next load.
    assert_eq!(
        host.bind_actions(r#"{ "jump": ["keyboard:J"], "nope": ["keyboard:K"] }"#),
        Status::Ok
    );
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    let actions = r["input"]["actions"].as_array().unwrap();
    let jump = actions.iter().find(|a| a["name"] == "jump").unwrap();
    assert_eq!(jump["rebound"], true);
    assert_eq!(jump["bindings"], serde_json::json!(["keyboard:J"]));
    let mv = actions.iter().find(|a| a["name"] == "move").unwrap();
    assert_eq!(mv["rebound"], false);
    assert!(
        actions.iter().all(|a| a["name"] != "nope"),
        "a rebind for an unknown action binds nothing"
    );
    assert_eq!(host.bind_actions("[1]"), Status::InvalidArgument);

    // Replace: Results alone, the coin gone with everything else.
    assert_eq!(host.load_scene(&cache, RESULTS), Status::Ok);
    let r = host.report();
    assert_eq!(r["mode"], "replace");
    assert_eq!(r["scene"]["name"], "Results");
    assert_eq!(r["scenes"].as_array().unwrap().len(), 1);
    assert_eq!(host.live(), 3);
    let jump = r["input"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "jump")
        .unwrap()
        .clone();
    assert_eq!(jump["rebound"], true, "{r:#}");
    let ev = host.events();
    assert_eq!(
        ev.iter()
            .filter(|e| e.0 == EventKind::SCENE_UNLOADED.0)
            .count(),
        1
    );
    assert_eq!(
        ev.iter().filter(|e| e.0 == EventKind::DESTROYED.0).count(),
        7,
        "Playground's 5 and the coin's anchor + entity"
    );
    assert_eq!(ev.last().map(|e| e.0), Some(EventKind::SCENE_READY.0));
    assert_eq!(host.bind_actions("{}"), Status::Ok);
    let r = host.report();
    let jump = r["input"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "jump")
        .unwrap()
        .clone();
    assert_eq!(jump["rebound"], false);
    assert_eq!(jump["bindings"].as_array().unwrap().len(), 3);
}

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
        let w = unsafe { fynkel_world_create(rt, ptr::null(), std::ptr::null_mut()) };
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
    // Camera, Robot, Sun, the "Crate A" instance (the crate prefab's root,
    // expanded by the cook, not here), the physics Floor and Crate B
    // (M3-08).
    assert_eq!(r["scene"]["entities"], 6);
    assert_eq!(r["scene"]["rejectedComponents"], 0);
    // The scene, its prefab, the robot, the material and its three maps,
    // the Robot's tone (M3-11).
    assert_eq!(r["scene"]["assets"], 8);
    assert_eq!(r["residency"]["retained"], 8);
    assert!(
        r["residency"]["meshes"].as_u64().unwrap() > 0,
        "CPU dependencies prepared before activation"
    );
    assert_eq!(host.live(), 6);
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
    assert!(!Entity::from_hex(b["entity"].as_str().unwrap())
        .unwrap()
        .is_null());

    // The same scene by id: `load_scene` replaces (M3-12) — the first
    // instance is unloaded, the world holds one Main again, residency
    // is the scene's closure alone.
    assert_eq!(host.load_scene(&cache, MAIN_SCENE), Status::Ok);
    assert_eq!(host.live(), 6);
    let r = host.report();
    assert_eq!(r["mode"], "replace");
    assert_eq!(r["residency"]["retained"], 8);
    assert_eq!(r["scenes"].as_array().unwrap().len(), 1);
    assert_eq!(r["scenes"][0]["name"], "Main");
    assert_eq!(r["scenes"][0]["entities"], 6);

    // Not a scene, not in the cache, not a UUID.
    let prefab = "c0000000-0000-4000-8000-000000000040";
    assert_eq!(host.load_scene(&cache, prefab), Status::LoadFailed);
    let r = host.report();
    assert_eq!(r["ok"], false);
    assert_eq!(r["diagnostics"][0]["code"], "FYN-R0006");
    assert_eq!(host.live(), 6, "a failed load leaves the world alone");
    let unknown = "c0000000-0000-4000-8000-0000000000ff";
    assert_eq!(host.load_scene(&cache, unknown), Status::LoadFailed);
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0003");
    assert_eq!(host.load_scene(&cache, "robot"), Status::InvalidArgument);
}

// Foundation 5.4: a scene loaded by id decoded its meshes and textures at
// their first draw, so that frame hitched. Now `load_scene` decodes its
// closure before it returns (and queues it for the GPU warm-up); the
// project's startup load still decodes at first draw, so Play from any
// scene starts as fast as before.
#[test]
fn load_scene_decodes_what_the_scene_uses_before_it_returns() {
    let cooked = Cooked::new("preload");
    let cache = cooked.text();
    let host = Host::new();
    assert_eq!(host.load_project(&cache), Status::Ok);
    assert!(host.report()["residency"]["meshes"].as_u64().unwrap() > 0);
    assert_eq!(host.report()["readiness"]["gpu"], "deferred");

    assert_eq!(host.load_scene(&cache, MAIN_SCENE), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    // The robot's mesh, the crate prefab's mesh and the material's maps.
    assert!(
        r["residency"]["meshes"].as_u64().unwrap() > 0,
        "{:#}",
        r["residency"]
    );
    assert!(
        r["residency"]["textures"].as_u64().unwrap() > 0,
        "{:#}",
        r["residency"]
    );
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
    assert_eq!(host.live(), 6);

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

    let desc = |id: &str, name: &str, parent: Entity| FynkelInstantiateDesc {
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
        unsafe { fynkel_world_instantiate(host.w, &desc(CRATE, "", Entity::NULL)) },
        Status::LoadFailed
    );
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0001");
    assert_eq!(host.live(), 0);

    assert_eq!(host.load_project(&cache), Status::Ok);
    assert_eq!(host.live(), 6);
    let retained = host.report()["residency"]["retained"].as_u64().unwrap();

    // The prefab's name by default, at the transform, a root.
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc(CRATE, "", Entity::NULL)) },
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
        "the prefab's entities, the root among them"
    );
    assert_eq!(i["rejectedComponents"], 0);
    assert_eq!(i["behaviours"].as_array().unwrap().len(), 0);
    assert_eq!(host.live(), 7);
    let root = Entity::from_hex(i["root"].as_str().unwrap()).unwrap();
    assert_eq!(
        r["residency"]["retained"].as_u64().unwrap(),
        retained,
        "the prefab's closure is already in the store"
    );
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        assert_eq!(ecs.copy::<Name>(root).unwrap().value, "Crate");
        assert_eq!(scene::parent_of(ecs, root), None);
        let t = ecs.copy::<Transform>(root).unwrap();
        assert_eq!(
            t.position,
            Vec3::new(2.0, 0.0, -3.0),
            "the given transform is the root's"
        );
        assert!(scene::children_of(ecs, root).is_empty());
    }

    // Named and parented under the first instance.
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc(CRATE, "Cargo", root)) },
        Status::Ok
    );
    let r = host.report();
    assert_eq!(r["instance"]["name"], "Cargo");
    let second = Entity::from_hex(r["instance"]["root"].as_str().unwrap()).unwrap();
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        assert_eq!(ecs.copy::<Name>(second).unwrap().value, "Cargo");
        assert_eq!(scene::parent_of(ecs, second), Some(root));
        assert_eq!(scene::children_of(ecs, root), vec![second]);
    }
    assert_eq!(host.live(), 8);

    // Every later instantiate is an in-memory copy, as Unity's (foundation
    // 3.1): the prefab is not read again, so its cooked files can go.
    std::fs::rename(cooked.dir.join("objects"), cooked.dir.join("objects-gone")).unwrap();
    for _ in 0..3 {
        assert_eq!(
            unsafe { fynkel_world_instantiate(host.w, &desc(CRATE, "", Entity::NULL)) },
            Status::Ok,
            "{:#}",
            host.report()
        );
    }
    assert_eq!(host.live(), 11);
    assert_eq!(host.report()["instance"]["name"], "Crate");
    std::fs::rename(cooked.dir.join("objects-gone"), cooked.dir.join("objects")).unwrap();

    // Not a prefab, unknown, a dead parent, malformed input.
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc(MAIN_SCENE, "", Entity::NULL)) },
        Status::LoadFailed
    );
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0006");
    assert_eq!(
        unsafe {
            fynkel_world_instantiate(
                host.w,
                &desc("c0000000-0000-4000-8000-0000000000ff", "", Entity::NULL),
            )
        },
        Status::LoadFailed
    );
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0003");
    assert_eq!(
        unsafe {
            fynkel_world_instantiate(
                host.w,
                &desc(CRATE, "", Entity::from_bits(0x0000_0001_0000_ffff)),
            )
        },
        Status::LoadFailed
    );
    assert_eq!(host.report()["ok"], false);
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc("crate", "", Entity::NULL)) },
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
    assert_eq!(r["layers"], serde_json::json!(["default", "props"]));
    assert_eq!(r["tags"], serde_json::json!([]));
    assert!(r["physics"].get("layers").is_none());
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
        unsafe { fynkel_world_begin_frame(host.w, 0.05, 1.0, &mut plan) },
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
        unsafe { fynkel_world_begin_frame(host.w, 1.0 / 60.0, 1.0, &mut plan) },
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
    assert_eq!(host.live(), 6);
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
    assert_eq!(host.live(), 6);
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
    assert_eq!(host.live(), 11);
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
    assert_eq!(host.live(), 11, "a refused load leaves the world alone");
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
        parent: Entity::NULL,
        position: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc) },
        Status::Ok
    );
    assert_eq!(host.live(), 12);
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

    // Unload Main: its 6 go, the coin (a runtime spawn) and Playground
    // stay; unloading it again is FYN-R0010.
    assert_eq!(host.unload_scene(MAIN_SCENE), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["unloaded"]["name"], "Main");
    assert_eq!(r["unloaded"]["id"], MAIN_SCENE);
    assert_eq!(r["unloaded"]["entities"], 6);
    assert_eq!(r["scenes"].as_array().unwrap().len(), 1);
    assert_eq!(r["scenes"][0]["name"], "Playground");
    assert_eq!(host.live(), 6);
    let ev = host.events();
    assert_eq!(
        ev.iter().filter(|e| e.0 == EventKind::DESTROYED.0).count(),
        6
    );
    let gone = ev
        .iter()
        .find(|e| e.0 == EventKind::SCENE_UNLOADED.0)
        .unwrap();
    let gone = SceneUnloadedEvent::decode(&gone.1, &gone.2).unwrap();
    assert_eq!(gone.scene.asset.to_string(), MAIN_SCENE);
    assert_eq!(gone.entities, 6);
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
        6,
        "Playground's 5 and the coin"
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

/// ADR-0037: a root marked through the ABI outlives a replace load, the
/// report counts it, and what it draws stays resident though the new
/// scene uses none of it; a child is refused.
#[test]
fn dont_destroy_on_load_through_the_abi() {
    const CRATE: &str = "c0000000-0000-4000-8000-000000000040";
    const CATALOGUE: &str = "c0000000-0000-4000-8000-000000000032";
    let cooked = Cooked::new("persistent");
    let cache = cooked.text();
    let host = Host::new();
    assert_eq!(host.load_project(&cache), Status::Ok);
    let desc = FynkelInstantiateDesc {
        prefab_id_utf8: CRATE.as_ptr(),
        prefab_id_len: CRATE.len() as u32,
        name_utf8: ptr::null(),
        name_len: 0,
        parent: Entity::NULL,
        position: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &desc) },
        Status::Ok
    );
    let root = Entity::from_hex(host.report()["instance"]["root"].as_str().unwrap()).unwrap();
    let under = FynkelInstantiateDesc {
        parent: root,
        ..desc
    };
    assert_eq!(
        unsafe { fynkel_world_instantiate(host.w, &under) },
        Status::Ok
    );
    let child = Entity::from_hex(host.report()["instance"]["root"].as_str().unwrap()).unwrap();
    assert_eq!(
        unsafe { fynkel_world_dont_destroy_on_load(host.w, child) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_world_dont_destroy_on_load(host.w, Entity::NULL) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_world_dont_destroy_on_load(host.w, root) },
        Status::Ok
    );

    assert_eq!(host.load_scene(&cache, CATALOGUE), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["persistent"], 1);
    let w = unsafe { &*host.w };
    assert!(w.core().ecs().contains(root));
    assert!(w.core().ecs().contains(child), "with its subtree");
    let assets = w.assets().unwrap();
    for id in [
        "c0000000-0000-4000-8000-000000000001",
        "c0000000-0000-4000-8000-000000000020",
    ] {
        assert!(
            assets.is_resident(id.parse().unwrap()),
            "{id}: the crate still draws it"
        );
    }
    // Destroyed, nothing keeps it: the next replace load releases it.
    let mut edit = edit::Edit::new(host.w);
    edit.destroy(root);
    edit.commit();
    assert_eq!(host.step(), Status::Ok);
    assert_eq!(host.load_scene(&cache, CATALOGUE), Status::Ok);
    assert_eq!(host.report()["persistent"], 0);
    let assets = unsafe { &*host.w }.assets().unwrap();
    assert!(!assets.is_resident("c0000000-0000-4000-8000-000000000020".parse().unwrap()));
}

/// Polls a background load until it is no longer loading.
fn wait_ready(host: &Host, ticket: u32) -> (u32, f32) {
    let started = std::time::Instant::now();
    loop {
        let (mut state, mut progress) = (0u32, 0f32);
        assert_eq!(
            unsafe { fynkel_world_load_progress(host.w, ticket, &mut state, &mut progress) },
            Status::Ok
        );
        if state != FYNKEL_LOAD_LOADING {
            return (state, progress);
        }
        assert!(
            started.elapsed().as_secs() < 30,
            "the background load never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn load_async(host: &Host, cache: &str, id: &str) -> (Status, u32) {
    let mut ticket = 0u32;
    let s = unsafe {
        fynkel_world_load_scene_async(
            host.w,
            cache.as_ptr(),
            cache.len() as u32,
            id.as_ptr(),
            id.len() as u32,
            &mut ticket,
        )
    };
    (s, ticket)
}

/// ADR-0038: a scene read and made resident on a background thread while
/// the world runs on, then activated: the same report as a load, and the
/// activation reads nothing from the cache (it is gone by then).
#[test]
fn a_scene_loads_in_the_background_then_activates() {
    let cooked = Cooked::new("async");
    let cache = cooked.text();
    let host = Host::new();
    assert_eq!(host.load_project(&cache), Status::Ok);
    let before = host.live();

    let (s, ticket) = load_async(&host, &cache, PLAYGROUND);
    assert_eq!(s, Status::Ok);
    assert_ne!(ticket, 0);
    assert_eq!(host.step(), Status::Ok, "the world runs on meanwhile");
    let (state, progress) = wait_ready(&host, ticket);
    assert_eq!(state, FYNKEL_LOAD_READY);
    assert_eq!(progress, 1.0);
    assert_eq!(host.live(), before, "nothing spawned before activation");
    let assets = unsafe { &*host.w }.assets().unwrap();
    assert!(
        assets.stats().meshes > 0,
        "the meshes are in memory already"
    );

    // Everything is in memory: the activation needs no file.
    std::fs::rename(&cooked.dir, cooked.dir.with_extension("moved")).unwrap();
    let status = unsafe { fynkel_world_load_activate(host.w, ticket, FYNKEL_LOAD_ADDITIVE) };
    std::fs::rename(cooked.dir.with_extension("moved"), &cooked.dir).unwrap();
    let r = host.report();
    assert_eq!(status, Status::Ok, "{r:#}");
    assert_eq!(r["mode"], "additive");
    assert_eq!(r["scene"]["name"], "Playground");
    assert_eq!(host.live(), before + 5);
    // The ticket is spent.
    assert_eq!(
        unsafe { fynkel_world_load_activate(host.w, ticket, 0) },
        Status::InvalidArgument
    );

    // A missing scene fails in the background, reported at activation.
    let (s, bad) = load_async(&host, &cache, "c0000000-0000-4000-8000-0000000000ff");
    assert_eq!(s, Status::Ok);
    assert_eq!(wait_ready(&host, bad).0, FYNKEL_LOAD_FAILED);
    assert_eq!(
        unsafe { fynkel_world_load_activate(host.w, bad, 0) },
        Status::LoadFailed
    );
    assert_eq!(host.report()["diagnostics"][0]["code"], "FYN-R0003");
    assert_eq!(
        host.live(),
        before + 5,
        "a failed load leaves the world alone"
    );

    // Cancelled: dropped, the ticket unknown after.
    let (_, dropped) = load_async(&host, &cache, RESULTS);
    assert_eq!(
        unsafe { fynkel_world_load_cancel(host.w, dropped) },
        Status::Ok
    );
    let (mut state, mut progress) = (0u32, 0f32);
    assert_eq!(
        unsafe { fynkel_world_load_progress(host.w, dropped, &mut state, &mut progress) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_world_load_cancel(host.w, dropped) },
        Status::InvalidArgument
    );

    // A replace activation takes the world down then, not before.
    let (_, results) = load_async(&host, &cache, RESULTS);
    assert_eq!(wait_ready(&host, results).0, FYNKEL_LOAD_READY);
    assert_eq!(host.live(), before + 5);
    assert_eq!(
        unsafe { fynkel_world_load_activate(host.w, results, 0) },
        Status::Ok
    );
    assert_eq!(host.report()["mode"], "replace");
    assert_eq!(host.live(), 3);
    // A world dropped with a load still running is fine.
    let _ = load_async(&host, &cache, PLAYGROUND);
}

/// Live Play mode (ADR-0021 P7): the running world answers in the scene
/// document's terms — its ids, including the ones the cook derived for a
/// prefab instance's children — and takes a write at once.
#[cfg(feature = "authoring")]
#[test]
fn a_running_world_answers_live_calls_in_document_terms() {
    fn live(w: *mut FynkelWorld, kind: &str, args: &str) -> (Status, serde_json::Value) {
        let mut view = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        let status = unsafe {
            fynkel_native::authoring::fynkel_world_live_call(
                w,
                kind.as_ptr(),
                kind.len() as u32,
                args.as_ptr(),
                args.len() as u32,
                &mut view,
            )
        };
        if view.data.is_null() {
            return (status, serde_json::Value::Null);
        }
        let bytes = unsafe { std::slice::from_raw_parts(view.data, view.length as usize) };
        (status, serde_json::from_slice(bytes).unwrap())
    }

    let cooked = Cooked::new("live");
    let host = Host::new();
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    assert_eq!(host.step(), Status::Ok);

    let (status, table) = live(host.w, "entities", "{}");
    assert_eq!(status, Status::Ok, "{table}");
    let rows = table["entities"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let authored: Vec<String> = (0xe0..=0xe5)
        .map(|n| format!("c0000000-0000-4000-8000-0000000000{n:x}"))
        .collect();
    for r in rows {
        let id = r["id"].as_str().unwrap();
        assert!(
            !id.starts_with("runtime:"),
            "every entity came from the scene: {r}"
        );
        if !authored.iter().any(|a| a == id) {
            assert_eq!(
                r["parent"], "c0000000-0000-4000-8000-0000000000e2",
                "the crate the Crate A instance expands to"
            );
        }
    }

    let robot = "c0000000-0000-4000-8000-0000000000e1";
    let (status, r) = live(
        host.w,
        "rename",
        &format!(r#"{{"id":"{robot}","name":"Hero"}}"#),
    );
    assert_eq!(status, Status::Ok, "{r}");
    let (_, e) = live(host.w, "entity", &format!(r#"{{"id":"{robot}"}}"#));
    assert_eq!(e["name"], "Hero");
    assert_eq!(e["components"][0]["type"], "fynkel.Transform");

    let (status, r) = live(
        host.w,
        "remove",
        &format!(r#"{{"id":"{robot}","type":"fynkel.Camera"}}"#),
    );
    assert_eq!(status, Status::CommandsRejected, "the world refuses: {r}");
    assert_eq!(r["ok"], false);
    let (status, r) = live(host.w, "fly", "{}");
    assert_eq!(status, Status::InvalidArgument);
    assert_eq!(r["diagnostics"][0]["code"], "FYN-T0003");
    let (status, _) = live(host.w, "entities", "[]");
    assert_eq!(status, Status::InvalidArgument, "args must be an object");
    let (status, r) = live(ptr::null_mut(), "entities", "{}");
    assert_eq!(
        (status, r),
        (Status::InvalidArgument, serde_json::Value::Null)
    );
}

/// Prefab variants and prefabs holding instances (ADR-0022 amendment
/// 2026-09-25) cook flat and spawn through the ABI: a scene holding an
/// instance of a prefab that holds an instance of a variant, and each
/// instantiated at run time.
#[test]
fn variants_and_nested_instances_cook_and_instantiate() {
    const CRATE: &str = "c0000000-0000-4000-8000-000000000040";
    const CRATE_ROOT: &str = "c0000000-0000-4000-8000-0000000000b1";
    const CRATE_TRANSFORM: &str = "c0000000-0000-4000-8000-0000000000c2";
    const VARIANT: &str = "c0000000-0000-4000-8000-000000000700";
    const HOLDER: &str = "c0000000-0000-4000-8000-000000000701";
    const SCENE: &str = "c0000000-0000-4000-8000-000000000702";
    let project =
        std::env::temp_dir().join(format!("fynkel-native-variants-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    copy_dir(&fixture(), &project);
    let transform = |y: f64| {
        serde_json::json!({ "type": "fynkel.Transform", "version": 1,
            "data": { "position": [0, y, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } })
    };
    let prefab_header = |id: &str, name: &str, root: &str| {
        serde_json::json!({ "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
            "formatVersion": 7, "id": id, "name": name, "root": root })
    };
    // The variant: the Crate twice as big, with a Flag of its own.
    let mut variant = prefab_header(
        VARIANT,
        "Crate Variant",
        "c0000000-0000-4000-8000-0000000007a0",
    );
    variant["entities"] = serde_json::json!({
        "c0000000-0000-4000-8000-0000000007a0": { "name": "Crate", "parent": null,
            "children": ["c0000000-0000-4000-8000-0000000007a1"], "components": {},
            "prefab": { "asset": { "asset": CRATE }, "instance": "c0000000-0000-4000-8000-0000000007e0",
                "overrides": [ { "op": "setField", "entity": CRATE_ROOT, "component": CRATE_TRANSFORM,
                                 "field": "scale", "value": [2, 2, 2] } ] } },
        "c0000000-0000-4000-8000-0000000007a1": { "name": "Flag",
            "parent": "c0000000-0000-4000-8000-0000000007a0", "children": [],
            "components": { "c0000000-0000-4000-8000-0000000007c1": transform(1.0) } }
    });
    // The holder: an object with the variant inside, its Flag raised.
    let mut holder = prefab_header(HOLDER, "Holder", "c0000000-0000-4000-8000-0000000007b0");
    holder["entities"] = serde_json::json!({
        "c0000000-0000-4000-8000-0000000007b0": { "name": "Holder", "parent": null,
            "children": ["c0000000-0000-4000-8000-0000000007b1"],
            "components": { "c0000000-0000-4000-8000-0000000007c2": transform(0.0) } },
        "c0000000-0000-4000-8000-0000000007b1": { "name": "Box",
            "parent": "c0000000-0000-4000-8000-0000000007b0", "children": [], "components": {},
            "prefab": { "asset": { "asset": VARIANT }, "instance": "c0000000-0000-4000-8000-0000000007e1",
                "overrides": [ { "op": "setField", "entity": "c0000000-0000-4000-8000-0000000007a1",
                                 "component": "c0000000-0000-4000-8000-0000000007c1",
                                 "field": "position", "value": [0, 5, 0] } ] } }
    });
    let scene = serde_json::json!({ "$schema": "https://fynkel.dev/schemas/scene.schema.json",
        "formatVersion": 7, "id": SCENE, "name": "Variants", "entities": {
            "c0000000-0000-4000-8000-0000000007d0": { "name": "Stand", "parent": null,
                "children": [], "components": {},
                "prefab": { "asset": { "asset": HOLDER }, "instance": "c0000000-0000-4000-8000-0000000007e2" } }
        } });
    for (path, doc) in [
        ("assets/prefabs/crate_variant.prefab.json", &variant),
        ("assets/prefabs/holder.prefab.json", &holder),
        ("assets/scenes/variants.scene.json", &scene),
    ] {
        std::fs::write(
            project.join(path),
            serde_json::to_string_pretty(doc).unwrap(),
        )
        .unwrap();
    }
    let dir = project.join(".fynkel/cache");
    let report = cook_project(&project, Some(&dir), &CookOptions::default())
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert!(!report.has_errors(), "{}", report.text());
    let cache = dir.display().to_string();
    let host = Host::new();

    // The scene: Stand (the Holder's root), Box (the variant's root, the
    // Crate's), Flag.
    assert_eq!(host.load_scene(&cache, SCENE), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["scene"]["entities"], 3);
    assert_eq!(host.live(), 3);

    let spawn = |id: &str| {
        let desc = FynkelInstantiateDesc {
            prefab_id_utf8: id.as_ptr(),
            prefab_id_len: id.len() as u32,
            name_utf8: ptr::null(),
            name_len: 0,
            parent: Entity::NULL,
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        };
        assert_eq!(
            unsafe { fynkel_world_instantiate(host.w, &desc) },
            Status::Ok
        );
        let r = host.report();
        assert_eq!(r["ok"], true, "{r:#}");
        (
            Entity::from_hex(r["instance"]["root"].as_str().unwrap()).unwrap(),
            r["instance"]["entities"].as_u64().unwrap(),
        )
    };
    // The variant: its root and its Flag.
    let (root, count) = spawn(VARIANT);
    assert_eq!(count, 2);
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        // Named after the prefab by default, like any instance.
        assert_eq!(ecs.copy::<Name>(root).unwrap().value, "Crate Variant");
        let kids = scene::children_of(ecs, root);
        assert_eq!(kids.len(), 1);
        assert_eq!(ecs.copy::<Name>(kids[0]).unwrap().value, "Flag");
        assert_eq!(
            ecs.copy::<Transform>(kids[0]).unwrap().position,
            Vec3::new(0.0, 1.0, 0.0)
        );
    }
    // The holder: the variant inside, twice as big, its Flag raised.
    let (root, count) = spawn(HOLDER);
    assert_eq!(count, 3);
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        assert_eq!(ecs.copy::<Name>(root).unwrap().value, "Holder");
        let kids = scene::children_of(ecs, root);
        assert_eq!(kids.len(), 1);
        let boxed = kids[0];
        assert_eq!(ecs.copy::<Name>(boxed).unwrap().value, "Box");
        assert_eq!(
            ecs.copy::<Transform>(boxed).unwrap().scale,
            Vec3::new(2.0, 2.0, 2.0)
        );
        let flag = scene::children_of(ecs, boxed)[0];
        assert_eq!(
            ecs.copy::<Transform>(flag).unwrap().position,
            Vec3::new(0.0, 5.0, 0.0)
        );
    }
    assert_eq!(host.live(), 8);
    drop(host);
    let _ = std::fs::remove_dir_all(&project);
}

/// `fynkel_world_set_material` (ADR-0026 §4): a material made at runtime
/// is what the store gives every renderer, across scene loads and clears;
/// a cooked one can be replaced and put back.
#[test]
fn runtime_materials_through_the_abi() {
    use fynkel_contracts::{AssetRef, Uuid};
    use fynkel_core::material::PbrMaterial;
    use fynkel_wire::material::encode_material;

    let cooked = Cooked::new("materials");
    let cache = cooked.text();
    let host = Host::new();
    let made = AssetRef::new(Uuid(*b"runtime-material"));
    let red = PbrMaterial {
        base_color: [1.0, 0.0, 0.0, 1.0].into(),
        roughness: 0.25,
        ..Default::default()
    };
    let payload = encode_material(&red);
    let set = |r: &AssetRef, payload: &[u8]| unsafe {
        fynkel_world_set_material(
            host.w,
            r.asset.0.as_ptr(),
            payload.as_ptr(),
            payload.len() as u32,
        )
    };
    let store = || unsafe { &*host.w }.assets().unwrap().clone();

    assert_eq!(
        set(&made, &payload),
        Status::LoadFailed,
        "no store before a load"
    );
    assert_eq!(host.load_project(&cache), Status::Ok);
    assert_eq!(set(&made, &payload), Status::Ok);
    assert_eq!(store().material(&made).unwrap(), red);

    // Across a scene load (same cache, same store) and a clear.
    assert_eq!(host.load_scene(&cache, PLAYGROUND), Status::Ok);
    store().clear();
    assert_eq!(store().material(&made).unwrap(), red);

    // Malformed or another build's layout: refused, the material kept.
    assert_eq!(
        set(&made, &payload[..payload.len() - 1]),
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_world_set_material(host.w, ptr::null(), ptr::null(), 0) },
        Status::InvalidArgument
    );
    assert_eq!(store().material(&made).unwrap(), red);

    // A cooked material replaced, then back to its cooked values.
    let rusted = AssetRef::new(Uuid(
        *"c0000000-0000-4000-8000-000000000020"
            .parse::<uuid::Uuid>()
            .unwrap()
            .as_bytes(),
    ));
    let cooked_rusted = store().material(&rusted).unwrap();
    assert_ne!(cooked_rusted, red);
    assert_eq!(set(&rusted, &payload), Status::Ok);
    assert_eq!(store().material(&rusted).unwrap(), red);
    assert_eq!(set(&rusted, &[]), Status::Ok);
    assert_eq!(store().material(&rusted).unwrap(), cooked_rusted);

    // Removed: the made one is gone.
    assert_eq!(set(&made, &[]), Status::Ok);
    assert!(store().material(&made).is_err());
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
}

/// `fynkel_world_set_mesh` (ADR-0026 §4): a mesh made at runtime is what
/// the store gives every renderer, with bounds from its positions, across
/// scene loads and clears; a payload that could not be drawn is refused
/// and the mesh kept.
#[test]
fn runtime_meshes_through_the_abi() {
    use fynkel_assets::{encode_mesh, Vertex};
    use fynkel_contracts::{AssetRef, Uuid};

    let cooked = Cooked::new("meshes");
    let cache = cooked.text();
    let host = Host::new();
    let made = AssetRef::new(Uuid(*b"runtime-mesh-001"));
    let at = |p: [f32; 3]| Vertex {
        position: p,
        ..Default::default()
    };
    let quad = [
        at([0.0, 0.0, 0.0]),
        at([2.0, 0.0, 0.0]),
        at([2.0, 1.0, 0.0]),
        at([0.0, 1.0, -3.0]),
    ];
    let indices = [0, 1, 2, 0, 2, 3];
    // Bounds that are wrong on purpose: the store recomputes them.
    let payload = encode_mesh(&quad, &indices, [0.0; 3], [0.0; 3]);
    let set = |r: &AssetRef, payload: &[u8]| unsafe {
        fynkel_world_set_mesh(
            host.w,
            r.asset.0.as_ptr(),
            payload.as_ptr(),
            payload.len() as u32,
        )
    };
    let store = || unsafe { &*host.w }.assets().unwrap().clone();

    assert_eq!(
        set(&made, &payload),
        Status::LoadFailed,
        "no store before a load"
    );
    assert_eq!(host.load_project(&cache), Status::Ok);
    let edits = store().mesh_edits();
    assert_eq!(set(&made, &payload), Status::Ok);
    let m = store().mesh(&made).unwrap();
    assert_eq!(
        (
            m.vertices.len(),
            m.indices.clone(),
            m.bounds_min,
            m.bounds_max
        ),
        (4, indices.to_vec(), [0.0, 0.0, -3.0], [2.0, 1.0, 0.0])
    );
    assert!(store().is_runtime_mesh(&made));
    assert_eq!(
        store().meshes_changed_since(edits),
        vec![made.clone()],
        "a renderer re-uploads it"
    );

    // Across a scene load (same cache, same store) and a clear.
    assert_eq!(host.load_scene(&cache, PLAYGROUND), Status::Ok);
    store().clear();
    assert_eq!(store().mesh(&made).unwrap().vertices.len(), 4);

    // What could not be drawn is refused; the mesh stays.
    let refused = [
        payload[..payload.len() - 1].to_vec(),
        encode_mesh(&quad, &[0, 1], [0.0; 3], [0.0; 3]),
        encode_mesh(&quad, &[0, 1, 4], [0.0; 3], [0.0; 3]),
        encode_mesh(&quad, &[], [0.0; 3], [0.0; 3]),
        encode_mesh(
            &[at([f32::NAN, 0.0, 0.0]), at([1.0; 3]), at([0.0; 3])],
            &[0, 1, 2],
            [0.0; 3],
            [0.0; 3],
        ),
        fynkel_assets::encode_skinned_mesh(
            &quad[..3],
            &[0, 1, 2],
            [0.0; 3],
            [0.0; 3],
            &[Default::default(); 3],
        ),
    ];
    for (i, p) in refused.iter().enumerate() {
        assert_eq!(set(&made, p), Status::InvalidArgument, "payload {i}");
    }
    assert_eq!(
        unsafe { fynkel_world_set_mesh(host.w, ptr::null(), ptr::null(), 0) },
        Status::InvalidArgument
    );
    assert_eq!(store().mesh(&made).unwrap().vertices.len(), 4);

    // Changed, then removed.
    assert_eq!(
        set(
            &made,
            &encode_mesh(&quad[..3], &[0, 1, 2], [0.0; 3], [0.0; 3])
        ),
        Status::Ok
    );
    assert_eq!(store().mesh(&made).unwrap().vertices.len(), 3);
    assert_eq!(set(&made, &[]), Status::Ok);
    assert!(store().mesh(&made).is_err());
    assert!(!store().is_runtime_mesh(&made));
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
}

/// `fynkel_world_set_clip` (ADR-0033 § 2): a clip made at runtime is one
/// the store and the world play like a cooked one, across scene loads and
/// clears; a payload that is not a valid clip is refused and the clip kept.
#[test]
fn runtime_clips_through_the_abi() {
    use fynkel_assets::{encode_animation, AnimationChannel, AnimationMarker, AnimationPayload};
    use fynkel_contracts::{AssetRef, Uuid};

    let cooked = Cooked::new("clips");
    let cache = cooked.text();
    let host = Host::new();
    let made = AssetRef::new(Uuid(*b"runtime-clip-001"));
    let clip = |times: Vec<f32>, values: Vec<f32>, property: u32| {
        encode_animation(&AnimationPayload {
            duration: 1.0,
            channels: vec![AnimationChannel {
                target_name: String::new(),
                target: "Arm".into(),
                property,
                interpolation: 1, // linear
                times,
                values,
            }],
            markers: vec![AnimationMarker {
                time: 0.5,
                name: "hit".into(),
            }],
        })
    };
    let payload = clip(vec![0.0, 1.0], vec![0.0, 0.0, 0.0, 0.0, 2.0, 0.0], 0);
    let set = |payload: &[u8]| unsafe {
        fynkel_world_set_clip(
            host.w,
            made.asset.0.as_ptr(),
            payload.as_ptr(),
            payload.len() as u32,
        )
    };
    let store = || unsafe { &*host.w }.assets().unwrap().clone();

    assert_eq!(set(&payload), Status::LoadFailed, "no store before a load");
    assert_eq!(host.load_project(&cache), Status::Ok);
    assert_eq!(set(&payload), Status::Ok);
    let c = store().clip(&made).unwrap();
    assert_eq!(
        (c.duration, c.channels.len(), c.markers[0].name.as_str()),
        (1.0, 1, "hit")
    );
    assert!(store().is_runtime_clip(&made));
    assert!(
        unsafe { &*host.w }.core().has_clip(&made),
        "the world has it at once"
    );

    // Across a scene load (same cache, same store) and a clear.
    assert_eq!(host.load_scene(&cache, PLAYGROUND), Status::Ok);
    store().clear();
    assert_eq!(store().clip(&made).unwrap().channels.len(), 1);

    // What is not a valid clip is refused; the clip stays.
    let refused = [
        payload[..payload.len() - 1].to_vec(),
        clip(vec![0.0, 1.0], vec![0.0; 6], 7),
        clip(vec![1.0, 0.5], vec![0.0; 6], 0),
        clip(vec![0.0, 2.0], vec![0.0; 6], 0),
        clip(vec![0.0, 1.0], vec![f32::NAN, 0.0, 0.0, 0.0, 0.0, 0.0], 0),
    ];
    for (i, p) in refused.iter().enumerate() {
        assert_eq!(set(p), Status::InvalidArgument, "payload {i}");
    }
    assert_eq!(
        unsafe { fynkel_world_set_clip(host.w, ptr::null(), ptr::null(), 0) },
        Status::InvalidArgument
    );
    assert_eq!(store().clip(&made).unwrap().channels[0].values[4], 2.0);

    // Removed.
    assert_eq!(set(&[]), Status::Ok);
    assert!(store().clip(&made).is_err());
    assert!(!store().is_runtime_clip(&made));
    assert_eq!(host.step(), Status::Ok);
    assert!(
        !unsafe { &*host.w }.core().has_clip(&made),
        "unowned removed runtime clips retire from the simulation registry"
    );
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
}

/// `fynkel_world_clone` (ADR-0026 §4): a live subtree copied, placed and
/// reported as source → copy pairs, without any project; a stale source
/// fails with a report and spawns nothing.
#[test]
fn clone_through_the_abi() {
    let host = Host::new();
    let (root, child) = {
        let w = unsafe { &mut *host.w }.core_mut();
        let root = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
        let child = scene::spawn(
            w.ecs_mut(),
            Transform::from_position(glam::Vec3::X),
            Some(root),
        )
        .unwrap();
        w.ecs_mut().insert(root, Name::new("Ship")).unwrap();
        (root, child)
    };
    let desc = |source: Entity, name: &str, parent: Entity| FynkelCloneDesc {
        source,
        name_utf8: name.as_ptr(),
        name_len: name.len() as u32,
        parent,
        position: [0.0, 3.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };
    assert_eq!(
        unsafe { fynkel_world_clone(host.w, &desc(root, "", Entity::NULL)) },
        Status::Ok
    );
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    let c = &r["clone"];
    let copy = Entity::from_hex(c["root"].as_str().unwrap()).unwrap();
    assert_eq!(c["entities"][0][0], root.to_hex());
    assert_eq!(c["entities"][0][1], copy.to_hex());
    assert_eq!(c["entities"][1][0], child.to_hex());
    assert_eq!(host.live(), 4);
    {
        let ecs = unsafe { &*host.w }.core().ecs();
        assert_eq!(ecs.copy::<Name>(copy).unwrap().value, "Ship");
        assert_eq!(ecs.copy::<Transform>(copy).unwrap().position.y, 3.0);
        let copied_child = Entity::from_hex(c["entities"][1][1].as_str().unwrap()).unwrap();
        assert_eq!(scene::children_of(ecs, copy), vec![copied_child]);
    }

    assert_eq!(
        unsafe { fynkel_world_clone(host.w, &desc(child, "Wing", copy)) },
        Status::Ok
    );
    assert_eq!(host.live(), 5);

    let stale = Entity::new(copy.world(), 7, 60_000);
    assert_eq!(
        unsafe { fynkel_world_clone(host.w, &desc(stale, "", Entity::NULL)) },
        Status::LoadFailed
    );
    assert_eq!(host.report()["ok"], false);
    assert_eq!(host.live(), 5);
    assert_eq!(
        unsafe { fynkel_world_clone(host.w, ptr::null()) },
        Status::InvalidArgument
    );
}

/// A project that names no startup scene (a game that builds its world
/// from code, ADR-0026) opens with nothing loaded: its layers, tags and
/// input bound, the cache's scenes loadable by id.
#[test]
fn a_project_without_a_startup_scene_opens_empty() {
    let project =
        std::env::temp_dir().join(format!("fynkel-native-nostartup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    copy_dir(&fixture(), &project);
    let _ = std::fs::remove_dir_all(project.join(".fynkel"));
    let file = project.join("fynkel.project.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("startupScene");
    std::fs::write(&file, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    let cache = project.join(".fynkel/cache");
    let report = cook_project(&project, Some(&cache), &CookOptions::default())
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert!(!report.has_errors(), "{}", report.text());

    let host = Host::new();
    let cache = cache.display().to_string();
    assert_eq!(host.load_project(&cache), Status::Ok, "{}", host.report());
    let r = host.report();
    assert!(r["scene"].is_null());
    assert_eq!(host.live(), 0);
    assert_eq!(r["layers"], serde_json::json!(["default", "props"]));
    assert!(!r["input"]["actions"].as_array().unwrap().is_empty());
    assert_eq!(host.step(), Status::Ok);

    assert_eq!(host.load_scene(&cache, MAIN_SCENE), Status::Ok);
    assert_eq!(host.live(), 6);
    let _ = std::fs::remove_dir_all(&project);
}

/// `fynkel_world_asset_json` (ADR-0027 §6): a document's cooked JSON from
/// the world's cache — any kind, whether or not a scene retains it — and a
/// diagnostic for no project, an unknown id or a source.
#[test]
fn a_documents_cooked_json_through_the_abi() {
    let uuid = |s: &str| s.parse::<fynkel_contracts::Uuid>().unwrap().0;
    let read = |host: &Host, id: &[u8; 16]| {
        let mut out = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        let status = unsafe { fynkel_world_asset_json(host.w, id.as_ptr(), &mut out) };
        let bytes = unsafe { std::slice::from_raw_parts(out.data, out.length as usize) };
        (
            status,
            serde_json::from_slice::<serde_json::Value>(bytes).unwrap(),
        )
    };
    let material = uuid("c0000000-0000-4000-8000-000000000020");
    let host = Host::new();
    let (status, why) = read(&host, &material);
    assert_eq!(status, Status::LoadFailed);
    assert_eq!(why["code"], "FYN-R0003", "{why}");

    let cooked = Cooked::new("asset-json");
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    let (status, doc) = read(&host, &material);
    assert_eq!(status, Status::Ok, "{doc}");
    assert_eq!(doc["name"], "Rusted");
    assert!(doc["$schema"]
        .as_str()
        .unwrap()
        .ends_with("material.schema.json"));

    // Not in the cache; a source, not a document.
    let (status, why) = read(&host, &uuid("c0000000-0000-4000-8000-0000000000ff"));
    assert_eq!(status, Status::LoadFailed);
    assert!(why["message"].is_string(), "{why}");
    let (status, why) = read(&host, &uuid("c0000000-0000-4000-8000-000000000006"));
    assert_eq!(status, Status::LoadFailed);
    assert!(
        why["message"].as_str().unwrap().contains("not a document"),
        "{why}"
    );
    assert_eq!(
        unsafe { fynkel_world_asset_json(host.w, ptr::null(), ptr::null_mut()) },
        Status::InvalidArgument
    );
}

/// `fynkel_world_set_shader_material` (ADR-0031): a material of a
/// `.shader` made from a document at runtime, checked against the shader
/// as the cook checks it; new values change the payload's block only.
#[test]
fn runtime_shader_materials_through_the_abi() {
    use fynkel_contracts::{AssetRef, Uuid};

    const SHADER_ID: &str = "c0000000-0000-4000-8000-000000000601";
    let project = std::env::temp_dir().join(format!(
        "fynkel-native-shader-project-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&project);
    copy_dir(&fixture(), &project);
    let shaders = project.join("assets/shaders");
    std::fs::create_dir_all(&shaders).unwrap();
    std::fs::write(
        shaders.join("tint.shader"),
        "shader \"Tests/Tint\" {\n    lighting unlit\n    properties {\n        tint color3 = (1, 1, 1)\n        power float = 1 range(0, 4)\n        glow texture = black\n    }\n}\n\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.base_color = material.tint * material.power + sample_glow(in.uv0).rgb;\n    return s;\n}\n",
    )
    .unwrap();
    std::fs::write(
        shaders.join("tint.shader.meta.json"),
        format!(r#"{{"$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":2,"id":"{SHADER_ID}","name":"tint.shader","importer":"fynkel.shader","importerVersion":1,"settings":{{}},"subAssets":{{}}}}"#),
    )
    .unwrap();
    let cache = project.join(".fynkel/cache");
    let report = cook_project(&project, Some(&cache), &CookOptions::default())
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert!(!report.has_errors(), "{}", report.text());

    let host = Host::new();
    let made = AssetRef::new(Uuid(*b"runtime-shader-m"));
    let doc = |params: &str| {
        format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/material.schema.json","formatVersion":2,"id":"72756e74-696d-652d-7368-616465722d6d","name":"Made","shader":{{"asset":"{SHADER_ID}"}},"parameters":{params}}}"#
        )
    };
    let set = |text: &str| unsafe {
        let mut out = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        let s = fynkel_world_set_shader_material(
            host.w,
            made.asset.0.as_ptr(),
            text.as_ptr(),
            text.len() as u32,
            &mut out,
        );
        let diagnostics = if out.length == 0 {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(std::slice::from_raw_parts(out.data, out.length as usize))
                .unwrap()
        };
        (s, diagnostics)
    };
    let store = || unsafe { &*host.w }.assets().unwrap().clone();
    let floats = |block: &[u8]| -> Vec<f32> {
        block
            .chunks(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    };

    let (s, d) = set(&doc(r#"{"tint":[1,0,0]}"#));
    assert_eq!(s, Status::LoadFailed, "no store before a load");
    assert_eq!(d[0]["code"], "FYN-R0003");
    assert_eq!(host.load_project(&cache.display().to_string()), Status::Ok);

    let (s, d) = set(&doc(r#"{"tint":[1,0,0],"power":2}"#));
    assert_eq!(s, Status::Ok, "{d}");
    let m = store().custom_material(&made).unwrap().unwrap();
    assert_eq!(
        m.shader,
        *SHADER_ID.parse::<uuid::Uuid>().unwrap().as_bytes()
    );
    // tint at 0, power at 16 (a vec3 takes a whole slot).
    assert_eq!(floats(&m.block)[..5], [1.0, 0.0, 0.0, 0.0, 2.0]);
    assert_eq!(m.slots.len(), 1);
    let seen = store().material_edits();

    // New values: the same material, a new block.
    let (s, _) = set(&doc(r#"{"tint":[0,1,0]}"#));
    assert_eq!(s, Status::Ok);
    let m = store().custom_material(&made).unwrap().unwrap();
    assert_eq!(
        floats(&m.block)[..5],
        [0.0, 1.0, 0.0, 0.0, 1.0],
        "power back to its default"
    );
    assert_eq!(store().materials_changed_since(seen), vec![made.clone()]);

    // Checked as the cook checks: a value out of range, an unknown name.
    let (s, d) = set(&doc(r#"{"power":9}"#));
    assert_eq!(s, Status::InvalidArgument);
    assert_eq!(d[0]["code"], "FYN-D0303", "{d}");
    let (s, d) = set(&doc(r#"{"powr":1}"#));
    assert_eq!(s, Status::InvalidArgument);
    assert_eq!(d[0]["code"], "FYN-D0301", "{d}");
    let m = store().custom_material(&made).unwrap().unwrap();
    assert_eq!(
        &floats(&m.block)[..3],
        &[0.0, 1.0, 0.0],
        "the material unchanged"
    );

    // A shader the cache does not have.
    let (s, d) = set(&doc("{}").replace(SHADER_ID, "c0000000-0000-4000-8000-0000000006ff"));
    assert_eq!(s, Status::LoadFailed);
    assert!(d.as_array().is_some_and(|a| !a.is_empty()), "{d}");

    // Removed.
    let (s, _) = unsafe {
        let mut out = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        (
            fynkel_world_set_shader_material(
                host.w,
                made.asset.0.as_ptr(),
                ptr::null(),
                0,
                &mut out,
            ),
            (),
        )
    };
    assert_eq!(s, Status::Ok);
    assert!(store().custom_material(&made).unwrap_or(None).is_none());
    assert_eq!(unsafe { fynkel_world_is_poisoned(host.w) }, 0);
    drop(host);
    let _ = std::fs::remove_dir_all(&project);
}

/// ADR-0039: the project's `persistentObjects` are made by
/// `load_project`, once, kept across scene loads (no duplicates), listed
/// in the report with their scripts, and made afresh by the next
/// `load_project`. A prefab the cache does not have is a diagnostic, not
/// a failed load.
#[test]
fn persistent_objects_are_made_once_by_load_project() {
    const CRATE: &str = "c0000000-0000-4000-8000-000000000040";
    const CATALOGUE: &str = "c0000000-0000-4000-8000-000000000032";
    let root = std::env::temp_dir().join(format!("fynkel-native-autoload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("project");
    copy_dir(&fixture(), &project);
    let file = project.join("fynkel.project.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    v["persistentObjects"] = serde_json::json!([
        { "name": "GameManager", "prefab": { "asset": CRATE } },
    ]);
    std::fs::write(&file, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let cache_dir = root.join("cache");
    let report = cook_project(&project, Some(&cache_dir), &CookOptions::default())
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert!(!report.has_errors(), "{}", report.text());
    let cache = cache_dir.display().to_string();

    let host = Host::new();
    assert_eq!(host.load_project(&cache), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["persistent"], 1, "{r:#}");
    let made = &r["persistentObjects"];
    assert_eq!(made.as_array().map(Vec::len), Some(1), "{r:#}");
    assert_eq!(made[0]["name"], "GameManager");
    assert_eq!(made[0]["prefab"], CRATE);
    assert!(made[0]["behaviours"].is_array());
    let first = Entity::from_hex(made[0]["entity"].as_str().unwrap()).unwrap();
    let w = unsafe { &*host.w };
    assert!(w.core().is_persistent(first));
    assert_eq!(
        w.core()
            .ecs()
            .get::<Name>(first)
            .map(|n| n.value.clone())
            .ok(),
        Some("GameManager".to_string())
    );

    // A scene load keeps it and makes no second one.
    assert_eq!(host.load_scene(&cache, CATALOGUE), Status::Ok);
    assert_eq!(host.report()["persistent"], 1);
    assert!(
        host.report()["persistentObjects"].is_null(),
        "only load_project makes them"
    );
    assert!(unsafe { &*host.w }.core().ecs().contains(first));

    // Failed project activation keeps the old scene, project roots and store.
    let before_count = unsafe { &*host.w }.core().live_entities();
    assert_eq!(
        host.load_project(&root.join("missing-cache").display().to_string()),
        Status::LoadFailed
    );
    assert_eq!(unsafe { &*host.w }.core().live_entities(), before_count);
    assert!(unsafe { &*host.w }.core().is_persistent(first));

    // The next load_project makes them afresh: the old one goes.
    assert_eq!(host.load_project(&cache), Status::Ok);
    let r = host.report();
    assert_eq!(r["persistent"], 1, "{r:#}");
    let second = Entity::from_hex(r["persistentObjects"][0]["entity"].as_str().unwrap()).unwrap();
    assert_ne!(second, first);
    assert!(!unsafe { &*host.w }.core().ecs().contains(first));

    // A prefab the cache does not have: reported, the game still starts.
    let index = cache_dir.join("index.json");
    let text = std::fs::read_to_string(&index).unwrap();
    std::fs::write(
        &index,
        text.replace(
            &format!("\"prefab\": \"{CRATE}\""),
            "\"prefab\": \"c0000000-0000-4000-8000-0000000000ff\"",
        ),
    )
    .unwrap();
    let host = Host::new();
    assert_eq!(host.load_project(&cache), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["persistent"], 0);
    assert!(r["persistentObjects"][0]["entity"].is_null(), "{r:#}");
    assert!(
        r["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["message"].as_str().unwrap_or("").contains("GameManager")),
        "{r:#}"
    );
    drop(host);
    let _ = std::fs::remove_dir_all(&root);
}

/// Foundation 5.1: a deleted file leaves its references empty, as in
/// Unity; the documents that use it still cook and the scenes load. The
/// startup scene uses the rusted material (whose maps are the deleted
/// PNGs), the robot model and the crate prefab; the rig scene uses the
/// rig whose skeleton model is deleted.
#[test]
fn missing_references_cook_and_load_as_none() {
    const MAIN: &str = "c0000000-0000-4000-8000-000000000030";
    const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
    const RIG_SCENE: &str = "c0000000-0000-4000-8000-000000000034";
    let root = std::env::temp_dir().join(format!("fynkel-native-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("project");
    copy_dir(&fixture(), &project);
    let assets = project.join("assets");
    // A PNG deleted in Finder: its sidecar stays behind.
    std::fs::remove_file(assets.join("textures/rusted_mr.png")).unwrap();
    // A PNG deleted with its sidecar.
    std::fs::remove_file(assets.join("textures/rusted_normal.png")).unwrap();
    std::fs::remove_file(assets.join("textures/rusted_normal.png.meta.json")).unwrap();
    // A model and a prefab the startup scene uses.
    std::fs::remove_file(assets.join("models/robot.glb")).unwrap();
    std::fs::remove_file(assets.join("models/robot.glb.meta.json")).unwrap();
    std::fs::remove_file(assets.join("prefabs/crate.prefab.json")).unwrap();
    // The rig's skeleton model.
    std::fs::remove_file(assets.join("models/rig.glb")).unwrap();

    let cache_dir = root.join("cache");
    let report = cook_project(&project, Some(&cache_dir), &CookOptions::default())
        .unwrap_or_else(|d| panic!("{d:?}"));
    let cooked = |id: &str| {
        report
            .assets
            .iter()
            .find(|a| a.id.to_string() == id)
            .is_some_and(|a| a.key.is_some())
    };
    assert!(cooked(MAIN), "{}", report.text());
    assert!(cooked(RUSTED), "{}", report.text());
    assert!(cooked(RIG_SCENE), "{}", report.text());
    // Only the rig (its skeleton is its content) and the blend space (its
    // clips are the rig's) fail themselves; a missing reference warns.
    let errors: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == fynkel_contracts::Severity::Error)
        .map(|d| d.code.0)
        .collect();
    assert!(
        errors
            .iter()
            .all(|c| *c == "FYN-D0500" || *c == "FYN-D0601"),
        "{:?}",
        report.diagnostics
    );
    let warned = |code: &str, text: &str| {
        report
            .diagnostics
            .iter()
            .any(|d| d.code.0 == code && d.message.contains(text))
    };
    assert!(warned("FYN-A0002", "rusted_mr.png is missing"));
    assert!(warned("FYN-A0004", "c0000000-0000-4000-8000-000000000003"));
    assert!(warned("FYN-D0401", "c0000000-0000-4000-8000-000000000040"));
    assert!(warned("FYN-A0105", "rigs/column.rig.json failed"));

    let cache = cache_dir.display().to_string();
    let host = Host::new();
    assert_eq!(host.load_project(&cache), Status::Ok);
    let r = host.report();
    assert_eq!(r["ok"], true, "{r:#}");
    // The crate instance is an empty placeholder with its name.
    let w = unsafe { &*host.w };
    let ecs = w.core().ecs();
    let names: Vec<String> = ecs
        .entities()
        .filter_map(|e| ecs.copy::<Name>(e).ok().map(|n| n.value))
        .collect();
    assert!(names.iter().any(|n| n == "Crate A"), "{names:?}");
    assert_eq!(host.load_scene(&cache, RIG_SCENE), Status::Ok);
    assert_eq!(host.report()["ok"], true, "{:#}", host.report());
    drop(host);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn failed_rebinding_before_project_load_does_not_persist_overrides() {
    let host = Host::new();
    assert_eq!(
        host.bind_actions(r#"{"jump":["keyboard:J"]}"#),
        Status::LoadFailed
    );
    let cooked = Cooked::new("rebind-no-project-rollback");
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    let report = host.report();
    let jump = report["input"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "jump")
        .unwrap();
    assert_eq!(jump["rebound"], false);
    assert_eq!(jump["bindings"].as_array().unwrap().len(), 3);
}

#[test]
fn binding_warning_reports_and_accepted_overrides_match_after_a_later_load() {
    let host = Host::new();
    let cooked = Cooked::new("rebind-shared-preparation");
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    assert_eq!(
        host.bind_actions(r#"{"jump":["keyboard:J","not-a-binding"],"unknown":["keyboard:K"]}"#),
        Status::Ok
    );
    let input = host.report()["input"].clone();
    assert_eq!(input["bindingErrors"].as_array().unwrap().len(), 1);
    assert_eq!(input["bindingErrors"][0]["action"], "jump");
    assert_eq!(input["bindingErrors"][0]["binding"], "not-a-binding");
    assert!(input["actions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["name"] != "unknown"));
    assert_eq!(host.load_scene(&cooked.text(), MAIN_SCENE), Status::Ok);
    assert_eq!(
        host.report()["input"],
        input,
        "load and explicit rebinding use the same preparation and grammar"
    );
}

#[test]
fn rebinding_observes_committed_state_and_does_not_consume_lifecycle_events() {
    let host = Host::new();
    let cooked = Cooked::new("rebind-journal");
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    host.events();
    let core = unsafe { &mut *host.w }.core_mut();
    let commit = core
        .edit_gameplay(|edit| edit.spawn("committed", Transform::IDENTITY, None))
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    core.edit_gameplay(|edit| edit.destroy(entity)).unwrap();
    let tick = core.tick();
    assert_eq!(host.bind_actions(r#"{"jump":["keyboard:J"]}"#), Status::Ok);
    assert_eq!(unsafe { &*host.w }.core().tick(), tick);
    assert_eq!(
        host.events()
            .iter()
            .filter(|event| event.0 == EventKind::DESTROYED.0)
            .count(),
        1
    );
    assert!(host.events().is_empty());
    assert!(unsafe { &mut *host.w }
        .core_mut()
        .edit_gameplay(|edit| {
            edit.spawn(
                "discarded",
                Transform::from_position(Vec3::splat(f32::NAN)),
                None,
            )
        })
        .is_err());
    assert_eq!(host.bind_actions(r#"{"jump":["keyboard:K"]}"#), Status::Ok);
    assert_eq!(host.load_scene(&cooked.text(), MAIN_SCENE), Status::Ok);
    let report = host.report();
    let jump = report["input"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "jump")
        .unwrap();
    assert_eq!(jump["bindings"], serde_json::json!(["keyboard:K"]));
}

#[path = "support/edit.rs"]
mod edit;

#[test]
fn asset_registration_waits_for_successful_transaction_commit() {
    use edit::Edit;
    use fynkel_core::{Animator, ComponentValue};
    let cooked = Cooked::new("transaction-assets");
    let host = Host::new();
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    let clip = fynkel_contracts::AssetRef::with_sub(
        "c0000000-0000-4000-8000-000000000005".parse().unwrap(),
        "anim/wave",
    );
    let mut edit = Edit::new(host.w);
    let owner = edit.spawn("owner", Default::default(), None);
    let dead = edit.spawn("dead", Default::default(), None);
    let commit = edit.commit();
    let owner = commit.resolve(owner);
    let dead = commit.resolve(dead);
    let mut edit = Edit::new(host.w);
    edit.destroy(dead);
    edit.commit();
    let value = ComponentValue::Animator(Animator {
        clip: clip.clone(),
        playing: false,
        ..Default::default()
    });
    assert!(!unsafe { (*host.w).core().has_clip(&clip) });
    let mut edit = Edit::new(host.w);
    assert_eq!(edit.component(dead, &value), Err(Status::CommandsRejected));
    assert_eq!(edit.finish(), Status::CommandsRejected);
    assert!(!unsafe { (*host.w).core().has_clip(&clip) });
    // Preparing a valid asset is still rolled back if a later operation fails.
    let mut edit = Edit::new(host.w);
    edit.component(owner, &value).unwrap();
    assert_eq!(edit.try_destroy(dead), Status::CommandsRejected);
    assert_eq!(edit.finish(), Status::CommandsRejected);
    assert!(!unsafe { (*host.w).core().has_clip(&clip) });
    assert!(!unsafe { (*host.w).core().ecs().has::<Animator>(owner) });
    assert_eq!(host.step(), Status::Ok);
    assert!(!unsafe { (*host.w).core().has_clip(&clip) });
    let mut edit = Edit::new(host.w);
    edit.component(owner, &value).unwrap();
    edit.commit();
    assert!(unsafe { (*host.w).core().has_clip(&clip) });
    assert!(unsafe { (*host.w).core().ecs().has::<Animator>(owner) });
}

#[test]
fn runtime_material_journal_publishes_once_and_preserves_store_on_abort() {
    use fynkel_contracts::{AssetRef, Uuid};
    use fynkel_core::material::PbrMaterial;
    use fynkel_native::gameplay::{
        fynkel_world_gameplay_begin, fynkel_world_gameplay_end, fynkel_world_gameplay_exchange,
    };
    let cooked = Cooked::new("journal-materials");
    let host = Host::new();
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    let assets = unsafe { &*host.w }.assets().unwrap().clone();
    let key = AssetRef::new(Uuid(*b"journal-material"));
    let old = PbrMaterial::default();
    assets
        .set_runtime_material(&key, Some(old.clone()))
        .unwrap();
    let revision = assets.live_edits();
    for commit in [0, 1] {
        let mut sequence = 0;
        let mut out = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        assert_eq!(
            unsafe { fynkel_world_gameplay_begin(host.w, &mut sequence) },
            Status::Ok
        );
        for roughness in [0.25, 0.75] {
            let material = PbrMaterial {
                roughness,
                ..old.clone()
            };
            let payload = fynkel_wire::material::encode_material(&material);
            let id = key.asset.to_string();
            let mut request = Vec::new();
            for word in [1u32, 29, id.len() as u32] {
                request.extend(word.to_le_bytes());
            }
            request.extend(id.as_bytes());
            request.extend((payload.len() as u32).to_le_bytes());
            request.extend(payload);
            assert_eq!(
                unsafe {
                    fynkel_world_gameplay_exchange(
                        host.w,
                        sequence,
                        request.as_ptr(),
                        request.len() as u32,
                        &mut out,
                    )
                },
                Status::Ok
            );
            assert_eq!(
                assets.material(&key).unwrap(),
                old,
                "staging does not mutate render store"
            );
            assert_eq!(assets.live_edits(), revision);
        }
        assert_eq!(
            unsafe { fynkel_world_gameplay_end(host.w, sequence, commit, &mut out) },
            Status::Ok
        );
        assert_eq!(assets.live_edits(), revision + commit as u64);
        assert_eq!(
            assets.material(&key).unwrap().roughness,
            if commit == 0 { old.roughness } else { 0.75 }
        );
    }
}

#[test]
fn required_dependency_failures_never_become_ready_or_replace_the_live_world() {
    let good = Cooked::new("f5-good");
    let dependency: fynkel_contracts::Uuid =
        "c0000000-0000-4000-8000-000000000002".parse().unwrap();
    for missing in [true, false] {
        let bad = Cooked::new(if missing { "f5-missing" } else { "f5-corrupt" });
        let assets = fynkel_assets::Assets::open(&bad.dir).unwrap();
        let object = assets.entry(dependency).unwrap().file.unwrap();
        drop(assets);
        if missing {
            std::fs::remove_file(object).unwrap();
        } else {
            std::fs::write(object, b"broken cooked texture").unwrap();
        }
        let host = Host::new();
        assert_eq!(host.load_project(&good.text()), Status::Ok);
        let before = host.live();
        assert_eq!(host.load_scene(&bad.text(), MAIN_SCENE), Status::LoadFailed);
        assert_eq!(host.live(), before);
        assert!(
            host.report()["diagnostics"]
                .to_string()
                .contains(&dependency.to_string()),
            "{}",
            host.report()
        );
        let (status, ticket) = load_async(&host, &bad.text(), MAIN_SCENE);
        assert_eq!(status, Status::Ok);
        assert_eq!(wait_ready(&host, ticket).0, FYNKEL_LOAD_FAILED);
        assert_eq!(
            unsafe { fynkel_world_load_activate(host.w, ticket, 0) },
            Status::LoadFailed
        );
        assert_eq!(host.live(), before);
        assert!(
            host.report()["diagnostics"]
                .to_string()
                .contains(&dependency.to_string()),
            "{}",
            host.report()
        );
        assert_eq!(host.step(), Status::Ok);
    }
}

#[test]
fn repeated_scene_unloads_release_cache_and_simulation_registrations() {
    let cooked = Cooked::new("f5-unload-baseline");
    let host = Host::new();
    for _ in 0..12 {
        assert_eq!(host.load_scene(&cooked.text(), MAIN_SCENE), Status::Ok);
        assert!(unsafe { &*host.w }.core().audio().clip_count() > 0);
        assert_eq!(host.unload_scene(MAIN_SCENE), Status::Ok);
        assert_eq!(host.step(), Status::Ok);
        let world = unsafe { &*host.w };
        assert_eq!(world.core().live_entities(), 0);
        assert_eq!(world.core().registered_clips().count(), 0);
        assert_eq!(world.core().registered_skins().count(), 0);
        assert_eq!(world.core().registered_controllers().count(), 0);
        assert_eq!(world.core().registered_blends().count(), 0);
        assert_eq!(world.core().physics().mesh_count(), 0);
        assert_eq!(world.core().physics().material_count(), 0);
        assert_eq!(world.core().audio().clip_count(), 0);
        let stats = world.assets().unwrap().stats();
        assert_eq!(stats.retained, 0);
        assert_eq!(stats.bytes, 0);
    }
}

#[test]
fn runtime_budget_failure_rolls_back_the_gameplay_edit_and_resource_publication() {
    use edit::Edit;
    use fynkel_contracts::{AssetRef, Uuid};
    let cooked = Cooked::new("f5-runtime-atomic");
    let host = Host::new();
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    let assets = unsafe { &*host.w }.assets().unwrap().clone();
    assets.set_runtime_budget(0).unwrap();
    let before = host.live();
    let key = AssetRef::new(Uuid(*b"budget-rejected!"));
    let data =
        fynkel_wire::material::encode_material(&fynkel_core::material::PbrMaterial::default());
    let id = key.asset.to_string();
    let mut payload = (id.len() as u32).to_le_bytes().to_vec();
    payload.extend(id.as_bytes());
    payload.extend((data.len() as u32).to_le_bytes());
    payload.extend(data);
    let mut edit = Edit::new(host.w);
    edit.spawn("must not publish", Default::default(), None);
    edit.operation(29, &payload).unwrap();
    assert_eq!(edit.finish(), Status::CommandsRejected);
    assert_eq!(host.live(), before);
    assert_eq!(assets.runtime_bytes(), 0);
    assert!(assets.material(&key).is_err());
    assert_eq!(host.step(), Status::Ok);
    assets.set_runtime_budget(1024).unwrap();
    let mut edit = Edit::new(host.w);
    edit.operation(29, &payload).unwrap();
    edit.commit();
    assert!(assets.material(&key).is_ok());
}

#[test]
fn a_queued_sound_owns_its_registration_between_commit_and_the_first_tick() {
    use edit::Edit;
    let cooked = Cooked::new("f5-queued-sound-owner");
    let host = Host::new();
    assert_eq!(host.load_project(&cooked.text()), Status::Ok);
    assert_eq!(host.unload_scene(MAIN_SCENE), Status::Ok);
    let clip =
        fynkel_contracts::AssetRef::new("c0000000-0000-4000-8000-000000000006".parse().unwrap());
    let record = fynkel_core::schema::PlaySoundCommandRecord {
        clip: clip.clone(),
        bus: fynkel_core::AudioBus::Sfx,
        volume: 1.0,
        pitch: 1.0,
        priority: 0,
    };
    let mut strings = fynkel_wire::StringTable::new();
    let mut payload = record.to_vec(&mut strings);
    payload.extend((strings.as_bytes().len() as u32).to_le_bytes());
    payload.extend(strings.as_bytes());
    let mut edit = Edit::new(host.w);
    edit.operation(20, &payload).unwrap();
    edit.commit();
    assert!(unsafe { &*host.w }.core().has_sound(&clip));
    // A second boundary must not retire the queued request either.
    Edit::new(host.w).commit();
    assert_eq!(host.step(), Status::Ok);
    assert_eq!(unsafe { &*host.w }.core().audio_stats().started, 1);
    assert_eq!(host.step(), Status::Ok);
    assert_eq!(unsafe { &*host.w }.core().audio_stats().started, 1);
}

#[test]
#[ignore = "manual F5 activation timing qualification"]
fn asset_activation_qualification() {
    use std::time::{Duration, Instant};
    let cooked = Cooked::new("f5-activation-bench");
    let cache = cooked.text();
    let mut runs = Vec::new();
    for cycle in 0..24 {
        let host = Host::new();
        assert_eq!(host.load_project(&cache), Status::Ok);
        let started = Instant::now();
        let (status, ticket) = load_async(&host, &cache, PLAYGROUND);
        assert_eq!(status, Status::Ok);
        let mut ticks = Vec::new();
        loop {
            let step = Instant::now();
            assert_eq!(host.step(), Status::Ok);
            ticks.push(step.elapsed().as_secs_f64() * 1e6);
            let (mut state, mut progress) = (0, 0.0);
            assert_eq!(
                unsafe { fynkel_world_load_progress(host.w, ticket, &mut state, &mut progress) },
                Status::Ok
            );
            if state == FYNKEL_LOAD_READY {
                break;
            }
            assert_eq!(state, FYNKEL_LOAD_LOADING);
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(1));
        }
        let preparation_us = started.elapsed().as_secs_f64() * 1e6;
        let activation = Instant::now();
        assert_eq!(
            unsafe { fynkel_world_load_activate(host.w, ticket, FYNKEL_LOAD_ADDITIVE) },
            Status::Ok
        );
        let activation_us = activation.elapsed().as_secs_f64() * 1e6;
        assert_eq!(host.report()["readiness"]["cpu"], "ready");
        let peak = unsafe { &*host.w }.assets().unwrap().stats().bytes;
        assert_eq!(host.unload_scene(PLAYGROUND), Status::Ok);
        assert_eq!(host.unload_scene(MAIN_SCENE), Status::Ok);
        assert_eq!(host.step(), Status::Ok);
        assert_eq!(host.live(), 0);
        assert_eq!(unsafe { &*host.w }.assets().unwrap().stats().bytes, 0);
        runs.push(serde_json::json!({"cycle":cycle,"preparation_wall_us":preparation_us,"activation_us":activation_us,"simulation_steps_us":ticks,"cpu_cache_peak_bytes":peak,"post_unload_cpu_cache_bytes":0}));
    }
    let result = serde_json::json!({"workload":"24 fresh hosts running fixture Main (6 entities), asynchronously prepare and atomically activate additive Playground (5 entities), then unload both. Cache-only runtime. 1 ms progress polling; OS cache uncontrolled. Not a production latency guarantee.","runs":runs});
    if let Ok(path) = std::env::var("FYNKEL_F5_ACTIVATION_OUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    }
    println!("{result}");
}

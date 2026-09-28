//! Objects at scale (foundation 3.9): a headless benchmark of what a game
//! does with many objects — instantiate a prefab until the world holds
//! about 50 000 entities, tick it idle and with some roots moving,
//! reparent, flip an object inactive, spawn under a parent, destroy.
//! Every step goes through the native world as a host drives it
//! (`fynkel_world_instantiate`, `fynkel_world_step`), so the numbers are
//! the ones a game sees. See `benchmarks/objects/README.md`.
//!
//! ```sh
//! cargo run --release -p fynkel_native --example objects            # 50 000 entities
//! cargo run --release -p fynkel_native --example objects -- 20000   # fewer
//! ```

use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{Duration, Instant};

use fynkel_contracts::{Entity, Status};
use fynkel_core::{Command, Lane};
use fynkel_import::{cook_project, CookOptions};
use fynkel_native::load::*;
use fynkel_native::*;
use fynkel_scene::Transform;
use glam::Vec3;

const ENEMY: &str = "e0000000-0000-4000-8000-000000000001";
/// Entities per instance of the generated prefab.
const PER_INSTANCE: usize = 6;

fn main() {
    let target: usize = std::env::args()
        .nth(1)
        .map(|a| a.parse().expect("the entity count"))
        .unwrap_or(50_000);
    let instances = target / PER_INSTANCE;

    let project = project();
    let cache = std::env::temp_dir().join("fynkel-objects-cache");
    let _ = std::fs::remove_dir_all(&cache);
    let report = cook_project(&project, Some(&cache), &CookOptions::default())
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert!(!report.has_errors(), "{}", report.text());
    let cache = cache.display().to_string();

    let rt = fynkel_runtime_create();
    let w = unsafe { fynkel_world_create(rt, ptr::null()) };
    assert!(!w.is_null());
    let status = unsafe { fynkel_world_load_project(w, cache.as_ptr(), cache.len() as u32) };
    assert_eq!(status, Status::Ok);
    step(w);
    HEAP_WORLD.with(|c| c.set(w));
    let base_entities = live(w);
    let base_heap = heap_kib();

    println!("objects: {instances} instances of a {PER_INSTANCE}-entity prefab");
    println!();

    // 1. Instantiate (3.1): per call, early and late, and the memory kept.
    let mut roots = Vec::with_capacity(instances);
    let mut first = Duration::ZERO;
    let mut last = Duration::ZERO;
    let window = 1000.min(instances / 4).max(1);
    let started = Instant::now();
    for i in 0..instances {
        let t = Instant::now();
        let x = (i % 100) as f32 * 2.0;
        let z = (i / 100) as f32 * 2.0;
        roots.push(instantiate(w, [x, 0.0, z]));
        let took = t.elapsed();
        if i < window {
            first += took;
        }
        if i >= instances - window {
            last += took;
        }
    }
    let total = started.elapsed();
    let after_heap = heap_kib();
    let entities = live(w);
    row(
        "instantiate, all",
        total,
        &format!("{} entities", entities - base_entities),
    );
    row_us(
        &format!("instantiate, first {window} (each)"),
        first / window as u32,
    );
    row_us(
        &format!("instantiate, last {window} (each)"),
        last / window as u32,
    );
    println!(
        "  live heap after instantiate: +{} MiB ({} bytes per entity)",
        (after_heap - base_heap) / 1024,
        (after_heap - base_heap) * 1024 / (entities - base_entities) as i64
    );

    // The first tick applies every instance's component writes.
    let t = Instant::now();
    step(w);
    row("first tick after instantiate", t.elapsed(), "");
    phases(w);

    // 2. An idle tick (3.3): nothing moved.
    let idle = ticks(w, 100);
    row_ms("tick, idle (mean of 100)", idle);
    phases(w);

    // 3. A tick with 1% of the roots moved (3.3).
    let movers = (instances / 100).max(1);
    let mut phase = 0.0f32;
    let t = Instant::now();
    for _ in 0..100 {
        phase += 0.1;
        let commands = roots
            .iter()
            .take(movers)
            .map(|&entity| Command::SetTransform {
                entity,
                transform: Transform::from_position(Vec3::new(phase.sin(), 0.0, phase.cos())),
            })
            .collect();
        world(w).core_mut().submit(Lane::Simulation, commands);
        step(w);
    }
    row_ms(
        &format!("tick, {movers} roots moved (mean of 100)"),
        t.elapsed() / 100,
    );

    // 4. Keep-world reparenting (3.3): 1000 roots under others, one batch.
    let count = 1000.min(instances / 2);
    let commands = (0..count)
        .map(|i| Command::SetParent {
            child: roots[2 * i + 1],
            parent: Some(roots[2 * i]),
            keep_world: true,
        })
        .collect();
    world(w).core_mut().submit(Lane::Simulation, commands);
    let t = Instant::now();
    step(w);
    row(
        &format!("tick with {count} keep-world reparents"),
        t.elapsed(),
        "",
    );

    // 5. One inactive object, then spawns under a parent (3.4).
    world(w).core_mut().submit(
        Lane::Simulation,
        vec![Command::SetActive {
            entity: roots[instances - 1],
            active: false,
        }],
    );
    step(w);
    let t = Instant::now();
    for i in 0..100 {
        world(w).core_mut().submit(
            Lane::Simulation,
            vec![Command::Spawn {
                temp_id: i + 1,
                transform: Transform::IDENTITY,
                parent: Some(roots[0]),
                name: None,
            }],
        );
        step(w);
    }
    row_ms(
        "tick with a parented spawn, one inactive (mean of 100)",
        t.elapsed() / 100,
    );

    // 6. Destroy every instance in one batch.
    // The reparented ones go with their new parents.
    let commands = roots
        .iter()
        .enumerate()
        .filter(|(i, _)| !(i % 2 == 1 && *i < 2 * count))
        .map(|(_, &e)| Command::Destroy(e))
        .collect();
    world(w).core_mut().submit(Lane::Simulation, commands);
    let t = Instant::now();
    step(w);
    row(
        "tick destroying every instance",
        t.elapsed(),
        &format!("{} left", live(w)),
    );
    let idle = ticks(w, 100);
    row_ms("tick, idle after (mean of 100)", idle);
    let end_heap = heap_kib();
    println!(
        "  live heap at the end: {:+} KiB over the start",
        end_heap - base_heap
    );

    // 7. The same again, three times: a world that grows once and is
    //    reused keeps its memory flat (no leak per instantiate, 3.1).
    let mut previous = end_heap;
    for cycle in 2..=4 {
        let t = Instant::now();
        let again: Vec<Entity> = (0..instances)
            .map(|i| instantiate(w, [(i % 100) as f32 * 2.0, 0.0, (i / 100) as f32 * 2.0]))
            .collect();
        step(w);
        let commands = again.iter().map(|&e| Command::Destroy(e)).collect();
        world(w).core_mut().submit(Lane::Simulation, commands);
        step(w);
        let now = heap_kib();
        row(
            &format!("cycle {cycle}: instantiate all, tick, destroy all"),
            t.elapsed(),
            &format!("live heap {:+} KiB", now - previous),
        );
        previous = now;
    }

    unsafe {
        fynkel_world_destroy(w);
        fynkel_runtime_destroy(rt);
    }
}

fn row(what: &str, took: Duration, note: &str) {
    println!("  {what:<58} {:>10.2} ms  {note}", took.as_secs_f64() * 1e3);
}

fn row_ms(what: &str, took: Duration) {
    row(what, took, "");
}

fn row_us(what: &str, took: Duration) {
    println!("  {what:<58} {:>10.1} µs", took.as_secs_f64() * 1e6);
}

/// The last step's phases that took time (`FYNKEL_QUERY_SYSTEMS`).
fn phases(w: *mut FynkelWorld) {
    let times: Vec<String> = world(w)
        .core()
        .system_times()
        .iter()
        .filter(|t| t.micros > 0)
        .map(|t| format!("{:?} {} µs", t.system, t.micros))
        .collect();
    println!("    last tick's phases: {}", times.join(", "));
}

fn world<'a>(w: *mut FynkelWorld) -> &'a mut FynkelWorld {
    unsafe { &mut *w }
}

/// One tick as a host runs it: step, then drain the events.
fn step(w: *mut FynkelWorld) {
    let mut r = FynkelStepResult::default();
    assert_eq!(unsafe { fynkel_world_step(w, &mut r) }, Status::Ok);
    unsafe { fynkel_world_drain_events(w) };
}

fn ticks(w: *mut FynkelWorld, n: u32) -> Duration {
    let t = Instant::now();
    for _ in 0..n {
        step(w);
    }
    t.elapsed() / n
}

fn live(w: *mut FynkelWorld) -> usize {
    let mut s = FynkelWorldStats::default();
    assert_eq!(unsafe { fynkel_world_stats(w, &mut s) }, Status::Ok);
    s.live_entities as usize
}

fn instantiate(w: *mut FynkelWorld, position: [f32; 3]) -> Entity {
    let desc = FynkelInstantiateDesc {
        prefab_id_utf8: ENEMY.as_ptr(),
        prefab_id_len: ENEMY.len() as u32,
        name_utf8: ptr::null(),
        name_len: 0,
        parent: 0,
        position,
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };
    assert_eq!(unsafe { fynkel_world_instantiate(w, &desc) }, Status::Ok);
    let v = unsafe { fynkel_world_load_report(w) };
    let bytes = unsafe { std::slice::from_raw_parts(v.data, v.length as usize) };
    let report: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    Entity::from_bits(report["instance"]["root"].as_u64().unwrap())
}

/// The live Rust heap in KiB (`FynkelWorldStats::native_bytes`, the
/// engine's counting allocator): what a leak grows — the process's
/// resident size also holds freed memory the allocator keeps for reuse.
fn heap_kib() -> i64 {
    let w = HEAP_WORLD.with(|c| c.get());
    let mut s = FynkelWorldStats::default();
    assert_eq!(unsafe { fynkel_world_stats(w, &mut s) }, Status::Ok);
    s.native_bytes as i64 / 1024
}

thread_local! {
    static HEAP_WORLD: std::cell::Cell<*mut FynkelWorld> = const { std::cell::Cell::new(ptr::null_mut()) };
}

/// The fixture project copied to a temporary directory with the
/// benchmark's prefab: an Enemy root with a mesh, a Body (mesh) holding a
/// Head and an Arm, Legs, and a Marker that follows the Head by name — the
/// shape of a small game object, and a prefab-internal reference (3.5).
fn project() -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap();
    let dir = std::env::temp_dir().join("fynkel-objects-project");
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&fixture, &dir);
    let _ = std::fs::remove_dir_all(dir.join(".fynkel"));
    std::fs::write(dir.join("assets/prefabs/enemy.prefab.json"), enemy()).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn enemy() -> String {
    let id = |n: u32| format!("e0000000-0000-4000-8000-{n:012x}");
    let transform = |n: u32, y: f32| {
        format!(
            r#""{}": {{ "type": "fynkel.Transform", "version": 1, "data": {{ "position": [0, {y}, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] }} }}"#,
            id(n)
        )
    };
    let mesh = |n: u32| {
        format!(
            r#""{}": {{ "type": "fynkel.MeshRenderer", "version": 1, "data": {{ "mesh": {{ "asset": "c0000000-0000-4000-8000-000000000001", "sub": "mesh/Cube" }}, "material": {{ "asset": "c0000000-0000-4000-8000-000000000020" }}, "cast_shadows": true, "receive_shadows": true, "visible": true }} }}"#,
            id(n)
        )
    };
    let follow = format!(
        r#""{}": {{ "type": "fynkel.Follow", "version": 1, "data": {{ "target": "Head", "offset": [0, 0.5, 0], "smoothing": 0, "look_at": false }} }}"#,
        id(0x209)
    );
    let entity = |n: u32,
                  name: &str,
                  parent: Option<u32>,
                  children: &[u32],
                  components: Vec<String>| {
        let parent = parent.map_or("null".to_owned(), |p| format!("\"{}\"", id(p)));
        let children: Vec<String> = children.iter().map(|c| format!("\"{}\"", id(*c))).collect();
        format!(
            r#""{}": {{ "name": "{name}", "parent": {parent}, "children": [{}], "components": {{ {} }} }}"#,
            id(n),
            children.join(", "),
            components.join(", ")
        )
    };
    let entities = [
        entity(
            0x101,
            "Enemy",
            None,
            &[0x102, 0x105, 0x106],
            vec![transform(0x201, 0.0), mesh(0x202)],
        ),
        entity(
            0x102,
            "Body",
            Some(0x101),
            &[0x103, 0x104],
            vec![transform(0x203, 1.0), mesh(0x204)],
        ),
        entity(0x103, "Head", Some(0x102), &[], vec![transform(0x205, 0.8)]),
        entity(0x104, "Arm", Some(0x102), &[], vec![transform(0x206, 0.2)]),
        entity(0x105, "Legs", Some(0x101), &[], vec![transform(0x207, 0.4)]),
        entity(
            0x106,
            "Marker",
            Some(0x101),
            &[],
            vec![transform(0x208, 2.0), follow],
        ),
    ];
    format!(
        r#"{{
  "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
  "formatVersion": 6,
  "id": "{ENEMY}",
  "name": "Enemy",
  "entities": {{ {} }},
  "root": "{}"
}}
"#,
        entities.join(",\n    "),
        id(0x101)
    )
}

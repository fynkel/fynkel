//! `fynkel_host_headless` — the Rust headless host (blueprint §5.2).
//!
//! No renderer, no GPU, no Dart: a test loop that owns the clock and calls
//! `step()` on `fynkel_core`, exactly like the other hosts, and reports what
//! happened as JSON. `fynkel test --headless` runs it; the Dart headless
//! game host (Behaviours) arrives with the Dart Behaviour lifecycle.

pub mod document;
pub mod scene;

use std::path::PathBuf;
use std::time::Instant;

use fynkel_core::{Extract, LoadedScene, World, WorldConfig};
use serde::{Deserialize, Serialize};

pub use document::{dump_json, dump_text, read_scene};
pub use scene::DemoScene;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeadlessConfig {
    /// Fixed ticks to simulate.
    pub steps: u64,
    pub seed: u64,
    /// Satellite cubes in the demo scene (plus one hub).
    pub satellites: u32,
    /// Host frame dt handed to `begin_frame`; defaults to one fixed step per
    /// frame. Larger values exercise catch-up and `MissedDeadline`.
    pub frame_dt: f32,
    pub fixed_dt: f32,
    pub max_steps_per_frame: u32,
    /// A `.scene.json` to load instead of the procedural demo scene; the
    /// world then runs `steps` ticks with no gameplay driver.
    pub scene: Option<PathBuf>,
    /// Directory scanned for `*.prefab.json` when the scene has prefab
    /// instances (none: instances fail with `FYN-D0401`).
    pub prefabs: Option<PathBuf>,
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        let world = WorldConfig::default();
        Self {
            steps: 600,
            seed: 42,
            satellites: 24,
            frame_dt: world.fixed_dt,
            fixed_dt: world.fixed_dt,
            max_steps_per_frame: world.max_steps_per_frame,
            scene: None,
            prefabs: None,
        }
    }
}

/// What loading a document scene produced (`HeadlessReport::scene`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SceneReport {
    pub path: String,
    pub loaded: bool,
    pub id: String,
    pub name: String,
    pub entities: u32,
    /// Component values the world rejected (see `diagnostics`).
    pub rejected_components: u32,
    /// §14.2 document diagnostics (validation and description).
    pub document_diagnostics: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Timing {
    pub p50_us: u32,
    pub p95_us: u32,
    pub max_us: u32,
}

/// What a headless run reports (`fynkel test --headless --json`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HeadlessReport {
    pub steps: u64,
    pub frames: u64,
    pub tick: u64,
    pub live_entities: u32,
    pub commands_applied: u64,
    pub commands_rejected: u64,
    pub events: u64,
    pub diagnostics: u64,
    pub missed_deadlines: u64,
    pub dropped_seconds: f32,
    pub extract_records: u32,
    /// FNV-1a 64 over the final extract's world matrices, hex. Equal runs
    /// (same config) must produce equal checksums (§5.6 determinism).
    pub checksum: String,
    pub step_time: Timing,
    pub total_ms: f32,
    pub scene_ready: bool,
    /// Present when `HeadlessConfig::scene` was set.
    pub scene: Option<SceneReport>,
}

impl HeadlessReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serialisable")
    }
}

/// Runs the demo scene (or `config.scene`) for `config.steps` fixed ticks.
pub fn run(config: &HeadlessConfig) -> HeadlessReport {
    run_with(config).0
}

/// [`run`] that also hands back the world and the loaded document scene
/// (for `fynkel state dump`).
pub fn run_with(config: &HeadlessConfig) -> (HeadlessReport, World, Option<LoadedScene>) {
    let start = Instant::now();
    let mut world = World::new(WorldConfig {
        fixed_dt: config.fixed_dt,
        max_steps_per_frame: config.max_steps_per_frame,
        ..WorldConfig::default()
    });
    let mut scene = DemoScene::new(config.satellites, config.seed);
    let mut report = HeadlessReport {
        steps: 0,
        frames: 0,
        tick: 0,
        live_entities: 0,
        commands_applied: 0,
        commands_rejected: 0,
        events: 0,
        diagnostics: 0,
        missed_deadlines: 0,
        dropped_seconds: 0.0,
        extract_records: 0,
        checksum: String::new(),
        step_time: Timing::default(),
        total_ms: 0.0,
        scene_ready: false,
        scene: None,
    };
    let mut step_us: Vec<u32> = Vec::with_capacity(config.steps as usize);

    // A document scene replaces the demo driver: loaded before tick 0,
    // then the world just ticks.
    let mut loaded: Option<LoadedScene> = None;
    if let Some(path) = &config.scene {
        let mut sr = SceneReport {
            path: path.display().to_string(),
            ..SceneReport::default()
        };
        match read_scene(path, config.prefabs.as_deref()) {
            Err(diags) => {
                sr.document_diagnostics = document::diagnostics_json(&diags);
                report.scene = Some(sr);
                report.total_ms = start.elapsed().as_secs_f32() * 1e3;
                return (report, world, None);
            }
            Ok(described) => {
                sr.document_diagnostics = document::diagnostics_json(&described.diagnostics);
                match world.load_scene(&described.description) {
                    Ok(l) => {
                        sr.loaded = true;
                        sr.id = l.id.to_string();
                        sr.name = l.name.clone();
                        sr.entities = l.len() as u32;
                        sr.rejected_components = l.rejected_components;
                        report.commands_rejected += l.rejected_components as u64;
                        report.diagnostics += world.diagnostics().len() as u64;
                        loaded = Some(l);
                    }
                    Err(e) => {
                        sr.document_diagnostics.push(serde_json::json!({
                            "code": "FYN-D0209",
                            "severity": "error",
                            "message": e.to_string(),
                            "document": path.display().to_string(),
                        }));
                        report.scene = Some(sr);
                        report.total_ms = start.elapsed().as_secs_f32() * 1e3;
                        return (report, world, None);
                    }
                }
            }
        }
        report.scene = Some(sr);
        scene = DemoScene::empty();
    }

    while report.steps < config.steps {
        let plan = world.begin_frame(config.frame_dt);
        report.frames += 1;
        if plan.dropped_seconds > 0.0 {
            report.missed_deadlines += 1;
            report.dropped_seconds += plan.dropped_seconds;
        }
        // Frame-level diagnostics (missed deadline) are cleared by the next
        // step, so count them here.
        report.diagnostics += world.diagnostics().len() as u64;
        for _ in 0..plan.steps {
            if report.steps >= config.steps {
                break;
            }
            let events = world.drain_events();
            report.events += events.len() as u64;
            scene.on_events(&events);
            let tick = world.tick();
            scene.fixed_update(&mut world, tick);
            let t0 = Instant::now();
            let r = world.step();
            step_us.push(t0.elapsed().as_micros() as u32);
            report.steps += 1;
            report.commands_applied += r.applied as u64;
            report.commands_rejected += r.rejected as u64;
            report.diagnostics += r.diagnostic_records as u64;
        }
        if plan.steps == 0 && config.frame_dt <= 0.0 {
            break; // nothing will ever advance
        }
    }
    let events = world.drain_events();
    report.events += events.len() as u64;
    scene.on_events(&events);

    let extract = world.extract();
    report.tick = world.tick();
    report.live_entities = world.live_entities();
    report.extract_records = extract.records.len() as u32;
    report.checksum = format!("{:016x}", checksum(&extract));
    report.scene_ready = match &loaded {
        Some(_) => scene.saw_scene_ready(),
        None => scene.is_ready(),
    };
    report.step_time = timing(&mut step_us);
    report.total_ms = start.elapsed().as_secs_f32() * 1e3;
    (report, world, loaded)
}

/// FNV-1a 64 over the extract's world matrices (f32 bit patterns) in
/// extract order. Bit-exact, so any nondeterminism shows up.
pub fn checksum(extract: &Extract) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(&extract.tick.to_le_bytes());
    for r in &extract.records {
        feed(&r.entity.to_bits().to_le_bytes());
        for v in r.world.to_cols_array() {
            feed(&v.to_bits().to_le_bytes());
        }
    }
    h
}

fn timing(v: &mut [u32]) -> Timing {
    if v.is_empty() {
        return Timing::default();
    }
    v.sort_unstable();
    let pct = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
    Timing {
        p50_us: pct(0.5),
        p95_us: pct(0.95),
        max_us: v[v.len() - 1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_are_deterministic_and_seed_dependent() {
        let cfg = HeadlessConfig {
            steps: 120,
            satellites: 8,
            ..HeadlessConfig::default()
        };
        let a = run(&cfg);
        let b = run(&cfg);
        assert_eq!(a.checksum, b.checksum);
        assert_eq!(a.tick, 120);
        assert_eq!(a.steps, 120);
        assert!(a.scene_ready, "hub and satellites spawned");
        assert_eq!(a.live_entities, 9);
        assert_eq!(a.extract_records, 9);
        assert_eq!(a.commands_rejected, 0);
        assert_eq!(a.diagnostics, 0);
        assert_eq!(a.missed_deadlines, 0);
        // tick 0: hub spawn; tick 1: 8 satellite spawns + hub move;
        // ticks 2..119: hub move + 8 satellite moves.
        assert_eq!(a.commands_applied, 1 + (8 + 1) + 118 * 9);

        let c = run(&HeadlessConfig { seed: 7, ..cfg });
        assert_ne!(a.checksum, c.checksum);
    }

    #[test]
    fn oversized_frames_drop_time_and_report_missed_deadlines() {
        let cfg = HeadlessConfig {
            steps: 40,
            satellites: 2,
            frame_dt: 0.2, // 12 ticks wanted, 4 allowed per frame
            ..HeadlessConfig::default()
        };
        let r = run(&cfg);
        assert_eq!(r.steps, 40);
        assert_eq!(r.frames, 10);
        assert_eq!(r.missed_deadlines, 10);
        assert!(r.dropped_seconds > 1.0);
        assert!(
            r.diagnostics >= 10,
            "one MissedDeadline diagnostic per frame"
        );
    }

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/fynkel_document/fixtures")
            .join(rel)
    }

    #[test]
    fn document_scene_loads_ticks_and_dumps_by_authoring_id() {
        let cfg = HeadlessConfig {
            steps: 5,
            scene: Some(fixture("valid/training_room.scene.json")),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let scene = report.scene.as_ref().unwrap();
        assert!(scene.loaded, "{:#?}", scene.document_diagnostics);
        assert_eq!((scene.entities, scene.rejected_components), (4, 0));
        assert_eq!(scene.name, "TrainingRoom");
        assert!(report.scene_ready);
        assert_eq!(report.tick, 5);
        assert_eq!(report.live_entities, 4);
        assert_eq!(
            (
                report.commands_applied,
                report.commands_rejected,
                report.diagnostics
            ),
            (0, 0, 0)
        );
        // Same document, same world: deterministic.
        assert_eq!(run(&cfg).checksum, report.checksum);

        let loaded = loaded.unwrap();
        let dump = dump_json(&world, &loaded);
        assert_eq!(dump["entityCount"], 4);
        assert_eq!(dump["tick"], 5);
        let text = dump.to_string();
        assert!(!text.contains("handle:"), "authoring ids only: {text}");
        assert_eq!(
            dump["entities"][1]["parent"],
            "f4d6d600-7931-44f5-a258-45b5a6850300"
        );
        assert_eq!(dump["entities"][1]["world"]["position"][1], 2.5);
        assert_eq!(
            dump["entities"][1]["components"][1]["type"],
            "fynkel.Camera"
        );
        assert_eq!(
            dump["entities"][1]["components"][1]["data"]["projection"],
            "perspective"
        );
        assert!(
            dump_text(&world, &loaded).contains("  Camera (a1b2c3d4-0000-4000-8000-000000000010)")
        );
    }

    #[test]
    fn invalid_document_scene_reports_and_loads_nothing() {
        let cfg = HeadlessConfig {
            steps: 5,
            scene: Some(fixture("scene/cycle.scene.json")),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let scene = report.scene.as_ref().unwrap();
        assert!(!scene.loaded);
        assert!(loaded.is_none());
        assert!(!report.scene_ready);
        assert_eq!(report.steps, 0);
        assert_eq!(world.live_entities(), 0);
        assert!(scene
            .document_diagnostics
            .iter()
            .any(|d| d["code"] == "FYN-D0203"));
        let missing = HeadlessConfig {
            scene: Some(fixture("nope.scene.json")),
            ..HeadlessConfig::default()
        };
        let r = run(&missing);
        assert_eq!(
            r.scene.unwrap().document_diagnostics[0]["code"],
            "FYN-D0900"
        );
    }

    #[test]
    fn report_serialises() {
        let r = run(&HeadlessConfig {
            steps: 3,
            satellites: 1,
            ..HeadlessConfig::default()
        });
        let json = r.to_json();
        let back: HeadlessReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}

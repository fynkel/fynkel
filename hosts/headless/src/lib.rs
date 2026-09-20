//! `fynkel_host_headless` — the Rust headless host (blueprint §5.2).
//!
//! No renderer, no GPU, no Dart: a test loop that owns the clock and calls
//! `step()` on `fynkel_core`, exactly like the other hosts, and reports what
//! happened as JSON. `fynkel test --headless` runs it; the Dart headless
//! game host (Behaviours) arrives with the Dart Behaviour lifecycle.

pub mod document;
pub mod recording;
pub mod scene;

use std::path::PathBuf;
use std::time::Instant;

use fynkel_assets::Assets;
use fynkel_contracts::{Diagnostic, Uuid};
use fynkel_core::{Extract, LoadedScene, World, WorldConfig};
use serde::{Deserialize, Serialize};

pub use document::{dump_json, dump_text};
pub use recording::{RecordedEvent, RecordedFrame, Recording};
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
    /// A cooked cache (`fynkel cook`) whose scene is loaded instead of the
    /// procedural demo scene; the world then runs `steps` ticks with no
    /// gameplay driver. The host reads the cache only (M3-03): source
    /// documents are never opened here.
    pub cache: Option<PathBuf>,
    /// The scene to load from `cache` (its asset id, text form); `None`
    /// loads the project's startup scene from the index.
    pub scene: Option<String>,
    /// An input recording (`*.input.json`) replayed frame by frame: each
    /// recorded frame is one host frame (its `dt`, its events captured
    /// before the ticks) and the run ends with the recording (or at
    /// `steps`, whichever comes first). The cooked project's
    /// `inputActions` are the binding table.
    pub input: Option<PathBuf>,
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
            cache: None,
            scene: None,
            input: None,
        }
    }
}

/// One action over a replayed recording (`InputReport::actions`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ActionReport {
    pub name: String,
    /// `button`, `axis1d`, `axis2d`.
    pub kind: String,
    /// Ticks that saw a `pressed` / `released` edge (each edge is seen by
    /// exactly one tick, §5.1).
    pub pressed: u64,
    pub released: u64,
    /// Ticks that saw the action held.
    pub held_ticks: u64,
    /// The state after the last frame.
    pub held: bool,
    pub x: f32,
    pub y: f32,
}

/// What replaying a recording produced (`HeadlessReport::input`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct InputReport {
    pub recording: String,
    /// Recorded frames replayed (a run capped by `steps` replays fewer).
    pub frames: u64,
    pub events: u64,
    pub actions: Vec<ActionReport>,
    /// `[action, binding, message]` per binding the table could not
    /// compile (`fynkel validate` reports these first).
    pub binding_errors: Vec<[String; 3]>,
}

/// What loading a cooked scene produced (`HeadlessReport::scene`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SceneReport {
    /// The cache directory the scene came from.
    pub cache: String,
    /// The scene's authoring path as the cache index records it.
    pub path: String,
    pub loaded: bool,
    pub id: String,
    pub name: String,
    /// The cooked object's cache key (hex).
    pub key: String,
    pub entities: u32,
    /// Component values the world rejected (see `diagnostics`).
    pub rejected_components: u32,
    /// Assets retained for the scene (its dependency closure).
    pub retained: u32,
    /// §14.2 document diagnostics and `FYN-R` runtime diagnostics.
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
    /// Present when `HeadlessConfig::cache` was set.
    pub scene: Option<SceneReport>,
    /// Present when `HeadlessConfig::input` was set.
    pub input: Option<InputReport>,
}

impl HeadlessReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serialisable")
    }
}

/// Opens the cache and reads the requested (or startup) scene.
fn read_cooked(
    cache: &std::path::Path,
    scene: Option<&str>,
) -> Result<(Assets, fynkel_assets::CookedScene), Vec<Diagnostic>> {
    let assets = Assets::open(cache).map_err(|d| vec![d])?;
    let id = match scene {
        Some(text) => text.parse::<Uuid>().map_err(|_| {
            vec![Diagnostic::error(
                fynkel_contracts::Code::RUNTIME_ASSET_MISSING,
                format!("`{text}` is not an asset id (UUID)"),
            )]
        })?,
        None => assets.startup_scene().map_err(|d| vec![d])?,
    };
    let cooked = assets.scene(id)?;
    Ok((assets, cooked))
}

/// Runs the demo scene (or the cooked scene from `config.cache`) for
/// `config.steps` fixed ticks.
pub fn run(config: &HeadlessConfig) -> HeadlessReport {
    run_with(config).0
}

/// [`run`] that also hands back the world and the loaded cooked scene
/// (for `fynkel state dump`). The world's assets stay resident for as
/// long as the returned residency lives.
pub fn run_with(config: &HeadlessConfig) -> (HeadlessReport, World, Option<LoadedScene>) {
    let (report, world, loaded, _residency) = run_retaining(config);
    (report, world, loaded)
}

/// [`run_with`] plus the store and the scene's residency.
pub fn run_retaining(
    config: &HeadlessConfig,
) -> (
    HeadlessReport,
    World,
    Option<LoadedScene>,
    Option<(Assets, fynkel_assets::Residency)>,
) {
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
        input: None,
    };
    let mut step_us: Vec<u32> = Vec::with_capacity(config.steps as usize);

    // A cooked scene replaces the demo driver: loaded before tick 0, then
    // the world just ticks.
    let mut loaded: Option<LoadedScene> = None;
    let mut retained: Option<(Assets, fynkel_assets::Residency)> = None;
    let mut binding_errors: Vec<[String; 3]> = Vec::new();
    // The recording is read before the scene so a bad file fails fast.
    let recording = match &config.input {
        Some(path) => match Recording::read(path) {
            Ok(r) => Some((path.display().to_string(), r)),
            Err(message) => {
                report.input = Some(InputReport {
                    recording: path.display().to_string(),
                    binding_errors: vec![[String::new(), String::new(), message]],
                    ..InputReport::default()
                });
                report.total_ms = start.elapsed().as_secs_f32() * 1e3;
                return (report, world, None, None);
            }
        },
        None => None,
    };
    if let Some(cache) = &config.cache {
        let mut sr = SceneReport {
            cache: cache.display().to_string(),
            ..SceneReport::default()
        };
        let (assets, cooked) = match read_cooked(cache, config.scene.as_deref()) {
            Err(diags) => {
                sr.document_diagnostics = document::diagnostics_json(&diags);
                report.scene = Some(sr);
                report.total_ms = start.elapsed().as_secs_f32() * 1e3;
                return (report, world, None, None);
            }
            Ok(ok) => ok,
        };
        sr.path = cooked.path.clone();
        sr.id = cooked.id.to_string();
        sr.name = cooked.name.clone();
        sr.key = cooked.key.clone();
        sr.document_diagnostics = document::diagnostics_json(&cooked.diagnostics);
        match world.load_scene(&cooked.description) {
            Ok(l) => {
                let residency = assets.retain(&cooked.closure);
                if let Some(p) = assets.project() {
                    binding_errors = world
                        .bind_input(&p.input_actions)
                        .into_iter()
                        .map(|(a, b, e)| [a, b, e.to_string()])
                        .collect();
                }
                sr.loaded = true;
                sr.entities = l.len() as u32;
                sr.rejected_components = l.rejected_components;
                sr.retained = residency.ids().len() as u32;
                report.commands_rejected += l.rejected_components as u64;
                report.diagnostics += world.diagnostics().len() as u64;
                loaded = Some(l);
                retained = Some((assets, residency));
            }
            Err(e) => {
                sr.document_diagnostics.push(serde_json::json!({
                    "code": "FYN-D0209",
                    "severity": "error",
                    "message": e.to_string(),
                    "document": sr.path,
                }));
                report.scene = Some(sr);
                report.total_ms = start.elapsed().as_secs_f32() * 1e3;
                return (report, world, None, None);
            }
        }
        report.scene = Some(sr);
        scene = DemoScene::empty();
    }

    let mut input = recording.as_ref().map(|(path, _)| InputReport {
        recording: path.clone(),
        actions: world
            .input()
            .actions()
            .map(|(name, kind)| ActionReport {
                name: name.to_string(),
                kind: kind.name().to_string(),
                ..ActionReport::default()
            })
            .collect(),
        binding_errors: binding_errors.clone(),
        frames: 0,
        events: 0,
    });
    let mut replay_frame = 0usize;
    while report.steps < config.steps {
        // A recording drives the frames: its dt, its events (captured
        // before the ticks, §5.1), and its end ends the run.
        let frame_dt = match &recording {
            Some((_, r)) => {
                if replay_frame >= r.frames.len() {
                    break;
                }
                r.frames[replay_frame].dt
            }
            None => config.frame_dt,
        };
        let plan = world.begin_frame(frame_dt);
        report.frames += 1;
        if let Some((_, r)) = &recording {
            let events = r.events(replay_frame);
            world.capture_input(&events);
            if let Some(ir) = &mut input {
                ir.frames += 1;
                ir.events += events.len() as u64;
            }
            replay_frame += 1;
        }
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
            if let Some(ir) = &mut input {
                for (a, s) in ir.actions.iter_mut().zip(world.input_view()) {
                    a.pressed += u64::from(s.pressed);
                    a.released += u64::from(s.released);
                    a.held_ticks += u64::from(s.held);
                }
            }
            let t0 = Instant::now();
            let r = world.step();
            step_us.push(t0.elapsed().as_micros() as u32);
            report.steps += 1;
            report.commands_applied += r.applied as u64;
            report.commands_rejected += r.rejected as u64;
            report.diagnostics += r.diagnostic_records as u64;
        }
        if plan.steps == 0 && config.frame_dt <= 0.0 && recording.is_none() {
            break; // nothing will ever advance
        }
    }
    if let Some(ir) = &mut input {
        for (a, s) in ir.actions.iter_mut().zip(world.input_view()) {
            a.held = s.held;
            a.x = s.x;
            a.y = s.y;
        }
    }
    report.input = input;
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
    (report, world, loaded, retained)
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

    /// The fixture project cooked into a cache of its own (removed on drop).
    pub(crate) struct Cooked(pub(crate) PathBuf);

    impl Cooked {
        pub(crate) fn new(name: &str) -> Cooked {
            let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/fynkel_document/fixtures/project");
            let dir =
                std::env::temp_dir().join(format!("fynkel-headless-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let report = fynkel_import::cook_project(
                &project,
                Some(&dir),
                &fynkel_import::CookOptions::default(),
            )
            .unwrap_or_else(|d| panic!("{d:#?}"));
            assert!(!report.has_errors(), "{}", report.text());
            Cooked(dir)
        }
    }

    impl Drop for Cooked {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cooked_scene_loads_ticks_and_dumps_by_authoring_id() {
        let cooked = Cooked::new("dump");
        let cfg = HeadlessConfig {
            steps: 5,
            cache: Some(cooked.0.clone()),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let scene = report.scene.as_ref().unwrap();
        assert!(scene.loaded, "{:#?}", scene.document_diagnostics);
        // Camera, Robot, Sun, the "Crate A" instance and its expanded crate.
        assert_eq!((scene.entities, scene.rejected_components), (5, 0));
        assert_eq!(scene.name, "Main");
        assert_eq!(scene.path, "scenes/main.scene.json");
        assert_eq!(scene.key.len(), 64);
        assert_eq!(scene.retained, 7);
        assert!(report.scene_ready);
        assert_eq!(report.tick, 5);
        assert_eq!(report.live_entities, 5);
        assert_eq!(
            (
                report.commands_applied,
                report.commands_rejected,
                report.diagnostics
            ),
            (0, 0, 0)
        );
        // Same cache, same world: deterministic.
        assert_eq!(run(&cfg).checksum, report.checksum);

        let loaded = loaded.unwrap();
        let dump = dump_json(&world, &loaded);
        assert_eq!(dump["entityCount"], 5);
        assert_eq!(dump["tick"], 5);
        let text = dump.to_string();
        assert!(!text.contains("handle:"), "authoring ids only: {text}");
        // The prefab's crate carries the id the cook derived for it, under
        // the instance entity.
        assert_eq!(
            dump["entities"][4]["parent"],
            "c0000000-0000-4000-8000-0000000000e2"
        );
        assert_ne!(
            dump["entities"][4]["id"], "c0000000-0000-4000-8000-0000000000b1",
            "the prefab's entity got a derived id, not its template id"
        );
        assert_eq!(
            dump["entities"][4]["components"][1]["data"]["cast_shadows"], false,
            "the instance override was applied by the cook"
        );
        assert_eq!(
            dump["entities"][1]["components"][1]["type"],
            "fynkel.MeshRenderer"
        );
        assert!(
            dump_text(&world, &loaded).contains("Camera (c0000000-0000-4000-8000-0000000000e0)")
        );

        // Any scene by id, not only the startup one.
        let by_id = HeadlessConfig {
            scene: Some("c0000000-0000-4000-8000-000000000030".into()),
            ..cfg.clone()
        };
        assert_eq!(run(&by_id).checksum, report.checksum);
    }

    #[test]
    fn missing_or_wrong_cache_reports_and_loads_nothing() {
        let cfg = HeadlessConfig {
            steps: 5,
            cache: Some(PathBuf::from("/nonexistent/fynkel/cache")),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let scene = report.scene.as_ref().unwrap();
        assert!(!scene.loaded);
        assert!(loaded.is_none());
        assert!(!report.scene_ready);
        assert_eq!(report.steps, 0);
        assert_eq!(world.live_entities(), 0);
        assert_eq!(scene.document_diagnostics[0]["code"], "FYN-R0001");

        let cooked = Cooked::new("wrong");
        let not_a_scene = HeadlessConfig {
            cache: Some(cooked.0.clone()),
            scene: Some("c0000000-0000-4000-8000-000000000040".into()),
            ..HeadlessConfig::default()
        };
        let r = run(&not_a_scene);
        assert_eq!(
            r.scene.unwrap().document_diagnostics[0]["code"],
            "FYN-R0006",
            "a prefab has no scene chunk"
        );
        let not_an_id = HeadlessConfig {
            scene: Some("robot".into()),
            ..not_a_scene
        };
        assert_eq!(
            run(&not_an_id).scene.unwrap().document_diagnostics[0]["code"],
            "FYN-R0003"
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

#[cfg(test)]
mod replay_tests {
    use super::*;

    #[test]
    fn a_recording_drives_the_projects_actions() {
        let cooked = tests::Cooked::new("replay");
        let recording = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/fynkel_document/fixtures/project/test/walk.input.json");
        let cfg = HeadlessConfig {
            steps: 1_000,
            cache: Some(cooked.0.clone()),
            input: Some(recording.clone()),
            ..HeadlessConfig::default()
        };
        let r = run(&cfg);
        assert!(r.scene.as_ref().is_some_and(|s| s.loaded), "{:?}", r.scene);
        let input = r.input.clone().expect("input report");
        assert_eq!(input.recording, recording.display().to_string());
        assert!(
            input.binding_errors.is_empty(),
            "{:?}",
            input.binding_errors
        );
        // Ten recorded frames: nine of one tick, one (50 ms) of three.
        assert_eq!(input.frames, 10, "{r:?}");
        assert_eq!(r.frames, 10);
        assert_eq!(r.steps, 12);
        assert_eq!(input.events, 9);
        let by_name = |n: &str| input.actions.iter().find(|a| a.name == n).unwrap().clone();
        let jump = by_name("jump");
        assert_eq!(jump.kind, "button");
        // Space across the three-tick frame: one press, three held ticks;
        // then the overlay press, released by the reset.
        assert_eq!((jump.pressed, jump.released), (2, 2));
        assert_eq!(jump.held_ticks, 3 + 1);
        assert!(!jump.held);
        let mv = by_name("move");
        assert_eq!(mv.kind, "axis2d");
        // Ticks 1–5 are frames 1–5, ticks 6–8 the 50 ms frame, ticks 9–12
        // frames 7–10. W is down for ticks 1–4, D for 3–8 (the catch-up
        // ticks included), the stick for 10–11; the reset ends it.
        assert_eq!(mv.held_ticks, 8 + 2);
        assert!(!mv.held && mv.x == 0.0 && mv.y == 0.0, "{mv:?}");
        // Same recording, same result (§5.6).
        assert_eq!(run(&cfg).input, Some(input));
    }

    #[test]
    fn a_bad_recording_is_reported_not_run() {
        let dir = std::env::temp_dir().join(format!("fynkel-replay-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.input.json");
        std::fs::write(
            &path,
            r#"{"fynkel":"input-recording","formatVersion":2,"frames":[]}"#,
        )
        .unwrap();
        let r = run(&HeadlessConfig {
            input: Some(path),
            ..HeadlessConfig::default()
        });
        let input = r.input.unwrap();
        assert_eq!(input.frames, 0);
        assert!(input.binding_errors[0][2].contains("formatVersion 2 is not 1"));
        assert_eq!(r.steps, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

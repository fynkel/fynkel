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
use fynkel_core::{EventPayload, Extract, LoadedScene, World, WorldConfig};
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
    /// Prefabs registered for the scene's `Emitter`s (M3-10).
    #[serde(default)]
    pub prefabs: u32,
    /// §14.2 document diagnostics and `FYN-R` runtime diagnostics.
    pub document_diagnostics: Vec<serde_json::Value>,
}

/// One `World::step` phase over the run (`HeadlessReport::systems`,
/// M3-10): the schema's `fynkel.SystemKind` name, what the last step
/// touched, and its wall time summed and at its worst over every step.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SystemReport {
    pub system: String,
    pub entities: u32,
    pub total_us: u64,
    pub max_us: u32,
    /// `total_us / steps`, for reading.
    pub mean_us: f32,
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
    /// `World::extract` once per frame, as a host does before it renders
    /// (M3-16); and the whole frame — `begin_frame`, the ticks with the
    /// scene's behaviour, the extract.
    #[serde(default)]
    pub extract_time: Timing,
    #[serde(default)]
    pub frame_time: Timing,
    pub total_ms: f32,
    pub scene_ready: bool,
    /// Present when `HeadlessConfig::cache` was set.
    pub scene: Option<SceneReport>,
    /// Present when `HeadlessConfig::input` was set.
    pub input: Option<InputReport>,
    /// Present when the world stepped any physics (M3-08).
    pub physics: Option<PhysicsReport>,
    /// Present when the scene registered a clip or a skin, or an animator
    /// ran (M3-19).
    #[serde(default)]
    pub animation: Option<AnimationReport>,
    /// Present when the scene registered a sound or a source played
    /// (M3-11).
    #[serde(default)]
    pub audio: Option<AudioReport>,
    /// Per-system time (M3-10), `World::system_times` accumulated over
    /// the run, in tick order (`step` first).
    #[serde(default)]
    pub systems: Vec<SystemReport>,
    /// Memory at the end of the run (M3-16, blueprint §15.4).
    #[serde(default)]
    pub memory: MemoryReport,
}

/// Memory at the end of a run (`HeadlessReport::memory`): the Rust heap
/// in use and its peak (the counting allocator — zeros in a binary that
/// does not install it), the process's resident set (0 where not
/// reported) and the CPU bytes of the resident assets.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MemoryReport {
    pub native_bytes: u64,
    pub native_peak_bytes: u64,
    pub resident_bytes: u64,
    pub residency_bytes: u64,
}

/// The audio of a run (`HeadlessReport::audio`, M3-11): the null backend
/// mixes nothing headless, the engine's clock still ends every clip on
/// its tick.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioReport {
    /// `AudioSource`s the audio system reconciled at the last tick.
    pub sources: u32,
    /// Sounds registered from the cooked scene (and its prefabs).
    pub sounds: u32,
    /// Of those, streamed (`STRM`; decoded as they play).
    pub streams: u32,
    /// Voices started, stolen and refused over the run (`AudioStats`).
    pub started: u32,
    pub stolen: u32,
    pub refused: u32,
    /// `PLAYBACK_ENDED` events over the run.
    pub ended: u64,
    /// `SOUND_UNAVAILABLE` and other audio diagnostics from the steps.
    pub diagnostics: u64,
}

/// The physics of a run (`HeadlessReport::physics`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PhysicsReport {
    pub bodies: u32,
    pub colliders: u32,
    /// Mesh colliders registered from the cooked scene.
    pub mesh_colliders: u32,
    /// `CharacterController`s stepped (M3-09).
    pub characters: u32,
    pub collision_enter: u64,
    pub collision_stay: u64,
    pub collision_exit: u64,
    pub trigger_enter: u64,
    pub trigger_exit: u64,
    /// `MESH_UNAVAILABLE` and other physics diagnostics from the steps.
    pub diagnostics: u64,
}

/// The animation of a run (`HeadlessReport::animation`, M3-19).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AnimationReport {
    /// `Animator`s and `Skin`s the animation system stepped at the last
    /// tick (its `SystemTimeRecord::entities`).
    pub entities: u32,
    /// Clips registered from the cooked scene (and its prefabs); a blend
    /// space (M3-19c) counts once, with its clips.
    pub clips: u32,
    /// Skins registered from the cooked scene (and its prefabs).
    pub skins: u32,
    /// Skinned meshes in the final extract, and the joint matrices they
    /// reference.
    pub skinned_meshes: u32,
    pub joints: u32,
    /// `ANIMATION_ENDED` events over the run.
    pub ended: u64,
    /// `ANIMATION_MARKER` events over the run (M3-19c).
    pub markers: u64,
    /// `CLIP_UNAVAILABLE` / `SKIN_UNAVAILABLE` and other animation
    /// diagnostics from the steps.
    pub diagnostics: u64,
}

impl HeadlessReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serialisable")
    }
}

fn count_ended(events: &[fynkel_core::Event]) -> u64 {
    events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::AnimationEnded { .. }))
        .count() as u64
}

fn count_playback_ended(events: &[fynkel_core::Event]) -> u64 {
    events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::PlaybackEnded { .. }))
        .count() as u64
}

fn count_markers(events: &[fynkel_core::Event]) -> u64 {
    events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::AnimationMarker { .. }))
        .count() as u64
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
        physics: None,
        audio: None,
        animation: None,
        systems: Vec::new(),
        step_time: Timing::default(),
        extract_time: Timing::default(),
        frame_time: Timing::default(),
        memory: MemoryReport::default(),
        total_ms: 0.0,
        scene_ready: false,
        scene: None,
        input: None,
    };
    let mut step_us: Vec<u32> = Vec::with_capacity(config.steps as usize);
    let mut extract_us: Vec<u32> = Vec::new();
    let mut frame_us: Vec<u32> = Vec::new();

    // A cooked scene replaces the demo driver: loaded before tick 0, then
    // the world just ticks.
    let mut loaded: Option<LoadedScene> = None;
    let mut retained: Option<(Assets, fynkel_assets::Residency)> = None;
    let mut binding_errors: Vec<[String; 3]> = Vec::new();
    let mut mesh_colliders = 0u32;
    let mut physics_diagnostics = 0u64;
    let mut registered_clips = 0u32;
    let mut registered_skins = 0u32;
    let mut animation_ended = 0u64;
    let mut animation_markers = 0u64;
    let mut animation_diagnostics = 0u64;
    let mut registered_sounds = 0u32;
    let mut playback_ended = 0u64;
    let mut audio_diagnostics = 0u64;
    let mut contact_counts = [0u64; 5];
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
        // Physics (M3-08): the project's gravity and the mesh colliders'
        // meshes, before the first step builds the bodies.
        if let Some(p) = assets.project() {
            world
                .physics_mut()
                .set_gravity(fynkel_core::physics::Vec3::from(p.physics.gravity));
        }
        let (meshes, mesh_diags) =
            assets.register_mesh_colliders(&cooked.description, world.physics_mut());
        mesh_colliders = meshes as u32;
        physics_diagnostics += mesh_diags.len() as u64;
        sr.document_diagnostics
            .extend(document::diagnostics_json(&mesh_diags));
        // The catalogue's prefabs (M3-10).
        let (prefabs, prefab_diags) = assets.register_prefabs(&cooked.description, &mut world);
        sr.prefabs = prefabs as u32;
        sr.document_diagnostics
            .extend(document::diagnostics_json(&prefab_diags));
        // The animators' clips and the skins (M3-19).
        let (clips, skins, anim_diags) = assets.register_animation(&cooked.description, &mut world);
        registered_clips = clips as u32;
        registered_skins = skins as u32;
        animation_diagnostics += anim_diags.len() as u64;
        sr.document_diagnostics
            .extend(document::diagnostics_json(&anim_diags));
        // The sources' sounds (M3-11).
        let (sounds, sound_diags) = assets.register_audio(&cooked.description, &mut world);
        registered_sounds = sounds as u32;
        audio_diagnostics += sound_diags.len() as u64;
        sr.document_diagnostics
            .extend(document::diagnostics_json(&sound_diags));
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
        let frame_start = Instant::now();
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
            animation_ended += count_ended(&events);
            animation_markers += count_markers(&events);
            playback_ended += count_playback_ended(&events);
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
            for (i, t) in world.system_times().iter().enumerate() {
                if report.systems.len() <= i {
                    report.systems.push(SystemReport {
                        system: t.system.name().to_string(),
                        ..SystemReport::default()
                    });
                }
                let s = &mut report.systems[i];
                s.entities = t.entities;
                s.total_us += u64::from(t.micros);
                s.max_us = s.max_us.max(t.micros);
            }
            report.commands_applied += r.applied as u64;
            report.commands_rejected += r.rejected as u64;
            report.diagnostics += r.diagnostic_records as u64;
            for e in world.after_physics_events() {
                match e.payload {
                    EventPayload::CollisionEnter(_) => contact_counts[0] += 1,
                    EventPayload::CollisionStay(_) => contact_counts[1] += 1,
                    EventPayload::CollisionExit(_) => contact_counts[2] += 1,
                    EventPayload::TriggerEnter(_) => contact_counts[3] += 1,
                    EventPayload::TriggerExit(_) => contact_counts[4] += 1,
                    _ => {}
                }
            }
            physics_diagnostics += world
                .diagnostics()
                .iter()
                .filter(|d| {
                    d.component == fynkel_contracts::ComponentId::RIGID_BODY
                        || d.component == fynkel_contracts::ComponentId::COLLIDER
                })
                .count() as u64;
            animation_diagnostics += world
                .diagnostics()
                .iter()
                .filter(|d| {
                    d.component == fynkel_contracts::ComponentId::ANIMATOR
                        || d.component == fynkel_contracts::ComponentId::SKIN
                })
                .count() as u64;
            audio_diagnostics += world
                .diagnostics()
                .iter()
                .filter(|d| {
                    d.component == fynkel_contracts::ComponentId::AUDIO_SOURCE
                        || d.code == fynkel_contracts::DiagnosticCode::SOUND_UNAVAILABLE
                        || d.code == fynkel_contracts::DiagnosticCode::AUDIO_DEVICE
                })
                .count() as u64;
        }
        // The frame's extract, as a host renders it (its records are what
        // the final checksum covers).
        let t0 = Instant::now();
        let extract = world.extract();
        extract_us.push(t0.elapsed().as_micros() as u32);
        std::hint::black_box(extract.records.len());
        frame_us.push(frame_start.elapsed().as_micros() as u32);
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
    for s in &mut report.systems {
        s.mean_us = s.total_us as f32 / report.steps.max(1) as f32;
    }
    let ps = world.physics().stats();
    if ps.bodies > 0 || ps.colliders > 0 || mesh_colliders > 0 {
        report.physics = Some(PhysicsReport {
            bodies: ps.bodies,
            colliders: ps.colliders,
            mesh_colliders,
            characters: ps.characters,
            collision_enter: contact_counts[0],
            collision_stay: contact_counts[1],
            collision_exit: contact_counts[2],
            trigger_enter: contact_counts[3],
            trigger_exit: contact_counts[4],
            diagnostics: physics_diagnostics,
        });
    }
    let events = world.drain_events();
    report.events += events.len() as u64;
    animation_ended += count_ended(&events);
    animation_markers += count_markers(&events);
    playback_ended += count_playback_ended(&events);
    scene.on_events(&events);

    let extract = world.extract();
    let animated = world
        .system_times()
        .iter()
        .find(|t| t.system == fynkel_core::schema::SystemKind::Animation)
        .map(|t| t.entities)
        .unwrap_or(0);
    let skinned_meshes = extract.meshes.iter().filter(|m| m.skin.is_some()).count() as u32;
    if animated > 0 || registered_clips > 0 || registered_skins > 0 || skinned_meshes > 0 {
        report.animation = Some(AnimationReport {
            entities: animated,
            clips: registered_clips,
            skins: registered_skins,
            skinned_meshes,
            joints: extract.joints.len() as u32,
            ended: animation_ended,
            markers: animation_markers,
            diagnostics: animation_diagnostics,
        });
    }
    let sources = world
        .system_times()
        .iter()
        .find(|t| t.system == fynkel_core::schema::SystemKind::Audio)
        .map(|t| t.entities)
        .unwrap_or(0);
    let audio_stats = world.audio_stats();
    if sources > 0 || registered_sounds > 0 || audio_stats.started > 0 || audio_diagnostics > 0 {
        report.audio = Some(AudioReport {
            sources,
            sounds: registered_sounds,
            streams: audio_stats.streams,
            started: audio_stats.started,
            stolen: audio_stats.stolen,
            refused: audio_stats.refused,
            ended: playback_ended,
            diagnostics: audio_diagnostics,
        });
    }
    report.tick = world.tick();
    report.live_entities = world.live_entities();
    report.extract_records = extract.records.len() as u32;
    report.checksum = format!("{:016x}", checksum(&extract));
    report.scene_ready = match &loaded {
        Some(_) => scene.saw_scene_ready(),
        None => scene.is_ready(),
    };
    report.step_time = timing(&mut step_us);
    report.extract_time = timing(&mut extract_us);
    report.frame_time = timing(&mut frame_us);
    report.memory = MemoryReport {
        native_bytes: fynkel_core::memory::heap_bytes(),
        native_peak_bytes: fynkel_core::memory::heap_peak_bytes(),
        resident_bytes: fynkel_core::memory::resident_bytes(),
        residency_bytes: retained.as_ref().map(|(a, _)| a.stats().bytes).unwrap_or(0),
    };
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
        // Camera, Robot, Sun, the "Crate A" instance and its expanded
        // crate, the physics Floor and Crate B (M3-08).
        assert_eq!((scene.entities, scene.rejected_components), (7, 0));
        assert_eq!(scene.name, "Main");
        assert_eq!(scene.path, "scenes/main.scene.json");
        assert_eq!(scene.key.len(), 64);
        assert_eq!(scene.retained, 8, "the Robot's tone (M3-11) too");
        assert!(report.scene_ready);
        assert_eq!(report.tick, 5);
        assert_eq!(report.live_entities, 7);
        // The Robot's `AudioSource` plays the (very short) tone from the
        // load and it ends within the run; the camera is the listener
        // (M3-11).
        let audio = report.audio.as_ref().expect("audio ran");
        assert_eq!(
            (
                audio.sources,
                audio.sounds,
                audio.started,
                audio.ended,
                audio.diagnostics
            ),
            (1, 1, 1, 1, 0),
            "{audio:#?}"
        );
        assert_eq!(world.audio().voice_count(), 0);
        assert_eq!(world.audio().listener_gain(), 1.0);
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
        assert_eq!(dump["entityCount"], 7);
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

    /// Blueprint §19.2d item 8 gate: the fixture's Crate B falls from
    /// y = 3 onto the static Floor and comes to rest, deterministically.
    #[test]
    fn fixture_crate_falls_onto_the_floor_deterministically() {
        let cooked = Cooked::new("physics");
        let cfg = HeadlessConfig {
            steps: 240,
            cache: Some(cooked.0.clone()),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let physics = report.physics.as_ref().expect("physics ran");
        assert_eq!((physics.bodies, physics.colliders), (1, 2));
        assert_eq!(physics.collision_enter, 1, "{physics:#?}");
        assert_eq!(physics.diagnostics, 0);
        assert_eq!(report.commands_rejected, 0);
        let loaded = loaded.unwrap();
        let crate_b = loaded
            .handle_of("c0000000-0000-4000-8000-0000000000e5".parse().unwrap())
            .unwrap();
        let t = fynkel_scene::local(world.ecs(), crate_b).unwrap();
        // Half extents 0.5 on a floor whose top is y = 0 (after the
        // restitution bounce settles).
        assert!((t.position.y - 0.5).abs() < 0.02, "{:?}", t.position);
        assert!((t.position.x + 2.0).abs() < 0.05, "{:?}", t.position);
        assert!(world.ecs().has::<fynkel_core::PhysicsOwned>(crate_b));
        assert!(
            world.physics().body_state(crate_b).unwrap().sleeping,
            "at rest after 4 s"
        );
        let again = run(&cfg);
        assert_eq!(again.checksum, report.checksum, "same inputs, same fall");
        assert_ne!(
            run(&HeadlessConfig {
                steps: 30,
                ..cfg.clone()
            })
            .checksum,
            report.checksum,
            "the crate is still falling at 0.5 s"
        );
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
mod catalogue_tests {
    use super::*;
    use glam::Vec3Swizzles;

    /// Blueprint §19.2d item 10: the fixture's Catalogue scene runs its
    /// six native components with no behaviour — the Beacon turns, the
    /// Buoy bobs, the Lamp follows it, the Sign faces the camera, the
    /// Puff despawns with its child at 1 s, the Fountain keeps 12 Sparks
    /// alive from the registered prefab — with per-system times in the
    /// report and a stable checksum.
    #[test]
    fn the_catalogue_scene_moves_without_a_behaviour() {
        let cooked = tests::Cooked::new("catalogue");
        let cfg = HeadlessConfig {
            steps: 300,
            cache: Some(cooked.0.clone()),
            scene: Some("c0000000-0000-4000-8000-000000000032".into()),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let scene = report.scene.as_ref().expect("scene report");
        assert!(scene.loaded, "{:#?}", scene.document_diagnostics);
        assert_eq!(scene.name, "Catalogue");
        assert_eq!((scene.entities, scene.rejected_components), (10, 0));
        assert_eq!(scene.prefabs, 1, "the Spark prefab was registered");
        assert_eq!(report.commands_rejected, 0);
        assert_eq!(report.diagnostics, 0);
        // Puff and its core gone; 12 Sparks alive, each an anchor + root.
        assert_eq!(report.live_entities, 10 - 2 + 12 * 2);
        let physics = report.physics.as_ref().expect("physics ran");
        assert_eq!(
            (physics.bodies, physics.colliders),
            (12, 13),
            "{physics:#?}"
        );
        assert!(physics.collision_enter > 0, "sparks land on the floor");

        let loaded = loaded.unwrap();
        let entity = |id: &str| loaded.handle_of(id.parse().unwrap()).unwrap();
        let ecs = world.ecs();
        let local = |e| fynkel_scene::local(ecs, e).unwrap();
        // 300 ticks at 1.5 rad/s: 7.5 rad about Y.
        let beacon = local(entity("c0000000-0000-4000-8000-0000000002e2"));
        let expected = glam::Quat::from_axis_angle(glam::Vec3::Y, 7.5);
        assert!(beacon.rotation.dot(expected).abs() > 0.999, "{beacon:?}");
        // 5 s at 0.5 Hz is 2.5 periods: the Buoy is back at its rest
        // height, 1 m.
        let buoy = local(entity("c0000000-0000-4000-8000-0000000002e4"));
        assert!((buoy.position.y - 1.0).abs() < 1e-2, "{buoy:?}");
        // The lamp trails the bobbing buoy (smoothing 5/s against a peak
        // speed of 1.57 m/s) on its axis, 2 m up.
        let lamp = local(entity("c0000000-0000-4000-8000-0000000002e5"));
        assert!(
            (lamp.position.xz()).length() < 1e-4 && (lamp.position.y - 3.0).abs() < 0.4,
            "the lamp follows the buoy: {lamp:?}"
        );
        let sign = local(entity("c0000000-0000-4000-8000-0000000002e6"));
        let facing = sign.rotation * glam::Vec3::Z;
        let to_camera =
            (glam::Vec3::new(0.0, 4.0, 10.0) - sign.position) * glam::Vec3::new(1.0, 0.0, 1.0);
        assert!(facing.dot(to_camera.normalize()) > 0.999, "{facing:?}");
        assert!(loaded
            .handle_of("c0000000-0000-4000-8000-0000000002e7".parse().unwrap())
            .is_some_and(|e| !ecs.contains(e)));
        assert!(loaded
            .handle_of("c0000000-0000-4000-8000-0000000002e8".parse().unwrap())
            .is_some_and(|e| !ecs.contains(e)));

        let by_name = |n: &str| report.systems.iter().find(|s| s.system == n).unwrap();
        assert_eq!(report.systems[0].system, "step");
        assert_eq!(report.systems.len(), 15);
        assert_eq!(by_name("rotator").entities, 1);
        assert_eq!(by_name("oscillate").entities, 1);
        assert_eq!(by_name("emitter").entities, 1);
        assert_eq!(by_name("follow").entities, 1);
        assert_eq!(by_name("lookAtCamera").entities, 1);
        // Counted before this tick's emission.
        assert!((11..=12).contains(&by_name("lifetimeDespawn").entities));
        assert!(by_name("physics").total_us > 0);
        assert!(by_name("step").total_us >= by_name("physics").total_us);

        assert_eq!(run(&cfg).checksum, report.checksum, "deterministic");
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

    /// Blueprint §19.2d item 9 gate: from a recorded stream the fixture's
    /// Player walks toward the Platform, jumps at frame 20, lands on its
    /// top (y = 0.8) and stops when W is released, with a stable checksum;
    /// the ThirdPersonCamera sits behind and above it.
    #[test]
    fn a_character_walks_jumps_and_lands_on_the_platform_from_a_recording() {
        let cooked = tests::Cooked::new("character");
        let recording = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/fynkel_document/fixtures/project/test/platform.input.json");
        let cfg = HeadlessConfig {
            steps: 1_000,
            cache: Some(cooked.0.clone()),
            scene: Some("c0000000-0000-4000-8000-000000000031".into()),
            input: Some(recording),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let scene = report.scene.as_ref().expect("scene report");
        assert!(scene.loaded, "{:#?}", scene.document_diagnostics);
        assert_eq!(scene.name, "Playground");
        assert_eq!((scene.entities, scene.rejected_components), (5, 0));
        assert_eq!((report.frames, report.steps), (120, 120));
        assert_eq!(report.commands_rejected, 0);
        let physics = report.physics.as_ref().expect("physics ran");
        // The character's kinematic capsule counts among the bodies and
        // colliders (Floor, Platform, capsule).
        assert_eq!(
            (physics.bodies, physics.colliders, physics.characters),
            (1, 3, 1),
            "{physics:#?}"
        );
        assert_eq!(physics.diagnostics, 0);
        let input = report.input.as_ref().unwrap();
        let by_name = |n: &str| input.actions.iter().find(|a| a.name == n).unwrap();
        assert_eq!(by_name("jump").pressed, 1);
        assert_eq!(by_name("move").held_ticks, 75);
        assert!(!by_name("move").held);

        let loaded = loaded.unwrap();
        let entity = |id: &str| loaded.handle_of(id.parse().unwrap()).unwrap();
        let player = entity("c0000000-0000-4000-8000-0000000001e1");
        let camera = entity("c0000000-0000-4000-8000-0000000001e0");
        let state = world
            .physics()
            .character_state(player)
            .expect("a character");
        assert!(state.grounded && !state.sliding, "{state:?}");
        assert_eq!(state.velocity, glam::Vec3::ZERO, "stopped: W released");
        let t = fynkel_scene::local(world.ecs(), player).unwrap();
        // Resting `skin_width` above the Platform's top (y = 0.4 + 0.4),
        // well inside its footprint (z in −6.5..−2.5), never off the −Z
        // line, facing −Z.
        assert!((t.position.y - 0.82).abs() < 5e-3, "{:?}", t.position);
        assert!(
            t.position.z < -3.0 && t.position.z > -6.0,
            "{:?}",
            t.position
        );
        assert!(t.position.x.abs() < 1e-4, "{:?}", t.position);
        assert!(
            t.rotation.abs_diff_eq(glam::Quat::IDENTITY, 1e-3),
            "{:?}",
            t.rotation
        );
        // Like a kinematic body, a character follows external transform
        // writes (a teleport), so it is not physics-owned.
        assert!(!world.ecs().has::<fynkel_core::PhysicsOwned>(player));

        // The rig: pitch 0.35, yaw 0, distance 5 from the pivot 1.5 m up.
        let c = fynkel_scene::local(world.ecs(), camera).unwrap();
        let pivot = t.position + glam::Vec3::new(0.0, 1.5, 0.0);
        let expected = pivot + glam::Vec3::new(0.0, 0.35f32.sin(), 0.35f32.cos()) * 5.0;
        assert!(
            c.position.abs_diff_eq(expected, 1e-3),
            "{:?} vs {expected:?}",
            c.position
        );
        let forward = c.rotation * glam::Vec3::NEG_Z;
        assert!((forward.normalize() - (pivot - c.position).normalize()).length() < 1e-3);

        // Same recording, same landing (§5.6); a shorter run is mid-air.
        assert_eq!(run(&cfg).checksum, report.checksum);
        let airborne = run_with(&HeadlessConfig {
            steps: 40,
            ..cfg.clone()
        });
        assert_ne!(airborne.0.checksum, report.checksum);
        let mid = airborne.1.physics().character_state(player).unwrap();
        assert!(!mid.grounded && mid.velocity.y != 0.0, "{mid:?}");
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

#[cfg(test)]
mod animation_tests {
    use super::*;

    /// Blueprint §19.2d item 19: the fixture's Rig scene animates without
    /// a behaviour — the `Animator` on Rig plays `wave` over the joints
    /// found by name below it, the Prop's non-looping `bob` ends once, the
    /// Body's skin resolves and the extract carries its palette — with a
    /// stable checksum. Item 19a: the scene's `Imported` entity instances
    /// the same rig from the source's own hierarchy, waving beside the
    /// hand-authored one over its own six joints — since 19b through the
    /// Column rig document (`column.rig.json#prefab`, its `anim/wave` a
    /// third registered clip with the source's `wave` bytes).
    #[test]
    fn the_rig_scene_animates_and_skins_without_a_behaviour() {
        let cooked = tests::Cooked::new("rig");
        let cfg = HeadlessConfig {
            // The first tick samples t = 0; thirty more reach 0.5 s.
            steps: 31,
            cache: Some(cooked.0.clone()),
            scene: Some("c0000000-0000-4000-8000-000000000034".into()),
            ..HeadlessConfig::default()
        };
        let (report, world, loaded) = run_with(&cfg);
        let scene = report.scene.as_ref().expect("scene report");
        assert!(scene.loaded, "{:#?}", scene.document_diagnostics);
        assert_eq!((scene.entities, scene.rejected_components), (15, 0));
        assert_eq!(report.diagnostics, 0);
        let anim = report.animation.as_ref().expect("animation report");
        assert_eq!(
            (
                anim.entities,
                anim.clips,
                anim.skins,
                anim.skinned_meshes,
                anim.joints
            ),
            (5, 3, 1, 2, 6),
            "{anim:#?}"
        );
        assert_eq!(anim.ended, 0, "bob (2 s) has not ended at 0.5 s");
        assert_eq!(anim.diagnostics, 0);

        // Half a second in (31 ticks at 1/60): hips' cubic 0 → 0.25 with
        // zero tangents is at its midpoint, spine is at its 45° key, head
        // (step) has reached its 90° key, the Prop's linear bob at 0.15.
        let loaded = loaded.unwrap();
        let local = |id: &str| {
            let e = loaded.handle_of(id.parse().unwrap()).unwrap();
            fynkel_scene::local(world.ecs(), e).unwrap()
        };
        let hips = local("c0000000-0000-4000-8000-0000000004e4");
        assert!((hips.position.y - 0.125).abs() < 1e-3, "{hips:?}");
        let spine = local("c0000000-0000-4000-8000-0000000004e5");
        let (axis, angle) = spine.rotation.to_axis_angle();
        assert!(
            (angle - std::f32::consts::FRAC_PI_4).abs() < 1e-3 && axis.z > 0.99,
            "{spine:?}"
        );
        let head = local("c0000000-0000-4000-8000-0000000004e6");
        let (axis, angle) = head.rotation.to_axis_angle();
        assert!(
            (angle - std::f32::consts::FRAC_PI_2).abs() < 1e-3 && axis.y > 0.99,
            "{head:?}"
        );
        let prop = local("c0000000-0000-4000-8000-0000000004e7");
        assert!((prop.position.y - 0.15).abs() < 1e-3, "{prop:?}");

        // The palette: joint world × inverse bind, in mesh space. The hips
        // joint has moved up from its bind pose (0.119 at tick 29, 0.125 at
        // tick 30 — the extract interpolates between the two), so its
        // matrix is that translation; the animator's own state reads back.
        let extract = world.extract();
        let skinned: Vec<_> = extract.meshes.iter().filter(|m| m.skin.is_some()).collect();
        assert_eq!(skinned.len(), 2, "the authored Body and the imported one");
        let skin = skinned[0].skin.unwrap();
        assert_eq!((skin.first_joint, skin.joint_count), (0, 3));
        let hips_palette = extract.joints[0].transform_point3(glam::Vec3::ZERO);
        assert!(
            hips_palette.x.abs() < 1e-5
                && hips_palette.z.abs() < 1e-5
                && (0.118..=0.1251).contains(&hips_palette.y),
            "{hips_palette:?}"
        );
        // The imported rig's palette is the same pose (its own animator,
        // its own joints found by name under the instance); the palette is
        // in mesh space, so the instance's x = −2 does not appear in it.
        let skin = skinned[1].skin.unwrap();
        assert_eq!((skin.first_joint, skin.joint_count), (3, 3));
        let imported_hips = extract.joints[3].transform_point3(glam::Vec3::ZERO);
        assert!(
            (imported_hips - hips_palette).length() < 1e-5,
            "{imported_hips:?} vs {hips_palette:?}"
        );
        let rig = loaded
            .handle_of("c0000000-0000-4000-8000-0000000004e2".parse().unwrap())
            .unwrap();
        let state = world.animator_state(rig).unwrap();
        assert!((state.time - 0.5).abs() < 1e-4 && state.playing && state.looping);
        assert!((state.duration - 1.0).abs() < 1e-5);

        // The Prop's bob ends at 2 s: one ANIMATION_ENDED, then it holds.
        let long = run(&HeadlessConfig {
            steps: 150,
            ..cfg.clone()
        });
        let anim = long.animation.as_ref().unwrap();
        assert_eq!(anim.ended, 1, "{anim:#?}");
        assert_eq!(run(&cfg).checksum, report.checksum, "deterministic");
    }
}

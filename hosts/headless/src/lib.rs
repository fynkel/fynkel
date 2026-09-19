//! `fynkel_host_headless` — the Rust headless host (blueprint §5.2).
//!
//! No renderer, no GPU, no Dart: a test loop that owns the clock and calls
//! `step()` on `fynkel_core`, exactly like the other hosts, and reports what
//! happened as JSON. `fynkel test --headless` runs it; the Dart headless
//! game host (Behaviours) arrives with the Dart Behaviour lifecycle.

pub mod scene;

use std::time::Instant;

use fynkel_core::{Extract, World, WorldConfig};
use serde::{Deserialize, Serialize};

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
        }
    }
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
}

impl HeadlessReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serialisable")
    }
}

/// Runs the demo scene for `config.steps` fixed ticks.
pub fn run(config: &HeadlessConfig) -> HeadlessReport {
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
    };
    let mut step_us: Vec<u32> = Vec::with_capacity(config.steps as usize);

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
    report.scene_ready = scene.is_ready();
    report.step_time = timing(&mut step_us);
    report.total_ms = start.elapsed().as_secs_f32() * 1e3;
    report
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

//! Native-only cooked-scene diagnostic: no Dart callbacks, rendering or catch-up.
//! cargo run --release -p fynkel_host_headless --example physics_profile -- CACHE
use fynkel_assets::Assets;
use fynkel_core::{World, WorldConfig};
use std::time::Instant;

fn main() {
    let cache = std::env::args().nth(1).expect("cooked cache path");
    let assets = Assets::open(std::path::Path::new(&cache)).expect("open cache");
    let cooked = assets
        .scene(assets.startup_scene().expect("startup scene"))
        .expect("scene");
    let _resident = assets.retain(&cooked.closure);
    let mut outcomes = Vec::new();
    let mut runs = Vec::new();
    for enabled in [false, true] {
        let mut world = World::new(WorldConfig::default());
        if let Some(project) = assets.project() {
            world
                .physics_mut()
                .set_gravity(project.physics.gravity.into());
        }
        let (_, diagnostics) =
            assets.register_mesh_colliders(&cooked.description, world.physics_mut());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let (_, diagnostics) = assets.register_prefabs(&cooked.description, &mut world);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let (_, _, diagnostics) = assets.register_animation(&cooked.description, &mut world);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let (_, diagnostics) = assets.register_audio(&cooked.description, &mut world);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let loaded = world.load_scene(&cooked.description).expect("load scene");
        assert_eq!(loaded.rejected_components, 0);
        for _ in 0..40 {
            world.step();
        }
        world.set_physics_profiling(enabled);
        let mut samples = Vec::new();
        for _ in 0..300 {
            let start = Instant::now();
            world.step();
            let step_us = start.elapsed().as_secs_f64() * 1e6;
            let profile = world.physics().step_profile();
            samples.push(serde_json::json!({
                "stepUs": step_us,
                "corePhysicsUs": world.physics_system_profile().map(|p| {
                    [p.propagation, p.adapter, p.publication, p.total].map(|d| d.as_secs_f64() * 1e6)
                }),
                "syncUs": profile.map(|p| {
                    let s = p.synchronization;
                    [s.collect, s.hierarchy, s.classify, s.apply].map(|d| d.as_secs_f64() * 1e6)
                }),
                "physicsUs": profile.map(|p| {
                    [p.sync, p.commands, p.characters, p.solver, p.writeback, p.events, p.total]
                        .map(|d| d.as_secs_f64() * 1e6)
                }),
            }));
        }
        // Compare gameplay state, excluding world-specific entity IDs and timings.
        let extract = world.extract();
        outcomes.push(
            extract
                .records
                .iter()
                .map(|r| {
                    (
                        r.world.to_cols_array(),
                        world.physics().body_state(r.entity),
                        world.physics().character_state(r.entity),
                    )
                })
                .collect::<Vec<_>>(),
        );
        runs.push(serde_json::json!({"profiling": enabled, "samples": samples}));
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "profiling changed simulation output"
    );
    println!(
        "{}",
        serde_json::json!({
            "scope": "Mac native-only cooked scene; no Dart callbacks, rendering or catch-up",
            "warmupTicks": 40, "measuredTicks": 300,
            "phaseOrder": ["sync", "commands", "characters", "solver", "writeback", "events", "total"],
            "corePhysicsPhaseOrder": ["propagation", "adapter", "publication", "total"],
            "coreAdapterContainsPhysicsPhases": true,
            "syncPhaseOrder": ["collect", "hierarchy", "classify", "apply"],
            "syncPhasesContainedInSync": true,
            "phasesOverlap": false, "totalIncludesTimerOverhead": true,
            "profiledAndUnprofiledTransformsAndMotionEqual": true, "runs": runs,
        })
    );
}

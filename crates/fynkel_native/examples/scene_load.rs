//! A level load, sync against async (ADR-0038 step 4): how long the game
//! stops. A cooked project's scene is loaded on a fresh world the old way
//! (`fynkel_world_load_scene`, the whole load in one frame) and the new
//! way (`fynkel_world_load_scene_async`: the world steps each frame while
//! a background thread loads, then `fynkel_world_load_activate` in one
//! frame). Each run opens the cache afresh, so both read every payload
//! from disk (the OS file cache is warm after the first round for both).
//! No GPU: the renderer's uploads come on top of the sync load's first
//! frames and, with a background load, are spread over the loading frames
//! instead. See `benchmarks/scene_load/README.md`.
//!
//! ```sh
//! cargo run --release -p fynkel_native --example scene_load -- <project dir> [scene path]
//! ```
//!
//! The project is cooked into a temporary cache; use a copy of an
//! example (the cook may write sidecars next to its assets).

use std::path::Path;
use std::ptr;
use std::time::{Duration, Instant};

use fynkel_contracts::Status;
use fynkel_import::{cook_project, CookOptions};
use fynkel_native::load::*;
use fynkel_native::*;

const ROUNDS: usize = 5;
/// A 60 Hz frame.
const FRAME: Duration = Duration::from_micros(16_667);

fn main() {
    let mut args = std::env::args().skip(1);
    let project = args.next().expect("the project directory");
    let wanted = args
        .next()
        .unwrap_or_else(|| "scenes/playground.scene.json".into());
    let cache = std::env::temp_dir().join("fynkel-scene-load-cache");
    let _ = std::fs::remove_dir_all(&cache);
    let report = cook_project(Path::new(&project), Some(&cache), &CookOptions::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    assert!(!report.has_errors(), "{}", report.text());
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("index.json")).unwrap()).unwrap();
    let scene = index["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["path"] == wanted.as_str())
        .unwrap_or_else(|| panic!("{wanted} is not in the cache"))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let cache = cache.display().to_string();
    println!("{wanted} ({scene}), {ROUNDS} rounds each, release build\n");

    let mut sync = Vec::new();
    let mut runs = Vec::new();
    for _ in 0..ROUNDS {
        sync.push(load_sync(&cache, &scene));
        runs.push(load_async(&cache, &scene));
    }
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let median = |mut v: Vec<Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    let sync_ms = ms(median(sync));
    let longest = ms(median(runs.iter().map(|r| r.longest_step).collect()));
    let background = ms(median(runs.iter().map(|r| r.background).collect()));
    let activation = ms(median(runs.iter().map(|r| r.activation).collect()));
    let frames = median(
        runs.iter()
            .map(|r| Duration::from_nanos(r.frames as u64))
            .collect(),
    )
    .as_nanos();
    println!("sync:  the load call (one frame)          {sync_ms:8.2} ms");
    println!(
        "async: background work                    {background:8.2} ms ({frames} frames at 60 Hz)"
    );
    println!("async: longest frame while loading        {longest:8.2} ms (a step)");
    println!("async: the activation (one frame)         {activation:8.2} ms");
    println!(
        "\nthe longest stop: {sync_ms:.2} ms → {:.2} ms",
        activation.max(longest)
    );
}

struct World {
    rt: *mut FynkelRuntime,
    w: *mut FynkelWorld,
}

impl World {
    fn new() -> Self {
        let rt = fynkel_runtime_create();
        let w = unsafe { fynkel_world_create(rt, ptr::null(), std::ptr::null_mut()) };
        assert!(!w.is_null());
        World { rt, w }
    }

    fn step(&self) -> Duration {
        let started = Instant::now();
        let mut plan = FynkelFramePlan::default();
        assert_eq!(
            unsafe { fynkel_world_begin_frame(self.w, 1.0 / 60.0, 1.0, &mut plan) },
            Status::Ok
        );
        for _ in 0..plan.steps {
            let mut r = FynkelStepResult::default();
            assert_eq!(unsafe { fynkel_world_step(self.w, &mut r) }, Status::Ok);
        }
        started.elapsed()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        unsafe {
            fynkel_world_destroy(self.w);
            fynkel_runtime_destroy(self.rt);
        }
    }
}

fn load_sync(cache: &str, scene: &str) -> Duration {
    let world = World::new();
    let started = Instant::now();
    let s = unsafe {
        fynkel_world_load_scene(
            world.w,
            cache.as_ptr(),
            cache.len() as u32,
            scene.as_ptr(),
            scene.len() as u32,
        )
    };
    let took = started.elapsed();
    assert_eq!(s, Status::Ok);
    took
}

struct AsyncRun {
    background: Duration,
    frames: usize,
    longest_step: Duration,
    activation: Duration,
}

fn load_async(cache: &str, scene: &str) -> AsyncRun {
    let world = World::new();
    let started = Instant::now();
    let mut ticket = 0u32;
    let s = unsafe {
        fynkel_world_load_scene_async(
            world.w,
            cache.as_ptr(),
            cache.len() as u32,
            scene.as_ptr(),
            scene.len() as u32,
            &mut ticket,
        )
    };
    assert_eq!(s, Status::Ok);
    let (mut frames, mut longest_step) = (0, Duration::ZERO);
    loop {
        let (mut state, mut progress) = (0u32, 0f32);
        unsafe { fynkel_world_load_progress(world.w, ticket, &mut state, &mut progress) };
        if state != FYNKEL_LOAD_LOADING {
            assert_eq!(state, FYNKEL_LOAD_READY);
            break;
        }
        // One frame: the game steps, then waits for the display.
        let frame = Instant::now();
        longest_step = longest_step.max(world.step());
        frames += 1;
        std::thread::sleep(FRAME.saturating_sub(frame.elapsed()));
    }
    let background = started.elapsed();
    let activated = Instant::now();
    assert_eq!(
        unsafe { fynkel_world_load_activate(world.w, ticket, 0) },
        Status::Ok
    );
    AsyncRun {
        background,
        frames,
        longest_step,
        activation: activated.elapsed(),
    }
}

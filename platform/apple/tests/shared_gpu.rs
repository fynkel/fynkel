//! Viewport ABI 10 (ADR-0030 §4): every viewport of the process draws with
//! one shared GPU device.
//!
//! `viewports_share_one_device` holds the behaviour: two viewports get the
//! same wgpu device (one `MTLCommandQueue`; the `MTLDevice` is the
//! system's one per GPU either way), both draw, the device outlives neither and a new one
//! is made after the last goes.
//!
//! `measure` is the measurement the ADR asks for (run it alone, in
//! release, with the display awake):
//!
//! ```sh
//! caffeinate -du cargo test --release -p fynkel_platform_apple --test shared_gpu -- --ignored --nocapture measure
//! ```
//!
//! It runs the same session twice, each in a fresh process (a device's
//! memory is the process's; Metal keeps caches once warm): one device per
//! viewport (before ABI 10), then one shared device. A session is the
//! editor's case: a Scene viewport (1600×1000, 400 lit, shadowed cubes),
//! then three Inspector previews (800×480) added one at a time, each timed
//! from `create` to its first frame on screen; then 240 frames of the Scene
//! view with a preview drawing beside it. Sharing stays if a second
//! viewport comes up in under 50 ms, memory grows less than with a device
//! each, and the Scene view's frame time changes by 5 % or less. The
//! numbers go into `docs/perf/shared-gpu.md`.

use std::process::Command;
use std::time::Instant;

use fynkel_contracts::Entity;
use fynkel_core::{
    Camera, DirectionalLight, Environment, Extract, ExtractCamera, ExtractDirectionalLight,
    ExtractEnvironment, ExtractRecord,
};
use fynkel_platform_apple::{DeviceChoice, Gpu, SyncMode, Viewport};
use glam::{Mat4, Vec3};

fn record(slot: u32, world: Mat4) -> ExtractRecord {
    ExtractRecord {
        entity: Entity::from_bits(slot as u64 + 1),
        world,
        overridden: false,
        layer_bit: 1,
    }
}

/// A camera over a 20×20 grid of placeholder cubes, a shadowing sun and an
/// ambient: the renderer's whole frame (shadows, AO, post) on real work.
fn scene(cubes: u32) -> Extract {
    let mut e = Extract::default();
    // From (0, 14, 22), tilted down to the grid's centre.
    let eye = Mat4::from_rotation_translation(
        glam::Quat::from_rotation_x(-(14.0f32 / 22.0).atan()),
        Vec3::new(0.0, 14.0, 22.0),
    );
    e.records.push(record(0, eye));
    let side = (cubes as f32).sqrt().ceil() as u32;
    for i in 0..cubes {
        let (x, z) = ((i % side) as f32, (i / side) as f32);
        let at = Vec3::new(
            (x - side as f32 / 2.0) * 1.6,
            0.0,
            (z - side as f32 / 2.0) * 1.6,
        );
        e.records.push(record(i + 1, Mat4::from_translation(at)));
    }
    e.camera = Some(ExtractCamera {
        record: 0,
        world: eye,
        camera: Camera::default(),
    });
    e.directional_lights.push(ExtractDirectionalLight {
        record: 0,
        direction: Vec3::new(-0.4, -1.0, -0.3).normalize(),
        light: DirectionalLight {
            color: Vec3::ONE,
            intensity: 20_000.0,
            cast_shadows: true,
        },
    });
    e.environment = Some(ExtractEnvironment {
        record: 0,
        environment: Environment {
            ambient_color: Vec3::ONE,
            ambient_intensity: 5_000.0,
            sky: None,
            sky_intensity: 0.0,
            sky_rotation: 0.0,
        },
    });
    e
}

fn viewport(choice: DeviceChoice, width: u32, height: u32) -> Viewport {
    let gpu = Gpu::get(choice).expect("gpu");
    let mut vp =
        Viewport::with_gpu(gpu, width, height, 4, SyncMode::MetalCompletion).expect("viewport");
    vp.set_placeholders(true);
    vp
}

/// Draws one frame and waits until it is on screen (the host's
/// presenter takes it).
fn frame(vp: &mut Viewport, extract: &Extract) {
    assert!(vp.render(extract) >= 0, "a free slot");
    vp.ring().wait_idle();
    assert!(vp.ring().acquire_presented() >= 0);
}

#[test]
fn viewports_share_one_device() {
    if std::env::var("FYNKEL_VIEWPORT_DEVICE").as_deref() == Ok("own") {
        return;
    }
    let extract = scene(16);
    let mut a = Viewport::new(64, 48, 3, SyncMode::MetalCompletion).expect("a");
    let mut b = Viewport::new(32, 32, 3, SyncMode::MetalCompletion).expect("b");
    a.set_placeholders(true);
    b.set_placeholders(true);
    let device = a.ring().gpu().mtl_queue_ptr();
    assert_eq!(device, b.ring().gpu().mtl_queue_ptr(), "one device");
    // Each draws into its own ring, at its own size.
    frame(&mut a, &extract);
    frame(&mut b, &extract);
    b.resize(48, 40).expect("resize");
    frame(&mut b, &extract);
    frame(&mut a, &extract);

    // A device of its own is another one.
    let own = Gpu::get(DeviceChoice::Own).expect("own");
    assert_ne!(own.mtl_queue_ptr(), device);
    drop(own);

    // The device stays while any viewport holds it.
    drop(a);
    let c = Viewport::new(16, 16, 2, SyncMode::MetalCompletion).expect("c");
    assert_eq!(c.ring().gpu().mtl_queue_ptr(), device);
    drop((b, c));
}

/// The process's physical footprint (what Activity Monitor calls Memory:
/// GPU allocations included on Apple silicon), from `proc_pid_rusage`.
fn footprint() -> u64 {
    #[repr(C)]
    struct RusageInfoV0 {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        proc_start_abstime: u64,
        proc_exit_abstime: u64,
    }
    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV0) -> i32;
    }
    let mut info = std::mem::MaybeUninit::<RusageInfoV0>::zeroed();
    let r = unsafe { proc_pid_rusage(std::process::id() as i32, 0, info.as_mut_ptr()) };
    assert_eq!(r, 0, "proc_pid_rusage");
    unsafe { info.assume_init() }.phys_footprint
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// One session in this process, by `FYNKEL_GPU_MEASURE` (`own` |
/// `shared`); prints one `MEASURE` line for [`measure`] to read.
#[test]
#[ignore = "run by `measure`, in a process of its own"]
fn measure_one() {
    let choice = match std::env::var("FYNKEL_GPU_MEASURE").as_deref() {
        Ok("own") => DeviceChoice::Own,
        Ok("shared") => DeviceChoice::Shared,
        _ => return,
    };
    let big = scene(400);
    let small = scene(16);
    let base = footprint();

    let t = Instant::now();
    let mut scene_vp = viewport(choice, 1600, 1000);
    frame(&mut scene_vp, &big);
    let scene_ms = t.elapsed().as_secs_f64() * 1e3;
    for _ in 0..30 {
        frame(&mut scene_vp, &big);
    }
    let after_scene = footprint();

    let mut previews = Vec::new();
    let mut create_ms = Vec::new();
    let mut grow = Vec::new();
    for _ in 0..3 {
        let before = footprint();
        let t = Instant::now();
        let mut p = viewport(choice, 800, 480);
        frame(&mut p, &small);
        create_ms.push(t.elapsed().as_secs_f64() * 1e3);
        for _ in 0..10 {
            frame(&mut p, &small);
        }
        grow.push(footprint().saturating_sub(before));
        previews.push(p);
    }
    let after_previews = footprint();

    // The Scene view with one preview drawing beside it, every frame: the
    // Scene frame from its render call to on screen, then the preview's.
    let preview = &mut previews[0];
    let mut scene_frames = Vec::new();
    let mut both = Vec::new();
    for i in 0..270 {
        let t = Instant::now();
        frame(&mut scene_vp, &big);
        let s = t.elapsed().as_secs_f64() * 1e3;
        frame(preview, &small);
        if i >= 30 {
            scene_frames.push(s);
            both.push(t.elapsed().as_secs_f64() * 1e3);
        }
    }
    drop(previews);
    drop(scene_vp);

    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    println!(
        "MEASURE mode={} scene_create_ms={:.1} preview_create_ms={:.1},{:.1},{:.1} \
         base_mb={:.1} scene_mb={:.1} preview_grow_mb={:.1},{:.1},{:.1} total_mb={:.1} \
         scene_frame_ms={:.3} both_frame_ms={:.3}",
        if choice == DeviceChoice::Own {
            "own"
        } else {
            "shared"
        },
        scene_ms,
        create_ms[0],
        create_ms[1],
        create_ms[2],
        mb(base),
        mb(after_scene.saturating_sub(base)),
        mb(grow[0]),
        mb(grow[1]),
        mb(grow[2]),
        mb(after_previews.saturating_sub(base)),
        median(scene_frames),
        median(both),
    );
}

fn run(mode: &str) -> std::collections::BTreeMap<String, String> {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "measure_one", "--nocapture"])
        .env("FYNKEL_GPU_MEASURE", mode)
        .output()
        .expect("run measure_one");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.starts_with("MEASURE "))
        .unwrap_or_else(|| {
            panic!(
                "no MEASURE line from {mode}:\n{text}\n{}",
                String::from_utf8_lossy(&out.stderr)
            )
        });
    println!("{line}");
    line.split_whitespace()
        .skip(1)
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn num(m: &std::collections::BTreeMap<String, String>, key: &str) -> f64 {
    m[key].parse().unwrap()
}

fn list(m: &std::collections::BTreeMap<String, String>, key: &str) -> Vec<f64> {
    m[key].split(',').map(|v| v.parse().unwrap()).collect()
}

#[test]
#[ignore = "the ADR-0030 measurement: run alone, in release"]
fn measure() {
    // One run first to warm Metal's shader cache (kept on disk across
    // processes), then the modes interleaved so drift hits both alike.
    run("shared");
    let mut own = Vec::new();
    let mut shared = Vec::new();
    for mode in ["own", "shared", "shared", "own", "own", "shared"] {
        let m = run(mode);
        if mode == "own" {
            own.push(m);
        } else {
            shared.push(m);
        }
    }
    type Runs = [std::collections::BTreeMap<String, String>];
    let med = |runs: &Runs, key: &str| median(runs.iter().map(|m| num(m, key)).collect());
    let first = |runs: &Runs, key: &str| median(runs.iter().map(|m| list(m, key)[0]).collect());
    let each = |runs: &Runs, key: &str| median(runs.iter().flat_map(|m| list(m, key)).collect());

    for (name, runs) in [("own", &own), ("shared", &shared)] {
        println!(
            "SUMMARY {name}: scene viewport {:.1} ms, preview {:.1} ms (first) / {:.1} ms (each), \
             scene +{:.1} MB, each preview +{:.1} MB, all four +{:.1} MB, \
             Scene frame {:.3} ms, both {:.3} ms",
            med(runs, "scene_create_ms"),
            first(runs, "preview_create_ms"),
            each(runs, "preview_create_ms"),
            med(runs, "scene_mb"),
            each(runs, "preview_grow_mb"),
            med(runs, "total_mb"),
            med(runs, "scene_frame_ms"),
            med(runs, "both_frame_ms"),
        );
    }
    let second = first(&shared, "preview_create_ms");
    let (own_mb, shared_mb) = (med(&own, "total_mb"), med(&shared, "total_mb"));
    let (own_frame, shared_frame) = (med(&own, "scene_frame_ms"), med(&shared, "scene_frame_ms"));
    let change = (shared_frame - own_frame) / own_frame * 100.0;
    println!(
        "VERDICT second viewport {second:.1} ms; memory {shared_mb:.1} MB shared vs {own_mb:.1} MB own; \
         Scene frame {shared_frame:.3} ms shared vs {own_frame:.3} ms own ({change:+.1} %)"
    );
    assert!(
        second < 50.0,
        "a second viewport in under 50 ms: {second:.1}"
    );
    assert!(
        shared_mb < own_mb,
        "memory grows less shared: {shared_mb:.1} vs {own_mb:.1}"
    );
    assert!(
        change <= 5.0,
        "the Scene view's frame changes by 5 % or less: {change:+.1} %"
    );
}

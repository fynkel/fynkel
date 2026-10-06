//! Engine comparison, run C3: Bevy 0.19.1 on the scene of
//! docs/engine-comparison.md ("Rules"). One binary, configured from the
//! environment:
//!
//!   CMP_LEVEL = plain | full   (default full)
//!   CMP_SCALE = render scale, e.g. 1.0 or 0.75 (default 1.0)
//!   CMP_SHOT  = 1: t frozen at 0, clips paused at 0.5 s, one PNG after 5 s
//!   CMP_DIAG  = 1: also add Bevy's RenderDiagnosticsPlugin and print which
//!               GPU spans it produces (debug only; it changes the GPU work)
//!
//! Prints one `[cmp] …` line a second (see `Stats`).

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::app::MainScheduleOrder;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::gltf::Gltf;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::{PendingCommandBuffers, RenderDevice, RenderGraph, RenderGraphSystems};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::render::view::window::prepare_windows;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{PresentMode, PrimaryWindow, WindowResized};
use bevy::world_serialization::WorldInstanceReady;

// ---------------------------------------------------------------- config

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Plain,
    Full,
}

#[derive(Resource, Clone)]
struct Cfg {
    level: Level,
    scale: f32,
    scale_label: String,
    shot: bool,
    diag: bool,
}

impl Cfg {
    fn from_env() -> Self {
        let level = match std::env::var("CMP_LEVEL").as_deref() {
            Ok("plain") => Level::Plain,
            Ok("full") | Err(_) => Level::Full,
            Ok(other) => panic!("CMP_LEVEL={other}: only plain|full exist for Bevy"),
        };
        let scale: f32 = std::env::var("CMP_SCALE")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(1.0)
            .clamp(0.1, 1.0);
        // Same spelling as the Dart hosts print a double: 1.0, 0.75.
        let scale_label = if scale.fract() == 0.0 {
            format!("{scale:.1}")
        } else {
            format!("{scale}")
        };
        let flag = |k: &str| std::env::var(k).is_ok_and(|v| v == "1" || v == "true");
        Self {
            level,
            scale,
            scale_label,
            shot: flag("CMP_SHOT"),
            diag: flag("CMP_DIAG"),
        }
    }

    fn level_name(&self) -> &'static str {
        match self.level {
            Level::Plain => "plain",
            Level::Full => "full",
        }
    }
}

// ---------------------------------------------------------------- app

/// Entry point for macOS (`main.rs`) and iOS (`main_rs`, called from the
/// Xcode shell's `main.m`).
pub fn run() {
    let cfg = Cfg::from_env();
    let stats = Stats::default();

    let mut app = App::new();
    let mut window = Window {
        title: "cmp bevy".into(),
        present_mode: PresentMode::AutoVsync,
        ..default()
    };
    if cfg!(target_os = "ios") {
        // Full screen at the native size: winit reports the screen's size.
        window.mode = bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Primary);
    } else {
        window.resolution = (1280, 720).into();
    }
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                level: bevy::log::Level::WARN,
                ..default()
            }),
    )
    .insert_resource(ClearColor(Color::linear_rgb(0.5, 0.6, 0.8)))
    // 2 000 lux uniform white ambient: Bevy's ambient is a radiance in
    // cd/m²; a uniform sky of radiance L gives E = π·L on a surface.
    .insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 2000.0 / PI,
        ..default()
    })
    // 4 cascades × 1024² = the same texels as Fynkel's and Unity's one 2048 atlas.
    .insert_resource(DirectionalLightShadowMap { size: 1024 })
    .insert_resource(cfg.clone())
    .insert_resource(stats.clone())
    .init_resource::<Scene>()
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            track_loading,
            animate_camera,
            fit_render_scale_target,
            take_shot,
            print_stats,
        ),
    );

    if cfg.diag {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        app.add_systems(Update, print_render_diagnostics);
    }

    // CPU time of the main world: from before `First` to after `Last`.
    app.init_schedule(CmpFrameBegin);
    app.init_schedule(CmpFrameEnd);
    {
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        order.insert_before(First, CmpFrameBegin);
        order.insert_after(Last, CmpFrameEnd);
    }
    app.add_systems(CmpFrameBegin, main_begin);
    app.add_systems(CmpFrameEnd, main_end);

    // Render world: CPU time of the `Render` schedule minus the swap-chain
    // acquire (the vsync / back-pressure wait), frames presented, and the
    // GPU frame span.
    let render_app = app.sub_app_mut(RenderApp);
    render_app
        .insert_resource(stats.clone())
        .insert_resource(GpuTimer::new(stats.clone()))
        .add_systems(
            Render,
            (
                render_begin.in_set(RenderSystems::ExtractCommands),
                acquire_begin.before(prepare_windows).in_set(RenderSystems::PrepareViews),
                acquire_end.after(prepare_windows).in_set(RenderSystems::PrepareViews),
                render_end.after(RenderSystems::PostCleanup),
            ),
        )
        .add_systems(
            RenderGraph,
            (
                gpu_frame_begin.in_set(RenderGraphSystems::Begin),
                gpu_frame_end
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            ),
        );

    app.run();
}

/// iOS entry point (the Xcode project's `main.m` calls it).
#[unsafe(no_mangle)]
pub extern "C" fn main_rs() {
    run();
}

// ---------------------------------------------------------------- scene

const CHARACTERS: usize = 20;

#[derive(Resource, Default)]
struct Scene {
    character_gltf: Handle<Gltf>,
    walk_graph: Option<(Handle<AnimationGraph>, AnimationNodeIndex)>,
    sponza_ready: bool,
    characters_ready: usize,
    /// `Time::elapsed_secs` when everything was spawned and posed.
    ready_at: Option<f32>,
    shot_taken: bool,
}

#[derive(Component)]
struct MainCamera;

#[derive(Component)]
struct Character(usize);

#[derive(Component)]
struct SponzaRoot;

/// The low-resolution colour target and the sprite that shows it, when
/// CMP_SCALE < 1.
#[derive(Resource)]
struct ScaledTarget {
    image: Handle<Image>,
    size: UVec2,
}

#[derive(Component)]
struct ScaledCanvas;

fn setup(
    mut commands: Commands,
    cfg: Res<Cfg>,
    assets: Res<AssetServer>,
    mut scene: ResMut<Scene>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // Camera at t = 0 (animate_camera moves it every frame).
    let mut camera = commands.spawn((
        MainCamera,
        Camera3d::default(),
        Camera::default(),
        Hdr,
        Msaa::Off,
        Tonemapping::AcesFitted,
        Exposure { ev100: 12.0 },
        Projection::Perspective(PerspectiveProjection {
            fov: 60f32.to_radians(),
            near: 0.1,
            far: 100.0,
            ..default()
        }),
        camera_transform(0.0),
    ));
    if cfg.level == Level::Full {
        // CMP_FULL_WITHOUT=ssao|bloom: debugging only, never for a measured run.
        let without = std::env::var("CMP_FULL_WITHOUT").unwrap_or_default();
        if !without.contains("ssao") {
            camera.insert(ScreenSpaceAmbientOcclusion::default());
        }
        if !without.contains("bloom") {
            camera.insert(Bloom::NATURAL);
        }
    }

    if cfg.scale < 1.0 || cfg.shot {
        // Shot mode always takes this path (at scale 1.0 too) so the PNG
        // can be read from the image: on macOS a window-surface screenshot
        // comes back black while the window is covered by another one.
        //
        // Render the 3D view into an image at scale × the window's physical
        // size, then draw that image stretched over the window with a 2D
        // camera (linear filtering) — Bevy has no built-in render scale
        // outside DLSS.
        let window = windows.single().expect("primary window");
        let size = scaled_size(window, cfg.scale);
        let image = images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        camera.insert(RenderTarget::Image(image.clone().into()));
        let layer = RenderLayers::layer(1);
        commands.spawn((
            ScaledCanvas,
            Sprite {
                image: image.clone(),
                custom_size: Some(Vec2::new(window.width(), window.height())),
                ..default()
            },
            layer.clone(),
        ));
        commands.spawn((
            Camera2d,
            Camera {
                order: 1,
                ..default()
            },
            Msaa::Off,
            Tonemapping::None,
            layer,
        ));
        commands.insert_resource(ScaledTarget { image, size });
    }

    // Sun: along (−0.3, −1.0, −0.25), 10 000 lux, 4 cascades.
    let sun_dir = Vec3::new(-0.3, -1.0, -0.25).normalize();
    commands.spawn((
        DirectionalLight {
            color: Color::WHITE,
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            contact_shadows_enabled: false,
            ..default()
        },
        Transform::IDENTITY.looking_to(sun_dir, Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            minimum_distance: 0.1,
            first_cascade_far_bound: 4.0,
            maximum_distance: 60.0,
            ..default()
        }
        .build(),
    ));

    // Eight warm point lights, 2 000 lm, 6 m range, no shadows.
    for x in [-10.5f32, -3.5, 3.5, 10.5] {
        for z in [-3.0f32, 3.0] {
            commands.spawn((
                PointLight {
                    color: Color::linear_rgb(1.0, 0.8, 0.6),
                    intensity: 2000.0,
                    range: 6.0,
                    shadow_maps_enabled: false,
                    contact_shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(x, 1.5, z),
            ));
        }
    }

    // Sponza as it is (its node carries the 0.008 scale).
    commands
        .spawn((
            SponzaRoot,
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset("sponza/Sponza.gltf"))),
        ))
        .observe(|_: On<WorldInstanceReady>, mut scene: ResMut<Scene>| {
            scene.sponza_ready = true;
        });

    // Twenty characters, facing +z (yaw 0), playing `walk`.
    scene.character_gltf = assets.load("character.glb");
    let character_scene = assets.load(GltfAssetLabel::Scene(0).from_asset("character.glb"));
    for i in 0..CHARACTERS {
        let x = -9.0 + i as f32 * 18.0 / 19.0;
        let z = if i % 2 == 0 { 1.2 } else { -1.2 };
        commands
            .spawn((
                Character(i),
                WorldAssetRoot(character_scene.clone()),
                Transform::from_xyz(x, 0.0, z),
            ))
            .observe(start_walk);
    }
}

fn scaled_size(window: &Window, scale: f32) -> UVec2 {
    UVec2::new(
        ((window.physical_width() as f32 * scale).round() as u32).max(1),
        ((window.physical_height() as f32 * scale).round() as u32).max(1),
    )
}

fn camera_transform(t: f32) -> Transform {
    let phi = 2.0 * PI * t / 30.0;
    let eye = Vec3::new(10.0 * phi.sin(), 2.0, 0.0);
    let dir = Vec3::new(phi.cos(), -0.05, phi.sin());
    Transform::from_translation(eye).looking_to(dir, Vec3::Y)
}

fn start_walk(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    cfg: Res<Cfg>,
    mut scene: ResMut<Scene>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    characters: Query<&Character>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Ok(&Character(i)) = characters.get(ready.entity) else {
        return;
    };
    if scene.walk_graph.is_none() {
        let gltf = gltfs
            .get(&scene.character_gltf)
            .expect("character.glb is loaded once its scene is");
        let walk = gltf
            .named_animations
            .get("walk")
            .expect("character.glb has a `walk` clip")
            .clone();
        let (graph, index) = AnimationGraph::from_clip(walk);
        scene.walk_graph = Some((graphs.add(graph), index));
    }
    let (graph, index) = scene.walk_graph.clone().unwrap();
    for child in children.iter_descendants(ready.entity) {
        if let Ok(mut player) = players.get_mut(child) {
            let active = player.play(index).repeat();
            if cfg.shot {
                active.seek_to(0.5).pause();
            } else {
                active.seek_to(i as f32 * 0.1);
            }
            commands.entity(child).insert(AnimationGraphHandle(graph.clone()));
        }
    }
    scene.characters_ready += 1;
}

fn track_loading(mut scene: ResMut<Scene>, time: Res<Time>) {
    if scene.ready_at.is_none() && scene.sponza_ready && scene.characters_ready == CHARACTERS {
        scene.ready_at = Some(time.elapsed_secs());
    }
}

fn animate_camera(
    cfg: Res<Cfg>,
    scene: Res<Scene>,
    time: Res<Time>,
    mut camera: Query<&mut Transform, With<MainCamera>>,
) {
    // t runs from the moment the scene is in place, frozen at 0 for shots.
    let t = match (cfg.shot, scene.ready_at) {
        (false, Some(t0)) => time.elapsed_secs() - t0,
        _ => 0.0,
    };
    if let Ok(mut transform) = camera.single_mut() {
        *transform = camera_transform(t);
    }
}

fn fit_render_scale_target(
    cfg: Res<Cfg>,
    target: Option<ResMut<ScaledTarget>>,
    mut resized: MessageReader<WindowResized>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut canvas: Query<&mut Sprite, With<ScaledCanvas>>,
) {
    if resized.read().count() == 0 {
        return;
    }
    let (Some(mut target), Ok(window)) = (target, windows.single()) else {
        return;
    };
    let size = scaled_size(window, cfg.scale);
    if size != target.size {
        if let Some(mut image) = images.get_mut(&target.image) {
            image.resize(bevy::render::render_resource::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
        }
        target.size = size;
    }
    if let Ok(mut sprite) = canvas.single_mut() {
        sprite.custom_size = Some(Vec2::new(window.width(), window.height()));
    }
}

fn take_shot(
    mut commands: Commands,
    cfg: Res<Cfg>,
    mut scene: ResMut<Scene>,
    time: Res<Time>,
    target: Option<Res<ScaledTarget>>,
) {
    if !cfg.shot || scene.shot_taken {
        return;
    }
    let Some(t0) = scene.ready_at else { return };
    if time.elapsed_secs() - t0 < 5.0 {
        return;
    }
    scene.shot_taken = true;
    let name = format!("cmp_shot_bevy_{}_{}.png", cfg.level_name(), cfg.scale_label);
    let path = shot_dir().join(name);
    println!("[cmp] shot {}", path.display());
    // The 3D camera's own picture (at the render scale, before the upscale
    // to the window).
    let shot = match &target {
        Some(t) => Screenshot::image(t.image.clone()),
        None => Screenshot::primary_window(),
    };
    commands.spawn(shot).observe(save_to_disk(path));
}

fn shot_dir() -> std::path::PathBuf {
    if cfg!(target_os = "ios") {
        // The app container's HOME; Documents is visible to devicectl.
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(home).join("Documents")
    } else {
        std::env::current_dir().unwrap_or_else(|_| ".".into())
    }
}

// ---------------------------------------------------------------- stats

/// Shared between the main and the render world.
#[derive(Resource, Clone, Default)]
struct Stats(Arc<StatsInner>);

#[derive(Default)]
struct StatsInner {
    /// Frames the render world presented.
    presented: AtomicU64,
    /// The render world's CPU time of its latest frame, in ns.
    last_render_ns: AtomicU64,
    /// Per main-world frame: main CPU + latest render CPU, in ms.
    cpu_ms: Mutex<Vec<f64>>,
    /// GPU frame spans that came back, in ms.
    gpu_ms: Mutex<Vec<f64>>,
    window: Mutex<Option<(Instant, u64)>>,
    main_begin: Mutex<Option<Instant>>,
}

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct CmpFrameBegin;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct CmpFrameEnd;

fn main_begin(stats: Res<Stats>) {
    *stats.0.main_begin.lock().unwrap() = Some(Instant::now());
}

fn main_end(stats: Res<Stats>) {
    let Some(begin) = *stats.0.main_begin.lock().unwrap() else {
        return;
    };
    let main_ms = begin.elapsed().as_secs_f64() * 1e3;
    let render_ms = stats.0.last_render_ns.load(Ordering::Relaxed) as f64 / 1e6;
    stats.0.cpu_ms.lock().unwrap().push(main_ms + render_ms);
}

#[derive(Resource, Default)]
struct RenderClock {
    begin: Option<Instant>,
    acquire_begin: Option<Instant>,
    acquire_ns: u64,
}

fn render_begin(mut commands: Commands, clock: Option<ResMut<RenderClock>>) {
    match clock {
        Some(mut c) => {
            c.begin = Some(Instant::now());
            c.acquire_ns = 0;
        }
        None => commands.insert_resource(RenderClock {
            begin: Some(Instant::now()),
            ..default()
        }),
    }
}

fn acquire_begin(clock: Option<ResMut<RenderClock>>) {
    if let Some(mut c) = clock {
        c.acquire_begin = Some(Instant::now());
    }
}

fn acquire_end(clock: Option<ResMut<RenderClock>>) {
    if let Some(mut c) = clock
        && let Some(b) = c.acquire_begin.take()
    {
        c.acquire_ns = b.elapsed().as_nanos() as u64;
    }
}

fn render_end(clock: Option<Res<RenderClock>>, stats: Res<Stats>) {
    let Some(c) = clock else { return };
    let Some(begin) = c.begin else { return };
    let total = begin.elapsed().as_nanos() as u64;
    stats
        .0
        .last_render_ns
        .store(total.saturating_sub(c.acquire_ns), Ordering::Relaxed);
    stats.0.presented.fetch_add(1, Ordering::Relaxed);
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

fn print_stats(stats: Res<Stats>, cfg: Res<Cfg>) {
    let now = Instant::now();
    let presented = stats.0.presented.load(Ordering::Relaxed);
    let mut window = stats.0.window.lock().unwrap();
    let Some((start, start_frames)) = *window else {
        *window = Some((now, presented));
        stats.0.cpu_ms.lock().unwrap().clear();
        stats.0.gpu_ms.lock().unwrap().clear();
        return;
    };
    let secs = now.duration_since(start).as_secs_f64();
    if secs < 1.0 {
        return;
    }
    *window = Some((now, presented));
    let fps = (presented - start_frames) as f64 / secs;
    let mut cpu = std::mem::take(&mut *stats.0.cpu_ms.lock().unwrap());
    let mut gpu = std::mem::take(&mut *stats.0.gpu_ms.lock().unwrap());
    cpu.sort_by(f64::total_cmp);
    gpu.sort_by(f64::total_cmp);
    println!(
        "[cmp] engine=bevy level={} scale={} fps={:.1} cpu_ms={:.2} gpu_ms={:.2} gpu_p95={:.2} thermal={}",
        cfg.level_name(),
        cfg.scale_label,
        fps,
        percentile(&cpu, 0.5),
        percentile(&gpu, 0.5),
        percentile(&gpu, 0.95),
        thermal_state(),
    );
}

#[cfg(target_vendor = "apple")]
fn thermal_state() -> i64 {
    objc2_foundation::NSProcessInfo::processInfo().thermalState().0 as i64
}

#[cfg(not(target_vendor = "apple"))]
fn thermal_state() -> i64 {
    0
}

/// CMP_DIAG=1 only: what Bevy's own render diagnostics report.
fn print_render_diagnostics(
    store: Res<bevy::diagnostic::DiagnosticsStore>,
    time: Res<Time>,
    mut last: Local<f32>,
) {
    if time.elapsed_secs() - *last < 5.0 {
        return;
    }
    *last = time.elapsed_secs();
    let mut gpu = Vec::new();
    let mut cpu_spans = 0;
    for d in store.iter() {
        let p = d.path().as_str();
        if !p.starts_with("render/") {
            continue;
        }
        if p.ends_with("/elapsed_gpu") {
            gpu.push(format!("{p}={:.3}", d.smoothed().unwrap_or(0.0)));
        } else if p.ends_with("/elapsed_cpu") {
            cpu_spans += 1;
        }
    }
    println!(
        "[cmp-diag] RenderDiagnosticsPlugin: {cpu_spans} spans with CPU time, {} with GPU time: {}",
        gpu.len(),
        gpu.join(" ")
    );
}

// ---------------------------------------------------------------- GPU time

/// The frame's GPU span on Metal: `GPUStartTime` of an (empty) command
/// buffer submitted ahead of all of Bevy's render-graph work to
/// `GPUEndTime` of one submitted after it, in the same `queue.submit`.
///
/// Why not `RenderDiagnosticsPlugin`: Bevy writes its pass timestamps
/// *inside* passes, which needs `TIMESTAMP_QUERY_INSIDE_PASSES`, which wgpu
/// does not offer on Apple GPUs (they sample counters only at stage
/// boundaries) — so on iOS/macOS the plugin reports CPU time only for
/// passes (encoder-level spans do get GPU times, via extra blit encoders).
#[derive(Resource)]
struct GpuTimer {
    frame: u64,
    edges: Arc<Mutex<HashMap<u64, (f64, f64)>>>,
    stats: Stats,
}

impl GpuTimer {
    fn new(stats: Stats) -> Self {
        Self {
            frame: 0,
            edges: Default::default(),
            stats,
        }
    }
}

fn gpu_frame_begin(
    mut timer: ResMut<GpuTimer>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    timer.frame += 1;
    let mut encoder = device.create_command_encoder(&default());
    hook_command_buffer(&mut encoder, &timer, true);
    pending.push([encoder.finish()]);
}

fn gpu_frame_end(
    timer: Res<GpuTimer>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let mut encoder = device.create_command_encoder(&default());
    hook_command_buffer(&mut encoder, &timer, false);
    pending.push([encoder.finish()]);
}

#[cfg(target_vendor = "apple")]
fn hook_command_buffer(encoder: &mut wgpu::CommandEncoder, timer: &GpuTimer, is_begin: bool) {
    use block2::RcBlock;
    use objc2::runtime::ProtocolObject;
    use objc2_metal::MTLCommandBuffer;
    use std::ptr::NonNull;

    let frame = timer.frame;
    let edges = timer.edges.clone();
    let stats = timer.stats.clone();
    // SAFETY: we only attach a completion handler; wgpu still ends and
    // commits the command buffer.
    unsafe {
        encoder.as_hal_mut::<wgpu::hal::api::Metal, _, _>(|hal| {
            let Some(cb) = hal.and_then(|h| h.raw_command_buffer()) else {
                return;
            };
            let block = RcBlock::new(move |cb: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                let cb = cb.as_ref();
                let t = if is_begin { cb.GPUStartTime() } else { cb.GPUEndTime() };
                let mut map = edges.lock().unwrap();
                let entry = map.entry(frame).or_insert((f64::NAN, f64::NAN));
                if is_begin {
                    entry.0 = t;
                } else {
                    entry.1 = t;
                }
                if !entry.0.is_nan() && !entry.1.is_nan() {
                    let (b, e) = map.remove(&frame).unwrap();
                    if b > 0.0 && e >= b {
                        stats.0.gpu_ms.lock().unwrap().push((e - b) * 1e3);
                    }
                }
                // Forget frames whose other half never came back.
                if map.len() > 64 {
                    map.retain(|&f, _| f + 32 > frame);
                }
            });
            cb.addCompletedHandler(RcBlock::as_ptr(&block));
        });
    }
}

#[cfg(not(target_vendor = "apple"))]
fn hook_command_buffer(_: &mut wgpu::CommandEncoder, _: &GpuTimer, _: bool) {}

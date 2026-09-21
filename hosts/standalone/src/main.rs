//! Standalone winit/wgpu host — the "engine alone" ruler (blueprint §8.1,
//! ADR-0002, ADR-0007).
//!
//! The host owns the loop: each redraw it feeds the frame dt to
//! `fynkel_core`, runs the demo scene's gameplay for every planned fixed
//! tick, steps the world, and renders the world's extract interpolated by
//! alpha. Prints frame-time statistics for the benchmark capture.
//!
//! `--cache <dir>` loads a cooked project's startup scene (or `--scene
//! <id>`) instead of the demo scene — a host never cooks, so run `fynkel
//! cook` first; `--quality <profile>` picks one of its profiles. This is
//! how `benchmarks/m3/capture.py` runs the stress scene.

mod stats;

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use fynkel_assets::{Assets, Residency};
use fynkel_core::{World, WorldConfig};
use fynkel_host_headless::DemoScene;
use fynkel_render::ExtractRenderer;
use stats::{FrameStats, FrameTiming};

/// A cooked project to run instead of the demo scene.
struct Project {
    cache: std::path::PathBuf,
    scene: Option<fynkel_contracts::Uuid>,
    quality: Option<String>,
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: ExtractRenderer,
}

impl Gpu {
    fn new(window: Arc<Window>, assets: Option<(Assets, Option<String>)>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("no suitable adapter");
        let info = adapter.get_info();
        log::info!(
            "adapter: {} ({:?}, {:?})",
            info.name,
            info.backend,
            info.device_type
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fynkel standalone device"),
            required_limits: fynkel_render::device_limits(&adapter),
            ..Default::default()
        }))
        .expect("request device");

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface not supported by adapter");
        // Fifo = vsync. The baseline is measured at display rate, like Flutter will be.
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        let mut renderer =
            ExtractRenderer::new(&device, config.format, config.width, config.height);
        if let Some((assets, quality)) = assets {
            renderer.placeholders = false;
            renderer.set_assets(Some(assets));
            if !renderer.set_quality_profile(quality.as_deref()) {
                log::error!(
                    "--quality {}: not a profile of the project (profiles: {})",
                    quality.unwrap_or_default(),
                    renderer.quality_profiles().join(", ")
                );
                std::process::exit(2);
            }
        }

        Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.renderer.resize(&self.device, width, height);
    }

    /// Returns `None` when the surface needs reconfiguring; otherwise the
    /// (acquire wait, cpu encode+submit) durations for this frame.
    fn render(&mut self, extract: &fynkel_core::Extract) -> Option<FrameTiming> {
        use wgpu::CurrentSurfaceTexture as C;
        let t_acquire = Instant::now();
        let frame = match self.surface.get_current_texture() {
            C::Success(f) => f,
            C::Suboptimal(f) => f,
            C::Timeout | C::Occluded => return Some(FrameTiming::skipped()),
            C::Outdated | C::Lost => return None,
            C::Validation => {
                log::error!("surface validation error; reconfiguring");
                return None;
            }
        };
        let t_cpu = Instant::now();
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer.prepare(&self.device, &self.queue, extract);
        for e in self.renderer.asset_errors() {
            log::error!("asset error: {e}");
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        self.renderer.draw(&mut encoder, &view);
        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(frame);
        Some(FrameTiming {
            acquire_wait: t_cpu.duration_since(t_acquire),
            cpu: t_cpu.elapsed(),
            presented: true,
            render: Some(self.renderer.stats()),
        })
    }
}

struct App {
    gpu: Option<Gpu>,
    start: Option<Instant>,
    last_frame: Option<Instant>,
    stats: FrameStats,
    world: World,
    /// The demo scene's gameplay; `None` when a cooked project is loaded.
    scene: Option<DemoScene>,
    /// The cooked project to load on resume (`--cache`).
    project: Option<Project>,
    /// The loaded scene's assets, kept resident while the host runs.
    residency: Option<Residency>,
    /// Auto-exit after this many seconds (`--seconds N`), for scripted captures.
    run_for: Option<f32>,
    /// Open the output device (`--audio`, M3-11); the null backend
    /// otherwise, so captures never touch a device.
    audio: bool,
}

impl App {
    fn new(satellites: u32, seed: u64, project: Option<Project>) -> Self {
        Self {
            gpu: None,
            start: None,
            last_frame: None,
            stats: FrameStats::default(),
            world: World::new(WorldConfig::default()),
            scene: project.is_none().then(|| DemoScene::new(satellites, seed)),
            project,
            residency: None,
            run_for: None,
            audio: false,
        }
    }

    /// Loads the project's scene into the world and returns the store the
    /// renderer reads from. Exits the process on a broken cache: a host
    /// never cooks.
    fn load_project(&mut self, p: &Project) -> (Assets, Option<String>) {
        let fail = |what: &str, diags: &[fynkel_document::Diagnostic]| -> ! {
            for d in diags {
                log::error!("{d}");
            }
            log::error!(
                "{what}: cannot load {} (run `fynkel cook` first)",
                p.cache.display()
            );
            std::process::exit(1);
        };
        let assets = Assets::open(&p.cache).unwrap_or_else(|d| fail("cache", &[d]));
        let scene_id = match p.scene {
            Some(id) => id,
            None => assets
                .startup_scene()
                .unwrap_or_else(|d| fail("startup scene", &[d])),
        };
        let cooked = assets
            .scene(scene_id)
            .unwrap_or_else(|ds| fail("scene", &ds));
        for d in &cooked.diagnostics {
            log::warn!("{d}");
        }
        // What the headless host registers before the first step (the
        // same order): the project's gravity and the mesh colliders'
        // meshes (M3-08), the catalogue's prefabs (M3-10), the animators'
        // clips and the skins (M3-19) — without them the rigs would draw
        // unskinned and the emitters spawn nothing.
        if let Some(p) = assets.project() {
            self.world
                .physics_mut()
                .set_gravity(fynkel_core::physics::Vec3::from(p.physics.gravity));
        }
        let (_, mesh_diags) =
            assets.register_mesh_colliders(&cooked.description, self.world.physics_mut());
        let (_, prefab_diags) = assets.register_prefabs(&cooked.description, &mut self.world);
        let (clips, skins, anim_diags) =
            assets.register_animation(&cooked.description, &mut self.world);
        let (sounds, sound_diags) = assets.register_audio(&cooked.description, &mut self.world);
        for d in mesh_diags
            .iter()
            .chain(&prefab_diags)
            .chain(&anim_diags)
            .chain(&sound_diags)
        {
            log::warn!("{d}");
        }
        let loaded = self
            .world
            .load_scene(&cooked.description)
            .unwrap_or_else(|e| {
                log::error!("scene: {e}");
                std::process::exit(1);
            });
        self.residency = Some(assets.retain(&cooked.closure));
        log::info!(
            "project {}: scene \"{}\" ({} entities, {} rejected components, {} assets retained, {clips} clips, {skins} skins, {sounds} sounds)",
            assets
                .project()
                .map(|p| p.name)
                .unwrap_or_else(|| "(no project)".to_string()),
            loaded.name,
            loaded.entities().len(),
            loaded.rejected_components,
            cooked.closure.len(),
        );
        (assets, p.quality.clone())
    }

    /// One host frame of simulation (ADR-0008 order): begin_frame → per
    /// planned tick {pull events, gameplay, step} → extract.
    fn simulate(&mut self, dt: f32) -> fynkel_core::Extract {
        let plan = self.world.begin_frame(dt);
        if plan.dropped_seconds > 0.0 {
            log::warn!("missed deadline: dropped {:.3}s", plan.dropped_seconds);
        }
        for _ in 0..plan.steps {
            let events = self.world.drain_events();
            if let Some(scene) = self.scene.as_mut() {
                scene.on_events(&events);
                let tick = self.world.tick();
                scene.fixed_update(&mut self.world, tick);
            }
            self.world.step();
        }
        let events = self.world.drain_events();
        if let Some(scene) = self.scene.as_mut() {
            scene.on_events(&events);
        }
        self.world.extract()
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Fynkel — standalone host")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let assets = self.project.take().map(|p| self.load_project(&p));
        // The output device (M3-11), after the load so the voices the
        // scene starts reach it; the null backend otherwise.
        if self.audio {
            if self.world.open_audio_device() {
                let info = self.world.audio().device_info();
                log::info!(
                    "audio: output device open at {} Hz, {} channels",
                    info.sample_rate,
                    info.channels
                );
            } else if let Some(d) = self.world.diagnostics().last() {
                log::warn!("{}", d.message);
            }
        }
        self.gpu = Some(Gpu::new(window, assets));
        self.start = Some(Instant::now());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.gpu.is_none() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.stats.report_final();
                self.world.audio_mut().close_device();
                event_loop.exit();
            }
            // The host's lifecycle pauses the device (M3-11): nothing
            // plays from a hidden window.
            WindowEvent::Occluded(occluded) => {
                self.world.audio_mut().suspend(occluded);
            }
            WindowEvent::Resized(size) => {
                self.gpu.as_mut().unwrap().resize(size.width, size.height)
            }
            WindowEvent::RedrawRequested => {
                let elapsed = self.start.map(|s| s.elapsed().as_secs_f32()).unwrap_or(0.0);
                if self.run_for.is_some_and(|limit| elapsed >= limit) {
                    self.stats.report_final();
                    event_loop.exit();
                    return;
                }
                let now = Instant::now();
                let dt = self
                    .last_frame
                    .map(|t| now.duration_since(t).as_secs_f32())
                    .unwrap_or(0.0);
                self.last_frame = Some(now);
                let extract = self.simulate(dt);
                let gpu = self.gpu.as_mut().unwrap();
                match gpu.render(&extract) {
                    Some(t) => self.stats.record(t),
                    None => {
                        let size = gpu.window.inner_size();
                        gpu.resize(size.width, size.height);
                    }
                }
                gpu.window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut satellites = 24;
    let mut seed = 42;
    let mut run_for = None;
    let mut cache = None;
    let mut scene = None;
    let mut quality = None;
    let mut audio = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seconds" => run_for = args.next().and_then(|v| v.parse().ok()),
            "--cache" => cache = args.next().map(std::path::PathBuf::from),
            "--scene" => {
                scene = args.next().map(|v| {
                    v.parse().unwrap_or_else(|_| {
                        eprintln!("--scene {v}: not a scene id (uuid)");
                        std::process::exit(2);
                    })
                })
            }
            "--quality" => quality = args.next(),
            "--audio" => audio = true,
            "--satellites" => {
                satellites = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(satellites)
            }
            "--seed" => seed = args.next().and_then(|v| v.parse().ok()).unwrap_or(seed),
            _ => {
                eprintln!(
                    "usage: fynkel_host_standalone [--seconds N] [--satellites N] [--seed N] [--audio] [--cache <dir> [--scene <id>] [--quality <profile>]]"
                );
                std::process::exit(2);
            }
        }
    }
    if cache.is_none() && (scene.is_some() || quality.is_some()) {
        eprintln!("--scene and --quality need --cache <dir>");
        std::process::exit(2);
    }
    let project = cache.map(|cache| Project {
        cache,
        scene,
        quality,
    });
    let mut app = App::new(satellites, seed, project);
    app.run_for = run_for;
    app.audio = audio;
    event_loop.run_app(&mut app).expect("run");
}

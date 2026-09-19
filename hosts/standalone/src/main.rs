//! Standalone winit/wgpu host — the "engine alone" ruler (blueprint §8.1,
//! ADR-0002, ADR-0007).
//!
//! The host owns the loop: each redraw it feeds the frame dt to
//! `fynkel_core`, runs the demo scene's gameplay for every planned fixed
//! tick, steps the world, and renders the world's extract interpolated by
//! alpha. Prints frame-time statistics for the benchmark capture.

mod stats;

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use fynkel_core::{World, WorldConfig};
use fynkel_host_headless::DemoScene;
use fynkel_render::ExtractRenderer;
use stats::{FrameStats, FrameTiming};

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: ExtractRenderer,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Self {
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

        let renderer = ExtractRenderer::new(&device, config.format, config.width, config.height);

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
        })
    }
}

struct App {
    gpu: Option<Gpu>,
    start: Option<Instant>,
    last_frame: Option<Instant>,
    stats: FrameStats,
    world: World,
    scene: DemoScene,
    /// Auto-exit after this many seconds (`--seconds N`), for scripted captures.
    run_for: Option<f32>,
}

impl App {
    fn new(satellites: u32, seed: u64) -> Self {
        Self {
            gpu: None,
            start: None,
            last_frame: None,
            stats: FrameStats::default(),
            world: World::new(WorldConfig::default()),
            scene: DemoScene::new(satellites, seed),
            run_for: None,
        }
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
            self.scene.on_events(&events);
            let tick = self.world.tick();
            self.scene.fixed_update(&mut self.world, tick);
            self.world.step();
        }
        let events = self.world.drain_events();
        self.scene.on_events(&events);
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
        self.gpu = Some(Gpu::new(window));
        self.start = Some(Instant::now());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.gpu.is_none() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.stats.report_final();
                event_loop.exit();
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
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seconds" => run_for = args.next().and_then(|v| v.parse().ok()),
            "--satellites" => {
                satellites = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(satellites)
            }
            "--seed" => seed = args.next().and_then(|v| v.parse().ok()).unwrap_or(seed),
            _ => {}
        }
    }
    let mut app = App::new(satellites, seed);
    app.run_for = run_for;
    event_loop.run_app(&mut app).expect("run");
}

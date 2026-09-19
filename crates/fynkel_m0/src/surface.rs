//! Path C experiment (blueprint §8.2, ADR-0002 fallback column): the engine
//! owns a `CAMetalLayer` that the host places *underneath* a transparent
//! Flutter view in the same window. Rendering and presentation run on an
//! engine thread, paced by the layer's vsync (`PresentMode::Fifo`), exactly
//! like the standalone host; Flutter's HUD is a separate layer that the
//! window server composites on top. No IOSurface hand-over, no Flutter
//! `Texture`, and no frame-level synchronization between the two layers.
//!
//! The host (Swift) creates the layer-backed `NSView`, keeps it sized under
//! the Flutter view, and forwards size/scale changes; Dart drives this ABI.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use fynkel_m0_scene::CubeRenderer;

#[derive(Default)]
pub struct SurfaceStats {
    pub frames: u64,
    pub skipped: u64,
    pub resizes: u64,
    pub interval_us: Vec<u32>,
    pub acquire_us: Vec<u32>,
    pub cpu_us: Vec<u32>,
    pub resize_us: Vec<u32>,
}

struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    pending_size: Mutex<Option<(u32, u32)>>,
    stats: Mutex<SurfaceStats>,
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: CubeRenderer,
}

pub struct SurfaceContext {
    shared: Arc<Shared>,
    /// The render thread hands the GPU objects back on exit so the surface
    /// (and its retained `CAMetalLayer`) is released on the host's thread.
    thread: Option<JoinHandle<Gpu>>,
}

impl SurfaceContext {
    /// # Safety
    /// `layer` must be a live `CAMetalLayer` that outlives this context.
    pub unsafe fn new(layer: *mut c_void, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
            .map_err(|e| format!("create surface from CAMetalLayer: {e}"))?;
        let adapter =
            super::pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .map_err(|e| format!("no adapter: {e}"))?;
        let (device, queue) =
            super::pollster_block(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("fynkel m0 path C device"),
                ..Default::default()
            }))
            .map_err(|e| format!("request device: {e}"))?;
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or("surface not supported")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        let renderer = CubeRenderer::new(&device, config.format, config.width, config.height);
        log::info!(
            "path C surface: {}x{} {:?} on {}",
            config.width,
            config.height,
            config.format,
            adapter.get_info().name
        );

        let gpu = Gpu {
            surface,
            device,
            queue,
            config,
            renderer,
        };
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            pending_size: Mutex::new(None),
            stats: Mutex::new(SurfaceStats::default()),
        });
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("fynkel-path-c-render".into())
                .spawn(move || render_loop(gpu, shared))
                .map_err(|e| e.to_string())?
        };
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn resize(&self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        *self.shared.pending_size.lock().unwrap() = Some((width, height));
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    pub fn take_stats(&self) -> SurfaceStats {
        std::mem::take(&mut *self.shared.stats.lock().unwrap())
    }
}

impl Drop for SurfaceContext {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // The returned Gpu (surface, device) drops here, on the caller's thread.
            let _ = t.join();
        }
    }
}

fn render_loop(mut gpu: Gpu, shared: Arc<Shared>) -> Gpu {
    use wgpu::CurrentSurfaceTexture as C;
    let start = Instant::now();
    let mut last_present: Option<Instant> = None;
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some((w, h)) = shared.pending_size.lock().unwrap().take() {
            let t = Instant::now();
            gpu.config.width = w;
            gpu.config.height = h;
            gpu.surface.configure(&gpu.device, &gpu.config);
            gpu.renderer.resize(&gpu.device, w, h);
            let mut s = shared.stats.lock().unwrap();
            s.resizes += 1;
            s.resize_us.push(t.elapsed().as_micros() as u32);
            last_present = None;
        }
        if shared.paused.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(8));
            last_present = None;
            continue;
        }

        // Fifo: `get_current_texture` blocks in `nextDrawable` once the
        // drawable pool is exhausted, which paces this loop to the display.
        let t_acquire = Instant::now();
        let frame = match gpu.surface.get_current_texture() {
            C::Success(f) | C::Suboptimal(f) => f,
            C::Timeout | C::Occluded => {
                // Miniaturised / fully covered window: nothing to present.
                shared.stats.lock().unwrap().skipped += 1;
                std::thread::sleep(std::time::Duration::from_millis(16));
                last_present = None;
                continue;
            }
            C::Outdated | C::Lost | C::Validation => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                continue;
            }
        };
        let t_cpu = Instant::now();
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        gpu.renderer.update(
            &gpu.queue,
            start.elapsed().as_secs_f32(),
            gpu.config.width,
            gpu.config.height,
        );
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("path C frame"),
            });
        gpu.renderer.draw(&mut encoder, &view);
        gpu.queue.submit(Some(encoder.finish()));
        let cpu = t_cpu.elapsed();
        gpu.queue.present(frame);
        let now = Instant::now();

        let mut s = shared.stats.lock().unwrap();
        s.frames += 1;
        s.acquire_us
            .push(t_cpu.duration_since(t_acquire).as_micros() as u32);
        s.cpu_us.push(cpu.as_micros() as u32);
        if let Some(prev) = last_present {
            s.interval_us
                .push(now.duration_since(prev).as_micros() as u32);
        }
        last_present = Some(now);
    }
    gpu
}

// ---- C ABI ---------------------------------------------------------------

#[repr(C)]
pub struct M0SurfaceStats {
    pub frames: u64,
    pub skipped: u64,
    pub resizes: u64,
    pub interval_p50_us: u32,
    pub interval_p95_us: u32,
    pub interval_p99_us: u32,
    pub acquire_p50_us: u32,
    pub acquire_p95_us: u32,
    pub acquire_p99_us: u32,
    pub cpu_p50_us: u32,
    pub cpu_p95_us: u32,
    pub cpu_p99_us: u32,
    pub resize_p50_us: u32,
    pub resize_max_us: u32,
}

/// # Safety
/// `layer` must be a live `CAMetalLayer` (retained by the host for the
/// lifetime of the returned context). Call on the main thread.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_surface_create(
    layer: *mut c_void,
    width: u32,
    height: u32,
) -> *mut SurfaceContext {
    if layer.is_null() {
        return std::ptr::null_mut();
    }
    match SurfaceContext::new(layer, width, height) {
        Ok(c) => Box::into_raw(Box::new(c)),
        Err(e) => {
            log::error!("fynkel_m0_surface_create: {e}");
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `ctx` must come from `fynkel_m0_surface_create` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_surface_destroy(ctx: *mut SurfaceContext) {
    if !ctx.is_null() {
        drop(Box::from_raw(ctx));
    }
}

/// New drawable size in physical pixels; applied by the render thread.
/// # Safety
/// `ctx` must be a live context.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_surface_resize(
    ctx: *const SurfaceContext,
    width: u32,
    height: u32,
) {
    if let Some(c) = ctx.as_ref() {
        c.resize(width, height);
    }
}

/// # Safety
/// `ctx` must be a live context.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_surface_set_paused(ctx: *const SurfaceContext, paused: i32) {
    if let Some(c) = ctx.as_ref() {
        c.set_paused(paused != 0);
    }
}

/// Drains and summarizes stats since the previous call.
/// # Safety
/// `ctx` must be a live context; `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_surface_take_stats(
    ctx: *const SurfaceContext,
    out: *mut M0SurfaceStats,
) {
    let (Some(c), Some(out)) = (ctx.as_ref(), out.as_mut()) else {
        return;
    };
    let mut s = c.take_stats();
    let p = super::pct;
    *out = M0SurfaceStats {
        frames: s.frames,
        skipped: s.skipped,
        resizes: s.resizes,
        interval_p50_us: p(&mut s.interval_us, 0.50),
        interval_p95_us: p(&mut s.interval_us, 0.95),
        interval_p99_us: p(&mut s.interval_us, 0.99),
        acquire_p50_us: p(&mut s.acquire_us, 0.50),
        acquire_p95_us: p(&mut s.acquire_us, 0.95),
        acquire_p99_us: p(&mut s.acquire_us, 0.99),
        cpu_p50_us: p(&mut s.cpu_us, 0.50),
        cpu_p95_us: p(&mut s.cpu_us, 0.95),
        cpu_p99_us: p(&mut s.cpu_us, 0.99),
        resize_p50_us: p(&mut s.resize_us, 0.50),
        resize_max_us: p(&mut s.resize_us, 1.0),
    }
}

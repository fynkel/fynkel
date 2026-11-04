//! The Apple target on screen (iOS): frames presented straight to a
//! `CAMetalLayer` the plugin puts under Flutter's see-through view, through
//! a wgpu surface (docs/ios-surface.md). iOS shows the layer beside
//! Flutter's, so Flutter's own frames hold its UI alone and never copy ours.
//!
//! As on Android, the layer comes from the platform side
//! ([`LayerPresenter::attach`]), with its size, and may be taken away
//! ([`LayerPresenter::detach`]); frames are skipped while there is none.
//! Each frame is drawn into the layer's next drawable and presented: Core
//! Animation waits for the GPU itself, so nothing here does.
//!
//! Unlike Android's window, the layer is kept alive by its surface, so the
//! platform side never waits for the render thread: a frame in progress
//! when the layer is replaced or taken away ends on the old one, which is
//! no longer on screen, and a new size is set by the render thread before
//! its next drawable (stage 14's BR5: waiting for a drawable can take a
//! second, and the main thread waited with it).

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::target::Target;

/// What frames are drawn in: the layer's BGRA, through an sRGB view.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// A layer the worker draws to.
struct Live {
    /// The `CAMetalLayer`, to know it again on a resize.
    layer: usize,
    surface: wgpu::Surface<'static>,
    /// Its configuration at the size it was made; the size changes.
    config: wgpu::SurfaceConfiguration,
    /// The size the platform side asked for last, as [`pack`]ed.
    wanted: AtomicU64,
    /// The size the surface is configured at, or 0 when it must be
    /// configured again: only the render thread configures it after it's
    /// made.
    configured: AtomicU64,
}

struct Shared {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    /// The layer drawn to next. Frames hold their own reference to theirs.
    layer: Mutex<Option<Arc<Live>>>,
    /// Frames presented to the layer.
    presented: AtomicU64,
}

/// The render worker's side: draws into the layer the platform side gave
/// it last, and skips frames while there is none.
pub struct Layer {
    queue: wgpu::Queue,
    shared: Arc<Shared>,
    size: (u32, u32),
}

/// The platform side: gives the worker its layer and takes it away. Any
/// thread may call it, and none waits for the render thread.
#[derive(Clone)]
pub struct LayerPresenter {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layer").field("size", &self.size).finish_non_exhaustive()
    }
}

impl std::fmt::Debug for LayerPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerPresenter").field("presented", &self.shared.presented).finish_non_exhaustive()
    }
}

/// One frame: the layer it's drawn to, kept until it's presented, and the
/// drawable.
pub struct LayerFrame {
    image: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
    _layer: Arc<Live>,
}

impl std::fmt::Debug for LayerFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerFrame").finish_non_exhaustive()
    }
}

impl Layer {
    /// A target with no layer yet, and its presenter. `instance` made
    /// `adapter`, which made `device` and `queue`.
    pub fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> (Self, LayerPresenter) {
        let shared = Arc::new(Shared {
            instance: instance.clone(),
            adapter: adapter.clone(),
            device: device.clone(),
            layer: Mutex::new(None),
            presented: AtomicU64::new(0),
        });
        let target = Self { queue: queue.clone(), shared: shared.clone(), size: (width.max(1), height.max(1)) };
        (target, LayerPresenter { shared })
    }
}

impl Target for Layer {
    type Frame = LayerFrame;

    fn format(&self) -> wgpu::TextureFormat {
        FORMAT
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Core Animation's three drawables, in 4 bytes a pixel.
    fn frame_bytes(&self) -> u64 {
        3 * u64::from(self.size.0) * u64::from(self.size.1) * 4
    }

    /// The layer's next drawable, or `None` with no layer. Configures it
    /// at the size last asked for first, then waits for Core Animation to
    /// free a drawable (it has three), holding nothing the platform side
    /// waits for.
    fn begin(&mut self) -> Option<LayerFrame> {
        let live = self.shared.layer.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
        let wanted = live.wanted.load(Ordering::Acquire);
        if wanted != live.configured.load(Ordering::Relaxed) {
            // It fails when another view's thread is submitting, so it's
            // tried again next frame. A size that can never work fails fast,
            // before the wait for the GPU.
            if configure(&self.shared.device, &live.surface, &live.config_at(wanted)).is_err() {
                return None;
            }
            live.configured.store(wanted, Ordering::Relaxed);
        }
        let image = match live.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(image) | wgpu::CurrentSurfaceTexture::Suboptimal(image) => image,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                // Configured again next frame: no size is 0.
                live.configured.store(0, Ordering::Relaxed);
                return None;
            }
            _ => return None,
        };
        let view =
            image.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(FORMAT), ..Default::default() });
        self.size = unpack(wanted);
        Some(LayerFrame { image, view, _layer: live })
    }

    fn view<'a>(&'a self, frame: &'a LayerFrame) -> &'a wgpu::TextureView {
        &frame.view
    }

    fn finish(&mut self, frame: LayerFrame, encoder: wgpu::CommandEncoder, _number: u64) {
        self.queue.submit([encoder.finish()]);
        let LayerFrame { image, view, _layer } = frame;
        drop(view);
        self.queue.present(image);
        self.shared.presented.fetch_add(1, Ordering::Relaxed);
    }

    /// The size comes with each layer ([`LayerPresenter::attach`]).
    fn resize(&mut self, _width: u32, _height: u32) -> Result<(), String> {
        Ok(())
    }
}

impl LayerPresenter {
    /// Draws the next frames to `layer` (a `CAMetalLayer`) at `width` ×
    /// `height` pixels. A frame in progress ends on the old one. The layer
    /// drawn to already, at a new size, keeps its surface: the render
    /// thread configures it at the new size before its next frame, and
    /// skips frames, trying each time, while it can't be configured.
    ///
    /// # Safety
    ///
    /// `layer` is a live `CAMetalLayer`, kept alive until [`detach`] or the
    /// next `attach` returns.
    ///
    /// [`detach`]: LayerPresenter::detach
    pub unsafe fn attach(&self, layer: NonNull<c_void>, width: u32, height: u32) -> Result<(), String> {
        let shared = &self.shared;
        let (width, height) = (width.max(1), height.max(1));
        if let Some(live) = shared.layer.lock().unwrap_or_else(PoisonError::into_inner).as_ref()
            && live.layer == layer.as_ptr() as usize
        {
            live.wanted.store(pack(width, height), Ordering::Release);
            return Ok(());
        }
        // SAFETY: a live CAMetalLayer, per this function's contract; wgpu
        // retains it for the surface's life.
        let surface = unsafe {
            shared.instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer.as_ptr()))
        }
        .map_err(|e| format!("couldn't make a wgpu surface: {e}"))?;
        let caps = surface.get_capabilities(&shared.adapter);
        let format = [FORMAT, FORMAT.remove_srgb_suffix()]
            .into_iter()
            .find(|f| caps.formats.contains(f))
            .ok_or_else(|| format!("the layer offers no BGRA8 format, only {:?}", caps.formats))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: if format == FORMAT { vec![] } else { vec![FORMAT] },
        };
        configure(&shared.device, &surface, &config).map_err(|e| format!("couldn't configure the layer: {e}"))?;
        let live = Live {
            layer: layer.as_ptr() as usize,
            surface,
            config,
            wanted: AtomicU64::new(pack(width, height)),
            configured: AtomicU64::new(pack(width, height)),
        };
        let old = shared.layer.lock().unwrap_or_else(PoisonError::into_inner).replace(Arc::new(live));
        // Dropped out of the lock: the old surface may be let go here.
        drop(old);
        Ok(())
    }

    /// Stops drawing to the layer. A frame in progress ends on it.
    pub fn detach(&self) {
        let old = self.shared.layer.lock().unwrap_or_else(PoisonError::into_inner).take();
        drop(old);
    }

    /// Frames presented so far.
    pub fn presented(&self) -> u64 {
        self.shared.presented.load(Ordering::Relaxed)
    }
}

impl Live {
    /// Its configuration at a [`pack`]ed size.
    fn config_at(&self, size: u64) -> wgpu::SurfaceConfiguration {
        let (width, height) = unpack(size);
        wgpu::SurfaceConfiguration { width, height, ..self.config.clone() }
    }
}

/// A width and height in one number, to be set and read at once.
fn pack(width: u32, height: u32) -> u64 {
    (u64::from(width) << 32) | u64::from(height)
}

fn unpack(size: u64) -> (u32, u32) {
    ((size >> 32) as u32, size as u32)
}

/// `surface.configure`, with its validation error returned instead of
/// panicking the thread.
fn configure(
    device: &wgpu::Device,
    surface: &wgpu::Surface<'_>,
    config: &wgpu::SurfaceConfiguration,
) -> Result<(), wgpu::Error> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(device, config);
    let mut pop = std::pin::pin!(scope.pop());
    match pop.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(Some(error)) => Err(error),
        _ => Ok(()),
    }
}

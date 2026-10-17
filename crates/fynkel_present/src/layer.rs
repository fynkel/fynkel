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

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::handoff::{Handoff, Lease};
use crate::target::Target;

/// What frames are drawn in: the layer's BGRA, through an sRGB view.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// A layer the worker draws to.
struct Live {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// It couldn't be configured again: frames to it are skipped.
    dead: AtomicBool,
}

struct Shared {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    layer: Arc<Handoff<Live>>,
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
/// thread may call it; each call waits for a frame in progress.
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

/// One frame: the layer it's drawn to, held until it's presented, and the
/// drawable.
pub struct LayerFrame {
    image: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
    _layer: Lease<Live>,
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
            layer: Arc::new(Handoff::new()),
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

    /// The layer's next drawable, or `None` with no layer. Waits for Core
    /// Animation to free one (it has three).
    fn begin(&mut self) -> Option<LayerFrame> {
        let live = self.shared.layer.lease()?;
        if live.dead.load(Ordering::Relaxed) {
            return None;
        }
        let image = match live.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(image) | wgpu::CurrentSurfaceTexture::Suboptimal(image) => image,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                if configure(&self.shared.device, &live.surface, &live.config).is_err() {
                    live.dead.store(true, Ordering::Relaxed);
                }
                return None;
            }
            _ => return None,
        };
        let view = image.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(FORMAT),
            ..Default::default()
        });
        self.size = (live.config.width, live.config.height);
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
    /// `height` pixels. Waits for a frame in progress on the old one.
    ///
    /// # Safety
    ///
    /// `layer` is a live `CAMetalLayer`, kept alive until [`detach`] or the
    /// next `attach` returns.
    ///
    /// [`detach`]: LayerPresenter::detach
    pub unsafe fn attach(&self, layer: NonNull<c_void>, width: u32, height: u32) -> Result<(), String> {
        let shared = &self.shared;
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
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: if format == FORMAT { vec![] } else { vec![FORMAT] },
        };
        configure(&shared.device, &surface, &config).map_err(|e| format!("couldn't configure the layer: {e}"))?;
        drop(shared.layer.put(Live { surface, config, dead: AtomicBool::new(false) }));
        Ok(())
    }

    /// Stops drawing to the layer, after a frame in progress.
    pub fn detach(&self) {
        drop(self.shared.layer.take());
    }

    /// Frames presented so far.
    pub fn presented(&self) -> u64 {
        self.shared.presented.load(Ordering::Relaxed)
    }
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

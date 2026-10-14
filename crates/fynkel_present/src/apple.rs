//! The Apple target (macOS and iOS): frames are drawn into IOSurfaces that
//! Flutter shows without a copy. Stage 1 plan §3.5.
//!
//! - The render worker draws into a ring of [`SLOTS`] IOSurfaces, each
//!   imported into wgpu as a Metal texture. [`Ring`] decides which slot
//!   each frame uses.
//! - After a frame is submitted, an empty Metal command buffer is committed
//!   behind it. Metal runs a queue's command buffers in order, so when that
//!   one completes, so has the frame. Its completion handler, on a Metal
//!   thread, marks the slot Ready and calls the frame-ready callback, which
//!   tells Flutter there's a new frame.
//! - Flutter's raster thread then calls [`ApplePresenter::acquire`] for the
//!   surface to show.
//!
//! A resize makes a new set of surfaces and resets the ring. The presenter
//! looks surfaces up under the same lock the swap happens under, so a slot
//! is never paired with the other set's surface.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth,
};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandQueue, MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};

use crate::{Ring, RingStats, SLOTS, Target, Ticket, Wake};

/// The views' format. The surfaces hold BGRA bytes, as CoreVideo and Flutter
/// expect; drawing through an sRGB texture encodes them in hardware.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// `'BGRA'`, CoreVideo's `kCVPixelFormatType_32BGRA`.
const PIXEL_FORMAT_BGRA: i32 = 0x4247_5241;

/// Called on a Metal thread each time a frame finishes, with the userdata
/// given to [`ApplePresenter::set_frame_ready`].
pub type FrameReady = unsafe extern "C" fn(userdata: *mut c_void);

/// Why the Apple target couldn't be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppleError(String);

impl std::fmt::Display for AppleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AppleError {}

/// The ring of IOSurfaces the render worker draws into. Pair it with the
/// [`ApplePresenter`] from [`Apple::new`], which shows them.
#[derive(Debug)]
pub struct Apple {
    device: wgpu::Device,
    queue: wgpu::Queue,
    metal: Metal,
    /// The current set's views, by slot. Each keeps its texture alive.
    views: Vec<wgpu::TextureView>,
    size: (u32, u32),
    shared: Arc<Shared>,
}

/// wgpu's own Metal device and queue.
#[derive(Debug)]
struct Metal {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
}

// SAFETY: MTLDevice and MTLCommandQueue are documented as thread-safe;
// objc2 just can't know that for protocol objects.
unsafe impl Send for Metal {}

#[derive(Debug)]
struct Shared {
    ring: Ring,
    /// The current set's surfaces, by slot. Replaced together with the
    /// ring's reset, under this lock.
    surfaces: Mutex<Vec<Surface>>,
    callback: Mutex<Option<Callback>>,
    /// The render worker's, called when a frame frees its slot.
    wake: WakeSlot,
}

#[derive(Default)]
struct WakeSlot(Mutex<Option<Wake>>);

impl std::fmt::Debug for WakeSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WakeSlot")
    }
}

/// An IOSurface. They are thread-safe objects.
#[derive(Clone, Debug)]
struct Surface(CFRetained<IOSurfaceRef>);

// SAFETY: IOSurfaces may be used from any thread; their pixels are guarded
// by IOSurfaceLock, which `Presented::read_rgba` takes.
unsafe impl Send for Surface {}
// SAFETY: as above.
unsafe impl Sync for Surface {}

#[derive(Clone, Copy, Debug)]
struct Callback {
    function: FrameReady,
    userdata: *mut c_void,
}

// SAFETY: `set_frame_ready`'s caller promises the function may be called
// with this userdata from any thread.
unsafe impl Send for Callback {}

impl Apple {
    /// A `width` × `height` ring on `device`, which must be a Metal device,
    /// and the presenter for it.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<(Self, ApplePresenter), AppleError> {
        let metal = Metal::of(device, queue)?;
        let (width, height) = (width.max(1), height.max(1));
        let (views, surfaces) = make_slots(device, &metal.device, width, height)?;
        let shared = Arc::new(Shared {
            ring: Ring::new(),
            surfaces: Mutex::new(surfaces),
            callback: Mutex::new(None),
            wake: WakeSlot::default(),
        });
        let presenter = ApplePresenter { shared: shared.clone() };
        let target = Self { device: device.clone(), queue: queue.clone(), metal, views, size: (width, height), shared };
        Ok((target, presenter))
    }
}

impl Metal {
    fn of(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self, AppleError> {
        let not_metal = || AppleError("the Apple target needs a Metal device".into());
        // SAFETY: the raw objects are only retained here, not changed, and
        // wgpu keeps using them as before.
        let device_raw = unsafe { device.as_hal::<wgpu_hal::api::Metal>() }.ok_or_else(not_metal)?.raw_device().clone();
        // SAFETY: as above.
        let queue_raw =
            Retained::from(unsafe { queue.as_hal::<wgpu_hal::api::Metal>() }.ok_or_else(not_metal)?.as_raw());
        Ok(Self { device: device_raw, queue: queue_raw })
    }
}

impl Shared {
    fn surfaces(&self) -> MutexGuard<'_, Vec<Surface>> {
        self.surfaces.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn callback(&self) -> MutexGuard<'_, Option<Callback>> {
        self.callback.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// On a Metal thread: the frame's GPU work is done, and its slot free
    /// for the next (the worker is told, after Flutter).
    fn finished(&self, ticket: Ticket) {
        self.complete(ticket);
        let wake = self.wake.0.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    fn complete(&self, ticket: Ticket) {
        // Stale (a resize made new surfaces meanwhile) is expected; the
        // frame is just not shown. Anything else is a ring bug, which its
        // own tests cover.
        if self.ring.complete(ticket).is_ok() {
            // Held while calling, so `set_frame_ready` waits for a call in
            // progress before the userdata may be freed.
            if let Some(callback) = *self.callback() {
                // SAFETY: promised by `set_frame_ready`'s caller.
                unsafe { (callback.function)(callback.userdata) };
            }
        }
    }
}

impl Target for Apple {
    type Frame = Ticket;

    fn format(&self) -> wgpu::TextureFormat {
        FORMAT
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn begin(&mut self) -> Option<Ticket> {
        self.shared.ring.reserve()
    }

    fn view<'a>(&'a self, frame: &'a Ticket) -> &'a wgpu::TextureView {
        &self.views[frame.slot()]
    }

    fn finish(&mut self, frame: Ticket, encoder: wgpu::CommandEncoder, _number: u64) {
        self.queue.submit([encoder.finish()]);
        // Metal calls the handler with no autorelease pool of its own on
        // this thread; without one, each frame would leak what Metal
        // autoreleases.
        objc2::rc::autoreleasepool(|_| {
            let Some(marker) = self.metal.queue.commandBuffer() else {
                // Metal had no command buffer to give. Wait for the frame the
                // slow way instead, so its slot still comes back.
                let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
                self.shared.finished(frame);
                return;
            };
            let shared = self.shared.clone();
            // Metal may call a block more than once in principle; the ticket
            // goes back exactly once.
            let ticket = Mutex::new(Some(frame));
            let handler = RcBlock::new(move |_: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                let ticket = ticket.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(ticket) = ticket {
                    // A panic must not unwind into Metal, which would abort.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.finished(ticket)));
                }
            });
            // SAFETY: Metal copies (retains) the block until it has run.
            unsafe { marker.addCompletedHandler(RcBlock::as_ptr(&handler)) };
            marker.commit();
        });
    }

    fn on_free(&mut self, wake: Wake) -> bool {
        *self.shared.wake.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(wake);
        true
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == self.size {
            return Ok(());
        }
        let (views, surfaces) = make_slots(&self.device, &self.metal.device, width, height).map_err(|e| e.0)?;
        {
            let mut current = self.shared.surfaces();
            *current = surfaces;
            self.shared.ring.reset();
        }
        // Frames in flight on the old textures finish as stale; wgpu keeps
        // the textures alive until then, and Flutter keeps its own
        // reference to the surface it shows.
        self.views = views;
        self.size = (width, height);
        Ok(())
    }
}

/// Shows the frames an [`Apple`] target finishes. Share it with the
/// Flutter plugin; every method may be called from any thread.
#[derive(Clone, Debug)]
pub struct ApplePresenter {
    shared: Arc<Shared>,
}

/// The surface to show, from [`ApplePresenter::acquire`].
#[derive(Debug)]
pub struct Presented {
    surface: Surface,
    /// Another finished frame is waiting: ask Flutter for one more frame.
    pub more: bool,
}

impl ApplePresenter {
    /// The surface to show now (Flutter's raster thread calls this once
    /// per frame), or `None` before the first frame of the current size.
    pub fn acquire(&self) -> Option<Presented> {
        let surfaces = self.shared.surfaces();
        let shown = self.shared.ring.acquire()?;
        Some(Presented { surface: surfaces[shown.slot].clone(), more: shown.more })
    }

    /// Sets the function called on a Metal thread whenever a frame
    /// finishes (`None` for none). Waits for a call in progress, so once
    /// this returns, the old userdata is no longer used.
    ///
    /// # Safety
    ///
    /// `function` must be safe to call with `userdata` from any thread,
    /// until it is replaced by another call to this.
    pub unsafe fn set_frame_ready(&self, function: Option<FrameReady>, userdata: *mut c_void) {
        *self.shared.callback() = function.map(|function| Callback { function, userdata });
    }

    /// The ring's counts.
    pub fn stats(&self) -> RingStats {
        self.shared.ring.stats()
    }
}

impl Presented {
    /// The IOSurface, retained once more for the caller, who must release it
    /// (Swift: `Unmanaged.takeRetainedValue`).
    pub fn into_raw(self) -> *mut c_void {
        CFRetained::into_raw(self.surface.0).as_ptr().cast()
    }

    pub fn size(&self) -> (u32, u32) {
        (self.surface.0.width() as u32, self.surface.0.height() as u32)
    }

    /// The pixels, as RGBA8 rows top to bottom. For tests and debugging: it
    /// locks the surface and copies it on the CPU.
    pub fn read_rgba(&self) -> Vec<u8> {
        let surface = &self.surface.0;
        let (width, height) = self.size();
        let row = surface.bytes_per_row();
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        // SAFETY: locked for reading while the bytes are read; the base
        // address is valid for height × bytes_per_row bytes while locked.
        unsafe {
            assert_eq!(surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()), 0, "IOSurfaceLock");
            let base = surface.base_address().as_ptr().cast::<u8>();
            for y in 0..height as usize {
                let bgra = std::slice::from_raw_parts(base.add(y * row), width as usize * 4);
                for p in bgra.chunks_exact(4) {
                    rgba.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
                }
            }
            surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        }
        rgba
    }
}

/// A set of [`SLOTS`] surfaces and their views.
fn make_slots(
    device: &wgpu::Device,
    metal: &ProtocolObject<dyn MTLDevice>,
    width: u32,
    height: u32,
) -> Result<(Vec<wgpu::TextureView>, Vec<Surface>), AppleError> {
    let mut views = Vec::with_capacity(SLOTS);
    let mut surfaces = Vec::with_capacity(SLOTS);
    for slot in 0..SLOTS {
        let surface = new_surface(width, height)?;
        views.push(import(device, metal, &surface, width, height, slot)?.create_view(&Default::default()));
        surfaces.push(surface);
    }
    Ok((views, surfaces))
}

fn new_surface(width: u32, height: u32) -> Result<Surface, AppleError> {
    // SAFETY: these are constant CFStrings exported by IOSurface.
    let keys: [&CFString; 4] =
        unsafe { [kIOSurfaceWidth, kIOSurfaceHeight, kIOSurfaceBytesPerElement, kIOSurfacePixelFormat] };
    let (w, h) = (CFNumber::new_i32(width as i32), CFNumber::new_i32(height as i32));
    let (bytes, format) = (CFNumber::new_i32(4), CFNumber::new_i32(PIXEL_FORMAT_BGRA));
    let properties = CFDictionary::<CFString, CFNumber>::from_slices(&keys, &[&*w, &*h, &*bytes, &*format]);
    // SAFETY: the properties are IOSurface's documented keys with number
    // values.
    let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) };
    surface.map(Surface).ok_or_else(|| AppleError(format!("IOSurfaceCreate failed for {width} × {height}")))
}

/// `surface` as a wgpu texture: a Metal texture on wgpu's own device whose
/// storage is the surface.
fn import(
    device: &wgpu::Device,
    metal: &ProtocolObject<dyn MTLDevice>,
    surface: &Surface,
    width: u32,
    height: u32,
    slot: usize,
) -> Result<wgpu::Texture, AppleError> {
    // SAFETY: a plain 2D descriptor with valid sizes.
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm_sRGB,
            width as usize,
            height as usize,
            false,
        )
    };
    descriptor.setUsage(MTLTextureUsage::RenderTarget);
    descriptor.setStorageMode(MTLStorageMode::Shared);
    let raw = metal
        .newTextureWithDescriptor_iosurface_plane(&descriptor, &surface.0, 0)
        .ok_or_else(|| AppleError(format!("Metal couldn't make a texture of surface {slot}")))?;
    // SAFETY: `raw` is a 2D, single-level, single-layer texture of FORMAT
    // and this size, made on wgpu's device; it's handed over uninitialised,
    // and the descriptor below describes it exactly.
    let texture = unsafe {
        let hal = wgpu_hal::metal::Device::texture_from_raw(
            raw,
            FORMAT,
            MTLTextureType::Type2D,
            1,
            1,
            wgpu_hal::CopyExtent { width, height, depth: 1 },
            None,
        );
        device.create_texture_from_hal::<wgpu_hal::api::Metal>(
            hal,
            &wgpu::TextureDescriptor {
                label: Some("apple slot"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
            wgpu::TextureUses::UNINITIALIZED,
        )
    };
    Ok(texture)
}

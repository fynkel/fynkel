//! A ring of IOSurface-backed Metal textures Bevy renders into and Flutter
//! presents (a reduced copy of platform/apple/src/ring.rs).
//!
//! Slot states: Free → Rendering → Ready → Presented → Retiring → Free.
//! A slot becomes Ready only when its GPU work completed. The presenter's
//! current and previous slots are never rendered into. With no Free slot the
//! oldest unseen Ready one is recycled; with none of those the frame drops.

use std::ffi::c_void;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth,
};
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};

/// `kCVPixelFormatType_32BGRA`.
const PIXEL_FORMAT_BGRA: i32 = 0x4247_5241;
/// Bevy writes sRGB-encoded bytes; Flutter reads the surface as sRGB.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

pub type FrameReadyFn = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Free,
    Rendering,
    Ready,
    Presented,
    Retiring,
}

struct Slot {
    state: State,
    seq: u64,
    surface: CFRetained<IOSurfaceRef>,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct Inner {
    slots: Vec<Slot>,
    generation: u64,
    next_seq: u64,
    width: u32,
    height: u32,
    presented_new: u64,
    /// Present finished frames in order (one per acquire) instead of
    /// jumping to the newest; see `acquire`.
    queue: bool,
}

pub struct Ring {
    device: wgpu::Device,
    mtl_device: Retained<ProtocolObject<dyn MTLDevice>>,
    inner: Mutex<Inner>,
    callback: Mutex<Option<(FrameReadyFn, usize)>>,
}

// IOSurface and MTLDevice are thread-safe (IOSurface is designed for
// cross-process sharing; Metal devices are free-threaded). The ring's own
// state is behind its mutexes.
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

/// A slot handed to one frame.
pub struct Reservation {
    pub index: usize,
    pub generation: u64,
    pub seq: u64,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

const SLOTS: usize = 5;

/// Queue mode: finished frames waiting for the presenter beyond this are
/// dropped (oldest first), so the queue adds at most this many frames of
/// latency.
const MAX_QUEUED: usize = 2;

impl Ring {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Result<Ring, String> {
        let mtl_device = unsafe {
            let hal = device
                .as_hal::<wgpu::hal::api::Metal>()
                .ok_or("device is not Metal")?;
            hal.raw_device().clone()
        };
        let slots = build_slots(device, &mtl_device, width, height)?;
        Ok(Ring {
            device: device.clone(),
            mtl_device,
            inner: Mutex::new(Inner {
                slots,
                generation: 0,
                next_seq: 1,
                width,
                height,
                presented_new: 0,
                queue: true,
            }),
            callback: Mutex::new(None),
        })
    }

    pub fn resize(&self, width: u32, height: u32) -> Result<(), String> {
        let slots = build_slots(&self.device, &self.mtl_device, width, height)?;
        let mut inner = self.inner.lock().unwrap();
        inner.slots = slots;
        inner.generation += 1;
        inner.width = width;
        inner.height = height;
        Ok(())
    }

    pub fn surfaces(&self) -> Vec<*mut c_void> {
        let inner = self.inner.lock().unwrap();
        inner
            .slots
            .iter()
            .map(|s| CFRetained::as_ptr(&s.surface).as_ptr() as *mut c_void)
            .collect()
    }

    pub fn reserve(&self) -> Option<Reservation> {
        let mut inner = self.inner.lock().unwrap();
        let index = match inner.slots.iter().position(|s| s.state == State::Free) {
            Some(i) => i,
            None => inner
                .slots
                .iter()
                .enumerate()
                .filter(|(_, s)| s.state == State::Ready)
                .min_by_key(|(_, s)| s.seq)
                .map(|(i, _)| i)?,
        };
        let seq = inner.next_seq;
        inner.next_seq += 1;
        let (generation, width, height) = (inner.generation, inner.width, inner.height);
        let slot = &mut inner.slots[index];
        slot.state = State::Rendering;
        slot.seq = seq;
        Some(Reservation {
            index,
            generation,
            seq,
            view: slot.view.clone(),
            width,
            height,
        })
    }

    /// The frame never reached the GPU.
    pub fn cancel(&self, r: &Reservation) {
        let mut inner = self.inner.lock().unwrap();
        if inner.generation == r.generation {
            let slot = &mut inner.slots[r.index];
            if slot.seq == r.seq && slot.state == State::Rendering {
                slot.state = State::Free;
            }
        }
    }

    /// GPU work for the frame completed (Metal completion thread).
    pub fn complete(&self, index: usize, generation: u64, seq: u64) {
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation {
                return;
            }
            let slot = &mut inner.slots[index];
            if slot.seq != seq || slot.state != State::Rendering {
                return;
            }
            slot.state = State::Ready;
        }
        let cb = self.callback.lock().unwrap();
        if let Some((func, ud)) = *cb {
            unsafe { func(ud as *mut c_void) };
        }
    }

    /// The presenter (Flutter's raster thread): -1 before the first frame.
    ///
    /// Newest mode: the newest Ready slot (older ones are dropped).
    /// Queue mode: the oldest Ready slot, with bit 8 set when another is
    /// waiting (the host then asks for one more frame). A frame that finishes
    /// a moment after a raster is shown on the next one instead of being
    /// replaced by its successor; that is what lost ~40 % of frames on the
    /// iPhone, where Bevy's ~8 ms per frame lands right at each vsync.
    pub fn acquire(&self) -> i32 {
        let mut inner = self.inner.lock().unwrap();
        let mut ready: Vec<(u64, usize)> = inner
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.state == State::Ready)
            .map(|(i, s)| (s.seq, i))
            .collect();
        ready.sort_unstable();
        let current = inner.slots.iter().position(|s| s.state == State::Presented);
        if ready.is_empty() {
            return current.map_or(-1, |i| i as i32);
        }
        let skip = if inner.queue {
            ready.len().saturating_sub(MAX_QUEUED)
        } else {
            ready.len() - 1
        };
        for &(_, i) in &ready[..skip] {
            inner.slots[i].state = State::Free;
        }
        let next = ready[skip].1;
        let more = ready.len() > skip + 1;
        for slot in inner.slots.iter_mut() {
            if slot.state == State::Retiring {
                slot.state = State::Free;
            }
        }
        if let Some(c) = current {
            inner.slots[c].state = State::Retiring;
        }
        inner.slots[next].state = State::Presented;
        inner.presented_new += 1;
        next as i32 | if more { 0x100 } else { 0 }
    }

    pub fn set_queue(&self, on: bool) {
        self.inner.lock().unwrap().queue = on;
    }

    pub fn take_presented(&self) -> u64 {
        std::mem::take(&mut self.inner.lock().unwrap().presented_new)
    }

    /// Waits for an in-progress callback, so `None` is safe before freeing `ud`.
    pub fn set_callback(&self, func: Option<FrameReadyFn>, ud: *mut c_void) {
        *self.callback.lock().unwrap() = func.map(|f| (f, ud as usize));
    }
}

fn build_slots(
    device: &wgpu::Device,
    mtl_device: &ProtocolObject<dyn MTLDevice>,
    width: u32,
    height: u32,
) -> Result<Vec<Slot>, String> {
    (0..SLOTS)
        .map(|i| {
            let surface = create_iosurface(width, height)?;
            let texture = import(device, mtl_device, &surface, width, height, i)?;
            let view = texture.create_view(&Default::default());
            Ok(Slot {
                state: State::Free,
                seq: 0,
                surface,
                _texture: texture,
                view,
            })
        })
        .collect()
}

fn create_iosurface(width: u32, height: u32) -> Result<CFRetained<IOSurfaceRef>, String> {
    let keys: [&CFString; 4] = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfacePixelFormat,
        ]
    };
    let w = CFNumber::new_i32(width as i32);
    let h = CFNumber::new_i32(height as i32);
    let bpe = CFNumber::new_i32(4);
    let fmt = CFNumber::new_i32(PIXEL_FORMAT_BGRA);
    let values: [&CFNumber; 4] = [&w, &h, &bpe, &fmt];
    let props: CFRetained<CFDictionary<CFString, CFNumber>> =
        CFDictionary::from_slices(&keys, &values);
    let props: &CFDictionary = props.as_opaque();
    unsafe { IOSurfaceRef::new(props) }.ok_or_else(|| "IOSurfaceCreate failed".to_string())
}

fn import(
    device: &wgpu::Device,
    mtl_device: &ProtocolObject<dyn MTLDevice>,
    surface: &IOSurfaceRef,
    width: u32,
    height: u32,
    index: usize,
) -> Result<wgpu::Texture, String> {
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm_sRGB,
            width as usize,
            height as usize,
            false,
        )
    };
    desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    desc.setStorageMode(MTLStorageMode::Shared);
    let raw: Retained<ProtocolObject<dyn MTLTexture>> = mtl_device
        .newTextureWithDescriptor_iosurface_plane(&desc, surface, 0)
        .ok_or_else(|| format!("newTextureWithDescriptor:iosurface: failed for slot {index}"))?;
    let hal = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            raw,
            FORMAT,
            MTLTextureType::Type2D,
            1,
            1,
            wgpu::hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
        )
    };
    Ok(unsafe {
        device.create_texture_from_hal::<wgpu::hal::api::Metal>(
            hal,
            &wgpu::TextureDescriptor {
                label: Some("bevy_flutter iosurface"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        )
    })
}

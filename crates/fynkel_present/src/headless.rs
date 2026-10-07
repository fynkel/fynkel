//! The headless target: frames are drawn into an offscreen texture and read
//! back to the CPU. The tests and CI use it; nothing is shown.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::Target;

/// The texture format. RGBA, so the bytes read back need no swizzle.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// An offscreen texture, read back after every frame. Pair it with the
/// [`HeadlessFrames`] from [`Headless::new`] to get the pixels.
#[derive(Debug)]
pub struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    /// Bytes per row in `readback`, padded as wgpu requires.
    padded_row: u32,
    shared: Arc<Shared>,
}

/// The pixels of the latest frame read back, shared with the readers.
#[derive(Debug)]
struct Shared {
    latest: Mutex<Latest>,
    changed: Condvar,
}

#[derive(Debug)]
struct Latest {
    /// 0 until the first frame.
    number: u64,
    width: u32,
    height: u32,
    /// RGBA8, rows unpadded.
    rgba: Vec<u8>,
}

/// Reads the frames a [`Headless`] target finished, from any thread.
#[derive(Clone, Debug)]
pub struct HeadlessFrames {
    shared: Arc<Shared>,
}

/// Why [`HeadlessFrames::read`] gave nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// No frame numbered `wanted` or later finished in time. `latest` is the
    /// last one that did (0 for none).
    Timeout { wanted: u64, latest: u64 },
    /// The buffer isn't width × height × 4 bytes.
    WrongSize { expected: usize, got: usize },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Timeout { wanted, latest } => {
                write!(f, "frame {wanted} didn't finish in time (the latest finished is {latest})")
            }
            ReadError::WrongSize { expected, got } => write!(f, "the buffer is {got} bytes, a frame is {expected}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl Headless {
    /// A `width` × `height` target on `device`, and the reader for it.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) -> (Self, HeadlessFrames) {
        let (width, height) = (width.max(1), height.max(1));
        let (texture, view, readback, padded_row) = buffers(device, width, height);
        let shared = Arc::new(Shared {
            latest: Mutex::new(Latest { number: 0, width, height, rgba: vec![0; (width * height * 4) as usize] }),
            changed: Condvar::new(),
        });
        let frames = HeadlessFrames { shared: shared.clone() };
        let target = Self { device: device.clone(), queue: queue.clone(), texture, view, readback, padded_row, shared };
        (target, frames)
    }
}

/// The texture, its view, the readback buffer and its padded row size.
fn buffers(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView, wgpu::Buffer, u32) {
    {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("headless readback"),
            size: u64::from(padded_row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        (texture, view, readback, padded_row)
    }
}

impl Target for Headless {
    type Frame = ();

    fn format(&self) -> wgpu::TextureFormat {
        FORMAT
    }

    fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }

    fn begin(&mut self) -> Option<()> {
        Some(())
    }

    fn view<'a>(&'a self, _frame: &'a ()) -> &'a wgpu::TextureView {
        &self.view
    }

    fn finish(&mut self, _frame: (), mut encoder: wgpu::CommandEncoder, number: u64) {
        let (width, height) = self.size();
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: None,
                },
            },
            self.texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("mapping the headless readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("waiting for the headless frame");
        {
            let data = slice.get_mapped_range().expect("mapped above");
            let mut latest = self.shared.latest.lock().unwrap_or_else(PoisonError::into_inner);
            if (latest.width, latest.height) != (width, height) {
                (latest.width, latest.height) = (width, height);
                latest.rgba.resize((width * height * 4) as usize, 0);
            }
            let row = (width * 4) as usize;
            for (y, padded) in data.chunks(self.padded_row as usize).take(height as usize).enumerate() {
                latest.rgba[y * row..(y + 1) * row].copy_from_slice(&padded[..row]);
            }
            debug_assert!(number > latest.number, "frame {number} finished after frame {}", latest.number);
            latest.number = number;
        }
        self.readback.unmap();
        self.shared.changed.notify_all();
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) != self.size() {
            (self.texture, self.view, self.readback, self.padded_row) = buffers(&self.device, width, height);
        }
        Ok(())
    }
}

impl HeadlessFrames {
    /// The size of the latest frame (the size asked for, before the first).
    pub fn size(&self) -> (u32, u32) {
        let latest = self.shared.latest.lock().unwrap_or_else(PoisonError::into_inner);
        (latest.width, latest.height)
    }

    /// Waits up to `timeout` for frame `wanted` or a later one to finish,
    /// copies its RGBA pixels into `out` (width × height × 4 bytes of that
    /// frame, rows top to bottom) and returns its number.
    pub fn read(&self, wanted: u64, timeout: Duration, out: &mut [u8]) -> Result<u64, ReadError> {
        let deadline = Instant::now() + timeout;
        let mut latest = self.shared.latest.lock().unwrap_or_else(PoisonError::into_inner);
        while latest.number < wanted {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ReadError::Timeout { wanted, latest: latest.number });
            }
            latest = self.shared.changed.wait_timeout(latest, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        if out.len() != latest.rgba.len() {
            return Err(ReadError::WrongSize { expected: latest.rgba.len(), got: out.len() });
        }
        out.copy_from_slice(&latest.rgba);
        Ok(latest.number)
    }
}

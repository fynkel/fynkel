//! The per-frame data, written where the GPU reads it.
//!
//! The frame uniforms and the instances change every frame.
//! `queue.write_buffer` would stage them and copy them on the GPU with a
//! blit, and on Apple GPUs a process that blits every frame keeps 128 MB of
//! driver memory dirty for as long as it does (docs/stage-1.md §6 q6). The
//! GPUs Fynkel targets share memory with the CPU, so the CPU writes each
//! frame's data straight into a buffer the GPU reads in place, which is
//! Metal's own advice for per-frame data. There are three buffers, used in
//! turn, so the CPU writes one while the GPU may still be reading the two
//! before it.
//!
//! A buffer goes round: mapped (the CPU may write it), in flight (written,
//! unmapped, read by the frame it was written for), mapping (that frame was
//! submitted; wgpu maps it again once the GPU is done with it).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Buffers in the ring.
pub(crate) const SLOTS: usize = 3;

/// Where a buffer's parts start: the uniforms at 0, then the shadow maps'
/// view matrices, each at its own dynamic offset, then the view's cells
/// (`clusters`), then the instances.
#[derive(Clone, Copy)]
struct Layout {
    uniforms: u64,
    views_at: u64,
    views: u64,
    cells_at: u64,
    instances_at: u64,
}

impl Layout {
    fn new(uniforms: u64, view_stride: u64, views: u64) -> Self {
        let views_at = uniforms.next_multiple_of(view_stride);
        // Storage offsets need 256 at most.
        let cells_at = (views_at + views).next_multiple_of(256);
        let instances_at = (cells_at + crate::clusters::BYTES).next_multiple_of(wgpu::VERTEX_ALIGNMENT);
        Self { uniforms, views_at, views, cells_at, instances_at }
    }
}

// Where a slot's buffer is. wgpu's map callback sets MAPPED or FAILED.
const MAPPED: u8 = 0;
const IN_FLIGHT: u8 = 1;
const MAPPING: u8 = 2;
const FAILED: u8 = 3;

struct Slot {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    views: wgpu::BindGroup,
    /// Shared with the map callback; one per buffer, so a steady frame
    /// allocates nothing for it.
    state: Arc<AtomicU8>,
}

/// What a frame binds: the uniforms' bind group, the shadow views' (a
/// map's at its index times the view stride), and the buffer holding the
/// instances from [`Written::instances`] on, the line vertices from
/// [`Written::lines`] on and the shadow casters from [`Written::casters`]
/// on.
pub(crate) struct Written<'a> {
    pub(crate) bind_group: &'a wgpu::BindGroup,
    pub(crate) views: &'a wgpu::BindGroup,
    pub(crate) buffer: &'a wgpu::Buffer,
    pub(crate) instances: u64,
    pub(crate) lines: u64,
    pub(crate) casters: u64,
}

/// A frame's data, as [`FrameRing::write`] writes it: the `uniforms`,
/// shadow `views`, `instances`, `lines` (vertices, straight after the
/// instances), shadow `casters` (after the lines) and the view's `cells`.
pub(crate) struct Parts<'a> {
    pub(crate) uniforms: &'a [u8],
    pub(crate) views: &'a [u8],
    pub(crate) instances: &'a [u8],
    pub(crate) lines: &'a [u8],
    pub(crate) casters: &'a [u8],
    pub(crate) cells: &'a [u8],
}

/// The three buffers. The uniforms are bound with `layout`'s binding 0,
/// the shadow views with `views_layout`'s.
pub(crate) struct FrameRing {
    layout: wgpu::BindGroupLayout,
    views_layout: wgpu::BindGroupLayout,
    parts: Layout,
    slots: Vec<Slot>,
    /// The slot the next frame writes.
    next: usize,
    /// The slot the last frame wrote, until it is asked to map again.
    last: Option<usize>,
}

impl FrameRing {
    /// A ring for `uniforms` bytes of uniforms, `views` bytes of shadow
    /// views (64-byte matrices `view_stride` apart), and room for
    /// `instance_bytes` of instances to start with.
    pub(crate) fn new(
        device: &wgpu::Device,
        (layout, uniforms): (wgpu::BindGroupLayout, u64),
        (views_layout, view_stride, views): (wgpu::BindGroupLayout, u64, u64),
        instance_bytes: u64,
    ) -> Self {
        let parts = Layout::new(uniforms, view_stride, views);
        let mut ring = Self { layout, views_layout, parts, slots: Vec::with_capacity(SLOTS), next: 0, last: None };
        for _ in 0..SLOTS {
            let slot = ring.create_slot(device, instance_bytes);
            ring.slots.push(slot);
        }
        ring
    }

    /// A slot whose buffer is mapped and holds `instance_bytes` of
    /// instances. Mapped at creation, so wgpu zeroes it on the CPU.
    fn create_slot(&self, device: &wgpu::Device, instance_bytes: u64) -> Slot {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame data"),
            size: self.parts.instances_at + instance_bytes.max(wgpu::COPY_BUFFER_ALIGNMENT),
            usage: wgpu::BufferUsages::MAP_WRITE
                | wgpu::BufferUsages::UNIFORM
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(self.parts.uniforms),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: self.parts.cells_at,
                        size: wgpu::BufferSize::new(crate::clusters::BYTES),
                    }),
                },
            ],
        });
        let views = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow views"),
            layout: &self.views_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: self.parts.views_at,
                    size: wgpu::BufferSize::new(64),
                }),
            }],
        });
        Slot { buffer, bind_group, views, state: Arc::new(AtomicU8::new(MAPPED)) }
    }

    /// Writes a frame's `parts` into the next buffer,
    /// waiting for the GPU if it still reads all three. The frame that
    /// binds what this returns must be submitted before the next `write`,
    /// which maps its buffer again.
    pub(crate) fn write(&mut self, device: &wgpu::Device, parts: Parts<'_>) -> Written<'_> {
        let Parts { uniforms, views, instances, lines, casters, cells } = parts;
        debug_assert_eq!(uniforms.len() as u64, self.parts.uniforms);
        debug_assert!(views.len() as u64 <= self.parts.views);
        debug_assert!(cells.len() as u64 <= crate::clusters::BYTES);
        if let Some(last) = self.last.take() {
            let slot = &self.slots[last];
            slot.state.store(MAPPING, Ordering::Relaxed);
            let state = slot.state.clone();
            slot.buffer.map_async(wgpu::MapMode::Write, .., move |result| {
                state.store(if result.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
            });
        }

        let index = self.next;
        self.next = (index + 1) % SLOTS;
        self.wait_until_mapped(device, index);

        let at = self.parts.instances_at;
        let lines_at = at + instances.len() as u64;
        let casters_at = (lines_at + lines.len() as u64).next_multiple_of(wgpu::VERTEX_ALIGNMENT);
        let data = casters_at - at + casters.len() as u64;
        if self.slots[index].buffer.size() < at + data {
            // Dropping the old buffer frees it once the GPU is done with it.
            self.slots[index] = self.create_slot(device, data.next_power_of_two());
        }

        let slot = &mut self.slots[index];
        {
            let mut view = slot.buffer.get_mapped_range_mut(..).expect("the slot is mapped");
            view.slice(..uniforms.len()).copy_from_slice(uniforms);
            let views_at = self.parts.views_at as usize;
            view.slice(views_at..views_at + views.len()).copy_from_slice(views);
            let cells_at = self.parts.cells_at as usize;
            view.slice(cells_at..cells_at + cells.len()).copy_from_slice(cells);
            view.slice(at as usize..lines_at as usize).copy_from_slice(instances);
            view.slice(lines_at as usize..lines_at as usize + lines.len()).copy_from_slice(lines);
            view.slice(casters_at as usize..casters_at as usize + casters.len()).copy_from_slice(casters);
        }
        slot.buffer.unmap();
        slot.state.store(IN_FLIGHT, Ordering::Relaxed);
        self.last = Some(index);
        Written {
            bind_group: &slot.bind_group,
            views: &slot.views,
            buffer: &slot.buffer,
            instances: at,
            lines: lines_at,
            casters: casters_at,
        }
    }

    /// Lets wgpu finish the mapping of slot `index`, waiting for the GPU
    /// only if it hasn't. A mapping that failed gets a new buffer.
    fn wait_until_mapped(&mut self, device: &wgpu::Device, index: usize) {
        let state = |slot: &Slot| slot.state.load(Ordering::Acquire);
        debug_assert_ne!(state(&self.slots[index]), IN_FLIGHT, "every frame but the last is mapping again");
        // Runs the callbacks of finished mappings without blocking.
        let _ = device.poll(wgpu::PollType::Poll);
        if state(&self.slots[index]) == MAPPING {
            // The GPU still reads it, three frames on: wait for it.
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
        if state(&self.slots[index]) != MAPPED {
            // Failed, or still pending after the GPU went idle; neither
            // happens unless the device is lost. A fresh buffer keeps the
            // ring going either way.
            let instance_bytes = self.slots[index].buffer.size() - self.parts.instances_at;
            self.slots[index] = self.create_slot(device, instance_bytes);
        }
    }
}

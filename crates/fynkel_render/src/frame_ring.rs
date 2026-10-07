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

/// Where the instances start in a buffer: after the uniforms, at the
/// alignment vertex buffers need.
fn instances_at(uniforms: u64) -> u64 {
    uniforms.next_multiple_of(wgpu::VERTEX_ALIGNMENT)
}

// Where a slot's buffer is. wgpu's map callback sets MAPPED or FAILED.
const MAPPED: u8 = 0;
const IN_FLIGHT: u8 = 1;
const MAPPING: u8 = 2;
const FAILED: u8 = 3;

struct Slot {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Shared with the map callback; one per buffer, so a steady frame
    /// allocates nothing for it.
    state: Arc<AtomicU8>,
}

/// What a frame binds: the uniforms' bind group, and the buffer holding
/// the instances from [`Written::instances`] on.
pub(crate) struct Written<'a> {
    pub(crate) bind_group: &'a wgpu::BindGroup,
    pub(crate) buffer: &'a wgpu::Buffer,
    pub(crate) instances: u64,
}

/// The three buffers. The uniforms are bound with `layout`'s binding 0.
pub(crate) struct FrameRing {
    layout: wgpu::BindGroupLayout,
    uniforms: u64,
    slots: Vec<Slot>,
    /// The slot the next frame writes.
    next: usize,
    /// The slot the last frame wrote, until it is asked to map again.
    last: Option<usize>,
}

impl FrameRing {
    /// A ring for `uniforms` bytes of uniforms and room for
    /// `instance_bytes` of instances to start with.
    pub(crate) fn new(
        device: &wgpu::Device,
        layout: wgpu::BindGroupLayout,
        uniforms: u64,
        instance_bytes: u64,
    ) -> Self {
        let mut ring = Self { layout, uniforms, slots: Vec::with_capacity(SLOTS), next: 0, last: None };
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
            size: instances_at(self.uniforms) + instance_bytes.max(wgpu::COPY_BUFFER_ALIGNMENT),
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(self.uniforms),
                }),
            }],
        });
        Slot { buffer, bind_group, state: Arc::new(AtomicU8::new(MAPPED)) }
    }

    /// Writes a frame's `uniforms` and `instances` into the next buffer,
    /// waiting for the GPU if it still reads all three. The frame that
    /// binds what this returns must be submitted before the next `write`,
    /// which maps its buffer again.
    pub(crate) fn write(&mut self, device: &wgpu::Device, uniforms: &[u8], instances: &[u8]) -> Written<'_> {
        debug_assert_eq!(uniforms.len() as u64, self.uniforms);
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

        let at = instances_at(self.uniforms);
        if self.slots[index].buffer.size() < at + instances.len() as u64 {
            // Dropping the old buffer frees it once the GPU is done with it.
            self.slots[index] = self.create_slot(device, (instances.len() as u64).next_power_of_two());
        }

        let slot = &mut self.slots[index];
        {
            let mut view = slot.buffer.get_mapped_range_mut(..).expect("the slot is mapped");
            view.slice(..uniforms.len()).copy_from_slice(uniforms);
            view.slice(at as usize..at as usize + instances.len()).copy_from_slice(instances);
        }
        slot.buffer.unmap();
        slot.state.store(IN_FLIGHT, Ordering::Relaxed);
        self.last = Some(index);
        Written { bind_group: &slot.bind_group, buffer: &slot.buffer, instances: at }
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
            let instance_bytes = self.slots[index].buffer.size() - instances_at(self.uniforms);
            self.slots[index] = self.create_slot(device, instance_bytes);
        }
    }
}

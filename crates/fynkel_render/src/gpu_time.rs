//! The GPU's time for each part of a frame (stage 14 phase 4), for
//! measuring: each pass writes the GPU's clock as it starts and ends, the
//! clocks are copied back a frame or two later, and the times add up per
//! part. Off unless asked for ([`Renderer::set_gpu_timing`]): copying the
//! clocks back is a blit every frame, which keeps a pool of driver memory
//! dirty on Apple GPUs (tests/gpu_memory.rs).
//!
//! On a tiled GPU (every phone's) a pass's start is when its vertices start
//! and its end when its last pixels are written, and the next pass's
//! vertices may start before that, so the parts may overlap a little and
//! add up to more than the frame.
//!
//! [`Renderer::set_gpu_timing`]: crate::Renderer::set_gpu_timing

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// The GPU's time for each part of a frame, in nanoseconds, added up over
/// [`frames`](Self::frames) timed frames since the renderer was made. Only
/// grows: subtract an earlier one for a stretch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuTimes {
    /// The frames timed so far.
    pub frames: u64,
    /// The sun's and the spot lights' shadow maps.
    pub shadows: u64,
    /// A new sky's light, worked out when the sky changes.
    pub sky_light: u64,
    /// Ambient occlusion: the solid scene's depth, the occlusion and its
    /// blur.
    pub ambient_occlusion: u64,
    /// The scene itself: solid, sky, see-through, particles and lines,
    /// with its depth kept and what's behind put back where they are.
    pub scene: u64,
    /// Bloom's chain of smaller and larger images.
    pub bloom: u64,
    /// The scene image onto the view: tone mapping, the glow added.
    pub finish: u64,
    /// The screen effects.
    pub effects: u64,
}

/// The part of a frame a pass belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Shadows,
    SkyLight,
    AmbientOcclusion,
    Scene,
    Bloom,
    Finish,
    Effects,
}

/// The passes timed in one frame; any beyond go untimed.
const PASSES: u32 = 64;
/// Frames whose clocks may be on their way back at once; a frame with no
/// room goes untimed.
const RING: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    /// Copied into by a frame not yet known to be sent.
    Written,
    /// Asked to be read, when the GPU is done with it.
    Mapping,
}

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

struct Slot {
    buffer: wgpu::Buffer,
    state: State,
    mapped: Arc<AtomicU8>,
    parts: Vec<Part>,
}

struct Queries {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
}

pub(crate) struct GpuTimer {
    device: wgpu::Device,
    /// Nanoseconds a tick of the GPU's clock.
    period: f64,
    on: bool,
    /// Made the first time timing is turned on.
    queries: Option<Queries>,
    /// The slot this frame's clocks go to, if it's timed.
    frame: Option<usize>,
    /// This frame's passes so far, each's part.
    passes: RefCell<Vec<Part>>,
    used: Cell<u32>,
    totals: GpuTimes,
}

impl GpuTimer {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            device: device.clone(),
            period: f64::from(queue.get_timestamp_period()),
            on: false,
            queries: None,
            frame: None,
            passes: RefCell::new(Vec::with_capacity(PASSES as usize)),
            used: Cell::new(0),
            totals: GpuTimes::default(),
        }
    }

    /// Turns timing on or off. Off where the device can't time passes.
    pub(crate) fn set(&mut self, on: bool) {
        self.on = on && self.device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        if self.on && self.queries.is_none() {
            let device = &self.device;
            let bytes = u64::from(PASSES * 2) * u64::from(wgpu::QUERY_SIZE);
            let buffer = |label, usage| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: bytes,
                    usage,
                    mapped_at_creation: false,
                })
            };
            self.queries = Some(Queries {
                set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("gpu time"),
                    ty: wgpu::QueryType::Timestamp,
                    count: PASSES * 2,
                }),
                resolve: buffer("gpu time resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
                slots: (0..RING)
                    .map(|_| Slot {
                        buffer: buffer("gpu time read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
                        state: State::Free,
                        mapped: Arc::new(AtomicU8::new(PENDING)),
                        parts: Vec::with_capacity(PASSES as usize),
                    })
                    .collect(),
            });
        }
    }

    /// The times so far, with any frames' clocks that are back added.
    pub(crate) fn times(&mut self) -> GpuTimes {
        self.collect();
        self.totals
    }

    /// At the start of a frame's encoding: earlier frames' clocks asked
    /// for (they've been sent by now) or added up, and a slot for this
    /// frame's, if it's timed and there's room.
    pub(crate) fn begin(&mut self) {
        self.collect();
        self.used.set(0);
        self.passes.get_mut().clear();
        self.frame = match (&self.queries, self.on) {
            (Some(q), true) => q.slots.iter().position(|s| s.state == State::Free),
            _ => None,
        };
    }

    /// The clocks for a pass of `part` to write, if this frame is timed.
    pub(crate) fn pass(&self, part: Part) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.frame?;
        let queries = self.queries.as_ref()?;
        let n = self.used.get();
        if n == PASSES {
            return None;
        }
        self.used.set(n + 1);
        self.passes.borrow_mut().push(part);
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &queries.set,
            beginning_of_pass_write_index: Some(n * 2),
            end_of_pass_write_index: Some(n * 2 + 1),
        })
    }

    /// At the end of a frame's encoding: its clocks copied to be read.
    pub(crate) fn end(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let (Some(slot), Some(queries)) = (self.frame.take(), &mut self.queries) else { return };
        let used = self.used.get();
        if used == 0 {
            return;
        }
        encoder.resolve_query_set(&queries.set, 0..used * 2, &queries.resolve, 0);
        let bytes = u64::from(used * 2) * u64::from(wgpu::QUERY_SIZE);
        let slot = &mut queries.slots[slot];
        encoder.copy_buffer_to_buffer(&queries.resolve, 0, &slot.buffer, 0, bytes);
        slot.state = State::Written;
        slot.parts.clear();
        slot.parts.extend_from_slice(self.passes.get_mut());
    }

    fn collect(&mut self) {
        let Some(queries) = &mut self.queries else { return };
        let mut mapping = false;
        for slot in &mut queries.slots {
            match slot.state {
                State::Free => {}
                State::Written => {
                    slot.state = State::Mapping;
                    slot.mapped.store(PENDING, Ordering::Relaxed);
                    let mapped = slot.mapped.clone();
                    let bytes = slot.parts.len() as u64 * 2 * u64::from(wgpu::QUERY_SIZE);
                    slot.buffer.slice(..bytes).map_async(wgpu::MapMode::Read, move |result| {
                        mapped.store(if result.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
                    });
                    mapping = true;
                }
                State::Mapping => mapping = true,
            }
        }
        if !mapping {
            return;
        }
        let _ = self.device.poll(wgpu::PollType::Poll);
        for slot in &mut queries.slots {
            if slot.state != State::Mapping {
                continue;
            }
            match slot.mapped.load(Ordering::Acquire) {
                MAPPED => {
                    let bytes = slot.parts.len() as u64 * 2 * u64::from(wgpu::QUERY_SIZE);
                    {
                        let Ok(view) = slot.buffer.get_mapped_range(..bytes) else {
                            slot.state = State::Free;
                            continue;
                        };
                        let clocks: &[u64] = bytemuck::cast_slice(&view);
                        for (part, clock) in slot.parts.iter().zip(clocks.chunks_exact(2)) {
                            // A pass the GPU skipped, or a clock that went
                            // back, adds nothing.
                            if clock[0] == 0 || clock[1] <= clock[0] {
                                continue;
                            }
                            let nanos = ((clock[1] - clock[0]) as f64 * self.period) as u64;
                            let t = &mut self.totals;
                            *match part {
                                Part::Shadows => &mut t.shadows,
                                Part::SkyLight => &mut t.sky_light,
                                Part::AmbientOcclusion => &mut t.ambient_occlusion,
                                Part::Scene => &mut t.scene,
                                Part::Bloom => &mut t.bloom,
                                Part::Finish => &mut t.finish,
                                Part::Effects => &mut t.effects,
                            } += nanos;
                        }
                    }
                    slot.buffer.unmap();
                    self.totals.frames += 1;
                    slot.state = State::Free;
                }
                FAILED => slot.state = State::Free,
                _ => {}
            }
        }
    }
}

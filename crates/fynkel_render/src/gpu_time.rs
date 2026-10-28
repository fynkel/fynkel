//! The GPU's time for each part of a frame (stage 14 phase 4), for
//! measuring: each pass writes the GPU's clock as it starts and ends, the
//! clocks are copied back a frame or two later, and the times add up per
//! part. Off unless asked for ([`Renderer::set_gpu_timing`]): copying the
//! clocks back is a blit every frame, which keeps a pool of driver memory
//! dirty on Apple GPUs (tests/gpu_memory.rs).
//!
//! A pass's clocks are read as its vertices start and as its last pixels
//! are written. On a tiled GPU (Apple's, every phone's) those are rough:
//! the next pass's vertices may start before that, a small pass's start
//! may be read after its end, and even its end before the end of the pass
//! it reads from. So each pass counts only the time it adds past the
//! latest end so far (this frame's or, for its first, the frame before's),
//! from its start if that's later: the parts never overlap, add up to the frame's time on the GPU less any waits between
//! passes, and a pass done within another's time counts nothing. They
//! say where the time goes, not to the tenth of a millisecond.
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
    /// Passes in the frames timed whose end the GPU didn't give (or, first
    /// in a frame, whose start): left out of the times above. Many of them say the times are short
    /// of the frames'.
    pub untimed: u64,
}

impl GpuTimes {
    /// Every part's time added up.
    pub fn total(&self) -> u64 {
        self.shadows + self.sky_light + self.ambient_occlusion + self.scene + self.bloom + self.finish + self.effects
    }
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

/// What Metal gives for a clock it didn't read (`MTLCounterErrorValue`).
const NO_CLOCK: u64 = u64::MAX;

/// One frame's clocks: written by its passes, copied back and read. Each
/// frame on its way has its own, so a frame's passes never write over the
/// clocks of the one before while they're being copied (Metal doesn't
/// order the two).
struct Slot {
    set: wgpu::QuerySet,
    buffer: wgpu::Buffer,
    state: State,
    mapped: Arc<AtomicU8>,
    parts: Vec<Part>,
    /// Which frame timed it was, counting from 1: they're read in order.
    sent: u64,
}

struct Queries {
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
    /// The frames given a slot so far.
    sent: u64,
    /// Where the GPU had got to, as of the last frame read: its last
    /// pass's end. The next frame's first pass counts from there, not
    /// from when its vertices started while that frame was still drawing.
    done: Option<u64>,
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
            sent: 0,
            done: None,
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
                resolve: buffer("gpu time resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
                slots: (0..RING)
                    .map(|_| Slot {
                        set: device.create_query_set(&wgpu::QuerySetDescriptor {
                            label: Some("gpu time"),
                            ty: wgpu::QueryType::Timestamp,
                            count: PASSES * 2,
                        }),
                        buffer: buffer("gpu time read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
                        state: State::Free,
                        mapped: Arc::new(AtomicU8::new(PENDING)),
                        parts: Vec::with_capacity(PASSES as usize),
                        sent: 0,
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
        let slot = &self.queries.as_ref()?.slots[self.frame?];
        let n = self.used.get();
        if n == PASSES {
            return None;
        }
        self.used.set(n + 1);
        self.passes.borrow_mut().push(part);
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &slot.set,
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
        let slot = &mut queries.slots[slot];
        encoder.resolve_query_set(&slot.set, 0..used * 2, &queries.resolve, 0);
        let bytes = u64::from(used * 2) * u64::from(wgpu::QUERY_SIZE);
        encoder.copy_buffer_to_buffer(&queries.resolve, 0, &slot.buffer, 0, bytes);
        slot.state = State::Written;
        self.sent += 1;
        slot.sent = self.sent;
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
        // The frames in the order they were sent, up to the first not back.
        while let Some(slot) = queries.slots.iter_mut().filter(|s| s.state != State::Free).min_by_key(|s| s.sent) {
            if slot.state != State::Mapping {
                break;
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
                        let done = &mut self.done;
                        for (part, clock) in slot.parts.iter().zip(clocks.chunks_exact(2)) {
                            let (start, end) = (clock[0], clock[1]);
                            let read = |c: u64| c != 0 && c != NO_CLOCK;
                            // From its start, or from where the pass before
                            // ended if that's later or its start is wrong.
                            let from = match (read(start) && start <= end, *done) {
                                (true, Some(done)) => Some(start.max(done)),
                                (true, None) => Some(start),
                                (false, done) => done,
                            };
                            let (true, Some(from)) = (read(end), from) else {
                                self.totals.untimed += 1;
                                continue;
                            };
                            // Ended within what's counted already: adds
                            // nothing.
                            *done = Some(done.map_or(end, |d| d.max(end)));
                            let nanos = (end.saturating_sub(from) as f64 * self.period) as u64;
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
                _ => break,
            }
        }
    }
}

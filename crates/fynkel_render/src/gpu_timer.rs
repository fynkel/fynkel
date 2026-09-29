//! GPU timestamps: the frame's span and each pass's, read back a few frames
//! later.

use super::*;

/// A read-back slot of the [`GpuTimer`]: `state` 0 free, 1 copied into by
/// a `draw` (the host submits it), 2 mapping requested, 3 mapped.
pub(crate) struct TimestampSlot {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    /// Which pass pairs the frame wrote (a skipped shadow, AO or present
    /// pass leaves stale values in its queries).
    pub(crate) shadow: bool,
    pub(crate) ao: bool,
    pub(crate) present: bool,
}

/// Pass timestamps (`wgpu::Features::TIMESTAMP_QUERY`, M3-16): a frame's
/// passes write begin/end timestamps into one of two query sets; the next
/// frame's `draw` resolves the previous set before its own passes (on a
/// tile-based GPU a resolve in the same command buffer can run before the
/// last pass's fragment stage has written its end stamp) and copies it
/// into a free read-back slot; `prepare` maps the slots the host has
/// submitted and reads the ones that came back — never blocking, so the
/// numbers describe a frame three or four behind the one being prepared.
pub(crate) struct GpuTimer {
    pub(crate) query_sets: [wgpu::QuerySet; 2],
    /// The set this frame's passes write; the other holds the previous
    /// frame's stamps until `draw` resolves it.
    pub(crate) writing: usize,
    /// The previous frame's pass flags, resolved with its set.
    pub(crate) pending: Option<(bool, bool, bool)>,
    pub(crate) resolve: wgpu::Buffer,
    pub(crate) slots: Vec<TimestampSlot>,
    pub(crate) next: usize,
    pub(crate) untimed: u32,
}

impl GpuTimer {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let query_sets = std::array::from_fn(|i| {
            device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some(&format!("fynkel pass timestamps {i}")),
                ty: wgpu::QueryType::Timestamp,
                count: TIMESTAMP_QUERIES,
            })
        });
        let size = u64::from(TIMESTAMP_QUERIES) * 8;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fynkel timestamp resolve"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..TIMESTAMP_SLOTS)
            .map(|i| TimestampSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("fynkel timestamp read-back {i}")),
                    size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Default::default(),
                shadow: false,
                ao: false,
                present: false,
            })
            .collect();
        Self {
            query_sets,
            writing: 0,
            pending: None,
            resolve,
            slots,
            next: 0,
            untimed: 0,
        }
    }

    /// The pass's timestamp writes for query pair `pair`.
    pub(crate) fn writes(&self, pair: u32) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.writes_part(pair, true, true)
    }

    /// Half a pair: the AO stage's three passes share pair 4, the first
    /// writing its beginning and the last its end.
    pub(crate) fn writes_part(
        &self,
        pair: u32,
        begin: bool,
        end: bool,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.query_sets[self.writing],
            beginning_of_pass_write_index: begin.then_some(pair * 2),
            end_of_pass_write_index: end.then_some(pair * 2 + 1),
        })
    }

    /// Before a frame's passes: resolves the previous frame's set into the
    /// next free slot (the frame goes untimed when every slot is still in
    /// flight) and swaps the sets; `shadow`/`ao`/`present` are this
    /// frame's pass flags, kept for its own resolve next time.
    pub(crate) fn begin_frame(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        shadow: bool,
        ao: bool,
        present: bool,
    ) {
        use std::sync::atomic::Ordering;
        if let Some((shadow, ao, present)) = self.pending.take() {
            let previous = 1 - self.writing;
            let i = self.next;
            let slot = &mut self.slots[i];
            if slot.state.load(Ordering::Acquire) != 0 {
                self.untimed += 1;
            } else {
                encoder.resolve_query_set(
                    &self.query_sets[previous],
                    0..TIMESTAMP_QUERIES,
                    &self.resolve,
                    0,
                );
                encoder.copy_buffer_to_buffer(
                    &self.resolve,
                    0,
                    &slot.buffer,
                    0,
                    u64::from(TIMESTAMP_QUERIES) * 8,
                );
                slot.shadow = shadow;
                slot.ao = ao;
                slot.present = present;
                slot.state.store(1, Ordering::Release);
                self.next = (i + 1) % TIMESTAMP_SLOTS;
            }
        }
        self.writing = 1 - self.writing;
        self.pending = Some((shadow, ao, present));
    }

    /// Reads back what has arrived and asks for what has been submitted:
    /// the pass times of the most recently mapped slot, in microseconds
    /// (shadow, ambient occlusion, scene, post, whole frame).
    pub(crate) fn collect(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<(u32, u32, u32, u32, u32)> {
        use std::sync::atomic::Ordering;
        // Map callbacks fire from the poll.
        let _ = device.poll(wgpu::PollType::Poll);
        let period = queue.get_timestamp_period();
        let mut latest = None;
        // Oldest first, so the newest mapped slot wins.
        for k in 0..TIMESTAMP_SLOTS {
            let slot = &self.slots[(self.next + k) % TIMESTAMP_SLOTS];
            match slot.state.load(Ordering::Acquire) {
                3 => {
                    let ticks: Vec<u64> = match slot.buffer.slice(..).get_mapped_range() {
                        Ok(view) => bytemuck::cast_slice::<u8, u64>(&view).to_vec(),
                        Err(_) => Vec::new(),
                    };
                    slot.buffer.unmap();
                    slot.state.store(0, Ordering::Release);
                    if ticks.len() < TIMESTAMP_QUERIES as usize {
                        continue;
                    }
                    let us = |ticks: u64| -> u32 {
                        (ticks as f64 * f64::from(period) / 1000.0).round() as u32
                    };
                    let span = |b: usize, e: usize| us(ticks[e].saturating_sub(ticks[b]));
                    let shadow = if slot.shadow { span(0, 1) } else { 0 };
                    let ao_span = 2 * AO_PAIR as usize;
                    let ao = if slot.ao {
                        span(ao_span, ao_span + 1)
                    } else {
                        0
                    };
                    let scene = span(2, 3);
                    let post = span(4, 5) + if slot.present { span(6, 7) } else { 0 };
                    // A tile-based GPU starts a pass's vertex stage before
                    // the previous pass's fragment stage ends, so the
                    // passes overlap: the frame is the span from the first
                    // begin to the last end, not their sum.
                    let mut pairs = vec![1, 2];
                    if slot.shadow {
                        pairs.push(0);
                    }
                    if slot.ao {
                        pairs.push(AO_PAIR as usize);
                    }
                    if slot.present {
                        pairs.push(3);
                    }
                    let first = pairs.iter().map(|p| ticks[p * 2]).min().unwrap_or(0);
                    let last = pairs.iter().map(|p| ticks[p * 2 + 1]).max().unwrap_or(0);
                    latest = Some((shadow, ao, scene, post, us(last.saturating_sub(first))));
                }
                1 => {
                    let state = slot.state.clone();
                    slot.state.store(2, Ordering::Release);
                    slot.buffer
                        .slice(..)
                        .map_async(wgpu::MapMode::Read, move |r| {
                            // A failed map (device lost) frees the slot; the
                            // frame stays untimed.
                            state.store(if r.is_ok() { 3 } else { 0 }, Ordering::Release);
                        });
                }
                _ => {}
            }
        }
        latest
    }
}

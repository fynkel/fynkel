//! Minimal frame-time instrumentation. Reports p50/p95/p99 every second and
//! on exit for: presented-frame interval, acquire wait (time blocked in
//! `get_current_texture`, i.e. vsync), and CPU encode+submit work; since
//! M3-16 also the simulation (ticks + extract), the renderer's prepare,
//! the GPU frame span from Metal timestamps, and memory (§15.4).
//! This is the seed of the benchmark manifest capture (blueprint §15.1).

use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct FrameTiming {
    /// The world's ticks and extract for this frame (M3-16).
    pub simulate: Duration,
    pub acquire_wait: Duration,
    pub cpu: Duration,
    pub presented: bool,
    /// The renderer's counts for this frame (draws, instances, culled …).
    pub render: Option<fynkel_render::FrameStats>,
}

impl FrameTiming {
    pub fn skipped() -> Self {
        Self {
            simulate: Duration::ZERO,
            acquire_wait: Duration::ZERO,
            cpu: Duration::ZERO,
            presented: false,
            render: None,
        }
    }
}

#[derive(Default)]
struct Series {
    interval_us: Vec<u32>,
    acquire_us: Vec<u32>,
    cpu_us: Vec<u32>,
    sim_us: Vec<u32>,
    prepare_us: Vec<u32>,
    /// GPU frame spans read back this window (they trail the draw).
    gpu_us: Vec<u32>,
    skipped: u32,
    /// The last presented frame's renderer counts.
    render: Option<fynkel_render::FrameStats>,
}

impl Series {
    fn drain_into(&mut self, other: &mut Series) {
        other.interval_us.append(&mut self.interval_us);
        other.acquire_us.append(&mut self.acquire_us);
        other.cpu_us.append(&mut self.cpu_us);
        other.sim_us.append(&mut self.sim_us);
        other.prepare_us.append(&mut self.prepare_us);
        other.gpu_us.append(&mut self.gpu_us);
        other.skipped += std::mem::take(&mut self.skipped);
        other.render = self.render.or(other.render);
    }
}

pub struct FrameStats {
    last_present: Option<Instant>,
    window_start: Instant,
    window: Series,
    total: Series,
    /// `FrameStats::gpu_timed` of the last recorded frame: a change means
    /// a new GPU read-back.
    gpu_timed: u32,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            last_present: None,
            window_start: Instant::now(),
            window: Series::default(),
            total: Series::default(),
            gpu_timed: 0,
        }
    }
}

impl FrameStats {
    pub fn record(&mut self, t: FrameTiming) {
        let now = Instant::now();
        if t.presented {
            if let Some(last) = self.last_present {
                self.window
                    .interval_us
                    .push(now.duration_since(last).as_micros() as u32);
            }
            self.last_present = Some(now);
            self.window
                .acquire_us
                .push(t.acquire_wait.as_micros() as u32);
            self.window.cpu_us.push(t.cpu.as_micros() as u32);
            self.window.sim_us.push(t.simulate.as_micros() as u32);
            if let Some(r) = t.render {
                self.window.prepare_us.push(r.prepare_us);
                if r.gpu_timestamps && r.gpu_timed != self.gpu_timed {
                    self.gpu_timed = r.gpu_timed;
                    self.window.gpu_us.push(r.gpu_frame_us);
                }
            }
            self.window.render = t.render.or(self.window.render);
        } else {
            self.window.skipped += 1;
        }

        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            print("1s", &self.window);
            self.window.drain_into(&mut self.total);
            self.window_start = now;
        }
    }

    pub fn report_final(&mut self) {
        self.window.drain_into(&mut self.total);
        print("total", &self.total);
    }
}

fn pct(v: &[u32], q: f64) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    let idx = ((s.len() as f64 - 1.0) * q).round() as usize;
    s[idx] as f32 / 1000.0
}

fn print(label: &str, s: &Series) {
    if s.interval_us.is_empty() {
        return;
    }
    let r = s.render.unwrap_or_default();
    log::info!(
        "[{label}] frames={} skipped={} | interval ms p50={:.2} p95={:.2} p99={:.2} | acquire-wait ms p50={:.2} p95={:.2} | cpu ms p50={:.3} p95={:.3} p99={:.3} | draws={} instances={} culled={} batches={} skinned={} shadow={}/{} | sim ms p50={:.3} p95={:.3} | prepare ms p50={:.3} p95={:.3} | gpu ms p50={:.3} p95={:.3} n={} untimed={} | mem MB native={:.1} resident={:.1} gpu={:.1}",
        s.interval_us.len() + 1,
        s.skipped,
        pct(&s.interval_us, 0.50),
        pct(&s.interval_us, 0.95),
        pct(&s.interval_us, 0.99),
        pct(&s.acquire_us, 0.50),
        pct(&s.acquire_us, 0.95),
        pct(&s.cpu_us, 0.50),
        pct(&s.cpu_us, 0.95),
        pct(&s.cpu_us, 0.99),
        r.draws,
        r.instances,
        r.culled,
        r.batches,
        r.skinned,
        r.shadow_casters,
        r.shadow_draws,
        pct(&s.sim_us, 0.50),
        pct(&s.sim_us, 0.95),
        pct(&s.prepare_us, 0.50),
        pct(&s.prepare_us, 0.95),
        pct(&s.gpu_us, 0.50),
        pct(&s.gpu_us, 0.95),
        s.gpu_us.len(),
        r.gpu_untimed,
        fynkel_core::memory::heap_bytes() as f64 / 1048576.0,
        fynkel_core::memory::resident_bytes() as f64 / 1048576.0,
        r.gpu_bytes as f64 / 1048576.0,
    );
}

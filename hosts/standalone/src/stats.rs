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
    /// Frames presented in the window. Not `interval_us.len() + 1`: a
    /// window's first interval reaches back into the one before.
    presented: u32,
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
        other.presented += std::mem::take(&mut self.presented);
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
        if let Some(line) = self.record_at(Instant::now(), t) {
            log::info!("{line}");
        }
    }

    /// [`record`](Self::record) at `now`; the finished window's line when
    /// this frame closed one.
    fn record_at(&mut self, now: Instant, t: FrameTiming) -> Option<String> {
        if t.presented {
            if let Some(last) = self.last_present {
                self.window
                    .interval_us
                    .push(now.duration_since(last).as_micros() as u32);
            }
            self.last_present = Some(now);
            self.window.presented += 1;
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
            let line = line("1s", &self.window);
            self.window.drain_into(&mut self.total);
            self.window_start = now;
            return line;
        }
        None
    }

    pub fn report_final(&mut self) {
        self.window.drain_into(&mut self.total);
        if let Some(line) = line("total", &self.total) {
            log::info!("{line}");
        }
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

/// The window's line, as `benchmarks/m3/capture.py` reads it; `None`
/// when it has no interval to report.
fn line(label: &str, s: &Series) -> Option<String> {
    if s.interval_us.is_empty() {
        return None;
    }
    let r = s.render.unwrap_or_default();
    Some(format!(
        "[{label}] frames={} skipped={} | interval ms p50={:.2} p95={:.2} p99={:.2} | acquire-wait ms p50={:.2} p95={:.2} | cpu ms p50={:.3} p95={:.3} p99={:.3} | draws={} instances={} culled={} batches={} skinned={} shadow={}/{} | sim ms p50={:.3} p95={:.3} | prepare ms p50={:.3} p95={:.3} | gpu ms p50={:.3} p95={:.3} n={} untimed={} | mem MB native={:.1} resident={:.1} gpu={:.1}",
        s.presented,
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
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presented() -> FrameTiming {
        FrameTiming {
            presented: true,
            ..FrameTiming::skipped()
        }
    }

    fn frames(line: &str) -> u32 {
        let at = line.find("frames=").unwrap() + "frames=".len();
        line[at..].split(' ').next().unwrap().parse().unwrap()
    }

    /// 60 frames a second for three seconds: every `[1s]` window says 60
    /// (the capture's frames per second is their median), and the total
    /// says 180.
    #[test]
    fn every_window_counts_the_frames_it_presented() {
        let start = Instant::now();
        let mut stats = FrameStats {
            window_start: start,
            ..FrameStats::default()
        };
        let mut windows = Vec::new();
        for i in 1..=180u64 {
            // A frame every 1/60 s, the last one of each second just past it.
            let now = start + Duration::from_micros(i * 16_667);
            if let Some(line) = stats.record_at(now, presented()) {
                windows.push(line);
            }
        }
        assert_eq!(windows.len(), 3, "{windows:?}");
        for w in &windows {
            assert!(w.starts_with("[1s] "), "{w}");
            assert_eq!(frames(w), 60, "{w}");
        }
        stats.window.drain_into(&mut stats.total);
        assert_eq!(frames(&line("total", &stats.total).unwrap()), 180);
    }

    #[test]
    fn a_skipped_frame_is_counted_apart_and_a_window_without_an_interval_says_nothing() {
        let start = Instant::now();
        let mut stats = FrameStats {
            window_start: start,
            ..FrameStats::default()
        };
        stats.record_at(start + Duration::from_millis(10), FrameTiming::skipped());
        stats.record_at(start + Duration::from_millis(20), presented());
        // One presented frame has no interval yet: the second ends silent.
        assert!(stats
            .record_at(start + Duration::from_millis(1000), FrameTiming::skipped())
            .is_none());
        stats.record_at(start + Duration::from_millis(1020), presented());
        stats.window.drain_into(&mut stats.total);
        let total = line("total", &stats.total).unwrap();
        assert!(total.contains("frames=2 skipped=2"), "{total}");
    }

    #[test]
    fn percentiles_pick_the_nearest_rank_in_milliseconds() {
        assert!(pct(&[], 0.5).is_nan());
        assert_eq!(pct(&[3000], 0.99), 3.0);
        let v: Vec<u32> = (1..=100).rev().map(|i| i * 1000).collect();
        assert_eq!(pct(&v, 0.0), 1.0);
        assert_eq!(pct(&v, 0.5), 51.0);
        assert_eq!(pct(&v, 0.95), 95.0);
        assert_eq!(pct(&v, 1.0), 100.0);
    }
}

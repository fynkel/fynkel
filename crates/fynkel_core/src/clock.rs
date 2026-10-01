//! Accumulator fixed step with render interpolation (ADR-0008).

use fynkel_contracts::{Tick, DEFAULT_FIXED_DT, DEFAULT_MAX_STEPS_PER_FRAME, MAX_FRAME_DT};

/// What a host frame must do, decided by [`FrameClock::begin_frame`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramePlan {
    /// Fixed ticks to run this frame (`0..=max_steps_per_frame`).
    pub steps: u32,
    /// Simulation time dropped because the cap was hit; `> 0` means a
    /// `MissedDeadline` diagnostic was recorded for this frame.
    pub dropped_seconds: f32,
    /// True when scaled gameplay dt exceeded `max_frame_dt` (debugger pause,
    /// window drag) and was clamped before accumulation.
    pub clamped: bool,
    pub real_delta: f64,
    pub gameplay_delta: f64,
    pub clamped_seconds: f64,
}

#[derive(Clone, Debug)]
pub struct FrameClock {
    fixed_dt: f32,
    max_steps_per_frame: u32,
    max_frame_dt: f32,
    accumulator: f32,
    tick: Tick,
    frame: u64,
    /// Ticks the current plan still owes; `step()` decrements it.
    pending_steps: u32,
    elapsed: f64,
    real_elapsed: f64,
    gameplay_elapsed: f64,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new(
            DEFAULT_FIXED_DT as f32,
            DEFAULT_MAX_STEPS_PER_FRAME,
            MAX_FRAME_DT as f32,
        )
    }
}

impl FrameClock {
    pub fn new(fixed_dt: f32, max_steps_per_frame: u32, max_frame_dt: f32) -> Self {
        assert!(
            fixed_dt > 0.0 && fixed_dt.is_finite(),
            "fixed_dt must be positive"
        );
        assert!(
            max_frame_dt.is_finite() && max_frame_dt > 0.0,
            "max frame dt must be positive"
        );
        assert!(max_steps_per_frame >= 1, "max_steps_per_frame must be >= 1");
        Self {
            fixed_dt,
            max_steps_per_frame,
            max_frame_dt,
            accumulator: 0.0,
            tick: 0,
            frame: 0,
            pending_steps: 0,
            elapsed: 0.0,
            real_elapsed: 0.0,
            gameplay_elapsed: 0.0,
        }
    }

    pub fn fixed_dt(&self) -> f32 {
        self.fixed_dt
    }
    pub fn tick(&self) -> Tick {
        self.tick
    }
    pub fn frame(&self) -> u64 {
        self.frame
    }
    /// Simulated seconds (`tick * fixed_dt`).
    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }
    pub fn real_elapsed(&self) -> f64 {
        self.real_elapsed
    }
    pub fn gameplay_elapsed(&self) -> f64 {
        self.gameplay_elapsed
    }
    pub fn pending_steps(&self) -> u32 {
        self.pending_steps
    }

    /// Interpolation factor between S[n] and S[n+1] for render extraction.
    pub fn alpha(&self) -> f32 {
        (self.accumulator / self.fixed_dt).clamp(0.0, 1.0)
    }

    /// Accumulates a frame's wall time and decides how many ticks to run.
    /// Non-finite or negative dt counts as zero.
    pub fn begin_frame(&mut self, dt: impl Into<f64>) -> FramePlan {
        self.begin_frame_scaled(dt.into(), 1.0)
    }

    /// Sample scale at frame begin. Gameplay advances before callbacks even
    /// when simulation drops time or no fixed tick runs.
    pub fn begin_frame_scaled(&mut self, dt: f64, time_scale: f64) -> FramePlan {
        assert!(
            time_scale.is_finite() && time_scale >= 0.0,
            "invalid time scale"
        );
        self.frame += 1;
        let real_delta = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let scaled = real_delta * time_scale;
        let gameplay_delta = scaled.min(self.max_frame_dt as f64);
        let clamped_seconds = scaled - gameplay_delta;
        let clamped = clamped_seconds > 0.0;
        self.real_elapsed += real_delta;
        self.gameplay_elapsed += gameplay_delta;
        // Simulation retains its f32 fixed-step accumulator. Absolute host
        // timelines use f64 and never derive from interpolation remainder.
        self.accumulator += gameplay_delta as f32;
        let wanted = (self.accumulator / self.fixed_dt).floor() as u32;
        let steps = wanted.min(self.max_steps_per_frame);
        let dropped_seconds = if wanted > steps {
            // Drop time beyond the cap; keep the sub-step remainder so alpha
            // stays continuous.
            let keep = steps as f32 * self.fixed_dt + (self.accumulator % self.fixed_dt);
            let dropped = self.accumulator - keep;
            self.accumulator = keep;
            dropped
        } else {
            0.0
        };
        self.pending_steps = steps;
        FramePlan {
            steps,
            dropped_seconds,
            clamped,
            real_delta,
            gameplay_delta,
            clamped_seconds,
        }
    }

    /// Consumes one fixed step. Returns the new tick number. Stepping more
    /// than the plan allowed still advances the tick (a headless test loop
    /// may step without a plan) but never lets the accumulator go negative.
    pub fn consume_step(&mut self) -> Tick {
        if self.pending_steps > 0 {
            self.pending_steps -= 1;
            self.accumulator = (self.accumulator - self.fixed_dt).max(0.0);
        }
        self.tick += 1;
        self.elapsed = self.tick as f64 * self.fixed_dt as f64;
        self.tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timelines_separate_gameplay_clamp_and_simulation_drop() {
        let mut c = FrameClock::new(0.01, 4, 0.25);
        let p = c.begin_frame_scaled(2.0, 2.0);
        assert_eq!(p.real_delta, 2.0);
        assert_eq!(p.gameplay_delta, 0.25);
        assert_eq!(p.clamped_seconds, 3.75);
        assert_eq!(c.gameplay_elapsed(), 0.25);
        assert_eq!(c.real_elapsed(), 2.0);
        assert_eq!(p.steps, 4);
        assert!(p.dropped_seconds > 0.19);
        for _ in 0..p.steps {
            c.consume_step();
        }
        assert!((c.elapsed() - 0.04).abs() < 1e-6);
        let p = c.begin_frame_scaled(1.0, 0.0);
        assert_eq!(p.steps, 0);
        assert_eq!(c.gameplay_elapsed(), 0.25);
        assert_eq!(c.real_elapsed(), 3.0);
        let before = c.alpha();
        c.consume_step(); // Editor Step: no frame/host time accepted.
        assert_eq!(c.alpha(), before);
        assert!((c.elapsed() - 0.05).abs() < 1e-6);
        assert_eq!(c.gameplay_elapsed(), 0.25);
        assert_eq!(c.real_elapsed(), 3.0);
    }
    #[test]
    fn zero_tick_frames_and_invalid_delta_have_consistent_timelines() {
        let mut c = FrameClock::new(0.1, 4, 0.25);
        assert_eq!(c.begin_frame_scaled(0.02, 0.5).steps, 0);
        assert_eq!(c.gameplay_elapsed(), 0.01);
        assert_eq!(c.real_elapsed(), 0.02);
        assert_eq!(c.elapsed(), 0.0);
        for dt in [f64::NAN, f64::INFINITY, -1.0] {
            let p = c.begin_frame_scaled(dt, 1.0);
            assert_eq!(p.real_delta, 0.0);
            assert_eq!(p.gameplay_delta, 0.0);
        }
        assert_eq!(c.gameplay_elapsed(), 0.01);
        assert_eq!(c.real_elapsed(), 0.02);
    }

    #[test]
    fn sixty_hz_runs_one_tick_per_frame() {
        let mut c = FrameClock::default();
        let mut ticks = 0;
        for _ in 0..600 {
            let p = c.begin_frame(1.0 / 60.0);
            for _ in 0..p.steps {
                c.consume_step();
                ticks += 1;
            }
            assert_eq!(p.dropped_seconds, 0.0);
        }
        assert!((598..=601).contains(&ticks), "{ticks}");
    }

    #[test]
    fn one_twenty_hz_ticks_every_other_frame_with_alpha() {
        let mut c = FrameClock::default();
        let steps: Vec<u32> = (0..8)
            .map(|_| {
                let s = c.begin_frame(1.0 / 120.0).steps;
                for _ in 0..s {
                    c.consume_step();
                }
                s
            })
            .collect();
        assert_eq!(steps.iter().sum::<u32>(), 4);
        assert!(steps.windows(2).all(|w| w[0] != w[1]), "{steps:?}");
        // After a non-tick frame alpha is ~0.5.
        let p = c.begin_frame(1.0 / 120.0);
        if p.steps == 0 {
            assert!((c.alpha() - 0.5).abs() < 1e-3, "{}", c.alpha());
        }
    }

    #[test]
    fn catch_up_is_capped_and_time_dropped() {
        let mut c = FrameClock::new(1.0 / 60.0, 4, 0.25);
        let p = c.begin_frame(0.2); // 12 ticks wanted
        assert_eq!(p.steps, 4);
        // 12 ticks wanted (11 after float floor), 4 run, sub-step remainder kept.
        assert!(
            (7.0 / 60.0 - 1e-4..=8.0 / 60.0 + 1e-4).contains(&p.dropped_seconds),
            "{}",
            p.dropped_seconds
        );
        for _ in 0..p.steps {
            c.consume_step();
        }
        assert!(c.alpha() < 1.0, "sub-step remainder only: {}", c.alpha());
        assert!(!p.clamped);
        let p = c.begin_frame(3.0);
        assert!(p.clamped);
        assert_eq!(p.steps, 4);
    }

    #[test]
    fn garbage_dt_is_zero() {
        let mut c = FrameClock::default();
        assert_eq!(c.begin_frame(f32::NAN).steps, 0);
        assert_eq!(c.begin_frame(-1.0).steps, 0);
        assert_eq!(c.alpha(), 0.0);
    }
}

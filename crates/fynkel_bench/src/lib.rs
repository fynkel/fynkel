//! The crates' benchmarks (stage 14's GT1): each times one thing the frame
//! path does, takes the median of many runs, and fails when it's over its
//! bar, so a slowdown fails the PR that makes it rather than being found
//! stages later.
//!
//! They're `#[ignore]`d, named `bench_…`, and run in release, on CI's
//! runner (the Mac) after the other tests:
//!
//! `cargo test --release -p <crate> -- --ignored bench_`
//!
//! A bar is the Mac's time when it was set, doubled: the runner shares
//! the Mac with whatever else runs on it. Halving the time a thing takes
//! is the time to lower its bar.
//!
//! A busy machine slows a run by more than that: with the lab's tests
//! running beside it, making a 100 × 100 grid took 0.41 ms against its
//! 0.17 (2026-10-27). So each try runs for at least [`MIN_TRY`], not a
//! few milliseconds a burst can fill, and one over its bar tries again,
//! up to [`TRIES`] times, keeping the best median: what else is running
//! comes and goes, and a slowdown in the code doesn't.

use std::time::{Duration, Instant};

/// How long a try runs at least, however few runs that takes.
pub const MIN_TRY: Duration = Duration::from_millis(200);

/// How many times a benchmark over its bar is tried.
pub const TRIES: usize = 3;

/// The median time `f` takes, in milliseconds, over at least `runs` runs
/// and at least `at_least` in all, after one that isn't counted (caches,
/// first allocations).
pub fn median_ms(runs: usize, at_least: Duration, mut f: impl FnMut()) -> f64 {
    assert!(runs > 0, "no runs");
    f();
    let start = Instant::now();
    let mut times = Vec::with_capacity(runs);
    while times.len() < runs || start.elapsed() < at_least {
        let run = Instant::now();
        f();
        times.push(run.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// Prints what `name` took against its bar, and fails if it's over it.
#[track_caller]
pub fn hold(name: &str, ms: f64, bar_ms: f64) {
    println!("{name}: {ms:.3} ms (bar {bar_ms} ms)");
    assert!(ms <= bar_ms, "{name} took {ms:.3} ms, over its bar of {bar_ms} ms");
}

/// Times `f` as [`median_ms`] does, for [`MIN_TRY`] at least, then holds
/// it to `bar_ms` as [`hold`]; over it, tries again, [`TRIES`] times in
/// all, and holds the best.
#[track_caller]
pub fn bench(name: &str, runs: usize, bar_ms: f64, mut f: impl FnMut()) {
    let mut best = f64::INFINITY;
    for _ in 0..TRIES {
        best = best.min(median_ms(runs, MIN_TRY, &mut f));
        if best <= bar_ms {
            break;
        }
        // Whatever else was running may be done.
        std::thread::sleep(Duration::from_millis(500));
    }
    hold(name, best, bar_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_is_the_middle_run_not_the_first() {
        let mut n = 0_u64;
        // The first run, not counted, sleeps; the rest don't.
        let ms = median_ms(5, Duration::ZERO, || {
            n += 1;
            if n == 1 {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        assert!(ms < 5.0, "{ms}");
    }

    #[test]
    fn a_try_runs_for_its_time_however_few_runs_it_asks_for() {
        let mut n = 0;
        median_ms(1, Duration::from_millis(20), || {
            n += 1;
            std::thread::sleep(Duration::from_millis(1));
        });
        assert!(n >= 10, "{n} runs");
    }

    #[test]
    fn a_slow_try_is_tried_again_and_the_best_held() {
        // The first two tries are slow, as on a busy machine; the third
        // isn't, so it passes.
        let mut n = 0;
        let start = Instant::now();
        bench("a thing", 3, 5.0, || {
            n += 1;
            if start.elapsed() < Duration::from_millis(700) {
                std::thread::sleep(Duration::from_millis(10));
            }
        });
    }

    #[test]
    #[should_panic(expected = "over its bar")]
    fn always_slow_fails() {
        bench("a thing", 3, 1.0, || std::thread::sleep(Duration::from_millis(2)));
    }

    #[test]
    #[should_panic(expected = "over its bar")]
    fn over_the_bar_fails() {
        hold("a thing", 2.0, 1.0);
    }

    #[test]
    fn under_the_bar_passes() {
        hold("a thing", 0.5, 1.0);
    }
}

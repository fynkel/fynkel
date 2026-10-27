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
//! the Mac with whatever else runs on it, and a median of a few hundred
//! runs moves by a third on a busy machine, not by half. Halving the time
//! a thing takes is the time to lower its bar.

use std::time::Instant;

/// The median time `f` takes, in milliseconds, over `runs` runs after one
/// that isn't counted (caches, first allocations).
pub fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    assert!(runs > 0, "no runs");
    f();
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// Prints what `name` took against its bar, and fails if it's over it.
#[track_caller]
pub fn hold(name: &str, ms: f64, bar_ms: f64) {
    println!("{name}: {ms:.3} ms (bar {bar_ms} ms)");
    assert!(ms <= bar_ms, "{name} took {ms:.3} ms, over its bar of {bar_ms} ms");
}

/// Times `f` as [`median_ms`] does, then holds it to `bar_ms` as [`hold`].
#[track_caller]
pub fn bench(name: &str, runs: usize, bar_ms: f64, f: impl FnMut()) {
    hold(name, median_ms(runs, f), bar_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_is_the_middle_run_not_the_first() {
        let mut n = 0_u64;
        // The first run, not counted, sleeps; the rest don't.
        let ms = median_ms(5, || {
            n += 1;
            if n == 1 {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        assert!(ms < 5.0, "{ms}");
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

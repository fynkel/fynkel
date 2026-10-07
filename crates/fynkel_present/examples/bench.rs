//! Times one frame's trip round the ring on one thread, so without
//! waiting for the lock (stage 1 step 3).
//!
//! `cargo run --release -p fynkel_present --example bench`
//!
//! Each line is the best of 5 runs, so it shows the cost of the code, not
//! of whatever else the machine was doing.

use std::hint::black_box;
use std::time::{Duration, Instant};

use fynkel_present::Ring;

const N: u32 = 1_000_000;
const RUNS: usize = 5;

fn main() {
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let ring = Ring::new();
        let t = Instant::now();
        for _ in 0..N {
            let ticket = black_box(ring.reserve().unwrap());
            ring.complete(ticket).unwrap();
            black_box(ring.acquire());
        }
        best = best.min(t.elapsed());
    }
    let ns = best.as_nanos() as f64 / f64::from(N);
    println!("one frame (reserve + complete + acquire): {ns:.1} ns, {:.1} ns per operation", ns / 3.0);
}

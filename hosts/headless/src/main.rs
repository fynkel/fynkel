//! `fynkel_host_headless [--steps N] [--seed S] [--satellites K] [--frame-dt SECS] [--json]`

use fynkel_host_headless::{run, HeadlessConfig};

/// The counting allocator (`fynkel_core::memory`, M3-16) behind the
/// report's memory numbers.
#[global_allocator]
static ALLOCATOR: fynkel_core::memory::CountingAllocator = fynkel_core::memory::CountingAllocator;

fn main() {
    let mut cfg = HeadlessConfig::default();
    let mut json = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .unwrap_or_else(|| panic!("{name} needs a value"))
        };
        match a.as_str() {
            "--steps" => cfg.steps = value("--steps").parse().expect("--steps"),
            "--seed" => cfg.seed = value("--seed").parse().expect("--seed"),
            "--satellites" => cfg.satellites = value("--satellites").parse().expect("--satellites"),
            "--frame-dt" => cfg.frame_dt = value("--frame-dt").parse().expect("--frame-dt"),
            "--json" => json = true,
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(1);
            }
        }
    }
    let report = run(&cfg);
    if json {
        println!("{}", report.to_json());
    } else {
        println!(
            "headless: {} ticks, {} entities, {} commands, {} events, checksum {}, step p50={}us p95={}us max={}us, total {:.1} ms",
            report.tick,
            report.live_entities,
            report.commands_applied,
            report.events,
            report.checksum,
            report.step_time.p50_us,
            report.step_time.p95_us,
            report.step_time.max_us,
            report.total_ms
        );
    }
    if !report.scene_ready || report.commands_rejected > 0 {
        std::process::exit(1);
    }
}

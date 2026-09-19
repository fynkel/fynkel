# M0 — standalone host baseline

Benchmark profile: `mac-m3max-2026` (see `docs/toolchain.md`).

| Field | Value |
| --- | --- |
| Date | 2026-09-19 |
| Commit | (this commit) |
| Host | `hosts/standalone` (winit 0.30.13 + wgpu 30.0.1, Metal) |
| Scene | one lit cube, depth test, `Fifo` present mode |
| Window | 1280×720 logical (2560×1440 physical, Retina) |
| Display | built-in ProMotion, 120 Hz |
| Build | `cargo build --release` (`debug = 1`) |
| Capture | `RUST_LOG=info ./target/release/fynkel_host_standalone --seconds 10` |
| Warm-up | none (first second included; one skipped frame at startup) |

## Result (10 s, 1208 frames)

| Metric | p50 | p95 | p99 |
| --- | --- | --- | --- |
| presented-frame interval (ms) | 8.33 | 8.67 | 8.85 |
| acquire wait — blocked on vsync (ms) | 8.21 | 8.52 | — |
| CPU encode + submit (ms) | 0.056 | 0.137 | 0.335 |

Interpretation: the host is vsync-locked at 120 Hz with ~0.06 ms of CPU work per frame. Almost the entire interval is spent waiting in `get_current_texture`. This is the ruler: the Flutter-hosted viewport (Path A) is compared against **CPU 0.14 ms p95** and a **presented interval of 8.67 ms p95** at this resolution.

Not yet measured: GPU pass time (needs timestamp queries), presentation latency (needs external capture). Both are M0 backlog items 6–7.

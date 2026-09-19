# M0-07 — latency, copies and composition overhead (Flutter macOS host, Path A)

Benchmark profile: `mac-m3max-2026`, 120 Hz built-in display, viewport
1600×1200 physical, ring of 4, 10 s runs, steady-state 1 s windows.
Build: `flutter build macos --release --dart-define=FYNKEL_M0_DYLIB=… [--dart-define=FYNKEL_M0_SYNC=0|1|2] [--dart-define=FYNKEL_M0_ACQUIRE_WAIT_US=N]`.

## 1. Completion signalling: three sync modes

| `SyncMode` | how "frame ready" is learned | `render()` CPU p95 (Dart isolate) | submit → ready signal p50 | present lag (frames) |
| --- | --- | --- | --- | --- |
| 0 async-poll (`wgpu::Queue::on_submitted_work_done`) | wgpu delivers the callback on the **next** poll/submit | 0.14 ms | 8.32 ms | 1 |
| 1 blocking (`device.poll(wait)`) | render() returns after the GPU is done | **0.96 ms (stall)** | 0.81 ms | 0 |
| 2 metal-completion (`MTLCommandBuffer.addCompletedHandler` on an empty fence buffer committed after the frame) | Metal's completion thread, immediately | 0.14 ms | **1.15 ms** | 1 (0 with consumer wait, §2) |

In isolation (Rust test `timing::completion_latency_in_isolation`, no
Flutter): metal-completion 0.79 ms, blocking 0.85 ms, async-poll 10.4 ms.
Real GPU time for the cube frame at this size is **~0.75 ms** (fence
timeline: kernelStart +0.05 ms, gpuStart/End +0.5…1.2 ms, handler +0.02 ms).

Conclusion: `on_submitted_work_done` is not usable for frame pacing; go
through Metal directly. Mode 2 is the default.

## 2. Presentation lag and the consumer-side wait

`present lag` = (newest submitted seq) − (seq the presenter acquired), per
`copyPixelBuffer`. Flutter's raster thread asks for the pixel buffer ~1 ms
after the tick — before the GPU has finished this tick's frame — so any
non-blocking mode shows the *previous* tick's frame (lag 1, +8.3 ms).

Fix: `acquire_presented` waits (condvar, bounded by `FYNKEL_M0_ACQUIRE_WAIT_US`,
default 3000) for the newest in-flight frame. The wait runs on Flutter's
raster thread, not on the Dart isolate.

| acquire wait | present lag p50/p95/max | raster-thread wait p50/p95/max | Dart tick interval p95 | drops |
| --- | --- | --- | --- | --- |
| 0 | 1 / 1 / 1 | 0 | 8.34 ms | 0 |
| 3000 µs | **0 / 0 / 0** | 0.64 / 0.67 / 0.74 ms | 8.34 ms | 0 |

End-to-end (tick → pixels on glass) is therefore: tick at vsync V →
submit +0.1 ms → GPU done +1.2 ms → composited into Flutter's frame V →
scanned out at V+1 (standard one-vsync present latency). Input → photon
was not measured with an external camera; nothing in the pipeline adds a
frame beyond the compositor's own.

## 3. Copies

Zero. Rust renders into the IOSurface; Swift's `copyPixelBuffer` returns a
retained `CVPixelBuffer` wrapping that same IOSurface; Impeller samples it.
Verified by construction (no blit/readback code path exists) and by the CPU
numbers: `render()` costs the same as the standalone host's encode+submit.

## 4. Composition overhead (Flutter's own frame timings, `SchedulerBinding.addTimingsCallback`)

Overlay = stats text, a Material button, a spinner; both cases animate at 120 Hz.

| configuration | UI build p50/p95 | raster p50/p95/p99 |
| --- | --- | --- |
| overlay only (`FYNKEL_M0_NO_VIEWPORT=true`) | 0.04 / 0.10 ms | 0.11 / 0.21 / 0.39 ms |
| overlay + `Texture` viewport, acquire wait 0 | 0.19 / 0.31 ms | 0.21 / 0.33 / 0.40 ms |
| overlay + `Texture` viewport, acquire wait 3 ms (default) | 0.13 / 0.25 ms | 0.85 / 0.91 / 1.30 ms |

- Added CPU cost of compositing the external texture: **≈0.1 ms p95** on the
  raster thread. The UI-build increase (~0.2 ms) is the Ticker → `render()`
  FFI call and stats bookkeeping, i.e. the engine, not composition.
- With the consumer-side wait the raster thread spends ~0.64 ms blocked on
  the GPU; that is the price of lag 0 and stays under 1 ms total.
- GPU-side Impeller cost of sampling the external texture was not isolated
  (needs Metal System Trace); at 120 Hz with ~0.75 ms of engine GPU work the
  frame has ample headroom, and no frame was dropped in any run.

Against ADR-0002's provisional **1.5 ms p95** composition budget: measured
≈0.1 ms (+0.64 ms optional wait). Budget met with margin.

## 5. Verdict for the M0 gate (Path A)

- No CPU readback or copy; same-device import asserted at startup.
- One-vsync presentation latency (lag 0) achievable without stalling Dart.
- Resize/DPI/lifecycle/dispose reliable (`lifecycle-harness.md`).
- Composition overhead ≈0.1 ms p95, far under budget.

No hard-kill criterion was hit. Path C is still prototyped for the
fallback column (M0 backlog #7), but Path A is the production path.

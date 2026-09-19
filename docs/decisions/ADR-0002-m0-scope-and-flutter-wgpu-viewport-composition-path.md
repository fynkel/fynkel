# ADR-0002: M0 scope and Flutter/wgpu viewport composition path

**Status:** Accepted (Path A and Path C measured 2026-09-19; Path A selected, Path C kept as fallback)  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

The single highest architectural risk is compositing Rust/wgpu output under Flutter without a CPU readback. The v0.3 blueprint also put a second graphics backend, a Wasm/WebGPU spike, an ECS wrapper, and the license decision into M0, which would have made M0 long for a solo developer and diluted the go/no-go question.

## Decision

M0 validates **macOS + Metal + Flutter Impeller only**. It builds a standalone winit/wgpu baseline, a Flutter Path A prototype (IOSurface-backed `CVPixelBuffer` → `CVMetalTextureCache` → `wgpu-hal` texture import), a Path C prototype, lifecycle/overlay/input harnesses, a C ABI round-trip with `ffigen`, and a benchmark manifest.

**Metal synchronization contract (must be demonstrated, not assumed).** `wgpu` only accepts a wrapped `wgpu-hal` texture that originates from the same device it created, with a correct descriptor. Therefore M0 must prototype and test:

- the imported texture and the wgpu device share the **same `MTLDevice`** (asserted at startup; failure is a hard kill for Path A);
- an IOSurface-backed `CVPixelBuffer` **ring of 2–3 buffers**;
- **GPU completion/fence ownership**: a buffer is handed to Flutter only after the wgpu command buffer rendering into it has completed; Flutter never samples a buffer wgpu is still writing;
- Flutter raster-thread lifetime: texture callbacks may run on Flutter's raster thread; no wgpu device access from there beyond handing over a finished buffer;
- resize/DPI change reallocates the ring without tearing, stale frames, or use-after-free.

**Path C is the only production fallback.** Path D (separate player window) is prototyped only as the Editor Play mechanism; it cannot satisfy the game-host gate because it does not demonstrate Flutter HUD composited over the Fynkel 3D world.

The 1.5 ms p95 composition budget is provisional. A readback-free Path A that lands modestly over it (e.g. ~2.5 ms) is accepted and the measured number recorded here. Hard kills are: full-frame readback, more than one frame of unexplained latency, unreliable resize/DPI/lifecycle, overlay/input failures, or resource-ownership conflicts.

Removed from M0: any second graphics backend, the Wasm/WebGPU spike, `fynkel_ecs`/`hecs`, and licensing.

## Alternatives considered

- Full Windows/D3D12 viewport in M0: rejected; doubles interop work before the first question is answered.
- Accepting Path D as the game-host gate: rejected; it would leave the core Fynkel promise (Flutter UI over the 3D world) unproven.
- Wasm spike: rejected; `wgpu` already targets WebGPU and browser is post-1.0.

## Consequences

M0 is answerable in weeks, not months. Windows risk is deferred to post-1.0 with a periodic `cargo check` as the only guard. Skia is not verified. Measured numbers to be appended:

| Metric (mac-m3max-2026, 120 Hz, ~2400×1480 physical viewport) | Standalone | Path A | Fallback (Path C) |
| --- | --- | --- | --- |
| presented interval p95 | 8.67 ms | 8.34 ms (vsync-locked, 0 drops) | 8.67 ms (own swapchain, vsync-locked, 0 skips) |
| engine CPU encode+submit p95 | 0.14 ms | 0.14 ms | 0.22 ms (engine thread competes with Flutter's raster thread) |
| added composition overhead p95 (Flutter raster thread) | — | **~0.1 ms** CPU (raster 0.21 → 0.33 ms) + optional 0.64 ms bounded wait for lag 0 | **~0.06 ms** CPU (raster 0.23 → 0.29 ms); window-server blend of a second layer not measurable in-process |
| Flutter frame total p95 (UI build + raster) | — | 0.34 + 0.95 ms | 0.19 + 0.29 ms |
| copies per frame | 0 | **0** | **0** |
| presentation latency (frames behind the tick's frame) | 0 | **0** with consumer-side wait; 1 without | 0 for the world (standalone-equivalent); HUD and world are **not frame-synchronized** (≤ 1 frame apart) |
| resize cost | — | ~1 ms, no drops | 0.1–1.1 ms surface reconfigure; engine rect follows Flutter layout one frame late |

Both paths are readback-free and inside the composition budget. **Path A is selected**: the world is an ordinary Flutter layer (same-frame composition with the HUD, clipping/effects/scrolling, several viewports for the editor) at ≈0.7 ms more raster-thread time per frame, most of it the optional bounded wait that buys lag 0. Path C stays the production fallback: it is cheaper on Flutter's threads and decouples the engine loop from Flutter entirely, but the world is an opaque rectangle outside Flutter's layer tree, HUD and world can land one frame apart, and the host must keep AppKit's responder chain intact (see `benchmarks/m0/path-c.md`).

Sources: `benchmarks/m0/standalone-baseline.md`, `benchmarks/m0/flutter-path-a.md`,
`benchmarks/m0/lifecycle-harness.md`, `benchmarks/m0/latency-and-sync.md`, `benchmarks/m0/path-c.md`,
`benchmarks/m0/results/2026-09-19-f958917.md` (captured by `benchmarks/m0/capture.py`).

**Measured synchronization design (supersedes the sketch above).** The ring
is 4 IOSurface-backed `MTLTexture`s created via `newTextureWithDescriptor:iosurface:plane:`
on wgpu's own `MTLDevice` and imported with `wgpu-hal` `texture_from_raw`
(no `CVMetalTextureCache` needed on the Rust side; Swift wraps the same
IOSurfaces as `CVPixelBuffer`s for `FlutterTexture`). Completion ownership:
after `queue.submit`, an empty `MTLCommandBuffer` is committed on the same
`MTLCommandQueue` with `addCompletedHandler`; Metal fires it ~1 ms after
submit. `wgpu::Queue::on_submitted_work_done` is **not** usable for this: it
is only delivered on the next poll/submit, i.e. one frame late. The presenter
(`copyPixelBuffer`, raster thread) may wait a bounded ~3 ms for the newest
in-flight frame; this yields presentation lag 0 at a measured 0.64 ms wait
and zero cost on the Dart isolate. No hard kill was hit: no readback, no
unexplained latency, resize/DPI/lifecycle reliable, overlays composite.

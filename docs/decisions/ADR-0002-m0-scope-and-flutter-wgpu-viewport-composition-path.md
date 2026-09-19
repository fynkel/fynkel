# ADR-0002: M0 scope and Flutter/wgpu viewport composition path

**Status:** Accepted (numbers pending M0 measurement)  
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

| Metric | Standalone | Path A | Fallback |
| --- | --- | --- | --- |
| frame p95 @1080p | _tbd_ | _tbd_ | _tbd_ |
| added composition overhead p95 | — | _tbd_ | _tbd_ |
| copies per frame | 0 | _tbd_ | _tbd_ |
| presentation latency (frames) | _tbd_ | _tbd_ | _tbd_ |

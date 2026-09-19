# M0 architecture report and go/no-go

**Date:** 2026-09-19 · **Milestone:** M0 (blueprint §19, ADR-0002) · **Verdict: GO — proceed to M1 with Path A.**

M0 asked one question: can Rust/wgpu output be composited under Flutter on
macOS with no CPU readback, reliably, at a cost small enough to build the
engine on. The answer is yes, measured, with Path A selected and Path C proven
as the fallback. Everything below is reproducible with
`python3 benchmarks/m0/capture.py` on profile `mac-m3max-2026`.

## 1. Gate criteria (blueprint §19 "M0 — Gate")

| Gate | Result | Evidence |
| --- | --- | --- |
| A readback-free Path A **or** Path C composites Flutter UI over the 3D world with controllable input | **Pass — both.** Path A: wgpu → IOSurface ring → `Texture` widget, 0 copies; Path C: engine `CAMetalLayer` under a transparent `FlutterView`, 0 copies. HUD widgets (text, `TextField`, button, spinner) over the world; pointer/keyboard focus behaves like ordinary Flutter in both. | `benchmarks/m0/flutter-path-a.md`, `overlay-input-focus.md`, `path-c.md` |
| Metal synchronization contract demonstrated, not assumed | **Pass.** Same `MTLDevice` asserted at creation (hard-kill check). Ring of 4 IOSurface-backed `MTLTexture`s with explicit slot states (Free/Rendering/Ready/Presented/Retiring) and per-slot sequence numbers. Frame handed to Flutter only after GPU completion (`MTLCommandBuffer.addCompletedHandler`, ~1.15 ms after submit). Raster thread only calls `acquire_presented` (no device access). Resize rebuilds the ring under a generation counter so stale completions cannot promote new slots; teardown order proven (clear callback → detach → unregister → destroy). Rust tests cover the state machine, the resize race and the lag-0 wait. | `crates/fynkel_m0/src/lib.rs` tests; `latency-and-sync.md`, `lifecycle-harness.md` |
| Composition overhead recorded; ≤ 1.5 ms p95 target | **Pass, inside budget.** Path A adds ≈ 0.1 ms p95 CPU on Flutter's raster thread (0.21 → 0.33 ms) plus an optional bounded wait of ≈ 0.65 ms that buys presentation lag 0; total raster p95 0.95 ms. Path C adds ≈ 0.06 ms. Dart isolate: one FFI call of 0.13 ms p95 per frame. Engine CPU is unchanged from the standalone host (0.14 ms p95). | ADR-0002 table; `benchmarks/m0/results/2026-09-19-f958917.md` |
| Resize/DPI/lifecycle reliable under repeat testing | **Pass** (DPI by construction only). Live resize bursts: ~1 ms each, 0 drops, 120 Hz held. Minimise/hide → rendering stops; restore → 120 Hz within a second. Destroy/recreate: clean, no late callbacks, repeated. A second display / DPI change was **not exercised live** (single built-in display); the code path is the same as a resize. | `lifecycle-harness.md`, `path-c.md` |
| C ABI round trip works from Dart with `ffigen` bindings | **Pass.** `fynkel.h` v0 (16-byte header, fixed LE records, two lanes, status codes, diagnostics with messages, poisoning on panic) → `ffigen` → Dart `NativeWorld`. Spawn/set/destroy round trip, protocol errors apply nothing, 1000 entities/tick: 0.15 ms Dart encode+submit (JIT), 25 µs Rust step, 10 µs Dart read. | `bridge-round-trip.md`, `packages/fynkel/test` |

No hard-kill criterion was hit: no readback, no unexplained latency (the one
observed frame of lag was explained and removed), no lifecycle failure, no
overlay/input failure, no device mismatch, no resource-ownership conflict.

## 2. Measured numbers (120 Hz, ~2400×1480 physical viewport, 10 s after 2 s warm-up)

| Metric | Standalone | Path A (selected) | Path C (fallback) | Flutter, no viewport |
| --- | --- | --- | --- | --- |
| frames or ticks per second | 122 | 120 | 120 | 121 |
| presented interval p50 / p95 | 8.33 / 8.69 ms | 8.33 / 8.34 ms (Dart tick) | 8.32 / 8.67 ms | — |
| engine CPU encode+submit p95 | 0.17 ms | 0.13 ms | 0.22 ms | — |
| submit → GPU complete p50 / p95 | — | 0.78 / 0.83 ms | — | — |
| Flutter UI build p50 / p95 | — | 0.26 / 0.34 ms | 0.16 / 0.19 ms | 0.10 / 0.14 ms |
| Flutter raster p50 / p95 | — | 0.84 / 0.95 ms | 0.24 / 0.29 ms | 0.17 / 0.23 ms |
| copies per frame | 0 | 0 | 0 | — |
| presentation lag (frames) | 0 | 0 | 0 (own swapchain; HUD ≤ 1 frame apart) | — |
| drops / stale presents in sample | 0 | 0 / 0 | 0 skipped | — |
| resize cost | — | ~1 ms, no drops | 0.1–1.1 ms | — |

Bridge: 1000 entities/tick = 62.5 KiB in, 46.9 KiB out; 0.15 ms + 25 µs + 10 µs.
Numbers are the median of per-second p50/p95; single-second p99 outliers of
one missed vsync (16.6 ms) occurred in some runs on both hosts and are
consistent with a busy desktop, not with the composition path.

## 3. What was learned (and would have been guessed wrong)

1. **Completion signalling is the whole problem.** `wgpu::Queue::on_submitted_work_done`
   fires on the *next* poll/submit, i.e. one frame late — unusable for pacing
   presentation. A blocking wait costs the Dart isolate ~0.9 ms. Committing an
   empty `MTLCommandBuffer` with `addCompletedHandler` on wgpu's own queue
   (`Queue::as_hal`) signals ~1.15 ms after submit at zero Dart cost. This is
   `SyncMode::MetalCompletion`, the default.
2. **Lag 0 needs the consumer to wait, briefly.** Flutter's raster thread asks
   for the pixel buffer ~1 ms after the Dart tick, before the GPU has finished
   that tick's frame; without a wait it composites the previous frame (lag 1).
   A bounded wait (default 3 ms, measured 0.65 ms) on the raster thread — never
   on Dart — yields lag 0 every frame. It is a tunable; a host that prefers
   0.65 ms of raster time over one frame of latency turns it off.
3. **Impeller composites a `Texture` for ≈ 0.1 ms.** The feared cost was not
   there. The GPU-side cost of Impeller sampling the IOSurface is not isolated
   (needs Instruments); the frame budget shows no sign of it at 120 Hz.
4. **Teardown order is a correctness rule, not hygiene.** Rust signals frame
   readiness into Swift from Metal's completion thread; destroying the context
   before Swift detaches is a use-after-free. Order: stop loop → clear callback
   (synchronous, waits for in-flight) → detach + unregister → destroy.
5. **Path C is easier at the platform layer and harder at the product layer.**
   A wgpu surface on a `CAMetalLayer` needs nothing unsafe beyond the pointer,
   and the engine loop is fully decoupled from Flutter. But the world is then an
   opaque rectangle outside Flutter's layer tree (no clipping, effects,
   scrolling, multiple viewports without host help), the HUD and world can land
   one frame apart, and AppKit silently drops `mouseDown` if Flutter is hosted as
   a child view controller (keep `FlutterViewController` as
   `contentViewController`; put the engine view in its wrapper).
6. **Tooling that mattered:** Flutter's bundled Dart (Homebrew's is too old
   for `ffigen` 20), `cliclick` for real input events (System Events clicks do
   not reach Flutter), and per-second `[m0]` stat lines as the capture contract
   between hosts and `capture.py`.

## 4. Decision

- **Path A is the viewport path** (ADR-0002 status updated). Reason: the game
  host and the editor both want the world to be an ordinary Flutter layer —
  same-frame composition with HUD, clipping, effects, docked multiple
  viewports. The price (≈ 0.1 ms raster CPU + optional 0.65 ms wait) is inside
  budget.
- **Path C is retained as the production fallback and as the shape of a
  "Flutter HUD over a standalone game host"** where per-frame coupling is
  unnecessary. It is kept buildable (`FYNKEL_M0_MODE=c`) and in the capture.
- **Path D** was not needed for the gate and was not built; it remains the
  Editor Play mechanism per ADR-0016.

## 5. Carried into M1

- Real bridge: `fynkel_native` grows from the v0 toy world to `fynkel_core`
  + `fynkel_ecs` (ADR-0003, ADR-0007); codecs generated from Rust schemas
  (ADR-0012) replace the hand-written v0 records; request IDs, query and
  resource ABI families (ADR-0004).
- Native-assets `hook/build.dart` so `package:fynkel` loads its dylib without
  `FYNKEL_NATIVE_LIB` / `--dart-define` (ADR-0015).
- Move the Path A viewport out of the throwaway `fynkel_m0` crate into
  `platform/macos` with the synchronization contract as documented invariants
  and the M0 tests as regression tests; keep the standalone host as the ruler.
- Exercise DPI change on a second display; isolate Impeller's GPU-side cost with
  Instruments; run the capture on a second machine before trusting absolute
  numbers.
- Windows: `cargo check` only (ADR-0009); no viewport work.

## 6. Backlog closure (blueprint §19.2)

| # | Item | Commit |
| --- | --- | --- |
| 1 | Toolchain matrix frozen (`docs/toolchain.md`) | `1d897ac` |
| 2 | Reference-game spec frozen, Tier 1/2 split | ADR-0010 (`c1cb290`) |
| 3–4 | Standalone host + baseline | `1d897ac` |
| 5 | Flutter Path A viewport | `9dddb5c` |
| 6 | Copies, latency, composition overhead | `aba5c81` |
| 7 | Path C prototype | `d2a78cb` |
| 8 | Resize/DPI/lifecycle/dispose harness | `0e5d17c` |
| 9 | Overlay + pointer/keyboard focus | `3b3b689` |
| 10–11 | C ABI v0 + ffigen + batched round trip | `edbbef2` |
| 12 | Benchmark manifest + capture script | `f958917` |
| 13 | This report; ADR-0002 numbers | this commit |

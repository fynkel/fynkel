# Hosts sharing the core (M1-07, iOS smoke M1-09)

The engine is a library and the host owns the loop (ADR-0007). Three hosts
drive the same `fynkel_core` with the same per-frame sequence (ADR-0008):

```text
begin_frame(dt) -> plan
for each planned fixed tick:
    pull events            (drain_events)
    gameplay               (stage commands on the simulation lane)
    step()                 (apply staged commands, propagate, snapshot, publish events)
presentation writes        (presentation lane; override this frame's extract)
extract()                  (S[n-1]..S[n] interpolated by alpha, overrides applied)
render                     (host-specific target)
```

| Host | Crate / app | Loop source | Gameplay | Render target |
| --- | --- | --- | --- | --- |
| Rust headless | `hosts/headless` (`fynkel_host_headless`, lib + bin) | test loop, one fixed step per frame by default | Rust `DemoScene` (commands) | none; reports JSON |
| Standalone | `hosts/standalone` (`fynkel_host_standalone`) | winit `RedrawRequested`, vsync (`Fifo`) | Rust `DemoScene` | winit swapchain via `fynkel_render` |
| Flutter macOS + iOS | `hosts/flutter_apple` (`macos/`, `ios/`) | Flutter `Ticker` on the main isolate | Dart `DemoScene` on `package:fynkel` | `platform/apple` IOSurface ring → Flutter `Texture` |

All three render (or checksum) the same scene: a tumbling hub cube with
`satellites` child cubes on their own rings, driven by one `SetTransform`
batch per fixed tick — the simplest thing that exercises hierarchy
propagation, staged commands and alpha interpolation.

## `fynkel_render`

`ExtractRenderer` (crate `fynkel_render`) draws an `Extract` as instanced
lit unit cubes with one perspective camera and one directional light. It
is host-agnostic: it takes a `wgpu::Device`, a target format and a
`TextureView`. `render_offscreen` is the readback helper for tests. This is
the seed of the Renderer V1 (blueprint §8.5), not its shape.

## Headless host and `fynkel test`

```sh
cargo run -p fynkel_cli -- test --headless --steps 600 --seed 42 --satellites 24 --json
cargo run -p fynkel_host_headless -- --steps 600 --json
```

The report carries tick count, live entities, commands applied/rejected,
events, diagnostics, missed deadlines, step-time percentiles and a
**checksum**: FNV-1a 64 over the final extract's world matrices. Equal
configs must give equal checksums (blueprint §5.6); the crate's tests assert
this and that seeds differ. The Dart headless game host (Behaviours) plugs
into the same command later.

`--scene <path>` loads a `.scene.json` instead of the demo scene (validated
against the embedded schemas, then `World::load_scene`; the world just ticks
`--steps` times). The report gains `scene: { path, loaded, id, name,
entities, rejected_components, document_diagnostics[] }`. Read-side:

```sh
cargo run -p fynkel_cli -- state dump --scene assets/scenes/main.scene.json --steps 60 --json
```

prints the runtime state keyed by **authoring id** (never handles): each
entity's `id`, `name`, `parent`, `children`, `local`/`world` transform and
typed `components[]` in the document's own JSON shapes; exit 1 when the
document does not load or a component value was rejected. This is the M2
gate "project can be validated/imported headlessly".

## Flutter host

`hosts/flutter_apple` uses **one** dylib, `libfynkel_platform_apple.dylib`,
which exports both the engine ABI (`fynkel_*`, used by `package:fynkel`) and
the viewport ABI (`fynkel_apple_*`, `platform/apple/include/fynkel_apple.h`).
It arrives as the code asset `package:fynkel/fynkel_native` from
`packages/fynkel/hook/build.dart` (ADR-0015): explicit `native_lib`
user-define → source checkout `cargo build` → checksummed prebuilt named by
`fynkel.lock.json` → `cargo build` fallback when Rust is installed → an
error that names the fix. All bindings are `@Native` on that asset; nothing
calls `DynamicLibrary.open`. The Flutter toolchain bundles it as a
universal framework, so the hook builds once per architecture.
Each Flutter frame: `World.frame(dt, fixedUpdate: …)` (gameplay + `step()`
per planned tick, presentation flush), then
`fynkel_apple_viewport_render(viewport, world)` which extracts the world in
Rust and renders into a free ring slot. Swift's `FynkelViewportTexture`
hands the newest GPU-complete slot to Flutter's `Texture`.

Measured on mac-m3max-2026 at 1600×1104 physical, 24 satellites: 120 Hz
frames, 60 fixed ticks/s, Dart simulate ≈ 0.02 ms, `render()` call ≈ 0.1 ms,
present lag 0, no dropped frames.

## iOS smoke test (M1-09)

The same host, Dart code, Swift texture glue (`FynkelViewportTexture.swift`
is byte-identical in `macos/Runner` and `ios/Runner`, `#if os(iOS)` for the
imports and registrar accessors) and Rust crate run on iOS: `platform/apple`
has no AppKit/UIKit dependency and the hook cross-compiles it for
`aarch64-apple-ios`, `aarch64-apple-ios-sim` and `x86_64-apple-ios`
(Flutter builds two simulator slices). Two things surfaced:

- the iOS simulator's Metal device sits below WebGPU's default limits (15
  inter-stage variables), so every `request_device` now asks for
  `fynkel_render::device_limits(adapter)` — the defaults lowered to what
  the adapter offers;
- a `MissedDeadline` diagnostic after a startup stall threw `FynkelError` in
  debug mode and stopped the host. Missed deadlines are informational
  (`FynkelDiagnostic.isInformational`): reported through `onDiagnostics`,
  never thrown.

Measured (profile mode, 1320×2508, 24 satellites):

| | iPhone 16 Pro Max (A18 Pro, iOS 26.6.1) | iPhone 16 Pro Max simulator (debug) |
| --- | --- | --- |
| Flutter frames | 120 Hz (ProMotion), tick interval p50 8.33 ms | 60 Hz |
| fixed ticks/s, missed deadlines | 60, 0 | 60, 0 |
| Dart simulate (gameplay+submit+step+snapshot) | p50 0.06 ms, p99 0.12 ms | p50 0.09 ms |
| `render()` call (extract+encode+submit) | p50 0.41 ms, p99 0.56 ms | p50 0.62 ms |
| submit → GPU complete | p50 1.84 ms, p99 2.51 ms | p50 1.31 ms |
| raster acquire wait | p50 0.90 ms, max 1.89 ms | p50 0.00 ms |
| present lag / dropped frames / stale presents | 0 / 0 / 0 | 0 / 0 / 0 |
| Flutter raster | p50 1.73 ms, p99 2.66 ms | p50 0.99 ms |

The cube renders on the device through the same interop; M1-09 gate item
met. Full iOS qualification (touch overlay, lifecycle, thermal, packaging)
is M6.

Path C (engine-owned `CAMetalLayer`) was an M0 prototype; it is not carried
into `platform/apple`. Its numbers remain in `benchmarks/m0/results`, its
code at the M0 commits (ADR-0002 keeps it as the documented fallback).

## Per-frame cost instrumentation (M1-10)

Three probes feed the M1 gate (`docs/m1-gate-report.md`) and are run by
`python3 benchmarks/m0/capture.py` alongside the host benchmarks:

- `dart run tool/alloc_bench.dart [--frames N] [--satellites K] [--json]` in
  `packages/fynkel`: the Flutter demo scene without a renderer, driven from
  Dart; reports heap bytes per frame (heap-usage deltas after forced GC, chunks
  with a GC event excluded), GC count and pause time from the VM service.
  JIT numbers; the class breakdown is indicative only. With the C1 writers
  (`TransformWriter`, `GameObject.writeLocal`) the 25-entity scene sits at
  ≈1.5 KB/frame with no GC in 3 000 frames; the remainder is per-frame SDK
  results (`FramePlan`/`StepResult`) and event/diagnostic views.
- `cargo test --release -p fynkel_native --test leak`: counting global
  allocator over repeated runtime + world lifetimes; bytes in use after
  teardown must equal the baseline.
- `cargo test --release -p fynkel_platform_apple --test lifetime`: repeated
  viewport create/render/resize/destroy; process RSS growth bounded.

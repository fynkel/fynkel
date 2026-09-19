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

Measured on the iPhone 16 Pro Max simulator (debug, 1320×2508): 60 Hz
frames, 60 fixed ticks/s, 25 entities, Dart simulate ≈ 0.09 ms, `render()`
≈ 0.6 ms, no dropped frames, no missed deadlines. Device numbers go in
`benchmarks/m1` once the phone run is captured (`flutter run -d <iphone>
--release`; the phone must be unlocked and trust the development
certificate). Full iOS qualification (touch overlay, lifecycle, thermal,
packaging) is M6.

Path C (engine-owned `CAMetalLayer`) was an M0 prototype; it is not carried
into `platform/apple`. Its numbers remain in `benchmarks/m0/results`, its
code at the M0 commits (ADR-0002 keeps it as the documented fallback).

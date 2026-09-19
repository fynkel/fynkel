# Hosts sharing the core (M1-07)

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
| Flutter macOS | `hosts/flutter_macos` | Flutter `Ticker` on the main isolate | Dart `DemoScene` on `package:fynkel` | `platform/macos` IOSurface ring → Flutter `Texture` |

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

`hosts/flutter_macos` uses **one** dylib, `libfynkel_platform_macos.dylib`,
which exports both the engine ABI (`fynkel_*`, used by `package:fynkel`) and
the viewport ABI (`fynkel_macos_*`, `platform/macos/include/fynkel_macos.h`).
It arrives as the code asset `package:fynkel/fynkel_native` from
`packages/fynkel/hook/build.dart` (ADR-0015): explicit `native_lib`
user-define → source checkout `cargo build` → checksummed prebuilt named by
`fynkel.lock.json` → `cargo build` fallback when Rust is installed → an
error that names the fix. All bindings are `@Native` on that asset; nothing
calls `DynamicLibrary.open`. The Flutter toolchain bundles it as a
universal framework, so the hook builds once per architecture.
Each Flutter frame: `World.frame(dt, fixedUpdate: …)` (gameplay + `step()`
per planned tick, presentation flush), then
`fynkel_macos_viewport_render(viewport, world)` which extracts the world in
Rust and renders into a free ring slot. Swift's `FynkelViewportTexture`
hands the newest GPU-complete slot to Flutter's `Texture`.

Measured on mac-m3max-2026 at 1600×1104 physical, 24 satellites: 120 Hz
frames, 60 fixed ticks/s, Dart simulate ≈ 0.02 ms, `render()` call ≈ 0.1 ms,
present lag 0, no dropped frames.

Path C (engine-owned `CAMetalLayer`) was an M0 prototype; it is not carried
into `platform/macos`. Its numbers remain in `benchmarks/m0/results`, its
code at the M0 commits (ADR-0002 keeps it as the documented fallback).

# M0 — Path C prototype: engine-owned surface, Flutter as native overlay layer

Backlog item #7 (blueprint §8.2, §19.2; ADR-0002 fallback column). Measured
2026-09-19 on profile `mac-m3max-2026`, commit after `f958917`.

## What was built

- `crates/fynkel_m0/src/surface.rs`: `fynkel_m0_surface_create(layer, w, h)`
  takes a `CAMetalLayer` (via `wgpu::SurfaceTargetUnsafe::CoreAnimationLayer`),
  builds its own wgpu device/queue/surface (`PresentMode::Fifo`) and spawns an
  engine render thread that acquires → renders the M0 cube → presents, paced by
  the layer's vsync exactly like `hosts/standalone`. `resize`, `set_paused`,
  `take_stats`, `destroy` (joins the thread; the surface is released on the
  caller's thread).
- `hosts/flutter_macos/macos/Runner/FynkelPathCHost.swift`: when
  `FYNKEL_M0_MODE=c`, creates a layer-backed `NSView` whose root layer is the
  `CAMetalLayer`, inserts it **below `FlutterView` inside the
  `FlutterViewController`'s wrapper view** (where Flutter itself places
  platform views), sets `FlutterViewController.backgroundColor = .clear`, and
  reports frame/scale changes and miniaturisation over `fynkel/m0/pathc`.
  Dart may also push the viewport widget's rect (`setFrame`) so the engine
  view follows Flutter layout.
- `hosts/flutter_macos/lib/main.dart`: `PathCPage` — transparent scaffold,
  the same HUD/input harness as Path A (stats box, text field, button,
  spinner, `Focus`+`Listener` over the world), stats every second; lifecycle
  `hidden`/`paused` pauses the engine thread.

Composition is done by the window server: two `CAMetalLayer`s in one window,
Flutter's on top with alpha. Flutter never sees an engine frame and the engine
never sees a Flutter frame. No IOSurface hand-over, no `Texture` widget, no
copies.

## Measured (10 s sample after 2 s warm-up, 120 Hz, viewport 2400×1480)

| | Standalone | Path A | **Path C** | Flutter, no viewport |
| --- | --- | --- | --- | --- |
| frames / ticks per second | 122 | 120 | **120** | 121 |
| engine presented interval p50 / p95 / p99 | 8.33 / 8.69 / 8.76 ms | 8.33 / 8.34 / 8.34 ms (Dart tick) | **8.32 / 8.67 / 16.6 ms** (one skipped vsync in 10 s) | — |
| engine CPU encode+submit p95 | 0.17 ms | 0.13 ms | **0.22 ms** | — |
| Flutter UI build p50 / p95 | — | 0.26 / 0.34 ms | **0.16 / 0.19 ms** | 0.10 / 0.14 ms |
| Flutter raster p50 / p95 | — | 0.84 / 0.95 ms (incl. 0.6 ms bounded wait for lag 0) | **0.24 / 0.29 ms** | 0.17 / 0.23 ms |
| copies per frame | 0 | 0 | **0** | — |
| drops / skips in sample | — | 0 / 0 | **0** | — |

Added cost on Flutter's raster thread with Path C ≈ **0.06 ms p95** (the
transparent surface has to be blended, but that happens in the window server,
outside the process; not measured here — Instruments needed). The engine thread's
own cost equals the standalone host; its p95 is slightly higher because it now
shares cores with Flutter's UI and raster threads.

## Harness results (same script as the Path A harness, `cliclick` + AppleScript)

- **Overlay + input**: HUD button, text field and the world's `Focus`/`Listener`
  behave exactly as in Path A — the Flutter view is on top and receives *all*
  input first; the engine receives what Dart forwards.
- **Resize**: live window resize → Swift `frameDidChange` → Dart →
  `fynkel_m0_surface_resize` → the render thread reconfigures in 0.1–1.1 ms.
  Shrink/grow of the viewport widget (host bar, inset) → Dart reports the widget
  rect after layout → the engine view moves/resizes **one Flutter frame later**
  (visible as a one-frame edge mismatch during live resize; Path A has none
  because the `Texture` is laid out by Flutter).
- **Lifecycle**: minimise → `AppLifecycleState.hidden` → engine paused (0 frames);
  restore → resumed at 120 Hz. While the window is occluded the swapchain returns
  `Occluded` and frames are skipped without rendering.
- **Dispose / recreate**: destroy joins the render thread and releases the
  surface; recreate attaches to the same layer; input works after recreation.
- **Metrics/DPI**: `didChangeMetrics` re-polls the host size; not exercised on a
  second display (same gap as Path A).

## Gotchas found

1. **Responder chain.** Hosting Flutter as a *child* view controller under a
   custom container view silently dropped every `mouseDown`: the chain printed
   correctly (`FlutterView → FlutterViewWrapper → FlutterViewController → …`) but
   AppKit still forwarded the wrapper's `mouseDown` to the superview, so the
   embedder's `dispatchMouseEvent` never ran (keys and hover use other routes and
   kept working). Keeping `FlutterViewController` as the window's
   `contentViewController` and inserting the engine view into its wrapper below
   `FlutterView` fixes it and is the placement Flutter uses for platform views.
2. **Initial size race.** The window's autosaved frame can be applied around
   the time Dart attaches; the first size read can be stale. Dart re-polls the
   host size on `didChangeMetrics` and every stats tick.
3. **Occluded drawables.** `get_current_texture` blocks up to 1 s on a hidden
   window; pausing on `hidden` before that happens avoids a stray ~1 s frame.

## Trade-offs versus Path A

| | Path A (Texture) | Path C (engine layer) |
| --- | --- | --- |
| World in Flutter's layer tree (clip, opacity, transforms, shaders, scrolling, RepaintBoundary) | yes | **no** — an opaque rectangle; only rect placement, one frame late |
| HUD and world composited in the same frame | yes | **no** — two independently presented layers, ≤ 1 frame apart; world-anchored HUD elements can jitter by a frame |
| Several viewports (editor: scene, game, material preview) | ordinary widgets | one `NSView` each; no clipping inside scroll views without host help |
| Engine loop coupling | Dart ticks `render()` once per Flutter frame; present bound to Flutter's frame | fully decoupled engine thread; can present uncapped (`Immediate`) |
| Flutter raster-thread cost p95 | +0.1 ms CPU, +0.6 ms optional wait | +0.06 ms |
| Dart isolate cost | one FFI call per frame (~0.05 ms) | none |
| Presentation latency | 0 frames with bounded wait | standalone-equivalent (own swapchain) |
| Platform surface area | `wgpu-hal` texture import + IOSurface/CVPixelBuffer + completion handlers | plain `wgpu` surface on a `CAMetalLayer`; nothing unsafe beyond the layer pointer |
| Portability | per-platform interop subsystem (D3D12 shared handles, Vulkan external memory) | per-platform *view embedding* only; wgpu surfaces already exist everywhere |

## Conclusion

Path C works, is cheap, and is simpler than Path A at the platform layer; it is
the right **fallback** and also the natural shape for a *standalone game host
with a Flutter HUD* where the HUD does not need per-frame world coupling.
Path A remains the **selected** viewport path (ADR-0002) because the editor and
the game host both want the world to be an ordinary Flutter layer: same-frame
composition with HUD, clipping, effects, multiple viewports. The measured price
(~0.1 ms raster CPU plus an optional 0.6 ms bounded wait for lag 0) is inside
the budget.

Not measured: the window server's GPU cost for blending the Flutter layer over
the engine layer (needs Instruments' Metal System Trace); a second display /
DPI change.

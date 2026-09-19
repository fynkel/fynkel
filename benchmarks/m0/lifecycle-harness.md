# M0-06 — resize / DPI / lifecycle / dispose harness (Flutter macOS host, Path A)

Benchmark profile: `mac-m3max-2026`. Same build as `flutter-path-a.md`.
Driven with `osascript` (System Events) plus manual clicks on the harness
buttons; results read from the `[m0]` stdout lines.

## What is exercised

| Case | Mechanism | Result |
| --- | --- | --- |
| Window live resize (burst of 4 sizes, 50 ms apart) | `LayoutBuilder` → `fynkel_m0_resize` (ring rebuilt, GPU drained) → Swift `rebindTexture` | 4 resizes applied, coalesced; **~1.0 ms each** end to end (Rust rebuild ≈ 0.9 ms incl. GPU drain + 4 IOSurfaces + textures + depth); 0 dropped frames, 0 frames skipped while resizing, 120 Hz held |
| Viewport resize without window resize (padding toggle) | same path | same; 1920×1000 ↔ 2400×1480 in ≤1 ms |
| Minimise (`AXMinimized`) | `AppLifecycleState.hidden` → `Ticker.muted = true` | render stops (n=0/s), no callbacks; restore → `inactive` → unmuted, 120 Hz within one second; one 2.1 s tick gap (the muted span) |
| Hide app (⌘H equivalent) | same as minimise | same |
| Focus loss (`inactive`) | none (deliberate) | keeps rendering |
| Destroy viewport (button) | ordered `teardown()`: stop ticker → clear Rust callback (sync, waits for in-flight callback) → Swift `detach` + `unregisterTexture` → `fynkel_m0_destroy` | "destroyed" logged; no crash, no late callback |
| Recreate viewport (button) | new context + new texture id | generation 1 up at 120 Hz, resize/lifecycle still work |
| DPI change | `didChangeMetrics` → same resize path (physical size = logical × dpr) | **not exercised live** (single built-in display); handled by construction — a dpr change is just a physical-size change to this code |

## Correctness details that mattered

- Completion callbacks are keyed by **(ring generation, slot, seq)**; a
  resize bumps the generation so a completion from an old ring can never mark
  a new slot Ready. Covered by the Rust test
  `resize_rebuilds_ring_and_ignores_stale_completions`.
- Old IOSurfaces are not freed by the resize: Flutter may still hold a retained
  `CVPixelBuffer` for an in-flight raster; CF refcounting keeps that one
  surface alive until the raster finishes.
- Teardown ordering is the only way to avoid a use-after-free: the raster
  thread (`copyPixelBuffer`) and Metal's completion thread both call into the
  Rust context, so Rust must be told to stop signalling and Swift must stop
  acquiring **before** the context is freed. `dispose()` alone cannot await,
  so the parent calls `teardown()` first; `dispose()` still runs it as a
  fallback (the async tail finishes after the widget is gone).
- Between `fynkel_m0_resize` and `rebindTexture` the presenter has no buffers
  (`acquire_presented` returns -1 → `copyPixelBuffer` returns nil); the Dart
  ticker skips `render()` in that window. In practice the window is shorter
  than one frame (0 skipped ticks observed).

## Verdict

Resize, lifecycle and dispose are handled without copies, stalls or crashes.
Nothing here threatens ADR-0002. The only item not physically tested is a
live DPI change; it will be re-checked when an external display is available
or on iOS (M1).

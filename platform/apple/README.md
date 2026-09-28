# `platform/apple` — Apple platform integration contract

Per blueprint §8.4, for the qualified target macOS (Metal) and, through the
same IOSurface/CVPixelBuffer interop, the iOS smoke target (ADR-0009; M1-09
smoke, M6 qualification). The crate has no AppKit/UIKit dependency and
builds unchanged for `aarch64-apple-ios[-sim]`:

| Item | Contract |
| --- | --- |
| Graphics backend | wgpu, Metal backend only (`Backends::METAL`). |
| Adapter/device ownership | Every viewport of the process shares one `wgpu::Device`/`Queue` (ABI 10, ADR-0030 §4): the first viewport makes it, the last one frees it; each viewport keeps its own ring and renderer. `FYNKEL_VIEWPORT_DEVICE=own` gives each viewport its own device instead. Numbers: `docs/perf/shared-gpu.md`. The engine core has no GPU dependency. |
| Flutter renderer relationship | Impeller composites the viewport as an ordinary `Texture` layer (Path A, ADR-0002). Flutter never touches the engine's device. |
| Native surface/texture ownership | Rust creates a ring of N `IOSurface`s (N ∈ 2..=8, default 4) and imports each as an `MTLTexture` on **wgpu's own `MTLDevice`** (asserted at import). Swift wraps the same surfaces as `CVPixelBuffer`s. IOSurfaces stay alive by CF refcount as long as either side holds them. |
| Synchronization | Explicit per-slot state machine `Free → Rendering → Ready → Presented → Retiring → Free`. A slot becomes `Ready` only when the GPU work into it has **completed** (`MTLCommandBuffer.addCompletedHandler` on a fence buffer committed behind the frame; wgpu `on_submitted_work_done` and a blocking wait exist for tests). The presenter's current and previous slots are never rendered into; a full ring **drops** the frame. `acquire_presented` may wait a bounded time (default 3 ms, host-set) for the newest in-flight frame so a present shows the frame rendered this tick (lag 0). |
| Required copies | None. No CPU readback on any per-frame path (`read_pixel` exists for tests only). |
| Colour space | `BGRA8Unorm` (`kCVPixelFormatType_32BGRA`), untagged; treated as sRGB-encoded by the compositor. Linear-light rendering with proper sRGB output is Renderer V1 work. |
| Alpha / premultiplication | Opaque: the renderer clears to alpha 1 and draws opaque geometry. Transparent viewports are out of scope for M1. |
| DPI scaling | The host sizes the ring in **physical** pixels (`LayoutBuilder` size × `devicePixelRatio`); the texture is drawn 1:1 into the widget bounds. |
| Resize path | Host loop: stop rendering → `fynkel_apple_viewport_resize` (waits for GPU idle, rebuilds the ring under a new *generation*; stale completions cannot promote new slots) → Swift re-wraps the new surfaces (`rebindTexture`) → resume. Old surfaces are released when nobody holds them. |
| Device loss | A lost shared device is logged and never handed to a new viewport (the next `create` makes a fresh one). Viewports already on it are not rebuilt yet (Metal on Apple silicon does not lose devices in practice); a failed `create` or `resize` returns an error/NULL and the host keeps the old state. |
| Lifecycle / background | The host mutes its ticker on `hidden`/`paused` (no rendering, simulation clock paused); `inactive` keeps rendering. Resume re-bases the frame dt so the pause is not fed to the accumulator. |
| Resource disposal | Ordered teardown: stop the host loop → clear the frame-ready callback (synchronous; waits for an in-progress callback) → Swift detaches and unregisters the texture (raster thread will not call into Rust afterwards) → `fynkel_apple_viewport_destroy` → dispose the world. |

Threads: `create`/`resize`/`render`/`destroy` on the host-loop thread;
`acquire_presented` on the presenter (raster) thread, which never touches the
device; the frame-ready callback fires on a Metal completion thread and only
signals (`textureFrameAvailable`).

The invariants are regression-tested in `tests/ring.rs` (state rotation and
drop-not-overwrite in all three sync modes, resize under in-flight frames,
lag-0 wait bounds, viewport over a stepped world).

## ABI

`include/fynkel_apple.h` (v4: v2 came with the M3 spike — the asset
store, placeholders — and grew the M3-05/05a/06 stats fields, v3 the
M3-16 instrumentation fields, v4 the editor's
`fynkel_apple_viewport_debug_lines` and `fynkel_apple_viewport_read_frame`
— M4-06, additive). The dylib built from this crate also exports
`fynkel_native`'s ABI (`crates/fynkel_native/include/fynkel.h`), so a Flutter
host loads one library. `fynkel_apple_viewport_render(viewport, world)` takes
the `FynkelWorld*` the SDK holds and extracts it in Rust; the host never sees
the renderer. A panic inside any viewport call is caught at the boundary
(ABI 11, foundation 4.3): the viewport is broken from then on, `render`
answers `FYNKEL_APPLE_RENDER_BROKEN` and `fynkel_apple_viewport_error`
has the message; the process lives on. `debug_lines` queues world-space segments (ten floats each,
linear RGBA) for the next render's depth-tested unlit debug pass and is
cleared by that render; `read_frame(slot, out, cap)` copies a rendered
slot as RGBA8, top row first, after waiting for the GPU — a screenshot
path (`fynkel render` and the editor's Save Frame agree to the byte).

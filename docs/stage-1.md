# Stage 1 plan: a lit cube, written in Dart

Status: accepted 2026-10-07. Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
That includes "Flutter provides the frame clock, and Rust does the work
on its own threads", which §3.5 puts into practice.

Stage 1 ends when this Dart program shows a lit, spinning cube at the
display rate on macOS, iOS and Android:

```dart
final world = World();
world.spawn(Camera(), Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero));
world.spawn(Light.sun(direction: Vec3(-0.4, -1, -0.3)));
final cube = world.spawn(Transform(), MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A))));

FynkelView(world: world, onFrame: (dt) => cube.transform.rotate(Vec3.up, dt));
```

That's all stage 1 does. Nothing else is in scope.

## 1. Lessons from Fynkel 1, and what we do differently

| Fynkel 1 | Fynkel 2 |
|---|---|
| The renderer depended on assets, cook, core and import, so it couldn't be tested alone. | `fynkel_render` depends only on wgpu, glam and bytemuck. It draws a plain `RenderScene` value and knows nothing about the world, platforms or Flutter. |
| Platform code (IOSurface, the ring) was mixed into the bridge. | The frame ring and the platform targets get their own crate, `fynkel_present`. The ring's state machine is plain Rust, tested without a GPU. |
| Dart↔Rust went through a versioned binary wire protocol with generated codecs (ABI v5, viewport ABI 15). | Direct `dart:ffi` calls with plain C types. A batched path is added **only if** a measurement shows the calls are too slow (step 4 has the benchmark). |
| The render worker and the frame queue came late (F8). They took the iPhone from 68 to 120 fps. | Both are in the design from the start. Rendering runs on a worker thread, at most 2 frames in flight, and the oldest Ready frame is shown first. |
| Resolution scale was found late and set by hand. | `FynkelView(resolutionScale:)` exists from day one, with default 1.0. It's never lowered silently. |
| Light units were unclear. The Bevy comparison showed how easily the picture drifts. | Units are written down (§3.6), and a golden image test catches any drift. |
| Per-frame allocations were found and fixed late (M3-16c). | A test proves that a steady-state frame makes zero Rust heap allocations. Dart allocations per frame are measured from step 5. |
| Performance was measured in separate benchmark projects. | Stats are built in (`world.stats`), and `examples/hello` can show them as an overlay. |

## 2. Layout

```
crates/
  fynkel_world/      hecs world, ObjectId, components, field descriptions
  fynkel_render/     wgpu: primitives, PBR forward pass, renders a RenderScene into a texture view
  fynkel_present/    the frame ring + targets: headless (readback), Apple (IOSurface), Android (surface)
  fynkel_ffi/        world → RenderScene extract, the render worker, the C ABI (the only `unsafe` boundary)
packages/
  fynkel/            pure Dart API over dart:ffi; a build hook compiles fynkel_ffi for the target
  fynkel_flutter/    FynkelView widget, ticker, texture plugin (Swift for Apple, Kotlin for Android)
examples/
  hello/             the spinning cube, the stage 1 acceptance app
tests/golden/        reference PNGs for the headless tests
```

Dependencies only point down: `fynkel_flutter` → `fynkel` → `fynkel_ffi` → {`fynkel_world`, `fynkel_render`, `fynkel_present`}.
`fynkel_world`, `fynkel_render` and `fynkel_present` don't know about each other.

Pinned versions: Rust 1.93.0, wgpu 30.0.1, hecs 0.11.2, glam 0.34, Flutter 3.47.2.

## 3. Design decisions

### 3.1 Objects and stable IDs (rule 3)

- Every object has an `ObjectId(u64)`. IDs start at 1, are never reused, and stay the same for the object's whole life. 0 means "none".
- The world maps `ObjectId → hecs::Entity`. Dart only ever sees `ObjectId`. A hecs handle never crosses the FFI.
- Using a despawned ID is a defined error (`FY_ERR_NO_OBJECT`), never a crash, and never acts on a different object.
- Meshes and materials are resources with their own IDs (`MeshId`, `MaterialId`). They live until they're disposed or the world is. Disposing one that's still in use is an error.

### 3.2 Components that describe themselves (rule 1)

Each component implements a small trait:

```rust
pub trait Component: hecs::Component {
    const NAME: &'static str;
    fn fields() -> &'static [Field];   // name, kind, default, optional range
}
```

`FieldKind` covers what stage 1 needs: `F32`, `Vec3`, `Quat`, `Color`, `Bool`, `Mesh`, `Material`.
For four components we write these by hand, and a test checks that each
listed default matches `Default::default()`. A derive macro waits until
there are enough components to justify one.

Stage 1 components:

- `Transform`: position, rotation and scale. There is no hierarchy in stage 1.
- `MeshRenderer`: a mesh and a material.
- `Camera`: vertical FOV, near and far planes, and clear colour. The first camera is the one rendered.
- `Light`: one directional sun (direction comes from the transform's rotation, plus colour and intensity) and an ambient term. At most one sun is used.

### 3.3 One public API (rule 2)

Everything a game can do goes through `World` in Dart, which calls `fynkel_ffi`.
Nothing is created any other way: no files, no hidden setup. An editor
later would be just another caller of the same API.

### 3.4 The FFI boundary

- C functions are named `fy_*`, with plain types only: integers, floats, pointers to `#[repr(C)]` structs and opaque handles.
- No Rust panic ever crosses into Dart. Every entry point uses `catch_unwind`, returns an error code, and leaves a readable message for `fy_last_error`.
- Calls that don't block use Dart's `isLeaf: true`, which makes each call a few tens of nanoseconds.
- One header, `include/fynkel.h`, generated by cbindgen and checked in CI against the Dart bindings, so the two can't drift apart.
- `fy_abi_version()` is checked by Dart at load time, so a mismatch fails loudly and early.

### 3.5 Threads and frames

```
Dart UI isolate          render worker (Rust)              Flutter raster thread / OS compositor
---------------          --------------------              -------------------------------------
game code → world
fy_frame():
  extract → RenderScene ──► mailbox (newest wins) ──► render into a Free slot
                                                     submit; GPU done → Ready ──► acquire(): oldest Ready
```

- `fy_frame` copies the world into a `RenderScene` and returns. It never waits for the GPU. The scene buffers are reused, so no allocations happen.
- The render worker owns the wgpu device and queue. Only one thread ever touches them.
- **The Apple ring:** 5 IOSurface slots. States are Free → Rendering → Ready → Presented → Retiring → Free. At most 2 frames are in flight; extra scenes are coalesced, not queued. `acquire` returns the **oldest** Ready slot (at most 2 waiting) and sets bit `0x100` when more are waiting, so the Swift side asks for another frame. These are the exact rules that reached 120 fps on the iPhone, now written as a state machine with its own tests.
- **Android:** Flutter's `SurfaceProducer` gives us a `Surface`. Rust turns it into an `ANativeWindow` and a wgpu surface, and the render worker presents to it directly. The OS does the queueing, so the ring isn't used. Both platforms implement the same small `Target` trait in `fynkel_present`.
- **Headless:** renders into an offscreen texture and reads it back. It is used by the tests and by CI.

### 3.6 Rendering

- A forward pass with 4× MSAA (cheap on tile GPUs, because the multisample buffer stays in on-chip memory), resolved into the target.
- Reversed-Z with `Depth32Float`, for better depth precision than standard Z.
- The material is metallic-roughness PBR: base colour, metallic and roughness. Colours are given in sRGB, as in a colour picker, and converted to linear once.
- **Units:** unitless and documented. A sun of intensity 1.0 shows a white, rough, sun-facing surface at its albedo. Ambient is a colour times an intensity. Output goes through the Khronos PBR Neutral tonemapper, which keeps albedo colours true, into an sRGB target.
- Primitives are generated in code: a cube with 24 vertices and flat normals, a UV sphere, and a plane.
- Not in stage 1: shadows, textures, more than one light, a hierarchy, culling or instancing. One draw per object is fine at this scale.

### 3.7 Measuring

`world.stats` reports:
- frames submitted, rendered, presented and dropped
- the `fy_frame` time and the render worker's CPU time (p50 and p95)
- GPU time, from timestamp queries where the device supports them

`examples/hello` has a stats overlay you can turn on and off.
The display rate is checked as "fresh frames per second", counted where Flutter actually takes a new frame, not where we render one.

## 4. Steps

Each step is one or a few small commits. Each has a "done when" that is
a test or a measurement, and where it makes sense the test is written to
fail first.

**Step 0: workspace and CI**
- Cargo workspace (the four crates, each still empty), `rustfmt.toml`, clippy with warnings as errors, `deny(unsafe_code)` everywhere except `fynkel_ffi` and the platform modules of `fynkel_present`.
- CI on the self-hosted runner (label `fynkel`) runs fmt, clippy and tests.
- *Done when* CI is green on a PR. A deliberately broken format makes it red, and that breakage is then reverted.

**Step 1: `fynkel_world`**
- `ObjectId`, spawn/despawn, the ID→entity map, the four components, `Component::fields()`, and mesh/material resource IDs.
- *Done when* tests show that IDs are never reused after despawn, a stale ID gives `NoObject`, every component's field defaults match `Default`, and spawning or despawning 100k objects stays within a measured budget (recorded, not guessed).

**Step 2: `fynkel_render`, headless**
- `RenderScene`, primitives, the PBR pipeline, MSAA, reversed-Z, the tonemapper, and the headless target in `fynkel_present`.
- Golden test: render the stage 1 scene at 256×256 and compare it with `tests/golden/cube.png`. Each channel may differ by at most 2/255, and at most 0.1 % of pixels may be outside that. References are only regenerated on purpose (`FYNKEL_BLESS=1`), and the image goes in the commit for review.
- More golden tests: a sphere lit from four sun directions, which checks the lighting maths; and a white plane at intensity 1.0, which checks the unit definition in §3.6.
- *Done when* the golden tests pass in CI and a test proves a steady-state frame makes zero heap allocations.

**Step 3: `fynkel_present`, the ring**
- The slot state machine and the frame-queue rules from §3.5, with no GPU.
- *Done when* table tests cover every state change (including illegal ones, which return an error rather than corrupting the state), and a multi-threaded stress test with 3 threads and 1M frames never shows a slot in two states and never loses a slot.

**Step 4: `fynkel_ffi` + `packages/fynkel`**
- The extract, the render worker, the C ABI, cbindgen, and the Dart API (`World`, `spawn`, `Object`, components, `Mesh`, `Material`, `Color`, `Vec3`, `Quat`), plus the build hook.
- *Done when*:
  - a Dart test builds the stage 1 scene and renders it headless to the same golden PNG as step 2;
  - a Dart test shows that a despawned object's ID throws a clear error;
  - an FFI benchmark of setting 10,000 transforms in one frame is recorded (decision rule: under 1 ms on the Mac keeps direct calls; over that, design a batched path then).

**Step 5: macOS — Apple target, `fynkel_flutter`, `examples/hello`**
- IOSurface slots imported as wgpu Metal textures, the Swift `FlutterTexture`, the `FynkelView` widget (ticker, resize, dispose, `resolutionScale`), and the hello app.
- *Done when*:
  - the cube spins at 120 fresh fps on the Mac's display (window and full screen), with no dropped frames over 60 s;
  - 10 create/destroy cycles and 10 resizes leave memory flat (± 5 MB);
  - Dart allocation per frame is recorded.

**Step 6: iOS**
- The same app on the iPhone 16 Pro Max.
- *Done when* it holds 120 fresh fps at native resolution over 60 s with no throttling, and the same create/destroy and resize checks pass.

**Step 7: Android**
- A Kotlin plugin (`SurfaceProducer`), the `ANativeWindow` → wgpu surface, the Android `Target`, and the build hook producing `arm64-v8a` and `x86_64`.
- *Done when* the cube runs on the emulator (`Medium_Phone_API_36.0`) and on a physical device at its display rate, including app pause/resume (the surface is destroyed and recreated) and rotation. The physical device needs one to be connected.

**Step 8: the stage 1 check**
- Run hello on all three platforms and write a short report with the numbers.
- *Done when* the report is committed and you have seen the cube on each device yourself.

## 5. Not in stage 1

Textures, glTF, shadows, a hierarchy, more than one light, input, physics,
audio, animation, saving, Windows and Linux. Each comes back later on its
own, under the quality rule in ADR-0001.

## 6. Open questions

1. **An Android phone.** Step 7 needs one connected over USB or Wi-Fi. Until then, only the emulator can be tested.
2. ~~Light units~~: unitless, confirmed 2026-10-07.
3. ~~The API shape~~: the snippet at the top was confirmed 2026-10-07.

## 7. Measurements

### Step 1: `fynkel_world` (2026-10-07, M3 Max Mac, release build)

`cargo run --release -p fynkel_world --example bench`, 100,000 objects,
best of 5 runs:

| Operation | Total | Per object |
|---|---|---|
| spawn + Transform + MeshRenderer | 8.60 ms | 86.0 ns |
| set Transform (validated) | 3.03 ms | 30.3 ns |
| get Transform | 1.84 ms | 18.4 ns |
| for_each2 Transform+MeshRenderer | 0.08 ms | 0.8 ns |
| despawn | 1.84 ms | 18.4 ns |

What this means: setting a transform, which is the call a game makes most
often, costs about 30 ns with full validation. A frame that moves 10,000
objects spends 0.3 ms in the world. Walking every drawable object, which
the renderer's extract will do each frame, is close to free. Nothing here
needs optimising now.

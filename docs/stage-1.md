# Stage 1 plan: a lit cube, written in Dart

Status: accepted 2026-10-07. Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
That includes "Flutter provides the frame clock, and Rust does the work
on its own threads", which §3.5 puts into practice.

Stage 1 ends when this Dart program shows a lit, spinning cube at the
display rate on macOS, iOS and Android:

```dart
final world = World();
world.spawn(Camera(), Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero));
world.spawn(Light.sun(), Transform(forward: Vec3(-0.4, -1, -0.3)));
final cube = world.spawn(Transform(), MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A))));

FynkelView(world: world, onFrame: (dt) => cube.rotate(Vec3.up, dt));
```

Step 4 changed two lines of the snippet confirmed on 2026-10-07; the user
accepted both the same day:
- The sun's direction is its transform's, as the engine has it, so
  `Light.sun()` comes with `Transform(forward: …)` instead of taking a
  `direction:`. A `direction` on the light would be a second place for the
  same fact, and it couldn't be read back.
- Components are values: reading `cube.transform` gives a copy. Changing
  a copy (`cube.transform.rotate(…)`) would silently do nothing, so the
  verbs are on the object: `cube.rotate(…)`, `cube.translate(…)`, or
  `cube.transform = cube.transform.rotated(…)`.

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
- Using a despawned ID is a defined error (`FY_STATUS_NO_OBJECT`; in Dart a `FynkelException` with `FynkelStatus.noObject`), never a crash, and never acts on a different object.
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
- **Units:** unitless. Before tonemapping, a sun of intensity 1.0 makes a rough surface that faces it reflect exactly its albedo (Lambert diffuse), plus a small specular term. The same goes for an ambient of 1.0 on its own. The Khronos PBR Neutral tonemapper then subtracts a small offset (0.04 in linear, to cancel the Fresnel reflection every surface has) and rolls highlights off above 0.76. So a lit mid-grey shows a few levels darker than its picked colour, and colours stay true otherwise. A test checks the GPU against a CPU copy of these formulas, to within 1/255.
- **sRGB views only.** The renderer draws into an sRGB view, so the hardware encodes and the MSAA resolve averages in linear light. Resolving into a plain format averaged encoded values and darkened every edge by up to 47/255 (found by a test). A plain-format texture or surface still works through an sRGB view of itself (`view_formats`).
- **Instancing.** Per-draw data lives in an instance buffer, and draws are sorted by mesh, so each mesh is one draw call. 1,000 cubes is 1 call. This cut wgpu's own allocations per frame from 1,072 to about 69.
- **No GPU copies per frame.** The frame uniforms and the instances go into one of three buffers that the CPU writes and the GPU reads in place (`frame_ring.rs`; wgpu's `MAPPABLE_PRIMARY_BUFFERS`, which every native backend has). Meshes are written the same way. `queue.write_buffer` would copy them with a blit, and on Apple GPUs a process that blits every frame keeps 128 MB of driver memory dirty (§6, question 6).
- Primitives are generated in code: a cube with 24 vertices and flat normals, a UV sphere, and a plane.
- Not in stage 1: shadows, textures, more than one light, a hierarchy, or culling.

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
- ✅ Done 2026-10-07 (PR #1).

**Step 1: `fynkel_world`**
- `ObjectId`, spawn/despawn, the ID→entity map, the four components, `Component::fields()`, and mesh/material resource IDs.
- *Done when* tests show that IDs are never reused after despawn, a stale ID gives `NoObject`, every component's field defaults match `Default`, and spawning or despawning 100k objects stays within a measured budget (recorded, not guessed).
- ✅ Done 2026-10-07 (PR #2).

**Step 2: `fynkel_render`, headless**
- `RenderScene`, primitives, the PBR pipeline, MSAA, reversed-Z and the tonemapper. Getting a headless GPU (`Gpu::headless`) lives in `fynkel_render`, because every caller needs it. The `Target` for headless frames comes with the ring in `fynkel_present`.
- Golden test: render the stage 1 scene at 256×256 and compare it with `tests/golden/cube.png`. Each channel may differ by at most 2/255, and at most 0.1 % of pixels may be outside that. References are only regenerated on purpose (`FYNKEL_BLESS=1`), and the image goes in the commit for review.
- More golden tests: a sphere lit from four sun directions, which checks the lighting maths, plus a mixed instanced scene. A plane facing the sun and camera checks the unit definition in §3.6 against the CPU formulas.
- *Done when* the golden tests pass in CI and a test proves a steady-state `prepare` makes zero heap allocations. wgpu allocates inside `encode` and submit, which our code can't avoid, so that number is recorded rather than asserted.
- ✅ Done 2026-10-07 (see §7).
- **Simulators.** The same GPU tests pass inside the iOS Simulator (Metal) and the Android emulator (Vulkan through MoltenVK), reference images included. `tools/test-mobile.sh` runs them, and CI runs it on every change. The Android emulator's Vulkan driver crashes when wgpu names GPU objects, so on Android names aren't passed to the driver (`instance_flags()`).

**Step 3: `fynkel_present`, the ring**
- The slot state machine and the frame-queue rules from §3.5, with no GPU.
- *Done when* table tests cover every state change (including illegal ones, which return an error rather than corrupting the state), and a multi-threaded stress test with 3 threads and 1M frames never shows a slot in two states and never loses a slot.
- ✅ Done 2026-10-07 (see §7). The rules are Fynkel 1's, with these improvements:
  - Every move goes through one checked table (`SlotState::can_become`).
  - Tickets are taken by value, so completing a frame twice doesn't compile. A ticket from before a resize returns `Stale`.
  - The in-flight cap holds across resizes.
  - `reserve` takes the slot that has been free longest, so every slot takes its turn and the compositor's last slot is reused last.
  - A frame that finishes after a newer one is on screen is dropped, so frames are never shown out of order.
  - `acquire` allocates nothing.
  - Debug builds check every invariant after every operation, including that each frame is accounted for.
  - Five deliberate bugs (wrong retire order, late frames shown, lowest slot first, stale tickets forgotten, a longer queue) were each caught by the tests.

**Step 4: `fynkel_ffi` + `packages/fynkel`**
- The extract, the render worker, the C ABI, cbindgen, and the Dart API (`World`, `spawn`, `GameObject`, components, `Mesh`, `Material`, `Color`, `Vec3`, `Quat`), plus the build hook.
- ✅ Done 2026-10-07 (see §7).
  - The plan's `Object` is `GameObject`: in Dart, `Object` is the type of every value, so the name can't be reused.
  - The C header is generated by cbindgen and checked against the source in a test; the Dart bindings are generated from the header by `tool/ffigen.dart` and checked in CI, so neither side can drift.
  - Every C call is guarded: a panic becomes a status and a message, and is never undefined behaviour. A view keeps its world alive, so freeing them in either order is safe.
  - The Dart API checks the engine's ABI version before the first world, frees worlds and views with native finalizers when they aren't disposed, and passes structs through scratch memory allocated once, so a call allocates no native memory.
  - A `Material` can be used in several worlds and changing it changes all of them; the material holds the worlds weakly, so a material kept in a global doesn't keep worlds alive.
  - A spawn whose component is refused despawns the half-made object.
  - Measuring the 10,000-transform frame showed the cost was in `World::insert`, not the FFI. Replacing a component now writes it in place (30.7 → 23 ns per call in step 1's benchmark).
  - Eight deliberate bugs in the Dart side (wrong rotation order, a lost sign or fallback in `lookRotation`, a half-made object left behind, a material change not sent, a swapped camera field, a wrong colour channel, a 1° turn of the cube) were each caught by the tests.
- *Done when*:
  - a Dart test builds the stage 1 scene and renders it headless to the same golden PNG as step 2;
  - a Dart test shows that a despawned object's ID throws a clear error;
  - an FFI benchmark of setting 10,000 transforms in one frame is recorded (decision rule: under 1 ms on the Mac keeps direct calls; over that, design a batched path then).

**Step 5: macOS — Apple target, `fynkel_flutter`, `examples/hello`**
- IOSurface slots imported as wgpu Metal textures, the Swift `FlutterTexture`, the `FynkelView` widget (ticker, resize, dispose, `resolutionScale`), and the hello app.
- ✅ Done 2026-10-07 (see §7).
  - **The memory rule, as read from step 5 on** (decided 2026-10-07): points of the same kind (the same view at the same size) must agree within ± 5 MB. Each point is the lowest of five readings while the view is drawing. One difference is allowed and must be named when it shows: Flutter's compositor sometimes keeps two more window-sized IOSurfaces (+14.7 MB at 1600 × 1200), visible in the IOSurface category and sized like the window, not like our surfaces. Flutter owns that pool and it isn't the engine's memory. Any other difference fails the step. Growth over cycles fails it too, so the 40-cycle run stays part of the check.
  - The Apple target draws into 5 IOSurfaces imported as Metal textures. When a frame's GPU work is done, a Metal completion handler marks it finished in the ring and tells Flutter. Flutter's raster thread calls `fy_apple_acquire`, which never blocks and allocates nothing (tested over 1,000 calls), and gets the surface retained once.
  - A resize makes new surfaces and resets the ring under the same lock, so a frame of the old size is never shown (tested). The Swift side needs no "rebind" call: it keeps a pixel buffer per surface and starts again when an unknown surface comes from a full cache.
  - The plugin's teardown order is: no more frame-ready calls (the engine waits for one in progress), no more acquires, unregister the texture, and only then does Dart free the view. A widget test with a real engine view proves the view is still alive when the plugin is told, also when the widget goes away while its texture is still being registered. Swapping the order fails the test.
  - The garbage collector never frees an `AppleView`, because Flutter's raster thread may still be using it; only `dispose` does, after the plugin has let go. Headless views still have finalizers.
  - `HeadlessView` and `AppleView` share `WorldView` (`frame`, `resize`, `stats`, `dispose`). `AppleView` is in `package:fynkel/platform.dart`, not in the game API.
  - `tool/measure.dart` reads the app's Dart heap from outside the app, through the VM service, so the measurement allocates nothing in the app it measures.
- *Done when*:
  - the cube spins at 120 fresh fps on the Mac's display (window and full screen), with no dropped frames over 60 s;
  - 10 create/destroy cycles and 10 resizes leave memory flat (± 5 MB);
  - Dart allocation per frame is recorded.

**Step 6: iOS**
- The same app on the iPhone 16 Pro Max.
- *Done when* it holds 120 fresh fps at native resolution over 60 s with no throttling, and the same create/destroy and resize checks pass. `examples/hello` must also run in the **iOS Simulator** with `flutter run`, with the cube spinning smoothly, and with hot reload of the Dart game code working. The simulator's frame rate isn't a performance number.
- In progress (see §7). Done so far:
  - the iPhone holds 120 fresh fps at native 1320 × 2868 over 60 s;
  - the simulator runs hello with `flutter run`, and hot reload works both ways.
  - **Hot restart leaked a render worker each time.** A restart makes a new Dart isolate without disposing anything, so the old views kept drawing. Now the plugin is sent `reset` once per isolate, before its first view registers. It releases every view it still holds and frees each one through `fy_view_free`, whose address Dart passes in at registration. A widget test checks that the reset comes first; dropping it fails the test. After 5 restarts, 1 worker runs.
  - **Autorelease pools on engine threads.** Rust threads have no autorelease pool, so what Metal autoreleases on them would only go when the thread ends. The render worker now drains a pool every pass, and view creation and freeing (called from Dart's thread) drain one too (`fynkel_present::autorelease_pool`). On its own, the Apple target leaked 0.8 MB over 400 resizes without the pool and nothing with it.
  - `tool/measure.dart --ios <device>` runs the same checks on an iPhone or a simulator. The settings go in as `--dart-define`s. The app's output is read from the VM service, because iOS output doesn't reach `flutter run`. The app reports its own footprint, read with `task_info`, which matches `footprint` on the Mac exactly. Each lifecycle point also shows the Dart heap.
- Still to do: the create/destroy and resize checks on the iPhone (the first 40-cycle run grew; see §7).

**Step 7: Android**
- A Kotlin plugin (`SurfaceProducer`), the `ANativeWindow` → wgpu surface, the Android `Target`, and the build hook producing `arm64-v8a` and `x86_64`.
- *Done when* the cube runs with `flutter run` on the **emulator** (`Medium_Phone_API_36.0`), with hot reload working, and on a physical device at its display rate, including app pause/resume (the surface is destroyed and recreated) and rotation. The physical device needs one to be connected.

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
4. ~~Two changes to that snippet~~: `Light.sun()` with `Transform(forward: …)`, and `cube.rotate(…)`. Accepted 2026-10-07.
5. ~~Names shared with Flutter~~: decided 2026-10-07, nothing is renamed. `Color`, `Material` and `Transform` are also Flutter names, so a file that imports both `package:flutter/material.dart` and `package:fynkel/fynkel.dart` must hide one or use a prefix. The convention: keep the game in its own files, which import only `package:fynkel`, and keep the Flutter side in files that don't import it (`examples/hello` does this: `lib/game.dart` and `lib/main.dart`). `package:fynkel_flutter` exports only `FynkelView` and `FynkelViewStats`, so it never clashes. A file that really needs both writes `import 'package:fynkel/fynkel.dart' as fy;`. Nothing is renamed.
6. ~~The engine's fixed GPU memory~~: explained, and cut from about 324 MB to 198 MB, on 2026-10-07. The memory isn't the engine's. Apple's Metal driver gives each process pools of its own when it first uses the GPU, and the footprint counts them: 195 MB once it runs a render pass, and another 128 MB once it runs a blit (a GPU copy or fill) or a compute pass. Plain Metal measures the same, with no wgpu involved (a test program: render only 195.5 MB, blit only 128 MB, both 323.5 MB). The driver frees a pool about a second after the GPU stops using it, and frees everything when the GPU goes idle. The engine paid the blit pool all the time because `queue.write_buffer` copies with a blit, and the renderer called it twice a frame. It now writes per-frame data where the GPU reads it (§3.6), and `crates/fynkel_render/tests/gpu_memory.rs` fails if a steady frame blits again. The render pool stays: every app that draws pays it, Flutter included (a Flutter app with a spinner holds 197 MB). Earlier figures were wrong in two ways. The "blank Flutter app, 54 MB" drew nothing, so it held no pools. And "wgpu adds 130 MB" was really our `write_buffer` calls. Still open, and not worth doing now: a new view still blits once (wgpu clears a buffer of its own when a device is made), which costs 128 MB for about a second. One device shared by all views would avoid that, along with the device's start-up time, once a game has more than one view.

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

### Step 2: `fynkel_render` (2026-10-07, M3 Max Mac, release build)

`cargo run --release -p fynkel_render --example bench`: a 1280×720 target
with 4× MSAA, moving cubes, best of 50 frames.

| Cubes | `prepare` (CPU) | Whole frame (prepare, encode, submit, wait for GPU) |
|---|---|---|
| 1 | 0.000 ms | 0.43 ms |
| 1,000 | 0.010 ms | 0.55 ms |
| 10,000 | 0.096 ms | 0.99 ms |

Allocations in a steady frame with 1,000 draws: 0 in `prepare` (asserted).
wgpu makes about 69 in `encode` + submit; that was 1,072 before draws were
instanced by mesh.

What this means: at 120 fps a frame has 8.3 ms. 10,000 moving cubes take
under 1 ms on this Mac, including waiting for the GPU. The phone will be
slower, and steps 5–7 measure it there.

### Step 3: the frame ring (2026-10-07, M3 Max Mac, release build)

`cargo run --release -p fynkel_present --example bench`: one frame's trip
round the ring (reserve, complete, acquire) on one thread, best of 5 runs
of 1M frames: **24 ns**, about 8 ns per operation, each taking the lock
once.

The stress test (`tests/stress.rs`) runs three threads at full speed (render
worker, GPU completion and presenter, with out-of-order completions,
cancels and a reset every 50,000 frames shown) for 1M frames. It takes
3.8 s in release and about 12 s in debug, where the invariants are checked
after every operation. Most of that time is the threads contending for
the lock on purpose, far more often than the three calls per frame they
make in real use.

What this means: the ring costs nothing measurable per frame. Its rules,
not its speed, are what matter, and they are now tested.

### Step 4: the C interface and `package:fynkel` (2026-10-07, M3 Max Mac)

`packages/fynkel/benchmark/transforms.dart`: 10,000 objects, one call per
object per frame, 300 frames after 50 warm-up frames.

| Per frame of 10,000 | AOT (`dart build cli`), median | p95 | JIT (`dart run`), median |
|---|---|---|---|
| set transform | **0.35 ms** | 0.39 ms | 0.55 ms |
| get transform | 0.43 ms | 0.48 ms | 0.38 ms |
| rotate (get, turn, set) | 1.25 ms | 1.31 ms | 1.20 ms |

Where the time goes, per call: crossing into Rust and back is 3 ns
(`fy_abi_version`); `fy_transform_set` is 34 ns: 23 ns in the world
(it was 31 ns before the in-place write), the rest the world's lock.
`rotate` adds a sine, a cosine, a quaternion product and four small Dart
objects to a get and a set.

Zero allocations on the frame clock's thread over 100 frames that each
set 1,000 transforms (`crates/fynkel_ffi/tests/alloc.rs`).

What this means: setting 10,000 transforms costs 0.35 ms, under the 1 ms
rule, so the calls stay direct and no batched path is designed. A game
that turns 10,000 objects every frame through `rotate` spends 1.25 ms of
its 8.3 ms. That's worth remembering, but not worth fixing before a
real game needs it.

### Step 5: macOS (2026-10-07, M3 Max MacBook Pro, 120 Hz built-in display)

`examples/hello/tool/measure.dart` on the profile build (AOT), with the
display kept awake (`caffeinate -du`), after 5 s of warm-up.

| 60 s of the spinning cube | Window, 1600 × 1200 pixels | Full screen, 3456 × 2168 |
|---|---|---|
| Fresh frames per second (median, min, max) | **120**, 120, 120 | **120**, 120, 120 |
| Frames drawn / shown | 7,139 / 7,139 | 7,139 / 7,139 |
| Coalesced, skipped, dropped | 0, 0, 0 | 0, 0, 0 |
| Flutter frames over 8.33 ms (build or raster) | 0 | 0 |
| Dart allocation per frame | 13.4 KB | 13.4 KB |

A first window run had 3 dropped frames and 2 slow Flutter frames in its
first seconds; three more runs had none.

**Dart allocation per frame** is 7.1 KB with no mouse over the view and
13.4 KB with one. While a mouse pointer is inside a Flutter view, Flutter
hit-tests its position after every frame to update hover; that's the
6 KB difference, and it isn't Fynkel's. Both runs above had the pointer
over the view. Fynkel 1's floor for a blank Flutter app with a ticker was
about 12 KB per frame. The stats line says whether a mouse was in the
view (`mouse=1`), so a measurement always says which case it was.

**Memory** is the process's physical footprint (`footprint`), which counts
GPU memory and IOSurfaces, not just the heap. One run, 10 cycles each,
before the GPU memory fix (§6, question 6):

| Points | Range |
|---|---|
| After each of 10 create/destroy cycles (full size) | 553.2 – 579.2 MB |
| After each resize back to full size | 563.4 – 564.6 MB (1.2 MB) |
| After each resize to 0.6 | 502.0 – 517.4 MB |

The memory at one size is the same each time, except for one thing that
isn't ours: Flutter's compositor keeps a pool of window-sized IOSurfaces
and sometimes holds 2 more (+14.7 MB: two 1600 × 1200 surfaces, and the
same at 0.6, where ours are smaller). Without it, the GPU memory after
the create/destroy cycles was 397.8 MB seven times, 399.7 MB twice, and
414.3 MB once, back to 399.7 MB at the next point. There's also one 7 MB
step in `MALLOC_LARGE` that happens once in a run (likely the system
allocator keeping freed large blocks). Over 40 cycles there is no growth:
cycles 15 to 39 stay within 0.8 MB. Read as "the engine's memory comes back", the rule
holds. Read as "the process total stays within ± 5 MB", it doesn't,
because of Flutter's pool. Your call.

Where the memory goes at 1600 × 1200 (about 564 MB in total, against
54 MB for a blank Flutter app with a ticker):
- 61 MB: the 4× MSAA colour and depth targets (32 bytes per pixel; this
  is the part that changes with the size);
- 37 MB: our 5 IOSurfaces;
- about 320 MB: fixed GPU memory of the engine's device, whatever the
  size (§6, question 6);
- the rest: Flutter, the Dart heap and the engine's CPU side.

What this means: on the Mac, the cube is at the display rate with every
frame fresh, in a window and full screen, and nothing builds up. The one
surprise was the fixed GPU memory, below.

**After the GPU memory fix** (2026-10-07, same Mac and build settings):

| | Before | After |
|---|---|---|
| The renderer alone, steady frames (GPU, dirty) | 326 MB | **198 MB** |
| `examples/hello`, steady, window (GPU, dirty) | 415.7 MB | **271.7 MB** |
| `examples/hello` after each create/destroy cycle (total) | 553.2 – 579.2 MB | **424.4 – 432.8 MB** |
| …its GPU part | 397.8 – 414.3 MB | **271.7 MB** every time |

That's 144 MB less in the app. The rest of hello's GPU memory: the
driver's render pool (about 198 MB, paid by any app that draws), our
61 MB of MSAA targets, and Flutter's own drawing. Frame rate is unchanged:
median 120 fresh fps, nothing coalesced or dropped.

Two measurement fixes came with it. Each lifecycle point is now the lowest
of five readings over two seconds, because a blit by anyone (a new view,
or Flutter updating text) holds the 128 MB pool for about a second, and a
single reading caught it at random. A leak is in every reading, so the
lowest can't hide one. And the tool now marks points where nothing was
drawn, because a hidden window gets no frames and an idle GPU frees its
pools: the readings drop to about 140 MB and mean nothing. All four
lifecycle runs after the fix stopped drawing partway through, 40 to 80 s
in, probably because the Mac was in use and the window got covered. (A
run with the stats printed drew all the way through, every create and
resize at 120 fps.) So the 8 created points above are the clean ones, and
the resize points need a run with the window left in front.

### Step 6: iOS (2026-10-07, iPhone 16 Pro Max, 120 Hz; iOS Simulator on the M3 Max)

`tool/measure.dart --ios <device>`: the profile build (AOT) on the
iPhone, the debug build in the simulator.

| 60 s of the spinning cube, iPhone | Native, 1320 × 2868 pixels |
|---|---|
| Fresh frames per second (median, min) | **120**, 118 |
| Coalesced, skipped, dropped | 0, 0, 0 |
| Flutter frames over 8.33 ms | 0 |
| Dart allocation per frame | 12.0 KB (the Mac: 7.1 KB with no mouse over the view) |

The extra Dart allocation on the iPhone isn't looked into yet.

**Memory, iPhone, before the autorelease pools**: 10 cycles stayed within
± 5 MB, but 40 cycles grew steadily, about 0.3 MB per create or resize:

| Points (40 cycles) | First → last |
|---|---|
| Created, full size | 367.7 → 380.4 MB |
| Resized back to full size | 380.5 → 391.8 MB |
| Resized to 0.6 | 255.4 → 266.7 MB |

That fails the rule. The simulator was used to find the cause, because
there the process can be inspected. Its footprint shows the CPU side.
Creates stayed flat (132 – 138 MB over 30 cycles); resizes grew
about 0.2 MB each (141.4 → 150.0 MB). What was found:
- **Not the engine.** The Apple target resized 400 times from Rust, with
  frames drawn and shown, stays flat once it has an autorelease pool
  (54.4 MB from start to end). The pool fix above went in anyway, but
  the simulator's growth didn't change with it.
- **Not our objects.** Between resize 4 and resize 28, malloc grows by
  about 2 MB, all of it allocated by Flutter or the Dart VM: Skia
  (0.8 MB), a Dart VM hash map (0.8 MB), the Dart timeline recorder
  (0.3 MB in 10,720 blocks) and Skia's text layout for the changing stats
  line. Our IOSurfaces, pixel buffers and textures appear as exactly one
  set of 5.
- **Not the Dart heap.** It stays between 72 and 79 MB with no trend.
- The rest is mapped memory: Dart's debug JIT code, and one 18.8 MB region
  that Flutter reserves when its view is created and that fills slowly
  (+2.8 MB). Both are bounded and neither is ours.
- With the view hidden during the resize phase, the same loop grows
  0.06 MB per cycle. So most of the growth comes with drawing frames,
  and the inspection above puts it on Flutter's side of them.

Still open: the iPhone's 40-cycle run with the pools in, now that the
tool shows the Dart heap at each point. The phone was unreachable when
this was written.

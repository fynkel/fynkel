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

Step 4 changed two lines of the snippet confirmed on 2026-10-07 (§6,
question 4):
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
- *Done when*:
  - the cube spins at 120 fresh fps on the Mac's display (window and full screen), with no dropped frames over 60 s;
  - 10 create/destroy cycles and 10 resizes leave memory flat (± 5 MB);
  - Dart allocation per frame is recorded.

**Step 6: iOS**
- The same app on the iPhone 16 Pro Max.
- *Done when* it holds 120 fresh fps at native resolution over 60 s with no throttling, and the same create/destroy and resize checks pass. `examples/hello` must also run in the **iOS Simulator** with `flutter run`, with the cube spinning smoothly, and with hot reload of the Dart game code working. The simulator's frame rate isn't a performance number.

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
4. **Two changes to that snippet** (step 4, explained under it): `Light.sun()` with `Transform(forward: …)`, and `cube.rotate(…)` for `cube.transform.rotate(…)`. Waiting for the user's OK; the other way is a small change.
5. **Names shared with Flutter.** `Color`, `Material` and `Transform` are also Flutter names, so a file that imports both `package:flutter/material.dart` and `package:fynkel/fynkel.dart` must hide one or use a prefix. To decide in step 5, when `examples/hello` first imports both.

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

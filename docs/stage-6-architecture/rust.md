# Stage 6, step 3: architecture review of the Rust engine

Reviewed at main `8940ebec`, 2026-10-12. Nothing in the repository was changed. Builds, tests and timings
used `CARGO_TARGET_DIR=/tmp/s6/arch/target` and, for edit timings, a copy of the crates in `/tmp/s6/arch/repo`.

## Summary verdict

**Sound, careful, and small. The core engineering is good. The gaps are in failure handling and in a renderer
built for stage 1 that will need reshaping before animation, shadows or particles.**

- The C interfaces are the strongest part. Every fallible `fy_*`/`fp_*` call goes through `guard()`, so a panic
  becomes a status and a message (`fynkel_ffi/src/error.rs:93-102`). Null pointers and lengths are checked.
  Handles are `Arc`/`Box` with clear lifetimes. The header is checked against the source (`tests/header.rs`),
  CI regenerates the Dart bindings from it, and Dart checks the ABI number at load (`packages/fynkel/lib/src/native.dart:24`).
  Unsafe code is denied across the workspace and allowed only at the boundary, with a `SAFETY` comment on each block.
- Measured: **347 tests pass, 0 fail, 1 ignored** (a measurement run by hand), and **0 clippy warnings**.
  Release build from clean (both libraries): **70 s**. Rebuilding after a one-line edit: **4–7 s** in release,
  **19 s** in debug for `fynkel_ffi`, which builds three crate types.
- The biggest risk: **the engine has no handling for GPU errors or a lost device.** wgpu's default handler
  panics, which kills the render worker for good. A GPU out-of-memory, a validation error, or an Android driver
  reset then freezes the view (finding 1).
- The biggest structural debt: **the renderer is one forward pass with one sun, no culling, no shadows and no
  transparency** (`renderer.rs`). That is fine for today's games. But skinning, particles, more lights and shadows
  all need a pass structure, light buffers and a vertex layout that can carry more than one kind of vertex
  (findings 2, 3, 9).
- Physics proved the package model: its own crate, its own library and ABI, no engine changes. Audio can copy
  it. Animation can't, because it has to reach into the render path.

## Findings

Severity: high = can lose a running game or data; medium = will hurt at scale or block the next features;
low = tidy-up. Effort: S = hours to a day, M = days, L = a week or more.

### 1. GPU errors and device loss kill the view permanently. **High**, M

*Evidence.*
- No `on_uncaptured_error` or device-lost callback is set anywhere. The only error scope in the crates is
  Android's `configure` (`fynkel_present/src/android.rs:407`).
- wgpu's default uncaptured-error handler panics. On the render thread, that panic is caught
  (`fynkel_ffi/src/worker.rs:99-107`) and recorded as `failure`. From then on, every `fy_view_frame` returns
  `WorkerFailed` (`worker.rs:126`), and the view never draws again.

*What triggers it.*
- **GPU out-of-memory** on texture or buffer creation (`texture.rs:221`, `renderer.rs:620`, `frame_ring.rs` grows
  its buffers). On 3–4 GB phones this is plausible.
- **A texture larger than the device allows.** `ImageData::MAX_SIZE` assumes 8192 "which every device meets"
  (`fynkel_load/src/lib.rs:88-90`). But `Gpu::from_adapter` lowers limits to whatever the adapter offers
  (`gpu.rs:61`), so a 4096-limit device turns a valid 8192 image into a validation panic.
- **Device loss**: an Android driver reset, or a GPU hang. `frame_ring.rs:186-192` notes that a lost device
  leaves buffers unmapped, and patches around it, but nothing reports the loss.
- The headless target `expect`s its map and poll (`headless.rs:147-148`). That affects tests only.

*Fix.*
1. Install `device.on_uncaptured_error` and a device-lost callback in `Gpu::from_adapter`. Record them in the
   worker's stats and message.
2. Add `FY_STATUS_DEVICE_LOST`.
3. On loss, have Dart (or `FynkelView`) free the view and make a new one. Recovery is nearly free here: a new
   view starts with an empty `Extract`, so it uploads every mesh and texture again from the world
   (`extract.rs:87-119`).
4. Check texture sizes against `device.limits().max_texture_dimension_2d` in the extract, and report a refused
   texture instead of panicking.
5. Add a test that injects a validation error and a lost device (wgpu exposes `Device::destroy`).

### 2. Rendering cost grows with every object, seen or not. **Medium**, S–M

*Evidence.*
- **No frustum culling.** The extract pushes every shown object with a mesh and material (`extract.rs:168-192`).
  `prepare` turns each into a 164-byte instance (`renderer.rs:497-504`), uploaded every frame
  (`frame_ring.rs:139-145`). The GPU transforms every vertex of everything.
- The extract runs on the **Dart UI thread, holding the world lock**. It costs 0.30 ms on the Mac and
  **2.0–2.6 ms on the Note10+** for 10,000 objects (`docs/stage-2-report.md:63`), which is about 15% of a
  60 Hz frame before any game code.
- The batching itself is good. Draws sort by (cut-out, faces, textures, mesh), and material values are
  per instance, so untextured materials still batch (`renderer.rs:459-505`).
- The sort recomputes a 3×3 determinant inside the sort key on every comparison (`renderer.rs:465-469`). That
  is O(n log n) determinants per frame, and `normal_matrix` computes it again (`renderer.rs:479`).

*Fix.*
1. Give each `MeshData` a bounding sphere, computed once at upload.
2. Cull against the camera frustum in `prepare` on the render thread, which costs the UI thread nothing.
   This is an S.
3. Precompute sort keys into the reused `order` buffer (S).
4. Later, if the scale check (step 5) shows the extract dominating on phones, make it incremental: dirty
   transforms only (M).

### 3. The renderer is one monolithic forward pass, and the next features need passes. **Medium**, L (staged)

*Evidence.*
- `Renderer::encode` records exactly one render pass (`renderer.rs:540-584`) into fixed MSAA targets.
- `FrameUniforms` holds one sun and an ambient term (`renderer.rs:37-43`).
- There are 6 pipelines: opaque or cut-out, times three face modes. No blended or transparent pipeline exists
  (`renderer.rs:229-284`).
- The extract **silently ignores** every directional light but the lowest-id one (`extract.rs:149-161`).
  `FY_LIGHT_*` has only directional and ambient (`abi.rs:102-105`).
- No shadows exist. The stage-5 comparison scene had Unity's sun shadows; Fynkel's didn't (`docs/stage-5.md:713`).

*Fix.* Before stage 7 adds features, split `Renderer` into:
- shared frame data: the camera, a light storage buffer, and the instance ring;
- an ordered list of passes: shadow depth (later), opaque, cut-out, transparent (sorted back to front, blended),
  and overlay (debug lines).

Each pass owns its pipelines. The extract should fill a light list rather than one sun, and warn once when it
drops lights.

This is mostly moving code: the pieces (`FrameRing`, `Textures`, batching) already separate cleanly.

### 4. One GPU device, renderer and upload set per view. **Medium**, M

*Evidence.*
- `new_view` calls `Gpu::headless()` for every view (`abi.rs:1934`). That means a new wgpu instance, adapter
  and device, 7 pipelines plus a mipmapper (`renderer.rs:185-358`), a render thread, and its own uploads of
  every mesh and texture (`Extract` is per view, `abi.rs:408-413`).
- Two views of one world hold every texture twice on the GPU.
- Making a view blocks the calling (UI) thread on adapter and device creation and shader compiles
  (`pollster::block_on`, `gpu.rs:42,53`). Every hot restart pays this again.
- Stage 7 plans "several views".

*Fix.*
1. Use one process-wide `Gpu`, made lazily and kept across hot restarts. This also fits the existing
   render-thread reuse (`worker.rs:180-249`).
2. Keep one GPU resource set per world, shared by its views. The per-view `Renderer` keeps only its targets
   and frame ring.
3. Compile pipelines once per format.
4. Measure how long view creation takes today on the Note10+ before deciding urgency.

### 5. The physics step thread aliases `&mut` across threads. **Medium**, S

*Evidence.*
- `fp_step_begin` gives the worker raw `*mut Physics` and `*mut Vec<Moved>` into the `FpWorld`
  (`fynkel_physics_ffi/src/lib.rs:1099-1107,1164`).
- While the step runs, `with_world` (`lib.rs:585-600`) and `fp_step_end` (`lib.rs:1188`) both create
  `&mut FpWorld` over the same struct. That is undefined behaviour under Rust's aliasing rules (Miri would flag
  it), even though `stepping` stops the body touching `physics`.
- It is benign today, but nothing guarantees that: more inlining (LTO, finding 8) gives the optimiser more
  room to exploit it.

*Fix.* Hold `physics: Option<Box<Physics>>` (and the moved list) and *move* them to the worker through the
channel, and back in `fp_step_end`. This needs no raw pointers and no `unsafe impl Send`, and "a step is
running" becomes `physics.is_none()`.

### 6. The two C interfaces duplicate code and differ in their conventions. **Low–medium**, S–M

*Evidence of duplication.* `Error`, `guard`, `LAST_ERROR`, `panic_message`, `write`/`read` and the cbindgen
header test exist twice: `fynkel_ffi/src/error.rs` + `worker.rs:334` versus `fynkel_physics_ffi/src/lib.rs:524-578`.

*Where the conventions differ:*

| | engine (`fy_`) | physics (`fp_`) |
|---|---|---|
| Thread model | `*const FyWorld` with an internal `Mutex`, callable from any thread (`abi.rs:357-374`) | `*mut FpWorld`, one thread at a time, caller-enforced (`lib.rs:10`) |
| Struct arguments | by pointer (`*const FyTransform`) | by value (`FpVec3 gravity`, `FpColliderSpec spec`, `lib.rs:909,1285`) |
| Flags | `bool` (`FyCamera.orthographic`, `FyMaterial.double_sided`) | `u32` 0/1 (`FpColliderSpec.is_trigger`, `FpBodySpec.kinematic`) |
| Status numbers | NullPointer=7, InvalidValue=6, Panic=12 | NullPointer=4, InvalidValue=3, Panic=6 |

- `FpTransform` claims "the same layout, so one buffer serves both libraries" (`lib.rs:100-108`), but no test
  checks that against `FyTransform`.

*Fix.*
- Create a small `fynkel_cabi` crate (guard, last error, null/length helpers, `utf8`, `write_all`, header-test
  helper) and link it statically into each library. Each library keeps its own thread-local, so nothing is
  shared at run time.
- Give the generic statuses the same numbers in both (Ok, NullPointer, InvalidValue, WrongSize, Panic,
  Poisoned), and start library-specific statuses at 100.
- Write down one convention for structs and flags, and apply it to new functions.
- Add a size-and-offset assertion test for the shared transform layout.

Audio (finding 11) would be the third copy, so do this first.

### 7. Thread safety, locks and frame flow: correct; two things to watch. **Low**

*How the frame flows today.*
1. A Flutter `Ticker` calls `world.update`, then `fy_view_frame` on the UI isolate's thread.
2. The extract runs under the world lock and posts to a newest-wins mailbox (`worker.rs:124-138`).
3. The render thread applies resource changes, then `prepare` and `encode` into the target.
4. On Apple, a marker command buffer's completion handler marks the ring slot Ready and calls the Swift
   callback (`apple.rs:156-168,190-217`). The raster thread then takes the slot with `fy_apple_acquire`.
5. On Android, the frame goes to `queue.present` on a Fifo surface (`android.rs:280-287`).
6. Physics steps on its own thread between frames, with `fp_step_begin`/`fp_step_end`.

*Lock orders are consistent, and I found no deadlock path:*
- view: clock → world → mailbox;
- raycast: world → raycaster;
- Apple: surfaces → ring, and callback lock alone.

*Back-pressure is right.* On Apple, at most 2 frames are in flight and at most 2 are queued, and older frames
are dropped (`ring.rs:36-43`). The mailbox coalesces, and scene buffers circulate, so a steady frame allocates
nothing.

*Watch items:*
- **(a) No engine-level pause.** On iOS, a scene already in the mailbox can be submitted after the app goes to
  the background. Metal rejects background GPU work: the frame comes back as an error, and that is harmless
  today. Stage 7 should add `fy_view_pause`/`resume`: stop taking scenes and wait for in-flight frames. S.
- **(b) Android surface swaps wait for the frame in progress.** `attach` and `detach` on the platform thread
  wait for the in-flight frame's lease (`handoff.rs:40-57`). That frame may itself be blocked in a Fifo
  `get_current_texture` (`android.rs:254`). This is bounded by a vsync or wgpu's acquire timeout, but worth a
  log line if it ever exceeds ~100 ms.
- **Finalizers block.** `NativeFinalizer`s call `fy_view_free` (it waits for the frame being drawn,
  `worker.rs:170-177`) and `fp_world_free` (it joins the physics thread). Blocking in a finalizer is allowed,
  but stalls whichever thread runs it. Platform views correctly opt out (`finalize:` in `view.dart:73`).
- **A few unguarded entry points.** `fy_world_free`, `fy_view_free`, `fy_mesh_free` and `fp_world_free` don't
  go through `guard`. A panic in a `Drop` there aborts the process (Rust ≥ 1.81 aborts on unwinding out of
  `extern "C"`). That is unlikely; accept it, or wrap them in `catch_unwind` (S).

### 8. Binary size and the release profile. **Low–medium**, S

There is no `[profile.release]`. The build hook always builds `--release` (`packages/fynkel/lib/hooks.dart:47`).

Measured on macOS arm64:

| | engine dylib | physics dylib | clean build |
|---|---|---|---|
| default | 7.2 MB | 3.2 MB | 70 s |
| `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"` | **5.0 MB (−31%)** | **2.5 MB (−23%)** | 91 s |

- Android ships the `.so` as built, so it gets the full benefit. iOS links the 44 MB staticlib into the app,
  where the linker strips some of it anyway.
- `fynkel_ffi` builds `rlib + staticlib + cdylib` (`Cargo.toml [lib]`), which roughly triples its link step.

*Fix.*
- Add a release profile with thin or fat LTO, one codegen unit and `strip`.
- Consider limiting the crate types per target in the hook (`cargo rustc --crate-type`).

### 9. Memory: CPU copies of every texture and mesh live as long as the world. **Low–medium**, M

*Evidence.*
- `World` keeps `Arc<ImageData>` (RGBA8) and `Arc<MeshData>` for every resource (`world.rs:102-107`), after
  the GPU has its own copy with mips (`texture.rs:217-240`).
- On a phone, a game with 150 MB of textures holds about 150 MB of CPU memory on top of the GPU's.
- There are no compressed GPU formats (Fynkel 1 had ASTC/ETC2).
- Every upload runs, with its mipmap pass, before the next frame (`worker.rs:285-298`), so loading a level with
  many textures hitches one frame.

*Fix.*
- Short term: a per-texture "keep CPU copy" flag (Unity's Read/Write), off by default, with the extract dropping
  pixels once every view has uploaded them. Raycasts still need mesh positions.
- A per-frame upload budget.
- Later: KTX2 with ASTC or ETC2. The step-5 30-minute memory runs will show whether this is urgent.

### 10. Crate layering: the engine's runtime lives in the C-interface crate. **Low–medium**, M

*Evidence.*
- `fynkel_ffi` holds the ABI (2,286 lines) *and* the extract (world → scene) *and* the render worker
  (`fynkel_ffi/src/lib.rs:9-14`).
- `fynkel_world` depends on `fynkel_load` (the glTF and image decoders) to own `ImageData` and `ModelData`
  (`fynkel_world/src/lib.rs:22`, `world.rs:10`).
- There are no cycles, and the rest is clean: `render` knows nothing of the world, and `present` knows nothing
  of the renderer.

*Fix.* When animation lands, move `extract` + `worker` + view into a `fynkel_engine` crate that Rust tests and a
future skinning or animation crate can use without the C layer. Move `ImageData` and `ModelData` into a
data-only crate (or `fynkel_mesh`), so the world doesn't pull in decoders.

### 11. Readiness for what's next. **Medium** (planning, not bugs)

| Feature | Does the structure leave room? | What needs reshaping |
|---|---|---|
| **Audio** | Yes. Copy physics: its own crate and C ABI, a Dart package that plugs in through `extend.dart`, positions read with `fy_world_transform_get_many` (`abi.rs:872`). | Do finding 6 first, or it becomes the third copy of the boilerplate. |
| **Animation / skinning** | No; it has to touch the engine. | The loader skips skins and animations (`fynkel_load/src/model.rs:270-274`). `Vertex` is fixed at 48 bytes (`fynkel_mesh/src/lib.rs:26-36`), so joints and weights need a second vertex stream or vertex kind. Instances need a joint-palette offset into a storage buffer, and the instancing key changes. The extract needs a new component. Clip sampling for many characters belongs in Rust: a `fynkel_anim` crate run before the extract. |
| **Particles** | Partly: instancing and the frame ring fit CPU-simulated particles. | Needs a blended, sorted transparent pass (finding 3). |
| **2D** | Orthographic cameras exist. | Needs sprites (quad mesh + atlas UVs, already expressible), a transparent pass and sorting layers. For UI-like 2D, Flutter may be the better tool. |
| **More lights / shadows** | No. | One sun in the uniforms and an extract that picks one (finding 3). Needs a light storage buffer, a shadow-depth pass before the forward pass, and new `FY_LIGHT_*` kinds with an ABI bump. |

### 12. Tests and tooling gaps. **Low**, S each

*What's covered well:* the ring's state table and invariants (`ring.rs:283-293`, debug builds), the handoff,
the worker's lifecycle and panics, header drift, zero allocation in a steady frame (`fynkel_ffi/tests/alloc.rs`,
`fynkel_render/tests/alloc.rs`), golden images, GPU memory accounting, and physics behaviour (113 tests).

*Gaps:*
- No test for GPU errors or a lost device (finding 1).
- The Android tests compile to 0 tests on the host and run only on a device.
- No fuzzing of the untrusted-bytes parsers (`fy_model_load`, `fy_texture_decode`). Panics are caught, but an
  out-of-memory abort is not.
- No Miri run. The pure crates (world, ring, handoff, physics FFI) could run under Miri, and it would have
  caught finding 5.
- The ABI number is bumped by hand. The header test catches header drift, but not a missed bump.
- `three_threads_and_a_million_frames` takes **62 s** in a debug build, about 40% of `cargo test`'s 158 s.

*Fix.*
- `cargo fuzz` targets for model and image decoding.
- A Miri CI job for those crates.
- A test that pins (ABI version, header hash), so a header change without a bump fails.
- Run the stress test in release, or with fewer frames in debug.

### Also noted (low, no action needed now)

- **glam is built twice** (0.33 for Rapier, 0.34 for the engine). They are separate libraries, so this costs
  build time only.
- **Unused bind groups pile up.** Bind groups for texture sets no material uses any more are only dropped when
  one of their textures goes (`texture.rs:263-265`). That is slow growth when materials change textures often.
- **Instance buffers never shrink.** They grow to the next power of two (`frame_ring.rs:136-138`). Fine.
- **The last error is per thread.** `fy_last_error` and `fp_last_error` are thread-local. That is correct for
  Dart, because a synchronous call and its error read stay on one thread.

## Keep: this is good

- **The boundary discipline:** `guard()` on every fallible call, a status plus a readable message, `Poisoned`
  after a panic mid-change, null and length checks, `SAFETY` comments, and `unsafe` denied everywhere else.
  The error messages say what to do (e.g. `abi.rs:289`, `world.rs:62-77`).
- **The handle lifetimes:** a view holds its world's `Arc`, so they can be freed in either order
  (`abi.rs:8-11,1925-1931`).
- **Drift caught three ways:** the header test, CI's ffigen check, and Dart's runtime ABI check.
- **The frame pipeline:** a newest-wins mailbox with recycled scene buffers (no allocation in a steady frame);
  render threads reused across views (`worker.rs:14-19`) so driver per-thread state doesn't leak on hot restart;
  per-frame data written into mapped buffers instead of blits (`frame_ring.rs:1-15`).
- **The Apple ring:** a checked transition table, invariant checks in debug, generation-stamped tickets across
  resizes, and IOSurfaces with no copy.
- **Android's `Handoff`:** a surface is never released mid-frame.
- **Instancing keyed by (pipeline, textures, mesh)** with materials per instance: few draw calls without a
  material system.
- **Physics as a separate crate, library and ABI:** proof that packages can be added without touching the engine.
- **Validation in one place** (`World::check` via `Describe`), and all-or-nothing batch writes
  (`insert_all`, `world.rs:205-219`).

## Top 5 recommendations, ranked

1. **Handle GPU errors and device loss** (finding 1, M). Install the uncaptured-error and device-lost handlers,
   add a `DEVICE_LOST` status, and let `FynkelView` remake a lost view; re-upload is automatic. Check texture
   sizes against the device's real limits. Today, one GPU out-of-memory or driver reset freezes the game for good.
2. **Restructure the renderer into passes, with a light buffer** (finding 3, L, staged). Do it before animation,
   particles or shadows. Warn when lights are dropped. Plan the vertex-stream and joint-palette design for
   skinning at the same time (finding 11).
3. **Frustum culling and cached sort keys** (finding 2, S–M). Cheap, on the render thread, and it directly helps
   step 5's 10,000-object scale check. Then measure whether the 2–2.6 ms extract on the Note10+ needs to become
   incremental.
4. **Share one GPU device and one resource set per world across views** (finding 4, M). Stage 7's "several
   views" otherwise duplicates every texture and pays device creation on the UI thread. Measure view creation
   on the Note10+ first.
5. **Clean up the C-interface foundation before audio** (findings 5 + 6 + 8, S–M).
   - Fix the physics step's aliasing by moving `Box<Physics>` to the worker.
   - Extract the shared `fynkel_cabi` crate, with the same numbers for the generic statuses and a layout test
     for the shared transform.
   - Add a release profile: LTO plus strip makes the engine library 31% smaller.

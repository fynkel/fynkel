# Bridge and view audit (stage 14) — reader: bridge

Scope: crates/fynkel_ffi (abi, worker, extract, device, jni, error), crates/fynkel_present, crates/fynkel_cabi,
packages/fynkel_flutter (Dart, Kotlin, Swift), packages/fynkel/tool/ffigen.dart's blocking list.
Hardening phase 1 (V1–V12) fixes were rechecked and still hold. The one exception is V11, which holds only partly (see BR2).

---

## BR1 — View size is never checked against the device's texture limit
- severity: bug
- size: S–M
- where:
  - crates/fynkel_ffi/src/abi.rs:5150–5199 (new_view checks only width/height != 0)
  - abi.rs:5474–5483 (fy_view_resize checks only non-zero)
  - crates/fynkel_render/src/gpu.rs:70 (`required_limits: Limits::default().or_worse_values_from(adapter)`, so max_texture_dimension_2d <= 8192)
  - packages/fynkel_flutter/lib/src/fynkel_view.dart:739–740 (Dart clamps to 16384)
  - fynkel_view.dart:228 (resolutionScale <= 4)
  - packages/fynkel/lib/src/world.dart:502 (renderScale <= 2)
- how it shows:
  - Any physical size times resolutionScale (and renderScale for the inner targets) that goes over 8192 passes both checks. Examples: a 4K Mac window at resolutionScale 2+, or a tablet at scale 4.
  - On resize, the worker creates oversize textures. That is a validation error inside the worker's scope, so the view gets WorkerFailed, and _lost retries up to 3 times a minute. Then the ErrorWidget stays for good.
  - At creation, Headless::new and Apple::new (make_slots, apple.rs:364–378) create textures on the Dart thread outside any error scope. The error reaches the uncaptured handler (device.rs:50–57), which loses the device for every view and every world.
  - Uncertain: whether Apple's from_hal path validates the size before Metal does.
- first fix idea:
  - In new_view and fy_view_resize, check width/height (times the renderer's scale) against `device.limits().max_texture_dimension_2d` and return FyStatus::InvalidValue.
  - Expose the limit (fy_device_max_texture or similar) and have Dart clamp to it instead of 16384.
- test that would fail today: `fy_view_new_headless(world, 9000, 100, ..)` should return InvalidValue. Today it returns Ok, and the device is reported lost on the next frame (or the creation call itself trips the uncaptured handler).

## BR2 — GPU work off the render thread escapes V11's error scopes; one bad pipeline loses every view
- severity: bug
- size: M
- where:
  - crates/fynkel_ffi/src/worker.rs:570–579 (scoped() wraps only the render thread)
  - crates/fynkel_render/src/renderer.rs:2275–2302 (the ready() "fynkel shader" thread calls own()/own_shadow/own_depth, i.e. create_shader_module/create_render_pipeline, with no push_error_scope)
  - new_view's target creation on the Dart thread (abi.rs:5150–5199, apple.rs:364–378)
  - device.rs:50–57 (an uncaptured error means lose())
- how it shows:
  - Error scopes are per thread. A validation error from a pipeline compiled on the shader thread therefore reaches the uncaptured handler, and Device::lose() fails every view in the process.
  - The next device (device.rs:36–44) compiles the same material's pipeline again, so the loss repeats until _maxLosses (3 a minute), and then every FynkelView shows the ErrorWidget.
  - The window is narrower than it sounds. CustomShader::new (shader.rs:545–548) already runs naga validation, so what remains is backend translation and device-limit errors (bind counts, the limits from or_worse_values_from), plus view targets (BR1).
- first fix idea:
  - Push/pop an error scope around each compile on the shader thread. On error, mark that shader failed and draw with the fallback material instead of losing the device.
  - Do the same around target creation in new_view, returning an error status for that one view.
- test that would fail today: a headless test that makes a custom shader whose pipeline goes over a device limit (for example more texture bindings than max_sampled_textures_per_shader_stage) and draws a frame. Expect only that material to fail. Today the device is lost (fy_view_frame then returns DeviceLost for an unrelated second view).

## BR3 — The blocking-call list in ffigen.dart is stale; long leaf calls stall the isolate group's GC
- severity: bug
- size: S
- where:
  - packages/fynkel/tool/ffigen.dart:14–29 (the _blocking list)
  - missing from it:
    - fy_sky_image_decode (abi.rs:1357; its doc says "tens of milliseconds"; called from Isolate.run at packages/fynkel/lib/src/sky.dart:151)
    - fy_shaders_waiting (abi.rs:1263–1270; calls Device::current, which can create a device after a loss; called from world.dart:203)
    - fy_shader_new (abi.rs:3188)
    - the stage-13 builders: fy_mesh_extrude 2430, fy_mesh_lathe 2463, fy_mesh_combine 2497, fy_mesh_sweep 2574, fy_mesh_set_sweep 2612, fy_mesh_recalculate 2778 (tangents and normals over every triangle; the same reason fy_mesh_new is already listed)
- how it shows:
  - A leaf call cannot reach a safepoint, and a GC needs every mutator in the group at a safepoint. Running the decode in Isolate.run therefore does not keep it off the UI isolate: a GC started in either isolate waits for the whole decode.
  - fy_shaders_waiting after a device loss creates a headless device (adapter plus device request, hundreds of ms) inside a leaf call. This is the same class as V3.
  - A large sweep or recalculate (a 100 m road is ~1 ms on the Note10+, and larger meshes scale linearly) blocks the same way.
- first fix idea:
  - Add these names to _blocking.
  - Better: mark blocking calls in abi.rs doc comments (for example `/// Blocking.`) and have ffigen.dart read that marker, with a test that the generated bindings' isLeaf flags match the marker.
- test that would fail today: a Dart test that loads the generated bindings and asserts that every function whose Rust doc mentions decode, compile, a device, or "milliseconds" is generated with isLeaf: false. fy_sky_image_decode fails it.

## BR4 — Handoff's Lease::drop wakes the taker before the value's Arc is released
- severity: bug (low impact)
- size: S
- where:
  - crates/fynkel_present/src/handoff.rs:97–102 (Drop decrements leases and calls notify; the `value: Arc<T>` field drops only after Drop::drop returns)
  - crates/fynkel_present/src/layer.rs:173 (`shared.layer.take().and_then(Arc::into_inner)`)
- how it shows:
  - The main thread in take() wakes as soon as leases reaches 0. It can then run Arc::into_inner while the render thread still holds the lease's clone. into_inner returns None, and attach rebuilds the surface (creates a new one) instead of reconfiguring the old one.
  - The old Live is then dropped on the render thread, not the main thread. CAMetalLayer and surface teardown off the main thread is what the layer code is written to avoid.
  - The window is a few instructions wide, so it is rare.
- first fix idea: hold the value in an Option (or ManuallyDrop) and drop/take it inside Drop::drop before decrementing and notifying.
- test that would fail today: a loop of 100k iterations, with thread A leasing and dropping and thread B calling take(), asserting that Arc::into_inner(taken) is always Some. It fails intermittently today.

## BR5 — The iOS layer target holds its lease across nextDrawable and the whole frame
- severity: slow
- size: S–M
- where:
  - crates/fynkel_present/src/layer.rs:119–150 (begin() takes the lease, calls get_current_texture, which blocks up to 1 s when drawables are starved, and keeps the lease until finish)
  - layer.rs:169–213 (attach/detach on the main thread wait in Handoff::take)
  - compare android.rs:332 (the presenter checks wanted() and drops its lease early; the V10 fix)
- how it shows:
  - On iOS, Flutter's UI and platform threads are merged, so Dart runs on the main thread. A rotation, a place change, or an unregister calls attach/detach on the main thread, which then waits for the render thread's whole frame.
  - That costs up to one engine frame of UI stall, or up to 1 s when the drawable pool is empty (a backgrounded or covered layer).
- first fix idea: check a wanted() flag (as Android does) before get_current_texture and between encode and present, and give the lease back when the main thread is waiting. Or take the drawable under a short lease, render without it, and re-lease only for present.
- test that would fail today: a fynkel_present unit test with a fake surface whose acquire sleeps 200 ms, asserting that take() from another thread returns in under 20 ms. Today it waits the full 200 ms.

## BR6 — Extract rescans every mesh, material and texture with hash inserts every frame, per view
- severity: slow
- size: S
- where: crates/fynkel_ffi/src/extract.rs:189–215 (mesh scan with a HashMap insert per mesh), 218–238 (texture/shader retain), 243–270 (material loop: pbr() plus HashSet inserts per slot)
- how it shows:
  - The cost is O(meshes + materials + slots) hashing on the Dart thread every frame, even when nothing changed, and repeated for each FynkelView of the same world (a minimap, a CameraView).
  - With hundreds of materials this is tenths of a ms on a Note10+, out of a 16 ms budget that already runs close.
  - Uncertain: I did not measure it. This size estimate comes from the loop shape.
- first fix idea: keep world-side generation counters (meshes/materials/textures), and skip or reuse the previous extract's sets when unchanged. Share one extract per world per frame across its views.
- test that would fail today: an allocation/time harness case with 500 static materials and two views, asserting the per-frame extract does no hash inserts after the first frame (count via a debug counter).

## BR7 — World widgets and on-screen placement allocate per widget per frame
- severity: slow
- size: S
- where:
  - packages/fynkel_flutter/lib/src/world_widget.dart:265–304 (_place: w.point, `- shake`, worldToScreenPoint, `(point - eye)`; each makes a new Vec3/ViewPoint)
  - world_widget.dart:332–336 (a new Matrix4 per scaled child)
  - fynkel_view.dart:897–940 (_sendPlace builds its Rect/list every frame)
- how it shows: 4–6 short-lived objects per world widget per frame. With 100 name tags that is ~500 allocations a frame, which goes against the hardening allocation budget (see the alloc harness). It shows up as more young-gen GCs, not as a crash.
- first fix idea: write into scratch Vec3/Matrix4 fields on the element, and use in-place math (setFrom/sub into scratch). Skip _sendPlace when the rect is unchanged (some of this exists through _sentPlace; extend it to the build step).
- test that would fail today: the existing allocation harness with a WorldWidget scene of 100 widgets, asserting per-frame allocations under the current budget.

## BR8 — ABI seams: the float-by-pointer convention and other drift across ABI 30→44
- severity: awkward (seam)
- size: S
- where:
  - abi.rs:3362 (fy_world_time_set takes f32 by value and is called every frame from world.dart:754) vs fy_world_animate 4113 and fy_world_particles_step 4962, which take dt through a pointer
  - `*const f32` means "optional" in fy_animator_speed_set/stop, but "required" in the dt calls
  - bool flags are C bools in view/particle calls but u32 flags on the physics side
  - Worker::post's `ops` argument is always empty in production (abi.rs:5388)
- how it shows: each convention was right in its own stage, but together a reader can't predict a signature. A null dt pointer gets InvalidArgument in one call and "use default" in another. The dead `ops` path still costs a branch and a Vec per post.
- first fix idea: write the convention down at the top of abi.rs (scalars by value; pointers only for optional or out values), and fold the outliers in at the next ABI bump. Drop `ops` from post, or wire it up.
- test that would fail today: none behavioural. A doc-lint test that flags `*const f32` params not documented as optional would list the dt calls.

## BR9 — Game code can't see a view's failure, and a failure is permanent
- severity: awkward
- size: S
- where: packages/fynkel_flutter/lib/src/fynkel_view.dart:951–962 (_fail swaps in ErrorWidget), 969 (_lost), 1033
- how it shows:
  - FynkelView has no onError/status callback. When a view fails (BR1's size, a device loss past _maxLosses, a Kotlin "failed"), the game sees a grey box in release with no hook to log it, report it, fall back, or retry.
  - Once _fail runs, the state never tries again, even if the cause goes away (for example the window shrinks back under the limit).
- first fix idea: add `onError: void Function(FynkelViewError)` and an errorBuilder. Clear the failed state when the size or the world changes, so a fresh _open is tried.
- test that would fail today: a widget test that pumps a FynkelView with a fake binding returning InvalidValue from view creation, then a smaller size; expect the second size to open a view. Today the ErrorWidget stays.

## BR10 — Rewrite: split abi.rs (5730 lines) into abi/ submodules
- severity: rewrite
- size: M
- where: crates/fynkel_ffi/src/abi.rs (types 1–660; handles 667–801; world 818–1160; settings 1160–1520; names/transforms/raycast 1520–1760; components 1760–1960; meshes 1961–3060; materials/shaders 3060–3380; textures 3381–3530; models 3532–3815; animator 3818–4160; particles 4164–5025; trails 5028–5140; views 5141–5730)
- how it shows:
  - Each stage appended a section, and its FFI types sit in the shared 660-line header, far from the functions that use them.
  - BR3 and BR8 are partly a result of this: no one sees the whole convention at once.
  - Merge conflicts between stage branches land in one file.
- first fix idea:
  - Create abi/{mod (guard, handles, helpers), objects, components, settings (quality/sky/fog/effects), meshes, materials (+shaders), assets (textures+models), animation, particles (+trails), views (+apple/android)}, with each module's repr(C) types and From impls moved beside its functions.
  - cbindgen follows `mod`, so the header is unchanged. Check by diffing the generated header and the fingerprint.
- test that would fail today: n/a (a refactor). The guard is a byte-identical generated header plus the ABI fingerprint test.

## BR11 — Rewrite: FynkelViewState interleaves four platform host protocols with flags
- severity: rewrite
- size: M
- where: packages/fynkel_flutter/lib/src/fynkel_view.dart:759–1028 (_open/_register/_sendSize/_sendPlace/_release with _surfaced, _noLayer, _sentWidth/_sentHeight, _onScreen, _sentPlace, _sentOver)
- how it shows:
  - The state class handles texture (macOS/iOS fallback), iOS layer, Android SurfaceProducer and Android SurfaceView with one set of booleans. Each lifecycle path (lost, resize, release, fail) has to get every flag right for all four.
  - Most V-items in hardening phase 1 were in exactly these transitions.
- first fix idea: give each platform its own `ViewHost` strategy (open/resize/place/release/lost) chosen once in _open, and keep the state class for Flutter lifecycle and frame pacing only.
- test that would fail today: n/a (a refactor). Widget tests should be run per host with fake channels.

---

## Looked at and fine
- Pointer/length checks on every slice entry point: items, utf8, write_all, loops, combine, transform_get_many/set_many, camera_effects, debug_lines, mesh_read*.
- Every extern fn runs inside guard() (catch_unwind into FyStatus). fy_apple_acquire has its own catch_unwind. JNI entry points catch. Panic strategy is unwind (no panic=abort in the profiles).
- Hardening V1–V12 hold (V11 only partly; see BR2):
  - post() returns the scene to spare on failure (V6)
  - the Apple finished callback wakes the worker (V5)
  - Kotlin retries attach twice, then reports "failed" (V4)
  - the Android presenter checks wanted() (V10)
- Worker: newest-wins mailbox, on_free retry and skip accounting, take_within timing, RenderThreads idle reuse (MAX_IDLE=1).
- Device-lost recovery builds a fresh WorldGpu. The uploading try_lock skips rather than blocks.
- FyMesh::own spare reuse is sound. Headless read handles a size mismatch. Textures shrink to the device limit (unlike views; see BR1).
- Camera.pixels is clamped to the view's height in the renderer (renderer.rs:978).
- Apple ring: SLOTS=5 with MAX_IN_FLIGHT=2, ring tests, and resize keeps the last surface until a new one is ready.
- Swift texture: the NSLock-guarded acquire. Release order (setFrameReady(nil), then detach, then unregister) is correct, and set_frame_ready waits for a call already in progress.
- Kotlin plugin: release order, the OnScreen SurfaceView lifecycle, and layer ids from 1<<40 (no clash with texture ids).
- detachFromEngine's releaseOrphans is safe because the platform and UI threads are merged on mobile.

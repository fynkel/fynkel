# Stage 6 step 3: the architecture review

The code itself, reviewed at main `8940ebec` (2026-10-12) by three agents:
one on the Dart packages (`fynkel`, `fynkel_flutter`, `fynkel_physics`),
one on the Rust engine (the crates and both C interfaces), and one that
sketched an audio package on `extend.dart` to see whether a second package
plugs in without changing the core. Their full reports, with file and line
for each claim and the numbers they measured, are in
[stage-6-architecture/](stage-6-architecture/) (`dart.md`, `rust.md`,
`audio.md`). I checked the claims that drive the fixes in the code before
writing them here: the view's unguarded tick and its `_fail` stopping the
ticker, `Physics._world` with no dispose check, the update loop's bound
not checked again after a dispose, no GPU error or device-lost handler in
the crates, the extract keeping one sun, the 8192 texture size against the
device's lowered limits, and the physics step's raw pointers.

**The verdict**: sound and small, with careful engineering where it
matters most. The frame is one documented function; one object's error
never stops it; nothing allocates in a steady frame, and tests hold it so;
every C call catches panics and returns a readable message; drift between
Rust and Dart is caught three ways; physics proved that a package can be
added without touching the engine, and audio can follow it. What needs
work falls in four groups:

- **Holes at the edges** where a game can freeze for good or write into
  freed memory: the view's ticker, a kept `Physics` after dispose, a GPU
  error or a lost device.
- **One structural decision that doesn't scale**: the hierarchy (parent,
  children, shown) lives only in the engine, so Dart asks for it again
  through FFI and rebuilds whole sets on any change. Pooling, reparenting
  and physics queries pay for it, and decision A2 (`position` = world)
  would make every child's position an FFI call.
- **Seams that the next packages need**: `extend.dart` lacks a hook after
  the updates, an order between systems, asset kinds, app pause and a
  moved log; the renderer is one pass with one sun.
- **Tidy-ups**: shared C-interface code, a release profile, invariants kept
  from outside `World`, CI gaps (the alloc-test flake among them).

Measured numbers are on the Mac (an M-series) unless said; the phones are
about 3–4× slower.

## 1. Holes at the edges (high; step 6, first)

1. **The view stops for good on any throw.** `FynkelView._tick`
   (fynkel_view.dart:289) has no guard, and Flutter's `Ticker` schedules
   the next tick only after the callback returns: one exception from
   `onFrame`, `world.update` or the view's frame, and the game is frozen
   with one line in the log. A failed plugin registration calls `_fail`,
   which stops the ticker on purpose. **This is finding 25**: in a game's
   widget test there is no plugin, so the first tick runs at dt 0, the
   registration fails, and the clock never moves (the walker's log, "HUD
   at 0.0 s"). The package's own tests hide it by mocking a channel only
   `src/` can reach.
   **Fix (S)**: guard the tick, report and keep ticking; without a plugin
   (a test), keep updating the world with no view; a public test seam
   (`package:fynkel_flutter/testing.dart`); a test that throws from
   `onFrame` and expects the next frame.
2. **`World.dispose()` from inside an update throws a `RangeError`**
   (world.dart:343: the update loop's bound isn't checked again), which in
   a view is hole 1. Calling `update` from inside an update isn't caught.
   **Fix (S)**: stop after each phase once disposed, and a dispose asked
   for mid-update finishes when the update ends; a re-entered `update`
   throws a `StateError`. When "dispose destroys the roots first" lands
   (API review B), the order is: destroy the roots (so colliders leave
   physics while it is alive), stop listeners, dispose the systems, free
   the engine. Today the systems go first.
3. **A kept `Physics` writes into freed memory after `World.dispose`.**
   `world.physics` checks, but `final physics = world.physics;` in a HUD's
   `State` doesn't: `physics.gravity = …` afterwards returns normally,
   having written through freed scratch and called the freed native world
   (confirmed by the agent). Physics' ~35 native buffers also have no
   finalizer, so a world dropped without `dispose` (a hot restart, a test
   without `addTearDown`) leaks them.
   **Fix (S)**: one `_live()` check before any native call, a `StateError`
   after dispose; the buffers in one arena with a `NativeFinalizer`; the
   rule written into `WorldSystem.dispose`'s doc for package authors.
4. **A GPU error or a lost device freezes the view for good.** No
   uncaptured-error or device-lost handler is installed, so wgpu's default
   panics on the render thread; the worker records the failure and every
   later frame returns `WorkerFailed`. Ways in: GPU memory running out on
   a phone; a texture over the device's real limit (the loader allows 8192
   "which every device meets", but `Gpu::from_adapter` takes the adapter's
   lower limits); an Android driver reset.
   **Fix (M)**: install both handlers; a `DEVICE_LOST` status; on loss
   `FynkelView` makes a new view, and since a new view uploads every mesh
   and texture from the world, recovery is nearly free; a texture over the
   limit is refused with a message, not a panic; a test that injects both
   (wgpu has `Device::destroy`).
5. **The physics step thread breaks Rust's aliasing rules.** The worker
   gets raw `*mut Physics` and `*mut Vec<Moved>`, while the calling thread
   can still make a `&mut FpWorld` over the same struct. Harmless today
   (a flag keeps them apart), but undefined behaviour that more inlining
   (LTO, §4) gives the optimiser room to exploit.
   **Fix (S)**: move the physics world to the worker and back through the
   channel; no raw pointers, no `unsafe impl Send`.

## 2. Scale (medium; step 6, before A2)

6. **Mirror the hierarchy in Dart.** Every `parent` read is an FFI call
   (140 ns); `ancestor<T>()` at depth 10 is 1.1 µs; physics walks
   ancestors through FFI for each moved collider after any spawn. Three
   cliffs come from it:
   - **Pooling**: the hidden set is thrown away and rebuilt, with an FFI
     call per node, on any add, destroy, show, hide or reparent. One
     toggle a frame costs +225 µs with 500 objects in the pool, +740 µs
     with 2,000. A8 (`world.all` skips hidden objects) puts that set
     behind every `world.all`.
   - **Reparenting** is O(moved × live): 10 reparents a frame among
     10,000 objects add 3.8 ms, a lost frame on a phone.
   - **A2**: with `position` meaning the world's, every child's position
     read or write would flush the unsent transforms and call the engine.

   **Fix (L)**: Dart keeps `parent`, the children, `activeInHierarchy`
   and a world stamp per object, changed only through `World` methods
   (Dart is the only writer, except a model's parts, read once when
   spawned). `parent`, `ancestor`, `getInParent` and `getInChildren`
   become pure Dart; the pooling and reparent costs go; A2's world
   position comes from a cached world transform refreshed by stamp. **Do
   it before A2's PR.** The engine stays the source of world transforms
   for drawing.
7. **A moved log for systems; physics follows only what moved.** After
   any transform is set, the next physics query visits every collider
   (90 µs with 5,000; the shooter paid 4.7 ms a frame, finding 9).
   Careful: the core's `World._moved` is not a moved list, as the API
   review assumed; it lists the objects reparented this frame, and the
   unsent list empties on any send.
   **Fix (M)**: rename `_moved` to `_reparented`; an append-only log of
   moved objects (transform, parent, layer) that each system reads from
   its own cursor (`movedSince` in `extend.dart`); physics keeps a map
   from an object to the colliders on and below it, so a log entry touches
   only those.
8. **Every object is drawn, seen or not.** No frustum culling; each shown
   object becomes a 164-byte instance uploaded every frame. The copy of
   the world into the renderer runs on the UI thread under the world's
   lock: 2.0–2.6 ms on the Note10+ at 10,000 objects (stage 2's
   numbers). The sort recomputes a determinant on every comparison.
   **Fix (S–M)**: a bounding sphere per mesh, culling in `prepare` on the
   render thread (costs the UI thread nothing), sort keys computed once.
   Step 5's 10,000-object check then says whether the copy itself must
   become incremental.
9. **Lists emptied and refilled each frame allocate.** Dart's VM gives
   back a list's storage on `clear()`, so the next frame grows it again:
   about log2(N) arrays a frame when N objects move. The alloc tests check
   idle frames only, so "a frame allocates nothing" holds there and not in
   a busy game. **Fix (S)**: a small fixed-storage bag for the per-frame
   lists, and an alloc test with 1,000 objects moving.

## 3. Seams for what comes next (medium; step 6 and stage 7)

10. **`extend.dart`, version 2.** The audio sketch plugged in with no
    core change for its main work: 3D sources follow their objects, the
    listener follows the camera, `time.scale = 0` pauses, hiding stops,
    destroying frees, 7 tests on a headless world. What it and the Dart
    review found missing, each with its smallest change:
    - **Asset kinds** a package can add (`AssetKind<T>`,
      `assets.load(name, kind)`): today `.wav` can't be preloaded, and a
      package copies the core's loading, sharing and freeing by itself.
      This also ties a clip to the object that loaded it, as textures are.
      About 40 lines in `assets.dart`.
    - **A hook between the updates and the late updates** (`afterUpdate`),
      where an animator belongs, as Unity's does.
    - **An order between systems**: today it's the order they were first
      touched, which may be inside a game's `onLoad`.
    - **`componentRemoved`**, and pointer targets a package can add
      (objects with a collider and no mesh can't be tapped today).
    - **App pause**: `World.applicationPaused` set by `FynkelView` from
      the app's lifecycle, a system hook, and `onApplicationPause` on
      objects, as Unity's (goes with stage 7's pause). And a constraint
      for stage 7's `world.paused`: systems still get their frame hooks
      while paused, or audio never learns to pause.
    - **Writing many transforms in one call** (`writeTransforms`, the
      mirror of `readWorldTransforms`): physics would use it now, and
      animation and particles need it.
    - **A hook that keeps throwing is turned off** after a few frames,
      reported once, instead of 60 errors a second.
    - Later, when a package lives outside this repository:
      `buildRustCrate(workspace:)`, and each hook watching only its own
      crate.

    When audio comes, the sketch recommends a Dart-only package over
    `flutter_soloud`, behind an `AudioBackend` interface (the sketch's
    code is kept in `/tmp/s6/audio-sketch`, not in the repository).
11. **One shared base for the two C interfaces** (`fy_` and `fp_`). The
    guard, the last error, the null and length checks and the header test
    exist twice; the status numbers differ (a null pointer is 7 in one, 4
    in the other), and so do flags (`bool` against `u32`) and how structs
    are passed. **Fix (S–M)**: a small `fynkel_cabi` crate linked into
    each library, the same numbers for the shared statuses, one written
    convention, a layout test for the transform both libraries share, and
    a test that fails when the header changes without an ABI bump. Before
    audio adds a third copy, if audio has native code at all.
12. **The renderer is one forward pass with one sun.** No shadows, no
    transparency, and every directional light after the first is dropped
    without a word. Animation (skinning needs a second vertex stream and a
    joint palette; the loader skips skins today), particles (a blended,
    sorted pass), more lights and shadows all need passes and a light
    buffer. **Fix (L, staged)**: shared frame data plus an ordered list of
    passes; a light list instead of one sun. Not for step 6: **the stage
    that brings animation or the look**. Step 6 only adds a one-time
    warning when lights are dropped.
13. **One GPU device per view.** Each view makes its own device and
    pipelines and uploads every texture again, blocking the UI thread when
    it is made (every hot restart too); two views of one world hold every
    texture twice. **Fix (M)**: one device per process, one resource set
    per world shared by its views. **Stage 7**, which plans several views;
    measure view creation on the Note10+ first.
14. **CPU copies of every texture and mesh live as long as the world**,
    on top of the GPU's, and a level's textures all upload, with mipmaps,
    in one frame. **Fix (M)**: a per-texture "keep the pixels" flag, off
    by default (Unity's Read/Write), and an upload budget per frame.
    **Step 5's** 30-minute memory runs decide how urgent; compressed
    formats later.
15. **The engine's runtime lives in the C-interface crate**, and the world
    crate depends on the decoders. **Fix (M)**: an engine crate under the
    C layer, and a data-only crate for images and models. With animation,
    not before.

## 4. Tidy-ups (low; step 6, as they fit)

16. **Invariants kept from outside `World`.** `game_object.dart` reaches
    into the world's private state 33 times, and the same bookkeeping on
    parent and show changes is repeated in four places. Keep the core as
    one library of `part` files, but move every hierarchy and visibility
    change into `World` methods (rides with item 6). The large files: the
    motion split is already decided (API review C); `input.dart` and
    `game_object.dart` are each one coherent family and stay.
17. **Duplicated logic**: three copies of the call loop (update, late,
    fixed), six `switch (owner)` copies for objects against behaviours,
    two copies of the screen-ray math (one goes with A6). And `Signal`
    knows the object model, and cancelling is O(n), so 500 enemies leaving
    one signal is O(n²); cancel by marking.
18. **A release profile**: LTO, one codegen unit and `strip` make the
    engine library 7.2 → 5.0 MB (−31%) and physics 3.2 → 2.5 MB (−23%), for
    a clean build of 91 s instead of 70.
19. **Native entry points that free things** (`fy_world_free`,
    `fy_view_free`, `fy_mesh_free`, `fp_world_free`) don't catch a panic,
    which aborts the process; and finalizers that block (freeing a view
    waits for its frame). Wrap the four; accept the blocking.
20. **The alloc-test flake.** About two `_List`s a frame, in three test
    files, one with no contacts at all, and with the fixed step too: not
    the contact code, not the physics thread. It reproduced 2 times in 18
    full runs, and not once in 32 with allocation tracing on, so its
    source is still unknown; it looks like a compiled state of the JIT,
    made likely by running the alloc tests alongside the rest. **Fix
    (S)**: the alloc tests in their own CI step, one at a time, with the
    agent's trace-on-failure helper so the next flake names its source;
    later an allocation check in AOT, which is what ships.
21. **Test gaps**: no test for items 1–4; no soak test (a headless
    100,000-frame run of the lab scenes reading memory would catch leaks
    on CI); no fuzzing of the model and image decoders; no Miri run (it
    would have caught item 5); `three_threads_and_a_million_frames` takes
    62 s of `cargo test`'s 158 s; the tumble example is only built.

## Keep: this is good

- **The frame in one place**, documented as it runs, in Unity's order.
- **Error isolation** without closures: one bug never stops the frame.
- **Allocation discipline as a CI gate**, and batched FFI (one call to
  send every moved transform).
- **The C boundary**: a guard on every fallible call, a status and a
  message that says what to do, null and length checks, a `SAFETY`
  comment on each unsafe block, unsafe code denied everywhere else.
- **Drift caught three ways**: the header test, CI's ffigen check, the ABI
  number checked when the library loads.
- **Native lifetimes**: finalizers that only queue, removal at the start
  of the next frame, a view holding its world so either can go first.
- **The frame pipeline**: a newest-wins mailbox with recycled buffers, the
  Apple ring with its checked state table, Android's surface handoff,
  render threads reused across hot restarts.
- **Instancing** keyed by pipeline, textures and mesh, with materials per
  instance: few draw calls without a material system.
- **Packages as crates, libraries and ABIs of their own**, and the
  headless `World` as the test harness that the fresh agents praised.

## What goes where

| When | Items |
|---|---|
| Step 6, first (holes) | 1, 2, 3, 4, 5 |
| Step 6, before A2 | 6, with 16 |
| Step 6 | 7, 8, 9, 10 (all but the outside-package hook), 11, 17, 18, 19, 20, 21; and 12's warning |
| Step 5 decides | 8's incremental copy, 14 |
| Stage 7 | 10's app pause with pause; 13 |
| With animation or the look | 12, 15 |

The API review's B list said physics would follow "the core's moved
list"; there isn't one yet, and item 7 makes it.

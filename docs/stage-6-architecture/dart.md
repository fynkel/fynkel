# Stage 6 step 3: architecture review, the Dart side

Scope: `packages/fynkel` (core), `packages/fynkel_flutter`, `packages/fynkel_physics`, at main 8940ebec
(2026-10-12). The API review (docs/stage-6-api.md) is taken as decided; this is about structure, the frame,
lifetimes, seams, robustness and scale. Numbers were measured in a copy (`/tmp/s6/archcopy`, release Rust,
`dart test` JIT on this Mac, M-series; phones are roughly 3-4x slower). The repository was not changed.

## Summary verdict

The Dart side is in good shape for its size. The frame is one function whose order is documented where it is
written (world.dart:253-394). One object's error never stops the frame. Allocation discipline is enforced by
tests. Native lifetimes use `NativeFinalizer`s plus a weak registry, and removal waits for the frame start.
Physics plugged in through `extend.dart` without the core knowing about it. The audio sketch
(/tmp/s6/audio-sketch/DESIGN.md) needed no core change for its frame work.

What doesn't scale traces back mostly to **one decision**: the hierarchy (parent, children, active) lives only
in the engine. So the Dart side asks for it again through FFI, and rebuilds whole sets whenever anything in it
changes. That decision is behind four costs:

- the hidden set is rebuilt on any add, destroy, show, hide or reparent: +225 µs/frame with 500 pooled objects
  (Mac);
- reparenting costs O(moved × live): +3.8 ms/frame for 10 reparents among 10,000 objects;
- physics walks ancestors through FFI;
- every physics query after any move walks every collider.

It will get worse with decision A2 (`position` = world), which turns every position read or write of a child
into an FFI call plus a transform flush.

There are also **robustness holes at the edges**:

- **The view freezes.** Any throw out of `FynkelView._tick`, or a plugin registration failure, stops the
  ticker for good. This is the likely cause of finding 25.
- **A stale `Physics` writes into freed memory.** Kept after `World.dispose`, it writes silently through
  freed native pointers. Confirmed.
- **Dispose inside `update` throws.** `World.dispose()` called from inside `update` throws a `RangeError`
  out of `update`. Confirmed.

`extend.dart` is a sound seam for "work on the frame". It is not yet a seam for what animation, particles or
picking would need: a hook after the updates, an order between systems, a moved list, writing transforms in
bulk, and pointer targets.

The CI flake (`_List` ≈ 2 per frame, in three different alloc tests) is not in the contact code. It comes from
the JIT's state and from running alloc tests concurrently with the rest of the suite. The fix is in how the
gate is run, plus a trace on failure (F15).

## Findings

Severity: **high** = can freeze or corrupt a game, or blocks a planned decision; **medium** = a scaling cliff
or design debt that will hurt in stage 7 or with a bigger game; **low** = tidy-up. Effort: S (< 1 day),
M (days), L (a week or more).

### F1 (high) The view's ticker dies for good on any throw or plugin failure; this is finding 25

**Evidence**

- `fynkel_view.dart:289-328` (`_tick`) has no guard: `onFrame`, `world.update`, `view.frame()`,
  `onViewStats` and `onStats` can all throw. `World.update` can throw from FFI `check`s in `_tidy`,
  `input._step`'s raycasts, `cameras._run` and `_debug._flush`, and see F4.
- Flutter's `Ticker._tick` (flutter/lib/src/scheduler/ticker.dart:272-285) only schedules the next tick after
  `_onTick` returns. One throw is reported once by the scheduler, and then the game is frozen silently.
- `_fail` (`fynkel_view.dart:396-407`) calls `_ticker.stop()` on purpose when `register`, `reset` or `resize`
  fails, and replaces the view with an `ErrorWidget`.

**Why the walker's test froze.** In a game's widget test there is no plugin. Tick 1 runs `world.update(0)`
(dt is 0 on the first tick, `fynkel_view.dart:293`). `_open()` then calls `reset` and `register`, which get a
`MissingPluginException`, and `_fail` stops the ticker. That fits the walker's log exactly (/tmp/s6/walker/LOG.md
§8: "clock didn't move", HUD at 0.0 s).

The package's own tests hide this: they mock the channel through `src/fynkel_view.dart`'s
`@visibleForTesting channel` (`test/fynkel_view_test.dart:127`), which a game can't reach without importing
`src/`.

**Fix (S)**

- Wrap the body of `_tick` in try/catch. Report through `FlutterError.reportError`, or into the error panel,
  and keep ticking.
- On a failure to register, keep updating the world and show the error in the panel instead of stopping. In a
  test (`MissingPluginException`), fall back to a `HeadlessView`, or to no view.
- Add a public test seam: `package:fynkel_flutter/testing.dart` with `mockFynkelPlugin()`, or document that
  plugin-less runs keep ticking.
- Add a widget test that throws from `onFrame` and expects the next frame to tick.

### F2 (high) A stale `Physics` after `World.dispose` writes into freed memory (native use-after-free)

**Evidence**

- `physics.dart:45-48`: the `_world` getter returns `_pointer` with no `_disposed` check.
- `dispose()` (`physics.dart:295-334`) frees the `FpWorld` (through `RawPhysics.dispose`) and every scratch
  `malloc` (`_vec`, `_setting`, `_hit`...).
- The `physics` getters go through `world.system`, which throws after dispose. But a reference kept in a
  field (`final physics = world.physics;`, which is natural in a HUD `State`, a manager or a test helper) is
  not checked.

Confirmed in the copy: after `world.dispose()`, `physics.gravity = ...` returns normally. It wrote through the
freed `_vec`, then called `fp_gravity_set` on the freed world. Queries are the same.

**Fix (S)**

- `_world` throws a `StateError('this world was disposed')` when `_disposed`.
- Every public member that writes scratch checks first (one `_live()` helper).
- Write it into `WorldSystem.dispose`'s doc as the rule for packages.

### F3 (high, scale) The hidden set is rebuilt on any hierarchy change, at a cost that grows with inactive objects

**Evidence**

- `world_loop.dart:278-288`: when `_shape` changed and any object is off, `_isHidden` throws `_hidden` away.
  It rebuilds it from `_subtreeIds` of every off object.
- `_subtreeIds` (`world_loop.dart:322-333`) recurses with one FFI `fy_children_get` per node, and allocates a
  `List` per node (`world_objects.dart:361-369`) and a `Set` per root.
- `_shape++` happens on every add, destroy, `active=`, `parent=` and load end (world_objects.dart:99, 214, 233,
  331; game_object.dart:441, 495).

Pooling, which finding 8 and decision A8 push games towards, is exactly "many objects off, one toggled per
frame". Measured, one `active` toggle per frame (Mac):

| Objects in the pool | Steady frame | Frame with one toggle |
|---:|---:|---:|
| 100 | 66 µs | 125 µs |
| 500 | 75 µs | 299 µs |
| 2,000 | 107 µs | 845 µs |

A8 (`all/first/only` leave hidden objects out) will also put `_isHidden` behind every `world.all` call.

**Fix:** see F5 (a Dart mirror with a cached `activeInHierarchy` flag per object, updated by walking only the
changed subtree in Dart). As a stopgap (S): update `_hidden` incrementally. Turning X off adds X's subtree;
turning X on removes it unless another off ancestor covers it. And bump `_shape` only for structural changes.

### F4 (medium) `World.dispose()` during `update`, and re-entrant `update`

**Evidence**

- `world.dart:343-345`: the update loop's bound `updated` is not checked again against `_live.length`.
  (The late-update loop at 368 is checked, and the fixed loop at world_loop.dart:207 is checked.)
- `dispose` clears `_live` (world.dart:119).
- Confirmed: an `onUpdate` that calls `world.dispose()` makes `update` throw
  `RangeError (length): Valid value range is empty` (world.dart:345). In a view, that is F1's freeze.
- Calling `world.update` from inside an `onUpdate`, or from a `WorldSystem` hook, isn't guarded either.

**Fix (S)**

- `if (isDisposed) return;` after each phase, and `i < _live.length` in the update loop.
- A `_inUpdate` flag that throws a `StateError` on re-entry.
- Disposing during update: mark it, and finish the dispose at the end of `update`.

**Order once "dispose destroys the roots first" lands (API review B).** Destroy the roots (so `onDisable` and
`onDestroy` run, and colliders leave physics), then stop listeners, then dispose the systems, then free the
engine. Today the systems are disposed (world.dart:97-106) before anything is destroyed. Reversing that is what
lets `Collider.onDisable` → `physics._remove` run while physics is still alive.

### F5 (medium, structural) The hierarchy lives only in the engine; Dart asks for it again through FFI

**Evidence (measured on the Mac)**

| Read | Cost |
|---|---|
| `parent` (one FFI call, `game_object.dart:425-428`) | 140 ns |
| `ancestor<T>()` at depth 10, a miss | 1.1 µs |
| set `transform`, then read `worldTransform` (a flush plus a read) | 730 ns |

**Where it is paid**

- `children` allocates an unmodifiable list on each call.
- `_subtreeIds` is behind `_refresh` (every `active=` and `parent=`), `_destroy`, `_isHidden` (F3) and
  `_tidy` (F6).
- Physics' `_Place._walk` (`rigidbody.dart:446-459`) walks `o.parent` through FFI, plus `get<Rigidbody>()`,
  plus a new list. It runs again for every moved collider whenever `hierarchyVersion` changed
  (rigidbody.dart:486-490), which happens on any spawn or destroy.
- `Rigidbody._put` for a body with a parent reads `ancestors.first.worldTransform` (rigidbody.dart:414). That
  flushes the transforms not yet sent and makes one FFI call per body per step.
- Decision A2 (`position` = world) would make every `position` read or write on a child do the same.

**Fix (L).** Mirror the structure in Dart. Dart is the only writer of parent, active and layer, except a
model's parts, which can be read once when spawned with one `fy_subtree_get`. The mirror holds:

- `_parent`;
- an intrusive child list;
- `activeInHierarchy`, maintained on change;
- a per-object `worldStamp` (the max of its own and its ancestors' stamps, pushed down on parent moves).

The engine stays the source of world transforms for drawing. What this removes or improves:

- F3 and F6 go away;
- `parent`, `ancestor`, `getInParent` and `getInChildren` become pure Dart;
- physics' `_Place` no longer needs an ancestor list;
- A2's world `position` can be served by a cached world transform per object, refreshed lazily by version.

**Do this before implementing A2.** Otherwise A2 ships an FFI call per `position` read on every child.

### F6 (medium, scale) Reparenting is O(moved × live) at the next frame

**Evidence:** `world_loop.dart:263-274`. For each reparented object, `_tidy` builds its subtree set through FFI
(F5), scans all of `_live` into a new `tail` list, `removeWhere`s over all of `_live`, then `addAll`s.

**Measured (Mac):** 10 reparents per frame

| Live objects | Steady frame | Frame with 10 reparents |
|---:|---:|---:|
| 1,000 | 311 µs | 479 µs |
| 10,000 | 377 µs | 4,175 µs (+3.8 ms) |

That is a lost frame on a phone for something like picking up items, attaching bullets, or a carried object.

**Fix (S, or free with F5).** Mark the moved subtrees, then do one stable pass over `_live` that moves every
marked object to the end. That is O(live) once per frame, not per reparent. Or keep a per-object depth-first
order key in the Dart mirror.

### F7 (medium, scale) A physics query after anything moved walks every collider (finding 9), and more after a spawn

**Evidence**

- `physics.dart:476-516`: `_follow` → `_followMoved` runs whenever `transformVersion` changed. It visits every
  body and every collider, reading `c.gameObject.layer` and `_Place.chain()` (the max stamp over the ancestor
  list).
- One moved collider is enough. After a spawn or destroy, each moved collider also walks its ancestors again
  through FFI (F5).
- Layer changes are found by polling every collider each follow (physics.dart:497-501).

**Measured, raycast cost (Mac; colliders at depth 3; a second suite was running, so the numbers are noisy):**

| Colliders | Nothing moved | One object moved | One moved, plus a spawn and destroy |
|---:|---:|---:|---:|
| 100 | 1.1 µs | 14 µs | 49 µs |
| 1,000 | 1.0 µs | 26 µs | 39 µs |
| 5,000 | 0.3 µs | 90 µs | 123 µs |

The shooter agent measured 4.7 ms per frame with 500 moving bullets, 300 enemies and casts in between
(finding 9).

**Fix (M).** Give systems an append-only log of moved objects with a per-system cursor (see F9). Physics keeps
a map from each object to the colliders on it or below it; `_Place._walk` already knows the ancestors, so the
map is built there. A log entry then touches only the colliders it affects. Layer changes go into the same log
as a kind of entry.

### F8 (medium) `extend.dart`: what is missing for the next packages

Audio needed nothing more for its frame (audio sketch §1). It did list asset kinds, the world's
`AssetSource`, app pause, `writeTransforms` and a moved list (its GAPs 1-8; not repeated here). Looking at the
seam for animation, particles, networking and picking adds the following.

**Missing**

- **A hook between the updates and the late updates.** `afterUpdate` is where Unity's Animator, and this
  core's own tweens (`_runMotions`, world.dart:365), run. An animation package can't put its pose in front
  of `onLateUpdate` today: `beforeUpdate` is too early and `afterLateUpdate` too late.
- **An order between systems.** Systems run in the order they were made (systems.dart:78-85), and they are
  made lazily on first use. So whether physics or animation runs first in `beforeFrame` or `afterFrame`
  depends on which the game touched first, maybe in an `onLoad`. Fix: an `order` on `WorldSystem` (an int,
  as Unity's `DefaultExecutionOrder`), sorted when a system is added.
- **`componentRemoved`**, and an object destroyed, hidden or shown event for systems that follow non-Behaviour
  components (`MeshRenderer` for particles or LOD). Physics avoids the need only because its components are
  Behaviours.
- **Pointer targets.** Fingers hit-test with the render raycast (`pointing.dart:15-30`) through a private
  `_Targets`. A physics game whose objects have colliders but no mesh can't be clicked, and physics can't
  offer collider picking (Unity's `PhysicsRaycaster`).
- **Systems can't see pause or app lifecycle** (audio GAPs 4 and 5).

**Leaks**

- `GameObjectSystems` (`transformStamp`, `componentCount`, `componentAt`) is exported from `fynkel.dart` too
  (fynkel.dart:9 hides only `SendTransforms`, `WorldSystem` and `WorldSystems`). This is already in API
  review C.
- `hierarchyVersion` is the private `_shape`, which also changes on load end and on show and hide. The doc
  says "every object's parent is what it was", but it bumps far more often than parents change.

**Fragile:** a system that throws in a hook throws every frame. `_runSystems` reports each one, so that is 60
errors a second into `World.errors` and the panel (capped at 100), and `FlutterError.reportError` on each.
Fix: after N failures in a row, disable the hook and report once.

**Fix (M):** `extend.dart` v2, with the items above and audio GAPs 1, 2 and 5, versioned in its doc.

### F9 (medium) No "moved since" list, and the core's own lists can't serve as one

**Evidence**

- `World._moved` (world.dart:191) is not a moved list. It is the list of objects reparented this frame, used
  by `_tidy`. The name invites the mistake the API review's B list makes ("from the core's moved list").
- `_unsent` (world.dart:214) is emptied by any send: a view frame, any raycast, `worldTransform`, or
  `readWorldTransforms` (world_loop.dart:35). So a system can't read from it.
- Neither covers children whose world transform changed because a parent moved.

**Fix (M, with F7).**

- Rename `_moved` to `_reparented`.
- Add an append-only `_movedLog` (object, stamp), written in `_setTransform`, `parent=` and `layer=`.
- Expose it in `extend.dart` as `movedSince(cursor)`, so each system keeps its own cursor. Trim the log to the
  slowest cursor.
- Children are covered either by the system's object-to-dependants map (F7) or by the per-object `worldStamp`
  (F5).

### F10 (medium) World invariants are kept from outside `World`; one library, many files

**Evidence**

- `game_object.dart` reaches into `World`'s private state 33 times. For example, `parent=` updates
  `_transformStamp`, `_moved` and `_shape`, then calls `_refresh` (game_object.dart:431-443). `active=`
  updates `_off` and `_shape`, then calls `_refresh` (480-497).
- The same bookkeeping is repeated in world_objects.dart:98-99, 212-214, 232-233 and 321-331.
- Other files: pointing 9, routines 7, assets 6, resources 4, motion 4.

**The `part` question.** Twenty files are one library, so privacy is all or nothing. That is what makes this
coupling free to write and hard to see.

- Compile and hot reload: a framework edit reloads the whole 8.6k-line library, which doesn't matter to games.
- Readability is fine file by file. The cost is that invariants have no owner.

**Recommendation.** Keep `part` for the object model: world, world_* files, game_object, components, systems.
But:

- **(S)** Move every hierarchy and visibility change into `World` methods (`_setParent`, `_setActive`,
  `_attached`, `_destroyed`), so `_shape`, `_off`, `_moved` and `_refresh` are touched in one place.
- **(M, optional)** Make the pieces that are already standalone into their own libraries with small internal
  interfaces:
  - `Input`, `keys` and `Finger`: `Input()` has a public constructor and `update`, and already talks to the
    world only through `_Targets`;
  - `Signal`: see F12;
  - `math`, `color` and `error` already are.
- The motion split is already decided (API review C).

### F11 (medium) `List.clear()` gives back the backing array in the Dart VM, so lists refilled every frame allocate

**Evidence:** Dart SDK `lib/_internal/vm/lib/growable_array.dart`.

- `clear()` sets `length = 0` (lines 510-512).
- Shrinking takes `shouldShrinkToFit = (0 + 0) < length` (lines 243-252), which is true whenever the list
  wasn't empty, so it calls `_shrink(0, 0)`, which gets the shared `_emptyList` (lines 372-378).
- `removeRange(0, n)` that empties a list does the same.
- The next `add` grows again, 0 → 3 → 7 → 15…, allocating a `_List` each time.

**Lists that are emptied and refilled in busy frames**

- `World._unsent` (world_loop.dart:35): every frame that moves objects;
- `_moved`, `_compact`;
- `Input._queue` (input.dart:326);
- physics `_leaving.removeRange` (physics.dart:671).

So "a frame allocates nothing" holds for idle frames (which is what the alloc tests check). In a real game,
moving N objects costs about log2(N) arrays per frame, plus about N element copies. That is small, but it is
a hidden contradiction of the stated rule, and the obvious suspect when an alloc test shows `_List`.

**Fix (S):** a tiny `_Bag<T>` (a fixed `List<T?>` plus a count; reset with `count = 0`) for the per-frame
lists. Add an alloc test with N objects moved per frame.

### F12 (low) `Signal` depends on the object model; cancelling is O(n)

**Evidence**

- `signals.dart` `_hear` switches on GameObject or Behaviour owners and calls `world._report`. The lowest-level
  primitive knows the highest-level types. So does `routines` (`_routinesOf`).
- `SignalSubscription.cancel` does `_subscriptions.remove(this)`. Destroying N listeners of one shared signal
  (500 enemies on `game.onWaveEnd`) is O(N²).

**Fix (S):** a private `_Owner` interface (`_reportError`, `_isPaused`). Cancel by marking, and compact
lazily, as `emit` already does while emitting.

### F13 (low) Duplicated logic

- **Three copies of the call loop:** update (world.dart:344-363), late update (368-387) and fixed update
  (world_loop.dart:206-226). One `_callAll(_Phase)` with a switch inside keeps the "no closures" rule.
- **Six `switch (owner)` copies** for GameObject vs Behaviour: `_objectOf`, `_motionsOf`, `_pausedOwner`
  (motion.dart:370-381, 561-565), `_routinesOf` (routines.dart:188), `_on` (signals.dart), `_start`
  (world_loop.dart:81-86). A shared private base for the two owner kinds holding `_routines`, `_motions` and
  `_listening` removes them (S).
- **Two copies of the pointer-hit math:** `screenToRay` and `worldToScreen` live on `GameObject`
  (game_object.dart:560-635) and are re-derived for pointing. They move to `Camera` per API review A6.

**Dead or near-dead code:** `Input.update(dt)` (input.dart:305) is for a standalone `Input` that nothing in
the repository constructs; keep it only if it's documented as such. Nothing else is clearly dead: the analyzer
is clean with `--fatal-infos`.

### F14 (low) Native memory not tied to a finalizer

- `Physics` holds about 35 `malloc` buffers (physics.dart:153-232), including the contact, hit and found
  buffers, which grow to the peak. Only `dispose` frees them. `RawPhysics` has a `NativeFinalizer`;
  `Physics` doesn't.
- A world that is garbage-collected without `dispose` leaks them: a hot restart, a test without
  `addTearDown`, or code dropping a world.
- The global `scratch` (native.dart:126) is never freed on the main isolate, which is fine.

**Fix (S):** allocate them as one arena owned by a `Finalizable` holder with a `NativeFinalizer(malloc.nativeFree)`,
as `HeadlessView` does for its pixels (view.dart).

### F15 (medium, CI) `contacts_alloc_test` and its siblings flake

**Evidence**

| Run | Test | Allocated |
|---|---|---|
| CI 38188923282 | `contacts_alloc_test` | `{'_List': 39173}` |
| CI 38187673095 | `contacts_fixed_alloc_test` | `{'_List': 39742}` |
| This copy, 1 of 6 full-suite runs | `contacts_fixed_alloc_test` | `{'_List': 39095}` |
| This copy, another run | `sleep_alloc_test` (no contacts at all) | `{'_List': 38700}` |

An older one (CI 38147996680) was `_Double`/`Vec3`, the boxing that #55 fixed. That is about 1.95 `_List`
per frame over 20,000 frames, in three different files, one of which has no contacts.

**What that tells:**

1. **It isn't the contact code.** The sleep test has no contact listeners. The common path is
   `World.update` plus the physics `WorldSystem` hooks.
2. **It isn't the physics thread's timing.** The fixed-step variant steps inline.
3. **The JIT's state is left as the cause.** The scenes are deterministic, so the run-to-run variable is the
   JIT and GC state. The code is semantically identical every run, so it must be code that allocates in one
   compiled state and not in another (unoptimized or deoptimized code, or the VM's own arrays).
4. **The retries don't help when that state persists.** The helper's three retries (allocations.dart:52-71)
   don't cure it. That fits a function that hit the deopt limit and stays unoptimized, or code that keeps
   recompiling.
5. **It is load-dependent.** CI runs `dart test` with default concurrency, so all 13 physics test files share
   the CPU and the background compiler (ci.yml, `dart test` in fynkel_physics).

**Not reproduced under tracing.** I patched the copy's `allocations.dart` to turn on
`setTraceClassAllocation(_List)` and print the top allocating stacks when a test fails. It then did not
reproduce in 32 more full-suite runs, 16 of them with two suites at once, against 2 failures in the 18 runs
before. So the stack is still unknown. The patched helper is in
`/tmp/s6/archcopy/packages/fynkel_physics/test/allocations.dart`, ready to copy.

**Fix (S):**

- Run the alloc tests in their own CI step: `dart test --concurrency=1` with an `alloc` tag, and
  `--exclude-tags alloc` for the rest.
- Keep the trace-on-failure code in the helper, so the next flake names its stack.
- Longer term, check allocations in AOT (`dart compile exe` of a small harness, or a profile build of the
  lab), which is what ships. Today the JIT forces workarounds into hot code ("no double args to non-inlined
  calls", "no `.ref`") that AOT doesn't need.
- See F11 for a real, non-flaky `_List` source to rule out first.

### F16 (low) CI and test gaps

- **No test** that the view keeps ticking after a throw (F1), that dispose during update is safe (F4), that a
  stale `Physics` throws (F2), or that 1,000 moving objects make a bounded number of allocations (F11).
- **No AOT or profile-mode run** of anything. The alloc gates are JIT-only (F15).
- **`examples/hello`** is only built. **`fynkel_physics/example` (tumble)** is analyzed and built, not tested.
- **No soak test.** The 30 minutes on phones is a manual step-5 check. A headless 100,000-frame run of the lab
  scenes, with RSS read at intervals, would catch native leaks in CI on the Mac.
- **The build hooks:** `lib/hooks.dart` is a public library of `package:fynkel`, so `hooks`, `code_assets`
  and `dart:io` are regular dependencies of every game. Every package's hook also declares every `.rs` in
  `crates/` as an input (hooks.dart:78-83). This is audio GAP 6. Low; fine until there is a package outside
  this repository.

## Lifetimes, threading and hot reload: what holds

- **GameObject ↔ engine id.** `World._objects` keeps the one Dart instance per live id (world.dart:472).
  Ids are never reused. Destroy is depth-first with `onDisable` before `onDestroy` (world_objects.dart:279-334),
  and `_gone` makes later use throw a clear `StateError` (game_object.dart:166-169).
- **Meshes, materials and textures.** They are registered per world through `Expando`s and weak references.
  A `Finalizer` on each resource (resources.dart:562-599) only queues the id (`_collect`), and the world
  removes it at the start of the next update (`_removeCollected`, world_resources.dart:131-146). So nothing
  native is freed from a finalizer in the middle of a frame. A resource still in use is kept for a later try.
  This is well designed.
- **Assets.** Assets are reference-counted by asker and freed when the last holding object is destroyed
  (assets.dart). One sharp edge: a texture loaded by object A and put on a shared material is disposed when A
  goes, unless B also asked for it through `assets`. That is by design, but worth one line in the guide.
- **Physics' thread.** Every call into the native world goes through `_world`, which first collects a running
  step (physics.dart:45-48). The few direct `_pointer` uses (contacts, broken joints, key forget, step begin)
  only run right after a collect. On the Rust side, `FpWorld` puts `worker` first, so a running step is joined
  before the physics is dropped (crates/fynkel_physics_ffi/src/lib.rs:497-500, 1143-1149). Calls from Flutter
  callbacks between frames are safe; they may join the step early.
- **Hot reload** keeps the world and its systems. `FynkelGame.restartOnReload` and F5 make a new world.
- **Hot restart** frees worlds through the `NativeFinalizer`s. It leaks F14's buffers and `scratch`.

## Keep: this is good

- **The frame in one place**, documented as it runs (world.dart:253-394), with the systems' hooks at named
  points. The order matches Unity's where a user would guess (starts before the first fixed step; objects
  added in an update start next frame).
- **Error isolation.** Every object or behaviour call is guarded inline, without closures; errors go to
  `World.errors` or the zone; one bug never stops the frame.
- **Batched FFI.** One `fy_transform_set_many` per send (world_loop.dart:6-37), one
  `readWorldTransforms`, and query scratch written float by float.
- **Allocation discipline as a CI gate**, with the JIT's pitfalls written down where they bite.
- **Safe native ownership:** `NativeFinalizer`s on engine objects, a weak per-world registry, deferred
  removal, and ABI checks on load for both libraries (native.dart:19-32, raw.dart:69-81).
- **The `WorldSystem` model:** made lazily per world, removable as a package, and enough for physics and,
  per the sketch, for audio's frame work.
- **The headless `World` as the test harness:** 378, 89 and 30 tests in the three packages. Physics and
  fingers are testable without a screen, and the fresh agents praised it.
- **The physics step between frames with collect-on-demand:** a real gain on phones (stage 5), with a
  simple safety rule.

## Top 5 recommendations, ranked

1. **Make the view unkillable (F1, with F4's guards).** Guard `_tick` and keep ticking. Don't stop on a plugin
   failure; fall back to a headless view or none when there is no plugin. Add a public test seam. Make
   `World.update` safe against dispose and re-entry. This fixes finding 25 and silent freezes. **S.**
2. **Close the native use-after-free (F2, F14).** Every `WorldSystem` member checks for dispose before
   touching native memory. Physics' buffers get a finalizer. Write the rule into `WorldSystem`'s doc for
   package authors. **S.**
3. **Mirror the hierarchy in Dart (F5, which also removes F3 and F6).** Store `_parent`, children,
   `activeInHierarchy` and `worldStamp` in Dart, maintained by `World` methods that own the invariants
   (F10). This removes the pooled-hide and reparent cliffs and the FFI ancestor walks, and is the
   precondition for A2 (`position` = world) without an FFI call per child read. Do it before A2's PR. **L.**
4. **A moved log for systems, and physics following only what moved (F9, F7; finding 9).** An append-only
   log with per-system cursors in `extend.dart`, and physics' object-to-colliders map. Rename `_moved` to
   `_reparented`. **M.**
5. **`extend.dart` v2 and a sound CI gate (F8, F15).** Add an `afterUpdate` hook, system order,
   `componentRemoved`, pointer targets, `writeTransforms`, asset kinds and app pause (audio GAPs 1, 2, 5, 7).
   Disable a system hook that keeps throwing. Run the alloc tests alone (`--concurrency=1`, tag `alloc`) with
   trace-on-failure, and add an AOT allocation check. **M.**

Next tier: F11 (the per-frame `_Bag`, S), F10's invariant methods (S, can ride with 3), F12 and F13 tidy-ups
(S), F16's soak test (M).

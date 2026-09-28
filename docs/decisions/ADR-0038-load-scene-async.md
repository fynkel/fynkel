# ADR-0038: Loading a scene in the background (LoadSceneAsync)

**Status:** Accepted 2026-09-28 (the user agreed, with the three changes from Unity in §6; docs/foundation.md 3.7)  
**Date:** 2026-09-28  
**Related:** ADR-0037 (DontDestroyOnLoad), [scenes.md](../scenes.md) § Loading, [hosts.md](../hosts.md) § Scenes at runtime

## Context

In Unity, `SceneManager.LoadSceneAsync(name, mode)` returns an
`AsyncOperation`. The scene's files are read and its assets loaded on
background threads while the game keeps running, for example to show a
loading bar. The operation has:

- `progress`: 0 to 0.9 while loading, 1 when done;
- `isDone`;
- `allowSceneActivation`: when false, the load waits at 0.9 until the
  game sets it to true, for example after a "press any key";
- `completed`: an event.

The objects themselves are made on the main thread in one frame, the
"activation".

In Fynkel every load is synchronous on the main isolate, including
`world.scenes.transition`, which only moves the load to the start of
the next frame. One call (`load` in `crates/fynkel_native/src/load.rs`)
does all of this while the game is stopped:

1. reads and decodes the cooked scene (`Assets::scene`);
2. registers what the scene uses: collider meshes, prefabs for
   emitters, clips and skins, sounds. Each of these reads its payloads
   from the cache;
3. spawns the entities (`World::load_scene_with`);
4. attaches the scripts (Dart).

Steps 1 and 2 are file reads and decoding. Meshes and textures, which
the renderer reads on first draw, also load on the main thread, during
the first frames after the load. On a big level that is a long freeze.

## Decision

### 1. The call

`world.scenes.loadAsync(Object sceneId, {LoadMode mode = LoadMode.replace})`
returns a `SceneLoad`, Unity's `AsyncOperation`:

- `double progress`: 0 to **1** while the background work runs (not
  Unity's 0.9; see §6).
- `bool isReady`: the background work is done and the load waits to
  activate.
- `bool isDone`: activated (or failed).
- `bool allowActivation`: true by default. When false a ready load waits
  until the game sets it back to true.
- `Future<LoadReport> done`: completes with the same report as
  `loadScene`, or with the `FynkelLoadError`.
- `void cancel()`: drops a load that has not activated.
- `String sceneId`, `LoadMode mode`.

`world.scenes.state` is `SceneState.loading` while any `SceneLoad` is
unfinished.

### 2. Background, then activation

**In the background** (a Rust thread, which holds only a clone of the
`Assets` store, never the world):

- read and decode the cooked scene;
- load every payload of its closure (meshes, textures, materials,
  clips, sounds, prefabs) into the store;
- hold a residency token for them, so nothing is evicted before
  activation.

`progress` is the share of the closure loaded.

**Activation** runs at the start of a frame, at the same point as
`transition`, once the background work is done and `allowActivation` is
true. It does steps 2–4 above, which now find everything in memory. A
replace activation takes the world down then, and not before, so the
old level plays on while the new one loads. Ready loads activate in
the order they were asked for; a load held by `allowActivation = false`
holds only itself, never the loads after it (§6).

### 3. ABI

These are additive functions (the ABI version stays):

- `fynkel_world_load_scene_async(world, cache, cache_len, id, id_len, out_ticket)`
  starts the background work.
- `fynkel_world_load_progress(world, ticket, out_progress)` answers
  pending, ready or failed.
- `fynkel_world_load_activate(world, ticket, flags)` does the
  activation and writes the same report as `fynkel_world_load_scene_with`.
- `fynkel_world_load_cancel(world, ticket)` drops a load that was not
  activated.

The native world keeps its unfinished loads by ticket. Disposing the
world detaches their threads: a thread only finishes filling a store no
one reads.

### 4. What stays synchronous

- `loadScene`, `scenes.load` and `transition` keep working as they do.
  `transition` is still the way to change level from inside a callback.
- Spawning and script creation run in the activation frame. Unity does
  the same with `Awake`. If one frame of spawning is still too long for
  big levels, spreading it over frames is later work, measured first.

### 5. The renderer warms up during the load

The renderer copies a mesh or texture to the GPU the first time it
draws it, so even a background load would stutter for a few frames after
the switch. While a load runs in the background, the renderer also
uploads the payloads it has made resident, a few per frame within a
small time budget (`Assets::warm_list`, drained by `fynkel_render`). At
activation most of the level is already on the GPU. A host without a
renderer (tests, a server) ignores the list.

### 6. Where this differs from Unity, on purpose

Agreed with the user 2026-09-28; listed in unity-parity.md § On purpose:

1. **`progress` reaches 1.** Unity's stops at 0.9 while a load waits
   for activation, which confuses nearly everyone. Here `progress`
   measures the loading, and `isReady` says it is waiting.
2. **A held load holds only itself.** In Unity, one operation with
   `allowSceneActivation = false` stalls every later async operation.
3. **The GPU warms during the load** (§5). Unity uploads textures
   asynchronously; here meshes and textures both are.

Also more than Unity: `cancel()` drops a load that has not activated
(Unity cannot cancel), and `done` is a `Future` to `await`.

### 7. The editor

Play needs nothing new: the Hierarchy updates when the activation
spawns. The Profiler shows the activation's load time, as it does for
any load.

## Consequences

- A game can show a loading screen that animates while the next level
  loads, and can keep the current level running until it is ready.
- The store is used from two threads at once. It is already behind a
  mutex and shared with the renderer, so no new locking is needed. The
  background thread must never hold the lock while it reads a file.
- Four ABI functions, one Dart class and a warm list the renderer drains. No document, cook cache or wire
  schema changes.

## Plan

1. Engine: `Assets::preload(ids, progress)` and the ticketed background
   load in `fynkel_native` (start, progress, activate, cancel), with
   tests that fail first. One test checks that activation reads no
   file: the cache is removed before activation and the load still
   succeeds.
2. SDK: `SceneLoad`, `scenes.loadAsync`, `SceneState.loading`,
   activation at the start of the frame in request order,
   `allowActivation`; tests in `packages/fynkel/test/scenes_test.dart`.
3. Renderer warm-up: `Assets::warm_list` filled by the background
   load, drained by `fynkel_render` within a per-frame budget.
4. Measure the reference game's level load, sync against async (the
   longest frame), in docs/perf.
5. Docs: scenes.md, hosts.md § Scenes at runtime, unity-parity.md,
   foundation.md 3.7; code_only's menu to level change uses it with a
   loading bar.

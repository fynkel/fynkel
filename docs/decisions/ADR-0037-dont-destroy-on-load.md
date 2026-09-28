# ADR-0037: Objects that survive a scene change (DontDestroyOnLoad)

**Status:** Proposed 2026-09-28 (docs/foundation.md 3.6)  
**Date:** 2026-09-28  
**Related:** ADR-0021 (Play in the editor), ADR-0035 (object references), ADR-0036 (everything is an Object), [scenes.md](../scenes.md) § Loading

## Context

In Unity a game keeps an object across levels with
`Object.DontDestroyOnLoad(obj)`: the music player, the game manager,
the player's save data. Unity moves the object to a special scene
named "DontDestroyOnLoad". A later `LoadScene` in Single mode (our
replace load) leaves it alone, and so does unloading the scene it came
from. Only `Destroy` removes it. The Hierarchy shows the special scene
during Play.

Fynkel has no way to do this. A replace load (`World::load_scene_with`,
`LoadMode::Replace`) unloads every scene and then destroys **every**
remaining root: runtime spawns, prefab instances, everything
(`unload_all` in `crates/fynkel_core/src/load.rs`). A game that wants
music to keep playing across levels has to rebuild it after every load.

## Decision

### 1. The call

`world.dontDestroyOnLoad(FynkelObject target)` in the Dart SDK:

- A `GameObject`, or a component (its `gameObject` is used), as in
  Unity.
- It works on **root** objects only. On a child it does nothing and
  reports a warning, as Unity does ("DontDestroyOnLoad only works for
  root GameObjects").
- It takes effect **at once**, not at the next tick. The usual call is
  in `onCreate` or `start`, and a `scenes.transition` runs at the start
  of the next frame, before the tick. A queued command would lose that
  race.
- A pending object (a `world.spawn` not yet made by a tick) is
  remembered and marked the moment the tick makes it.

### 2. The engine

`fynkel_core::World` keeps a set of **persistent roots**:

- `World::dont_destroy_on_load(entity)` adds a live root and refuses
  anything else. The refusal is a new `LoadError::NotARoot`.
- `unload_all` (a replace load) skips every root in the set. Their
  subtrees stay: children, components, physics bodies, audio.
- `take_down` (unloading a scene) skips every entity whose root is
  persistent, so unloading the scene an object came from does not take
  it.
- Children added later under a persistent root are persistent with it.
- If a persistent root is given a parent, the persistence is dropped
  and the object belongs to its new root. Unity does the same when you
  parent a DontDestroyOnLoad object under a scene object.
- A destroyed root leaves the set (entity generations make a stale
  handle never match; the set is pruned at each replace load).

A new ABI function, `fynkel_world_dont_destroy_on_load(world, entity)`,
answers a status. It is additive, so the ABI version stays the same.

### 3. Assets

Today a replace load drops every residency token: the old scenes' and
the in-memory prefabs'. Assets load again from the cache on demand, so
a persistent object would still draw. But its meshes and textures would
be dropped and read back from disk right after the level change.

So at a replace load, the native world first retains what the
persistent subtrees use: the asset ids their components name (mesh,
material, skin, clips, sounds, collider meshes, emitter prefabs) with
their dependencies (`Assets::closure`). That token is held until the
next replace load works it out again. The in-memory prefabs used by
persistent objects stay in `w.prefabs`.

### 4. What a load reports

- The load report gains `persistent`: the number of persistent roots
  that survived.
- Survivors get no `Destroyed` event, so their `GameObject`s and
  scripts stay as they were. Their object references to things that
  were destroyed go to null, as with any destroy (ADR-0036 step 2).
- `SceneManager.loaded` does not list the special scene. Unity's
  `sceneCount` does not count it either.
- `world.persistentObjects` lists the persistent roots.

### 5. The editor

During Play the Hierarchy shows the persistent roots under their own
header, **DontDestroyOnLoad**, below the loaded scenes, as Unity does.
The rows work like any live row: select, inspect, edit, delete. Apply
to Scene is not offered for them, because they are in no scene.

## Consequences

- A game can keep a manager, music or the player across a replace load
  with one line, as in Unity.
- The engine gains one set, one ABI function and one load-report field.
  No document format, cook cache or wire schema changes.
- Physics keeps the survivors' bodies. Their contacts with objects that
  were destroyed end with the usual exits (foundation 2.6).

## Not in this ADR

- `SceneManager.MoveGameObjectToScene` (moving an object into a scene,
  which also undoes DontDestroyOnLoad). It is possible later on top of
  the same set.
- Marking objects persistent in a document. In Unity this is always
  done from code.

## Plan

1. Engine: the persistent set in `fynkel_core` (`dont_destroy_on_load`,
   `unload_all` and `take_down` skip it, `NotARoot`), with a contract
   test that fails first. The ABI function, the residency token and the
   `persistent` report field in `fynkel_native`.
2. SDK: `world.dontDestroyOnLoad`, the pending case, the child warning,
   `persistentObjects`; tests in `packages/fynkel/test/scenes_test.dart`.
3. Editor: the DontDestroyOnLoad header in the Play Hierarchy, with a
   parity test.
4. Docs: scenes.md § Loading, design/dart-sdk.md, unity-parity.md,
   foundation.md 3.6.

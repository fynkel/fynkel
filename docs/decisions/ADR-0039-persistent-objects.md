# ADR-0039: Persistent objects in Project Settings

**Status:** Accepted 2026-09-29 (the user agreed, with both recommendations below)  
**Date:** 2026-09-29  
**Related:** ADR-0037 (DontDestroyOnLoad), ADR-0038 (LoadSceneAsync), ADR-0021 (Play in the editor), ADR-0026 (code first), [scenes.md](../scenes.md) § Objects kept across loads

## Context

ADR-0037 gave Fynkel Unity's `DontDestroyOnLoad`. It has two known
problems, the same as in Unity:

1. **Duplicates.** A game manager marked DontDestroyOnLoad in the Menu
   scene gets a second copy each time the Menu loads again. Every Unity
   project writes the same guard: "if one already exists, destroy
   myself".
2. **Play from any scene.** The manager exists only if the game started
   in the scene that makes it. Press Play in Level 3 and there is no
   music player and no save data. Unity projects work around this with
   a "bootstrap" scene and editor scripts that always start from it.

Godot solves both with **autoloads**: a list in Project Settings of
scenes the engine makes once, before the first scene, and never
destroys. Each has a name, and any script can reach it by that name.
Unreal has the same idea as the Game Instance and its subsystems.

ADR-0037 agreed to do this as a later ADR. This is that ADR.

## Decision

### 1. The setting

`fynkel.project.json` gains an optional list, `persistentObjects`:

```json
"persistentObjects": [
  { "name": "GameManager", "prefab": { "asset": "…uuid…" } },
  { "name": "Music",       "prefab": { "asset": "…uuid…" } }
]
```

- Each entry is a **name** and a **prefab** (an asset reference, as
  `startupScene` is).
- The name is unique in the list and not empty. The object the prefab
  makes takes that name.
- Order matters: the objects are made in list order.
- The key is optional, so the project format stays at version 2 and no
  migration is needed. `modules` and `tags` were added the same way.

### 2. When they are made

The engine makes them in `fynkel_world_load_project`, **before the
startup scene**:

1. The project opens (input, layers, tags, physics).
2. Each listed prefab is instantiated as a root and marked persistent
   (the ADR-0037 set).
3. The startup scene loads, if there is one.

The Dart side attaches their scripts first, so their `onCreate` runs
before any scene script's. A scene script can rely on
`world.persistentObject('GameManager')` in its own `onCreate`, as a
Godot script can rely on an autoload in `_ready`.

They are made **once per `loadProject`**: once per game start, and once
per Play in the editor. Scene changes never make them again, so there
are no duplicates and no guard code. Code-first projects (no
`startupScene`) get them too, since step 2 runs in any case.

### 3. Play from any scene

In-editor Play already goes through `loadProject` and then moves to the
open scene (ADR-0021). The persistent objects are made in
`loadProject` and survive that move, so **Play in any scene has them**.
This removes the bootstrap-scene workaround.

### 4. Reaching them from code

- `world.persistentObject(String name) → GameObject?`: the object, or
  null if the name is not in the list or the object was destroyed.
- Its scripts are reached as on any object:
  `world.persistentObject('Music')?.get<MusicPlayer>()`.
- `world.persistentObjects` (ADR-0037) lists them with any other
  persistent roots.

Question 1 below proposes a typed accessor made by codegen on top of
this.

### 5. Destroying one

`destroy` works on them as on any object, as in Godot. A destroyed
persistent object is **not** made again until the next `loadProject`.
`persistentObject(name)` then answers null.

### 6. Checks

- A new project rule, `FYN-P0020`: an entry whose prefab does not
  resolve to a prefab, or whose name is empty or used twice. It is an
  error, so the cook stops, as for a bad `startupScene`.
- At run time a prefab that fails to instantiate (a cache made before
  the prefab was deleted) is reported as a load diagnostic. The game
  still starts without that object; the load does not fail.

### 7. What the engine carries

- The cook writes the list to `index.json` (`project.persistentObjects`,
  with the prefabs resolved to ids). `ProjectInfo` reads it back. The
  cook cache version goes up by one.
- The load report gains `persistentObjects`: for each entry, `name`,
  `entity`, `prefab`, and the same `behaviours` list an instantiate
  report has.
- No new ABI function and no wire schema change: everything rides on
  the existing load call and its JSON report.

### 8. The editor

- A new Project Settings page, **Persistent Objects**, in the Project
  menu beside Tags and Layers and Modules. It is a list: a name field
  and a prefab field per row, add, remove and reorder. It writes
  through a new authoring op, `setPersistentObjects`, with one undo
  step, as `setModules` does.
- During Play the objects show under the existing **DontDestroyOnLoad**
  header in the Hierarchy. They are persistent roots, so no new
  Hierarchy work is needed.
- Dropping a prefab on the page adds it with the prefab's name.

## Consequences

- A manager, music player or save system is set up once in Project
  Settings, never duplicated, and present whichever scene Play starts
  in.
- `dontDestroyOnLoad` stays exactly as Unity's, for objects a scene or
  script makes.
- The engine gains one optional project key, one project rule, one
  load-report field and one cook-cache bump.
- Every Play and game start pays for instantiating the listed prefabs.
  An instantiate costs about 18 µs from memory (foundation 3.1), plus
  the first read of each prefab.

## Options not taken

- **A bootstrap scene** (the Unity workaround): needs editor scripts to
  always start from it, and still duplicates if the scene loads again.
- **Code only**, making the objects in the game's own `Game.load`:
  works for code-first games today, but the editor cannot show or edit
  the list, and every game writes the same code.
- **Scripts without objects** (Unreal's subsystems, plain Dart
  singletons): useful, but a service with no object cannot hold
  components (an `AudioSource`, a `Transform`). A game can already
  write a plain Dart singleton; nothing to add.

## Questions for the user (answered)

1. **A typed accessor.** Besides `world.persistentObject('Music')`,
   codegen could write a `Persistent` class into `assets.g.dart`, with
   `Persistent.music(world)` answering the `GameObject` and a spelling
   mistake failing to compile. Recommended: yes, as a last step.
   **Answer: yes.**
2. **Destroying one.** Section 5 lets a game destroy a persistent
   object, as Godot does. The other choice is to refuse and warn.
   Recommended: allow it, as written. **Answer: allow it.**

## Plan

Each step starts with a test that fails first.

1. Document: the `persistentObjects` key in `project.schema.json` and
   `Project`; the `FYN-P0020` rule; `fynkel validate` reports it.
2. Cook and engine: the list in `index.json` and `ProjectInfo`, the
   cache bump. `load_project` makes and marks them before the startup
   scene; the `persistentObjects` report field; the run-time
   diagnostic.
3. SDK: their scripts attach first; `world.persistentObject(name)`.
   Tests in `packages/fynkel/test/scenes_test.dart`, including Play
   starting in a scene other than the startup scene.
4. Editor: the `setPersistentObjects` authoring op, the Persistent
   Objects page, and a parity test.
5. The generated `Persistent` accessors.
6. Docs: scenes.md, documents.md (the project key), design/dart-sdk.md,
   unity-parity.md.

## Progress

All six steps done 2026-09-29: the document and `FYN-P0020` (b9ee6c0),
the cook and the engine (47a9fb8), the SDK (27b5357), the authoring op
and the editor page (e2005c2, a98106b), the generated `Persistent`
accessors (3b896ec), and the docs ([scenes.md](../scenes.md)
§ Persistent objects in Project Settings).

Three details differ from the text above:

- **No cook-cache bump (§7).** Every cook rewrites the index, and an
  index without the key reads as an empty list, so a cache version bump
  would only force a full recook.
- **Made after the startup scene spawns (§2).** The native call loads
  the scene first and then makes the objects. Their scripts are still
  attached and created first, which is what a script can observe.
- **No drop on the page (§8).** Project Settings pages are modal
  dialogs, so nothing can be dragged onto them. A new row takes its
  prefab from the picker (⊙) and is named after it.

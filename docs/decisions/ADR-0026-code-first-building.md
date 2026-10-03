# ADR-0026: Building a game from code

**Status:** Accepted — proposed by the assistant 2026-09-25, confirmed by the user 2026-09-25 ("ok let's start")  
**Date:** 2026-09-25  
**Related:** ADR-0006 (asset identity: references by id, never by path), ADR-0007 (engine as a library), ADR-0011 (Dart behaviours for the few, native components for the many), ADR-0012 (schema codegen), ADR-0022 (one-root prefabs), ADR-0023 (built-in primitives), `docs/code-first.md` (the checklist this ADR serves)

## Context

The user, 2026-09-24: working without any editor, fully from code, is
"the most important part of the whole game engine", and "if it is bad
then everything will be bad even if we built the ultimate editor around
it".

A scan of `package:fynkel` on 2026-09-24/25 found two halves:

- **Running and controlling a game from code is good.** Behaviours have
  Unity's lifecycle, coroutines and Invoke; objects spawn, parent, tag,
  activate and find by name or tag; there are scene transitions, physics,
  input, audio, animation playback and a Unity-shaped transform, the UI is
  Flutter, and everything runs headless under `dart test`.
- **Building a game from code is weak.** Everything a player sees has to
  exist as a cooked asset first. Scenes and prefabs are JSON documents
  with UUID ids, and there is no Dart way to write one. The reference
  game's three scenes, three prefabs and materials came from an 881-line
  Python script (`examples/reference_game/tools/playground.py`), not from
  Dart. Code names assets by UUID strings
  (`const burstPrefab = '88641ff3-…'`). There is no cloning of a live
  object, no primitive, material or mesh made at runtime, `children`
  returns raw entity numbers, and a native component is constructed with
  every field spelled out (`RigidBody` takes six).

Unity is the reference (`docs/editor.md`, `docs/unity-parity.md`): its code-first user writes
`new GameObject`, `AddComponent`, `Instantiate(obj)`,
`GameObject.CreatePrimitive`, `new Material`, `new Mesh`,
`Resources.Load("path")`, `GetComponentInChildren`, `FindObjectOfType`,
and keeps prefabs as the one thing that comes from the editor. Fynkel
should reach that, and go one step further: a scene or prefab described
in Dart can also be **saved** as the same document the editor opens, so
code and editor build the same things.

## Decision

1. **Code is a first-class way to build, equal to the editor.** Every
   object the editor can author, Dart can author. Nothing a game needs at
   runtime depends on the editor or on tools outside Dart and the
   `fynkel` CLI.

2. **One description, two outputs.** A new `ObjectSpec` (library
   `package:fynkel/gameplay.dart`) describes an object tree: name,
   transform, active, tags, layer, native components, behaviours (as
   instances), children, and prefab instances with overrides.
   - `world.build(spec, {parent, transform})` spawns it live and returns
     the root `GameObject`. A Dart function returning a spec is a **code
     prefab**: call it as often as needed.
   - `SceneSpec` (a list of root specs plus the scene's settings) and a
     root `ObjectSpec` can be **written** as `.scene.json` / `.prefab.json`
     through `package:fynkel/authoring.dart`, which game code never imports
     at runtime. Ids are derived (UUID v5 from the path of names, as
     `playground.py` does), so writing again produces the same bytes and
     the editor's edits to other objects survive. Behaviour fields are
     written through the generated `BehaviourType.fields`, so no
     hand-written serialisation is needed.
   - `fynkel build-scenes` (or a plain `dart run tool/scenes.dart`) runs
     the writer. It replaces the Python generators.

3. **Assets by name in code, by id on disk.** ADR-0006 stands: documents
   store ids. Code gets two ways to name an asset without a UUID:
   - **Generated constants.** `fynkel codegen` writes
     `lib/generated/assets.g.dart` from the project's sidecars:
     `Assets.prefabs.burst`, `Assets.models.crate.mesh('Crate')`,
     `Assets.scenes.playground`, `Assets.audio.coin`. Each carries the id,
     so renaming a file in the editor breaks the build where the old name
     is used. That is intended, the same as Flutter's generated assets.
   - **A runtime lookup.** `world.assets.find('prefabs/burst.prefab.json')`
     reads the path from the cook's index and returns an `AssetRef`, or
     null. This is for data-driven games (a level list in JSON).

4. **Making things at runtime.**
   - `GameObject.clone({parent, transform})`, Unity's `Instantiate(obj)`:
     copies the live subtree (native components through a new world
     command, behaviours through their registered fields; unregistered
     behaviours are a diagnostic, not a silent skip).
   - `world.createPrimitive(Primitive.cube)`: the ADR-0023 shapes with
     Unity's collider. The built-in `Primitives.glb` ships with every cook
     at its fixed id, whether or not the editor installed it into the
     project.
   - `world.createMaterial(PbrMaterial(...))` returns an `AssetRef` for a
     runtime material (the asset store's `override_material` path, which
     live material editing already uses). `MeshRenderer` takes it like any
     other.
   - `world.createMesh(MeshData(...))`, procedural meshes over the asset
     store's `insert_mesh`. It comes last in the order (see
     Consequences).

5. **The object graph speaks `GameObject`.** `children` returns
   `List<GameObject>`, `parent` returns `GameObject?`. The entity-number
   forms stay, as `childEntities` / `parentEntity`. New:
   `findObjectOfType<T>()` / `findObjectsOfType<T>()` for behaviours,
   `getInChildren<T>()` / `getInParent<T>()` for behaviours and native
   components. This is a breaking rename, and every caller in the
   repository moves in the same commit.

6. **Components have defaults.** Codegen gives every native component
   field the default of its Rust `Default` implementation, the same value
   the Inspector's Add Component uses (`fynkel_authoring::component_default`).
   So `const RigidBody()` is a dynamic 1 kg body, and `RigidBody(mass: 5)`
   changes one field. `fynkel codegen` reads the defaults through that
   function and writes them into the Dart constructors. A component
   without a `Default` keeps `required` fields. `copyWith` is unchanged.

7. **Motion and animation from code.** Tweens on top of coroutines:
   `transform.moveTo(target, seconds, ease: Ease.outCubic)`, `rotateTo`,
   `scaleTo`, and a general `tween(from, to, seconds, onValue)`, each
   returning the coroutine so it can be awaited or stopped. Clips built
   in code (`ClipBuilder` → a runtime clip, or saved as a document through
   `authoring.dart`) come after the Animator state machine (roadmap
   Phase 5), which they must be able to drive.

8. **The acceptance test is a whole game.** A new example,
   `examples/code_only/`, is a small level in the Playground's style (a
   player, coins, a gate, a switch, a hazard, a HUD, Menu → Level →
   Results). It has **no scene or prefab JSON written by hand or by
   Python**: only models, textures and audio, plus Dart. One variant
   builds the level live (`world.build`) and another writes its scenes
   and loads them. Both pass the same headless test, driven by an input
   recording that collects every coin. Every checklist row also gets its
   own test in `packages/fynkel/test/code_first/`, over the real native
   library.

## Consequences

- The ABI grows by the clone command, the runtime material and mesh
  registrations and the asset path lookup. Each is a separate crate
  commit, as for every shared-crate change.
- `package:fynkel` gains `authoring.dart`, a pure-Dart document writer
  with no native dependency. It must stay in step with the document
  schemas, and its output is checked by `fynkel validate` in its tests,
  so drift fails a test rather than a user's project.
- Component defaults change every generated Dart constructor, and nothing
  on the wire: the schema fingerprint does not move. Every `const X(...)`
  in the repository can get shorter.
- `children` / `parent` changing type breaks existing game code once,
  before 1.0, while the only games are in this repository.
- `playground.py` and its siblings stay until `examples/code_only/`
  passes. Whether the reference game moves off them is a separate
  decision: the user now edits the Playground in the editor
  (`c7e23f9`), so its scene document may already be the source of
  truth.
- Procedural meshes and code-built clips are the last rows. They are
  cheap natively but need a data format in Dart (vertex layouts, keyed
  tracks) that deserves its own look.

## Order

It goes before the Phase 3 extensibility ADRs, because the user ranks
code-first above the editor:

1. Component defaults, `children`/`parent` as objects, the find helpers
   (small, unblocks everything after).
2. Generated asset constants, then the runtime path lookup.
3. `ObjectSpec` + `world.build` (code prefabs).
4. `clone`, `createPrimitive`, `createMaterial`.
5. Tweens.
6. `authoring.dart` scene and prefab writing.
7. `examples/code_only/` end to end, including the acceptance test.
8. `createMesh`, code-built clips (after the Phase 5 Animator).

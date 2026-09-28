# ADR-0035: Built-in components hold object references, not names

**Status:** Accepted 2026-09-28 (the user confirmed it; docs/foundation.md 3.5)  
**Date:** 2026-09-28  
**Related:** ADR-0006 (documents hold ids, overrides never address by name), ADR-0018 and its 2026-09-24 amendment (script `GameObject?` fields), ADR-0019 (`lineTo` gizmo), ADR-0022 (one-root prefabs, derived ids), ADR-0026 (code-first), ADR-0012 (schema codegen), ADR-0033 §5 (IK targets)

## Context

In Unity a field that points at another object holds a **reference**:
`Follow.target` is a `Transform`, not a string. When a prefab is
instantiated, a reference to an object inside that prefab points at the
**copy's own** object, so ten enemies each follow their own head. A
reference to an object that is destroyed becomes "missing" and nothing
searches for a replacement.

Fynkel has two kinds of reference today:

- **Script fields** (`GameObject?` and script-typed `@Expose` fields,
  ADR-0018's amendment) already work the Unity way. The document stores
  `{ "entity": id }`. Prefab expansion rewrites a local id to the
  instance's derived id, and a runtime instantiate resolves the id
  against that instance's own map.
- **Built-in components** hold a **name**:
  - `fynkel.Follow.target` and `fynkel.ThirdPersonCamera.target` are
    `String`s. Every tick they are resolved **world-wide** to the entity
    with that name that has the lowest handle.
  - `fynkel.Skin.skeleton` is a `String` resolved under the object's
    tree root.
  - The character driver finds "its" camera by looking for the rig whose
    `target` equals the character's name.

The names cause real problems, measured by the foundation pass:

1. **Every instance binds to the same object.** Two instances of a
   prefab whose `Follow` targets "Head" both follow the first "Head"
   ever spawned.
2. **Renaming an object silently breaks the link.** Two objects with the
   same name make the link ambiguous.
3. **It costs time.** A target that is missing costs a scan of the whole
   world every tick. The objects benchmark (`benchmarks/objects`, 50 000
   entities) spends:
   - **750 ms** on the first tick after 8 333 instances, resolving their
     followers;
   - **1.4 ms** of every idle tick checking the cached names;
   - that is most of what an idle tick costs now (1.7 ms).
4. **Cross-checking is impossible.** Nothing validates a name at author
   or cook time.

## Decision

1. **A new field type, the object reference.** The schema system gets an
   authored field type `ObjectRef`:
   - **In a document** it is stored like a script's `GameObject?` field:
     `{ "entity": "<uuid>" }`, or `null` for None.
   - **At runtime and on the wire** it is an optional entity handle
     (`OptionalEntity`, `0` = None).
   - **Codegen** emits `GameObject?` in Dart and `Option<Entity>` in
     Rust. The document codecs convert between the two forms, so
     "authored records cannot hold entity handles" still holds for
     every other type.
2. **These fields become references:**
   - `Follow.target`;
   - `ThirdPersonCamera.target`;
   - `Skin.skeleton`, which is Unity's `SkinnedMeshRenderer.rootBone`;
     None means the object's tree root, as an empty name does today.
3. **Resolution happens once, at load.** A load or an instantiate:
   - spawns the entities;
   - maps each referenced id to its handle, through the map it already
     builds (`LoadedScene::handle_of`, the same pass as script
     references);
   - then applies the components with the handles filled in.
   No system resolves anything per tick after that. An id that is not in
   the load leaves the field None and reports the load warning
   `FYN-B0003`, as for scripts.
4. **Prefabs.** A reference to an object inside the prefab points at each
   instance's own copy:
   - for scenes, through the existing id rewriting in `prefab::expand`;
   - for runtime instantiates, through the per-instance map;
   - nested prefabs compose as their ids already do.
   A prefab **asset** cannot hold a reference to a scene object: the
   Inspector refuses it, as Unity's does. A scene **override** on an
   instance can reference a scene object, also as in Unity.
5. **A destroyed target is missing, never replaced.** The component keeps
   the stale handle; the system skips it:
   - Follow stays where it is;
   - the camera rig holds its last pose;
   - the skin keeps its bind pose.
   Nothing searches the world. Script code sees `target == null` (the
   handle is checked when read), like Unity's destroyed-object `null`.
6. **Setting from code** is an assignment, as in Unity:
   `follow.target = player;`, sent as an ordinary `SetComponent` with the
   handle.
   - The code-first builder (ADR-0026) takes an already-built
     `GameObject`. `examples/code_only` builds the player before its
     camera.
   - `world.find(name)` stays for code that wants a name lookup, as
     `GameObject.Find` does in Unity.
7. **The character's camera** is the rig whose `target` is the
   character's entity, found by comparing handles, not names.
8. **Format version 7, with a migration.** `fynkel migrate` (and the
   automatic load-time migration) turns each of these name strings into
   a reference:
   - **In a prefab**, the name is looked up among the prefab's own
     entities first, which is the fix. It falls back to None with a
     migration note if absent.
   - **In a scene**, it is looked up among the scene's entities in
     document order, including those inside prefab instances, which get
     their derived ids.
   - An **ambiguous name** picks the first match and writes a note.
   - An **unknown name** becomes None and writes a note.
9. **The editor.** These fields become Unity object fields: the existing
   `ObjectField` with picker, Hierarchy drop and × to clear. It shows
   `None (Transform)` or `Missing (Transform)`. The `lineTo` gizmo draws
   to the referenced object. The `kind: 'entity'` name hint goes away
   with its last users.

**Not changed:**
- Animation clip channels and the Animator Controller's IK bone names
  stay **paths relative to the animator**, as Unity's animation paths and
  Avatar bones do. They are per-instance already, because they are
  resolved under the animator.
- IK `target`/`pole` names in a controller asset stay as ADR-0033 §5
  decided: an asset cannot reference a scene object in Unity either, and
  code sets a target with `setIKTarget`.
- Cross-scene references: two additively loaded scenes cannot reference
  each other's objects in the document, as in Unity. Code can still
  assign one.

## Consequences

- A prefab's internal links work per instance. The per-tick name lookups
  go away, which removes Follow's 750 ms first tick and most of its idle
  cost in the objects benchmark.
- Renaming an object never breaks a link. Duplicate names no longer
  matter to these components.
- **Breaking, before 1.0:**
  - the Dart types of three fields change (`String` → `GameObject?`);
  - the wire fingerprint changes;
  - documents move to format version 7.
  The migration converts every document in the repo. The user's
  git-excluded copies (`hero.prefab.json`, `rg_hero`, `code_only_try`)
  need `fynkel migrate`, as with version 6.
- A name that matched nothing, or matched the wrong object, now shows as
  None or Missing in the Inspector instead of failing silently at
  runtime.
- Code that set these fields to a name string must set an object
  instead. `world.find` is the direct replacement.

## Plan

1. **Field type.** Add `ObjectRef` to the schema, the codegen (Rust and
   Dart codecs, JSON Schema) and the fingerprint, with tests for both
   forms.
2. **Loader.** The loader resolves `ObjectRef` fields at load and
   instantiate, reports `FYN-B0003`, and handles prefab expansion. The
   failing test first: two instances of a prefab whose `Follow` targets
   its own "Head" follow their own heads.
3. **Runtime.** Follow, camera rigs, the character-to-camera lookup and
   Skin read handles; delete `resolve_named` and `named_targets`.
4. **Migration.** Migration 6 → 7 plus the repo's documents; notes for
   ambiguous and unknown names.
5. **Dart.** The Dart API (`GameObject?`), the code-first builder,
   `examples/code_only`, the tests and docs.
6. **Editor.** Object fields in the Inspector and a `lineTo` by
   reference.
7. **Benchmark.** Rerun the objects benchmark and mark foundation 3.5.

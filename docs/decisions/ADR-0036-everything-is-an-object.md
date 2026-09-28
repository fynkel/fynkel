# ADR-0036: Everything is an Object, as in Unity

**Status:** Accepted 2026-09-28 (the user accepted it with the recommended answers below)  
**Date:** 2026-09-28  
**Related:** ADR-0035 (built-in components hold object references), ADR-0018 and its 2026-09-24 amendment (script `GameObject?` and script-typed fields), ADR-0026 (code-first, `ObjectSpec`), ADR-0027 (data assets), ADR-0016 (scripts and codegen), ADR-0012 (schema codegen)

## Context

In Unity almost everything a game works with is a `UnityEngine.Object`:
a `GameObject`, every component (`Transform`, `Rigidbody`, a
`MonoBehaviour`), and every asset (`Material`, `Mesh`, `AudioClip`, a
prefab, a `ScriptableObject`). They all share a few things:

- a **name**;
- an **identity**: two variables hold the same object, not two copies;
- **one kind of field**: `public Rigidbody body;`, `public Material
  mat;` and `public Enemy boss;` are all object fields. The Inspector
  shows the same picker for each, filtered by type, and accepts drag and
  drop from the Hierarchy or the Project;
- **`== null` after `Destroy`**: a reference to a destroyed object reads
  as null, so `if (target == null)` is the check everywhere.

ADR-0035 made built-in components hold object references instead of
names. The user asked to go the whole way: "everything should be a
reference like how unity works … everything is an Object we can
reference and use."

What Fynkel has today:

| Unity | Fynkel today | A script field of that type? |
|---|---|---|
| `GameObject` | `GameObject` (a live handle) | ✅ `GameObject?`, stored `{ "entity": id }` |
| `MonoBehaviour` | `Behaviour` (a live object) | ✅ `Enemy?`, stored `{ "entity", "component" }` |
| `Transform` | `TransformRef` (a live handle) | ❌ |
| `Rigidbody`, `Light`, `Collider`… | a **value record** (`get<RigidBody>()` returns a copy; `set` writes one back) | ❌ |
| `ScriptableObject` | `DataAsset` (a live object) | ✅ `Settings?`, stored `{ "asset": id }` |
| `Material`, `Mesh`, `AudioClip`, prefab… | `AssetRef`, an **untyped** value (uuid + sub) | ⚠️ `AssetRef` only: the picker can't tell a mesh from a sound |
| `Object` | nothing in common | — |

Four gaps are left:

1. **There is no common base.** Code can't take "any object", there is
   no one `name` or `isAlive` to rely on, and the Inspector has several
   picker code paths.
2. **A script can't reference a built-in component.** Unity's `public
   Rigidbody body;` has no equivalent. A script holds a `GameObject?`
   and calls `get<RigidBody>()`.
3. **Asset fields are untyped.** A field meant for a material accepts a
   sound.
4. **Some references are still names.**
   - The `Switch` scripts in `examples/code_only` and
     `examples/reference_game` hold the gate as a `String` with
     `@Expose(kind: 'entity')`. That is the last user of the `entity`
     hint.
   - Code-first specs can't point at each other. The scene writer skips
     reference fields, because a spec has no id to write until it is
     built.

A fifth difference can't be fully fixed. Dart can't make a destroyed
object `== null` as Unity's C# does, so the ADR has to choose what a
reference to a destroyed object reads as.

## Decision

### 1. One base: `FynkelObject`

A new abstract class, `FynkelObject`, is Unity's `UnityEngine.Object`.
It is named so to avoid clashing with Dart's `Object`. It has:

- `name`;
- `isAlive`: false once destroyed, or for an asset once unloaded;
- `destroy()`: what that means depends on the kind (see §5);
- identity equality: the same object is `==` only to itself, never by
  value.

Every engine object extends it:

```
FynkelObject
├── GameObject
├── Component            (gameObject, transform)
│   ├── Transform        (today's TransformRef, renamed)
│   ├── Behaviour        (scripts, unchanged apart from the base)
│   └── NativeComponent<T>   (a built-in component on one object, §2)
└── Asset                (ref: the AssetRef it points at)
    ├── DataAsset        (unchanged apart from the base)
    ├── Material, Mesh, Texture, AudioClip, AnimationClip, Prefab, Shader…
```

Unity's `GetComponent<T>()` works for every component kind:
`gameObject.getComponent<Enemy>()` returns the script, and
`gameObject.getComponent<RigidBody>()` returns the `NativeComponent`.
The value API stays: `get<RigidBody>()` / `set(...)` keep working, and
the handle is built on them.

### 2. References to built-in components

`NativeComponent<T>` is a live handle to component `T` on one object:

- `value` reads the record, with read-your-writes as `get<T>()` has
  today; `value = …` writes it;
- `gameObject`;
- `isAlive`: the object is alive and still has `T`.

A script field `NativeComponent<RigidBody>? body` is an object field
that only accepts objects that have a `RigidBody`. It is stored as `{
"entity": id }`: an object has at most one of each built-in component,
so the field's type already names the component. Loads, prefab
instances and `clone` resolve it exactly like a `GameObject?` field.

A later option, not decided here, is **generated per-type handles**
(`body.velocity = v` instead of `body.value = body.value.copyWith(...)`).
Codegen could emit them from the same schema. They would sit on top of
`NativeComponent<T>` and would not change documents.

### 3. Typed assets

The generated `Assets` class returns typed assets instead of bare
`AssetRef`s:

- `Assets.materials.crate` is a `Material`;
- `Assets.models.crate.meshes.crate` is a `Mesh`;
- `Assets.audio.jump` is an `AudioClip`;
- and so on for textures, clips, prefabs and shaders.

Each typed asset is a `FynkelObject` that wraps its `AssetRef` (`.ref`).
Rules:

- **Documents don't change.** An asset field is still stored as `{
  "asset": id, "sub"?: … }`. The field's declared type is written into
  `behaviours.g.json`, so the Inspector's picker and `fynkel check` only
  accept that kind of asset.
- **`AssetRef` stays** as the untyped escape hatch and as the value
  inside built-in component records, which are wire data. The API
  accepts either form where it takes an asset today, such as
  `MeshRenderer(mesh: …)` or `ObjectSpec(prefab: …)`.

### 4. Code-first specs reference each other

An `ObjectSpec` gets `object`, a `GameObject` that stays **pending**
until the spec is built. A script field can hold it before anything
exists:

```dart
final gate = ObjectSpec('Gate', behaviours: [Gate()]);
final pad = ObjectSpec('Switch', behaviours: [Switch()..door = gate.object]);
```

A script instance inside a spec is already an object, so a script-typed
field can hold it directly (`final g = Gate();` then `Switch()..gate =
g`).

- `world.build` resolves every pending reference to the built object.
- The scene writer writes the spec's id, as it does for any other
  object. The writer no longer skips reference fields.
- A reference to a spec that is not in the same build is an error at
  build time.

The `Switch` scripts then hold `Gate? gate` (a script reference, as
`public Gate gate;` in Unity). Their scenes are rewritten once, by the
same name-to-id rule the 6 → 7 migration used. The `kind: 'entity'`
hint is removed from `@Expose`: nothing uses it, and a `String` field is
a string.

### 5. Destroyed objects

Dart can't make `obj == null` true for a live variable, so:

- **Every reference field reads null after its target is destroyed.**
  - Built-in component fields already do (ADR-0035).
  - For script fields, the runtime clears the references to the objects
    it destroyed, at the end of the tick that destroyed them. The
    registry already knows every reference field of every script, since
    the generated code reads and writes them for `clone`.
  - The cost is paid only on ticks that destroy something, and is
    measured in the objects benchmark.
  - Unity's `if (target == null)` then works as written.
- **A local variable** (not a field) can't be cleared. `isAlive` is the
  check there, and using a destroyed object throws a clear
  `StateError`, as `GameObject` does today.
- **What `destroy()` does for each kind:**
  - a `GameObject` destroys its subtree (unchanged);
  - a `Behaviour` or `NativeComponent` removes that component;
  - a `Transform` can't be destroyed (Unity refuses it too);
  - an `Asset` can be destroyed only if code made it at runtime
    (`destroyMaterial`, `destroyMesh` today). An imported asset refuses.

### 6. The Inspector

The Inspector has **one object field** for every `FynkelObject` type:

- the ⊙ picker has a Scene tab and an Assets tab, filtered by the
  field's type;
- you can drop an object from the Hierarchy or an asset from the
  Project;
- a click pings the object;
- it shows None, or Missing when the id does not resolve.

A `NativeComponent<T>` field accepts an object that has `T`, as a script
field accepts an object that has that script.

### What does not change

- Built-in components stay **value records** on the wire and in the
  ECS. `NativeComponent<T>` is a Dart handle over them, not a new
  storage model.
- The document format (`formatVersion` 7), the wire and the fingerprint
  don't change. Only `behaviours.g.json` gains the declared types of
  fields.

## Consequences

- Scripts read like Unity scripts: `NativeComponent<RigidBody>? body`,
  `Material? highlight`, `Gate? gate`, `if (target == null)`.
- Every reference goes through one resolve path, whether it comes from
  a document, a prefab instance, a `clone` or a code-first spec, and
  shows in one Inspector field.
- **Breaking changes for scripts:**
  - `TransformRef` is renamed `Transform`. Today's `Transform` value
    record becomes `Pose`; that rename touches every script.
  - The typed `Assets` getters change type. Code that passes them where
    an `AssetRef` is expected keeps working if the API takes both;
    otherwise `.ref` is needed.
- Clearing the fields of destroyed objects adds a pass to ticks that
  destroy something. The pass is kept small by only visiting scripts
  that have reference fields.

## Answers (2026-09-28)

The user accepted the ADR with the recommended answers:

1. **The base is `FynkelObject`.**
2. **The live handle is `Transform`**, Unity's name (today's
   `TransformRef`). The value record `fynkel.Transform` is `Pose` in
   Dart: position, rotation and scale. Documents and the schema keep the
   name `fynkel.Transform`, so only Dart code changes.
3. **Per-type component handles** (`body.velocity = v`) come later, on
   top of `NativeComponent<T>`.

## Implementation notes

- **Typed assets end in `Asset`** (`MaterialAsset`, `MeshAsset`,
  `TextureAsset`, `AudioClipAsset`, `AnimationClipAsset`, `PrefabAsset`,
  `SceneAsset`, `ShaderAsset`, `AnimatorControllerAsset`,
  `VolumeProfileAsset`, `PhysicsMaterialAsset`, `SkinAsset`, `RigAsset`).
  Flutter and `dart:ui` already own `Material`, `Texture`, `Shader` and
  `Scene`, so Unity's bare names would clash in every Flutter file.
- **A typed asset is also an `AssetRef`** (it extends it), not a wrapper:
  every API that takes an `AssetRef` takes it with no `.ref`, and it
  equals a plain `AssetRef` of the same asset. `ref` returns itself.
- A model, a data asset and a blend stay plain `AssetRef`s in
  `assets.g.dart`: there is no one kind to type them with.

## Plan

As in the foundation pass, each step starts with a failing test.

1. **The base.** Add `FynkelObject`, `Component` and `Asset`, and make
   `GameObject`, `Behaviour`, the transform handle and `DataAsset` extend
   them. Add `getComponent<T>` for scripts.
2. **Destroyed references read null.** The failing test: a script whose
   `GameObject?` and `Enemy?` fields point at an object destroyed this
   tick reads null on the next tick. Then clear fields at the end of the
   tick and benchmark it.
3. **Built-in component references.** Add `NativeComponent<T>`, the
   generator's field kind, load and `clone` resolution, and the
   Inspector field.
4. **Typed assets.** Emit typed getters in `assets.g.dart`, add declared
   field types in `behaviours.g.json`, filter the picker, and make
   `fynkel check` report a mismatch.
5. **Code-first references.** Add `ObjectSpec.object`, build resolution
   and the writer. Convert both `Switch` scripts to `Gate? gate` and
   their scenes, and remove `kind: 'entity'`.
6. **One object field in the Inspector**, for every type.
7. **Docs.** Update `docs/design/dart-sdk.md`, `docs/unity-parity.md`
   rows, `examples/code_only`, and the `AGENTS.md` template.

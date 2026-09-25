# Code first

What a Unity user does from C# with no editor open, what Fynkel does
from Dart today, and the order the gaps are closed in. Started
2026-09-25 after the user said working fully from code is "the most
important part of the whole game engine". The decisions are in ADR-0026.
Unity is the reference, as in `docs/unity-parity.md`.

Every row that is fixed gets a test in `packages/fynkel/test/code_first/`
that runs over the **real native library** and checks what a player would
see or what the game would read, not only that a call returned. The
whole pass is accepted by `examples/code_only/`: a small game with no
scene or prefab JSON written by hand or by a script outside Dart
(ADR-0026 §8).

Status: ✅ works as in Unity · 🟡 works but differs · ❌ missing or broken
· 🔧 fixed in this pass (with the commit).

## Tier 1 — the object graph and the small things every script touches

| | Unity (C#) | Fynkel (Dart) |
|---|---|---|
| Make an empty object | `new GameObject("Crate")` | ✅ `world.spawn(name: 'Crate')` |
| Add a script | `AddComponent<Player>()` | ✅ `obj.addBehaviour(Player())` |
| Add a built-in component | `AddComponent<Rigidbody>()` with defaults | 🔧 `add(const RigidBody())`, `RigidBody(mass: 5)` — the Rust `Default`s, as Add Component writes them (ADR-0026 §6) |
| Get / set / remove a component | `GetComponent<T>()` | ✅ `get<T>()`, `set`, `remove<T>()` |
| Script lifecycle | Awake … OnDestroy | ✅ `onCreate` … `onDestroy` |
| Coroutines, Invoke | `StartCoroutine`, `Invoke` | ✅ `b92178e` |
| World transform, `LookAt`, `forward`, `Translate` | | ✅ `2e0183f` |
| Keys, `Debug.Log`, one script throwing | | ✅ `6b25b52`, `9bdf3d6`, `faf648d` |
| Parent and children | `transform.parent`, `GetChild(i)` are objects | 🔧 `parent` is a `GameObject?`, `children` a `List<GameObject>`, both showing this tick's `setParent` and spawns; the handles stay as `parentEntity` / `childEntities` (ADR-0026 §5) |
| Find by name / tag / path | `Find`, `FindWithTag`, `transform.Find("a/b")` | ✅ `find`, `findWithTag`, `findChild`, `findDescendant` |
| Find a script or component anywhere | `FindObjectOfType<T>()` | 🔧 `findObjectOfType<T>()` / `findObjectsOfType<T>()` for scripts and native components; `findObjectWith<T>()` / `findObjectsWith<T>()` for the objects carrying a native component |
| Search down / up the tree | `GetComponentInChildren<T>()`, `InParent` | 🔧 `getInChildren<T>()`, `getAllInChildren<T>()`, `getInParent<T>()`, self first, inactive skipped unless `includeInactive` |
| Another script on the same object | `GetComponent<Other>()` | ✅ `sibling<T>()` |
| Shared game state | a singleton / ScriptableObject | ✅ `context.install(GameState())`, `context.service<T>()` |
| Use an object the frame it spawned | yes | 🟡 almost every call works, `children` and `parent` included; `entity` (the runtime handle) and `findChild` / `findDescendant` wait for the next tick |
| SetActive | | ✅ |

## Tier 2 — assets and making things at runtime

| | Unity (C#) | Fynkel (Dart) |
|---|---|---|
| Name an asset in code | a serialized field, or `Resources.Load("prefabs/Burst")` | 🔧 `Assets.prefabs.burst` (generated `lib/generated/assets.g.dart`, kept current by the editor; `f4902e3`, `10914af`) or `world.assets.find('prefabs/burst')` for a path known only while running (`c9cf929`) (ADR-0026 §3) |
| Spawn a prefab | `Instantiate(prefab)` | ✅ `world.instantiate(ref)` |
| Copy a live object | `Instantiate(obj)` | 🔧 `obj.clone({parent, transform, name})`: made now, scripts copied through their registered fields, references inside the copy re-pointed (`6988733`, `97d0c5b`, ADR-0026 §4) |
| A cube / sphere / plane | `GameObject.CreatePrimitive` | 🔧 `world.createPrimitive(Primitive.cube)` or `ObjectSpec.primitive(...)`, with Unity's colliders; every cook ships the shapes (`b5dba47`, `c76c0bb`) |
| A material made in code | `new Material(shader)`, `material.color = …` | 🔧 `world.createMaterial(PbrMaterial(baseColor: …))` returns an `AssetRef` for `MeshRenderer`; `setMaterial` / `destroyMaterial` change and drop it (`b5dba47`, `c76c0bb`) |
| A mesh made in code | `new Mesh { vertices = … }` | ❌ |
| Load a scene | `SceneManager.LoadScene("Level1")` | 🔧 `scenes.transition(Assets.scenes.level1)`, or `world.assets.find('scenes/level1')` |
| Additive scenes | | ✅ `LoadMode.additive` |

## Tier 3 — building scenes and prefabs from code

| | Unity (C#) | Fynkel (Dart) |
|---|---|---|
| Build a level in code at start-up | `new GameObject` in a loop | 🔧 `world.build(ObjectSpec('Level', children: [...]))` with default components and `Assets.…` looks (`3795afb`) |
| A reusable object built in code | a method that builds it | 🔧 a function returning an `ObjectSpec`, built with `world.build(spec, parent:, transform:)`; `ObjectSpec.prefab(ref, …)` places a prefab asset with overrides (`3795afb`, ADR-0026 §2) |
| Save a scene or prefab from code | editor scripts (`PrefabUtility.SaveAsPrefabAsset`) | ❌ the reference game uses Python (`tools/playground.py`, 881 lines) |
| Move / rotate / scale over time | DOTween, or a coroutine by hand | 🔧 `transform.moveTo(p, 0.5, ease: Ease.outCubic)`, `moveLocalTo`, `rotateTo`, `scaleTo`; `tween(from, to, seconds, onValue)` on a script or the world; each a `Coroutine` to yield, `await tween.done` or `stop()` (`f8e090d`, ADR-0026 §7) |
| Play, crossfade, blend animations | Animator | ✅ `animation.play`, `crossfade`, `setParameter`, markers |
| An animation clip made in code | `AnimationClip.SetCurve` | ❌ after the Phase 5 Animator |

## Already good (no row needed)

UI is Flutter, bound to `GameModel` fields (`docs/hud.md`). Physics,
raycasts and the character controller, input actions and rebinding,
audio, scene transitions, and headless tests with a hand-driven clock
(`TestGameContext`, `cookProject`, input recordings) all work from
code as well as, or better than, Unity's.

## Log

- 2026-09-25 — checklist written from the scan (ADR-0026 proposed).
- 2026-09-25 — Tier 1 object graph (231945a, b1c375b).
- 2026-09-25 — assets by name: `assets.g.dart`, the editor regenerating it, the reference game using it (abf2787), `world.assets`.
- 2026-09-25 — `ObjectSpec` + `world.build`, code prefabs (3795afb).
- 2026-09-25 — `createPrimitive`, `createMaterial` (c76c0bb) and `GameObject.clone` (97d0c5b): step 4 done.
- 2026-09-25 — tweens (f8e090d): step 5 done.

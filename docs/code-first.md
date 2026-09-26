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
| A material made in code | `new Material(shader)`, `material.color = …`, `material.SetFloat(…)` | 🔧 `world.createMaterial(PbrMaterial(baseColor: …))` returns an `AssetRef` for `MeshRenderer`; `setMaterial` / `destroyMaterial` change and drop it (`b5dba47`, `c76c0bb`). A `.shader`'s: `world.createShaderMaterial('shaders/water.shader', values: {'deep': Vec3(…)})`, then `world.setMaterialValue(ref, 'dissolve', t)` — every frame is fine, it rewrites the uniform only; on a cooked shader material too; checked against the shader as the cook checks (ADR-0031) |
| A mesh made in code | `new Mesh { vertices = … }` | 🔧 `world.createMesh(MeshData(positions: …, triangles: …))` returns an `AssetRef` for `MeshRenderer` and a mesh `Collider`; normals and tangents computed when missing, `MeshData.grid(height: …)` for terrain; `setMesh` re-uploads it and rebuilds its colliders, `destroyMesh` drops it (`05020c3`, `e6a9324`, `485db00`) |
| Game data as an asset | a ScriptableObject | 🔧 a `@FynkelDataAsset` class extending `DataAsset` is a type, a `.data.json` one instance; a script's `Weapon? weapon` / `List<Weapon>` field is set at load to the one shared instance per asset, and `world.data<Weapon>(ref)` reads one from code (`d0c9606`, ADR-0027); `Weapon(damage: 3)` made in code is one with no file; `examples/code_only`'s coins are two `CoinKind` assets written from Dart (`5d738d0`) |
| Load a scene | `SceneManager.LoadScene("Level1")` | 🔧 `scenes.transition(Assets.scenes.level1)`, or `world.assets.find('scenes/level1')` |
| Additive scenes | | ✅ `LoadMode.additive` |

## Tier 3 — building scenes and prefabs from code

| | Unity (C#) | Fynkel (Dart) |
|---|---|---|
| Build a level in code at start-up | `new GameObject` in a loop | 🔧 `world.build(ObjectSpec('Level', children: [...]))` with default components and `Assets.…` looks (`3795afb`) |
| A reusable object built in code | a method that builds it | 🔧 a function returning an `ObjectSpec`, built with `world.build(spec, parent:, transform:)`; `ObjectSpec.prefab(ref, …)` places a prefab asset with overrides (`3795afb`, ADR-0026 §2) |
| Save a scene or prefab from code | editor scripts (`PrefabUtility.SaveAsPrefabAsset`) | 🔧 `package:fynkel/authoring.dart`: `ProjectWriter(dir).writeScene('scenes/level', SceneSpec('Level', specs))` `writePrefab('prefabs/lamp', spec)` and `writeMaterial('materials/brass', PbrMaterial(…))` (`84c15ea`) from the `ObjectSpec`s `world.build` takes — ids stable across runs, the file untouched when nothing changed, objects added in the editor kept, prefab instances as overrides; `fynkel validate` accepts it (`4a9dab8`, ADR-0026 §2); in a Fynkel package (`fynkel.package.json`) it writes the package's `assets` with ids of the package's own, and a prefab instance may name a package's prefab (ADR-0029) |
| Save game data from code | `AssetDatabase.CreateAsset(weapon, path)` | 🔧 `ProjectWriter(dir).writeData('data/sword', Weapon()..damage = 3)`: id stable across runs, the file untouched when unchanged; the instance then knows its asset, so a scene or data asset written afterwards names it, and one never written is an `ArgumentError` (ADR-0027 §8) |
| A nested value in a script or data asset | a `[Serializable]` class | 🔧 a `@FynkelRecord()` class exposed alone (`Wave boss = Wave();`) or in a `List<Wave>`: a JSON object of its fields, new instances on every load and clone, a foldout in the Inspector (`3fbb3e8`, ADR-0027 §5) |
| Start with nothing loaded | a bootstrap scene holding one script | 🔧 a project may name no `startupScene`: `loadProject` opens it empty — input, layers, tags, gravity and the cooked assets bound — for a game that builds its world (`53f62a8`, `6291e61`); `fynkel test --headless` and `fynkel render` then need `--scene` |
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
- 2026-09-25 — `authoring.dart` writing scenes and prefabs (4a9dab8; generated `toJson` d7cdc22; `fynkel validate` resolving the built-in shapes b7d4629): step 6 done.
- 2026-09-25 — `examples/code_only/`, the acceptance game: Menu → Level → Results with a player, five coins, a switch and gate, a sweeper and a HUD; built live or loaded from the scenes, materials and prefab `tool/scenes.dart` writes, both ways passing one headless test driven by an input route built in Dart. Engine fixes it needed: a project with no startup scene (53f62a8, 6291e61), `writeMaterial` (84c15ea), a body command before a destroy in one batch dropped (197a501). Step 7 done.
- 2026-09-25 — `createMesh` (05020c3 runtime meshes in the store and renderer, e6a9324 colliders following them, 485db00 Dart): step 8's first half done; code-built clips wait for the Phase 5 Animator.

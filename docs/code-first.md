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
| Add a built-in component | `AddComponent<Rigidbody>()` with defaults | ❌ every field spelled out: `RigidBody` takes six (ADR-0026 §6) |
| Get / set / remove a component | `GetComponent<T>()` | ✅ `get<T>()`, `set`, `remove<T>()` |
| Script lifecycle | Awake … OnDestroy | ✅ `onCreate` … `onDestroy` |
| Coroutines, Invoke | `StartCoroutine`, `Invoke` | ✅ `b92178e` |
| World transform, `LookAt`, `forward`, `Translate` | | ✅ `2e0183f` |
| Keys, `Debug.Log`, one script throwing | | ✅ `6b25b52`, `9bdf3d6`, `faf648d` |
| Parent and children | `transform.parent`, `GetChild(i)` are objects | ❌ `children` is `List<int>`, `parent` is `int?`; a caller has to `world.attach(e)` (ADR-0026 §5) |
| Find by name / tag / path | `Find`, `FindWithTag`, `transform.Find("a/b")` | ✅ `find`, `findWithTag`, `findChild`, `findDescendant` |
| Find a script anywhere | `FindObjectOfType<T>()` | 🟡 `world.behaviours.whereType<T>()` works; no named helper |
| Search down / up the tree | `GetComponentInChildren<T>()`, `InParent` | ❌ |
| Another script on the same object | `GetComponent<Other>()` | ✅ `sibling<T>()` |
| Shared game state | a singleton / ScriptableObject | ✅ `context.install(GameState())`, `context.service<T>()` |
| Use an object the frame it spawned | yes | 🟡 most calls work; `entity`, `children`, `parent` throw until the next tick |
| SetActive | | ✅ |

## Tier 2 — assets and making things at runtime

| | Unity (C#) | Fynkel (Dart) |
|---|---|---|
| Name an asset in code | a serialized field, or `Resources.Load("prefabs/Burst")` | ❌ a UUID string: `const burstPrefab = '88641ff3-…'` (ADR-0026 §3) |
| Spawn a prefab | `Instantiate(prefab)` | ✅ `world.instantiate(ref)` |
| Copy a live object | `Instantiate(obj)` | ❌ (ADR-0026 §4) |
| A cube / sphere / plane | `GameObject.CreatePrimitive` | ❌ only through the editor's GameObject menu (ADR-0023) |
| A material made in code | `new Material(shader)`, `material.color = …` | ❌ materials are files only |
| A mesh made in code | `new Mesh { vertices = … }` | ❌ |
| Load a scene | `SceneManager.LoadScene("Level1")` | 🟡 `scenes.transition(id)` works, but by UUID |
| Additive scenes | | ✅ `LoadMode.additive` |

## Tier 3 — building scenes and prefabs from code

| | Unity (C#) | Fynkel (Dart) |
|---|---|---|
| Build a level in code at start-up | `new GameObject` in a loop | 🟡 possible with `spawn` + `set`, but every look needs a UUID and every component every field |
| A reusable object built in code | a method that builds it | ❌ no description to build from: `ObjectSpec` + `world.build` (ADR-0026 §2) |
| Save a scene or prefab from code | editor scripts (`PrefabUtility.SaveAsPrefabAsset`) | ❌ the reference game uses Python (`tools/playground.py`, 881 lines) |
| Move / rotate / scale over time | DOTween, or a coroutine by hand | 🟡 by hand in a coroutine; no tweens (ADR-0026 §7) |
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

# Scenes at runtime, zones, settings (M3-12)

The reference for blueprint §19.2d item 12: how a running world loads,
stacks and unloads scenes, what a `fynkel.TriggerZone` is, how runtime
prefabs relate to scenes, and the user's `settings.json`. The Rust
contract is `crates/fynkel_core/src/load.rs` (tests
`crates/fynkel_core/tests/scenes.rs`), the ABI
[hosts.md](hosts.md) § Scenes at runtime, the Dart SDK
`packages/fynkel/lib/src/scenes.dart` and `settings.dart` (tests
`packages/fynkel/test/scenes_test.dart` — the item's gate).

## Loading

A scene is a cooked document in the project's cache, loaded by asset id.
The world holds a **list of loaded scenes** in load order, each
remembering the entities it spawned.

| Call | What happens |
| --- | --- |
| `World::load_scene(desc)` / `load_scene_with(desc, LoadMode::Replace)` — ABI `fynkel_world_load_scene`, Dart `world.loadScene(id)` | **Replace**: every loaded scene is unloaded, last first (`SceneUnloaded` each), then every remaining root subtree is destroyed — runtime prefabs, spawned objects, everything but the objects kept with `dontDestroyOnLoad` (below) — then the scene loads and `SceneReady { scene }` is emitted. |
| `LoadMode::Additive` — ABI `fynkel_world_load_scene_with(…, FYNKEL_LOAD_ADDITIVE)`, Dart `loadScene(id, mode: LoadMode.additive)` | The world is kept; the scene joins it. The same id twice is refused (`LoadError::AlreadyLoaded`, `FYN-R0011`) and nothing changes. |
| `World::unload_scene(id)` — ABI `fynkel_world_unload_scene`, Dart `unloadScene(id)` | The scene's entities that are **still alive** are destroyed with their subtrees (`Destroyed` events, then one `SceneUnloaded { scene, entities }` counting what went); the scene is forgotten and its assets released from residency. Entities spawned since — a behaviour's `spawn`, an `instantiate`d prefab, an `Emitter`'s instances — are not the scene's and stay. A scene the world does not hold is `LoadError::NotLoaded` (`FYN-R0010`). |

Rules that follow:

- A broken scene loads nothing and leaves the world as it was — a replace
  load checks the description first, before unloading anything.
- Ids are per load: loading the same scene after unloading it gives new
  entities. Handles of destroyed entities (Dart `GameObject`) go dead on
  the next event drain; a `Behaviour` on one gets `onDestroy`.
- Events arrive on the general lane in this order on the next drain:
  `DESTROYED` per entity, `SCENE_UNLOADED` per unloaded scene, then
  `SCENE_READY` for the new one. `SceneReady` now names its scene.
- Residency: a replace load clears the world's residency list but for
  what the kept objects use (ADR-0037), an unload drops the scene's token; prefab instances hold their own tokens
  (released with a replace load, kept by an unload).
- Input rebinds (`fynkel_world_bind_actions`, below) sit over the
  project's actions again after **every** load.

### Objects kept across loads (DontDestroyOnLoad, ADR-0037)

`world.dontDestroyOnLoad(obj)` — a root `GameObject`, or a component
(its object) — keeps the object and everything under it across scene
loads, as Unity's `DontDestroyOnLoad`:

```dart
@override
void onCreate() => world.dontDestroyOnLoad(this); // the music player lives on
```

- A replace load leaves it, and so does unloading the scene it came
  from. Its scripts and their fields stay as they are; no `onDestroy`.
- It takes effect at once, so a script can call it and then
  `scenes.transition` in the same callback. An object not spawned yet is
  kept the moment the tick makes it.
- Only a root: on a child it does nothing and logs a warning, as Unity
  does. Children made under a kept root later go with it. A kept root
  given a parent stops being kept (it belongs to its new root).
- `destroy()` ends it. `world.persistentObjects` lists the kept roots;
  `LoadReport.persistent` counts the ones a load left standing. They are
  in no loaded scene (`world.loadedScenes` does not list them).
- What the kept objects use (meshes, materials, clips, sounds, their
  dependencies) stays resident across the load.
- During Play the editor's Hierarchy shows them last, under a
  **DontDestroyOnLoad** header.

Engine: `World::dont_destroy_on_load(entity)` (a child is
`LoadError::NotARoot`), `is_persistent`, `persistent_roots`; ABI
`fynkel_world_dont_destroy_on_load(world, entity)`. Tests:
`crates/fynkel_core/tests/scenes.rs` (`dont_destroy_on_load_keeps_a_root_across_loads`),
`crates/fynkel_native/tests/load.rs` (`dont_destroy_on_load_through_the_abi`),
`packages/fynkel/test/scenes_test.dart`, the editor's
`test/play_document_test.dart`.

The Dart `SceneManager` (`world.scenes`) is the flow on top:
`load`/`unload` now (not from inside a callback), `transition(to, mode:)`
performed at the start of the **next** `World.frame` — the one point
where no behaviour callback is running, so a behaviour can ask for the
next level from `fixedUpdate` or a zone callback without pulling its own
object from under itself; it returns a `Future<LoadReport>` (the report,
or the `FynkelLoadError`). `state` is `empty | pending | ready | failed`
(`addListener`), `current` the scene of the last replace load (the
startup scene after `loadProject`), `loaded` / `world.loadedScenes` what
the world holds, `error` the last failure, `cancel()` drops a pending
request. `LoadReport.mode` and `.scenes` say what a load did.

## Zones

`fynkel.TriggerZone { event: String, once: bool }` (component 20) next
to a `fynkel.Collider` with `is_sensor: true` turns the physics-lane
trigger enter/exit into a **named general-lane event**: `ZONE_ENTERED`
(7) / `ZONE_EXITED` (8), body `fynkel.ev.Zone { zone, other, event }` —
`zone` is the zone's entity, `other` what came in or left, `event` the
zone's name. Produced right after the step, alongside the physics-lane
`TriggerEnter`/`TriggerExit` (which stay). A sensor overlaps dynamic
**and** kinematic bodies (the character controller's capsule walking
into a static pickup fires it; [physics.md](physics.md) § Contacts).
`once: true` fires one enter and then removes the `TriggerZone` (the
collider stays; no exit) — a pickup. A zone without a sensor collider is
the `FYN-H0009` warning from `fynkel validate` (it never fires); a blank
event is a value error at authoring and at the tick (`INVALID_PAYLOAD`).

From Dart: `Behaviour.onZoneEnter(ZoneEvent)` / `onZoneExit` on the
behaviours of **both** the zone's object and the visitor's, and
`world.zones.on('collect', (ZoneEntered e) { … })` /
`onExit(name, cb)` / `off(name)` anywhere — all run right after the tick
whose step produced them (the same point as the contact callbacks), with
the world's context current. Every zone event also reaches the next
`fixedUpdate`'s `events` like any general-lane event.

## Runtime prefabs

`World.instantiate(prefab, transform:, parent:, name:)`
([hosts.md](hosts.md) § Prefabs at runtime) spawns a cooked prefab now
and returns its **root** (ADR-0022: the instance is the root, no extra
entity) — an authored one by its id, or an imported model's own
hierarchy by `AssetRef(source, sub: World.importedPrefabSub)` (M3-19a). The spawn belongs to **no scene**: an unload of the scene
it was spawned in leaves it; a replace load destroys it with everything
else. Destroy it yourself (`root.destroy()`, the whole instance) when it is
collected.

## Pooling

Objects made and destroyed many times a second (bullets, sparks,
enemies) can come from a pool instead — Unity's
`UnityEngine.Pool.ObjectPool<T>`, in `package:fynkel` as
`ObjectPool<T>` (`packages/fynkel/lib/src/pool.dart`, tests
`packages/fynkel/test/pool_test.dart`):

```dart
final bullets = ObjectPool.gameObjects(() => world.instantiate(Assets.prefabs.bullet));

final b = bullets.get();          // switched on; a new one only when the pool is empty
b.transform.position = muzzle;
// … later, instead of b.destroy():
bullets.release(b);               // switched off, kept for the next get()
```

- `ObjectPool<T>(create:, onGet:, onRelease:, onDestroy:,
  collectionCheck:, maxSize:)` is the plain pool, for anything: `get()`
  hands out the last released object, or a new one from `create`;
  `release(x)` gives it back; `countAll`, `countActive`,
  `countInactive`; `clear()` destroys what waits in it.
- `ObjectPool.gameObjects(create)` sets it up for GameObjects: `get`
  switches the object on (`setActive(true)`), `release` switches it off,
  and one the pool lets go is destroyed. A pooled script resets itself in
  `onEnable` (it gets `onDisable`/`onEnable` at the next tick, as with any
  `setActive`).
- `prewarm(count)` makes objects ahead of need — before a level starts,
  so the first shots cost nothing — and puts them away switched off.
  Unity's `defaultCapacity` only reserves room.
- Releasing an object the pool already holds throws (`collectionCheck`,
  on by default). Past `maxSize` (10 000) a released object is destroyed.
- An object destroyed while it waits in the pool — a replace load took
  it, or the game destroyed it — is dropped by the next `get`, never
  handed out.

## Settings

One `settings.json` per user under the platform's application-support
directory (`schemas/settings.schema.json`, generated from the
`fynkel.Settings` record; the Rust reader/writer
`fynkel_document::settings`, the Dart `SettingsDocument`/`SettingsStore`):

```json
{
  "fynkel": "settings",
  "formatVersion": 1,
  "master_volume": 1.0,
  "music_volume": 0.5,
  "sfx_volume": 1.0,
  "muted": false,
  "quality": "high",
  "input": { "rebinds": { "jump": ["keyboard:J", "gamepad:south"] } }
}
```

No envelope: the record's fields are the top level, every one optional
(an older file reads with defaults), `input.rebinds` maps an action to
the bindings that replace the project's for this user (the grammar of
[input.md](input.md)); an action absent keeps the project's. Reading is
lenient on missing fields and strict on shape and range (`FYN-U0001` not
a settings document, `U0002` a value out of range or a rebind of the
wrong shape, `U0003` cannot read or write); `fynkel_document::settings::
validate(Some(&ProjectContext))` also warns on an unknown quality
profile or action. Writing goes through a temporary file and a rename so
a crash keeps the old file.

Dart: `SettingsStore.open(path, world: world)` — usually
`SettingsStore.defaultPath(appName)`: `~/Library/Application Support/
<app>/settings.json` on macOS, `$XDG_CONFIG_HOME/<app>` on Linux,
`%APPDATA%\<app>` on Windows; a Flutter host on iOS/Android passes the
sandbox path it knows — reads the file once (missing = defaults; invalid
= reported through `onError`, defaults used) and lays its rebinds over
the world's actions (`world.bindActions(rebinds)` →
`fynkel_world_bind_actions`); `update(doc)` / `edit((s) => s.copyWith(
musicVolume: 0.5).rebind('jump', ['keyboard:J']))` validates, writes the
file at once, re-applies the rebinds and runs listeners. It is the
`GameContext.settings` service (`world.useSettings(store)`; a memory-only
`SettingsStore()` is the default, what tests get). The audio gains are
values a host reads from `current` — there is no audio engine yet
(M3-11); the quality profile is what the Flutter viewport should select
(the starter's Q key writes it and reads it back at launch).

## Gate (item 12)

`packages/fynkel/test/scenes_test.dart`: a headless world loads the
fixture's Playground, instantiates the Coin prefab in the Player's path,
replays `test/platform.input.json`; the Player's behaviour gets
`onZoneEnter("collect")` (once; the hub callback too), destroys the coin
and asks `scenes.transition(results)`; the next frame holds Results
alone (Playground's 5 entities and the coin gone, the Player's handle
dead, its behaviour destroyed); a `SettingsStore` on a fresh file saves
a change with a rebind, a fresh store reads it back, and the next load
reports `jump` rebound. The Rust side of the same story is
`crates/fynkel_native/tests/load.rs`
(`scenes_stack_unload_replace_and_zones_fire_through_the_abi`).

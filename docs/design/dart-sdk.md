> **Living design draft** — extracted from blueprint v0.3 §21. Revisit at the end of M1 once the command/snapshot overlay is real. Where this conflicts with an ADR, the ADR wins. Known v0.4 impacts: the tick order is §5.1 of the blueprint; `Input`/`Time` facades resolve via `GameContext`; math types per ADR-0013; the native component catalogue per ADR-0011 belongs in §21.4; HUD state via Dart observable models per ADR-0008.

## 21. Fynkel Dart SDK and standard component specification

The **Fynkel Dart SDK** is the main public programming surface of Fynkel Engine. It must feel familiar to Unity users without copying Unity's API mechanically, and it must preserve the engine's native ownership and batched-bridge design. A developer should be able to understand common gameplay code without knowing Rust, wgpu, Rapier, or the internal ECS layout.

### 21.1 SDK design principles

The SDK must be small at the core, strongly typed, discoverable in an IDE, deterministic about object lifetime, and explicit when an operation is asynchronous or deferred. High-level convenience APIs may exist, but they must compile down to batched commands, local snapshots, generated bindings, or engine-managed handles rather than hidden per-property native calls.

Prefer composition over deep inheritance. `GameObject` is a stable handle to an entity, `Component` describes data or native capability, and `Behaviour` is a Dart-authored gameplay type with lifecycle callbacks. The public API may use familiar Unity-like naming where it improves onboarding, but engine semantics must remain internally consistent even when they differ from Unity.

A suggested top-level SDK layout is:

```text
package:fynkel/
  fynkel.dart
  math.dart
  scene.dart
  object.dart
  components.dart
  input.dart
  physics.dart
  animation.dart
  audio.dart
  rendering.dart
  assets.dart
  time.dart
  diagnostics.dart
  testing.dart
```

Keep Flutter-specific helpers in a separate package such as `package:fynkel_flutter/` so gameplay logic and headless tests do not depend on widget APIs.

*Implemented (M3-01):* `package:fynkel_flutter` (`packages/fynkel_flutter`) is that package — the Path A viewport plugin (`FynkelFlutterPlugin` for macOS + iOS), `FynkelViewportController`, the `FynkelViewport` widget and `FynkelGameView`, which owns the Ticker and calls `World.frame` at the §5.1 points before rendering. A game's `lib/main.dart` is `FynkelGameView(world: game.world, controller: FynkelViewportController(placeholders: false), overlay: <HUD>)` (the viewport draws from the store the world's `loadProject` opened; `assetCache` is only for a world that loaded nothing; `controller.quality = 'low'` switches to one of the project's quality profiles at runtime, M3-05); `package:fynkel` itself imports nothing from Flutter (`docs/hosts.md`, "Flutter host").

### 21.2 Core runtime objects

The minimum public object model should include:

| Type              | Responsibility                                       |
| ----------------- | ---------------------------------------------------- |
| `Fynkel`          | Process/runtime entry point and global capabilities  |
| `Game`            | User application lifecycle and startup configuration |
| `World`           | Runtime entity/component container                   |
| `Scene`           | Loaded scene instance and scene lifecycle            |
| `GameObject`      | Stable Dart handle to an ECS entity                  |
| `ComponentRef<T>` | Typed access/view over a native component            |
| `Behaviour`       | Dart gameplay logic attached to a GameObject         |
| `AssetRef<T>`     | Stable typed asset reference                         |
| `PrefabRef`       | Reference to a prefab asset                          |
| `Time`            | Frame and fixed-step timing data                     |
| `Input`           | Named actions and device state                       |
| `EngineEvent`     | Structured runtime event base type                   |

A `GameObject` must not own native component memory. It stores only the minimum identity required to address the runtime entity safely, for example world ID, entity index, and generation. Invalid or destroyed handles must fail predictably with development diagnostics rather than aliasing a new entity.

Suggested usage:

```dart
final player = world.findByName('Player');
final transform = player.get<Transform>();
transform.position = const Vec3(0, 1, 0);

player.add(AudioSource(clip: assets.audio('jump')));
player.destroy();
```

The exact syntax can change before 1.0, but the semantic contract should be stable: reads come from a coherent snapshot or local staged state; writes become commands applied at a documented boundary.

**Implemented (M3-07a, `packages/fynkel/lib/src/world.dart`, `behaviour.dart`; the wire in `crates/fynkel_native/src/bridge.rs`).** `ComponentRef<T>` is not a class: the generated component classes themselves (`MeshRenderer`, `Camera`, `DirectionalLight`, `PointLight`, `SpotLight`, `Environment`, plus `Transform` and `Name`, all immutable values with `copyWith`) are what a `GameObject` reads and writes, and a `Behaviour` forwards the same four calls:

```dart
final mesh = get<MeshRenderer>()!;                       // null when the object has none
set(mesh.copyWith(visible: false));                      // a command at the next tick; reads back now
if (!has<PointLight>()) set(const PointLight(color: Vec3(1, .5, .2), intensity: 800, range: 12));
remove<SpotLight>();                                     // Transform and Name cannot go
world.spawn(name: 'Twin', transform: t)..add(mesh)..add(light);   // writes wait for the spawn
```

The contract as implemented: **reads** are the value the last step left, fetched through `FYNKEL_QUERY_COMPONENTS` once per object per tick on the first read (every native component framed by a `ComponentHeader { component, size }`, decoded through the generated `componentCodecs` table — `World.componentsOf(entity)` is the raw list), merged with this tick's staged writes so a value `set` reads back before the tick applies it; `get<Transform>()` is `localTransform` (the snapshot/overlay path). **Writes** are `fynkel.cmd.SetComponent` (the header record + the component's own bytes, one command for every component kind) and `fynkel.cmd.RemoveComponent` on the simulation lane — `setPresentation(value)` uses the presentation lane, where plain data applies immediately — validated by the world exactly as the loader's values are (`INVALID_PAYLOAD` for a negative light range, a removed `Transform`, a `Name` sent as a value); a rejection is a diagnostic on the next step and the object's read falls back to what the world has. There is no mutable Dart mirror of a native component (blueprint §3): the per-object map lives for one tick and is cleared with the transform overlay after every step and on a rejection. Reads are one FFI query per object per tick — right for a behaviour on its own object, not for a loop over thousands (snapshots and future typed views are for that). Since the M3 allocation-gap pass (2026-09-22) the query's bytes are copied into a buffer the object keeps (re-allocated only when the length changes) and a read decodes **only the component it asks for** from that copy, the ids present read from the headers as a bitmask — `get<ThirdPersonCamera>()` on an object with six components allocates one record, not six; `components` decodes them all. `GameObject.components` lists what the object has; `componentIds` maps every component class to its id, so `get<T>()` with a type that is not a native component is an `ArgumentError`. `RigidBody`/`Collider` (M3-08) join the same path by being added to `ComponentValue`.

**Implemented (M3-08b, `packages/fynkel/lib/src/world.dart`, `diagnostics.dart`; `crates/fynkel_core/src/world.rs` for the temp references, `load.rs` for `instantiate`).** The everyday lookups and the spawn-and-configure pattern the spec's `findByName` sketch assumes: `World.find(name)` / `findAll(name)` and `GameObject.findChild('Arm/Hand')` (direct children per segment, scene order) / `findDescendant(name)` (depth first) read the names and the hierarchy **as the last tick left them** — the SDK fetches the whole entity table (`FYNKEL_QUERY_ENTITIES`, one `EntityInfoRecord { entity, parent, name }` per live entity) at most once per tick and per load, so a rename or reparent staged this tick shows up next tick — and since the allocation-gap pass it re-decodes the table (names, parents, children) only when the query's bytes differ from the retained copy of the last decode, so a `find` every tick in a world whose structure stands still costs one FFI call and a byte comparison, no allocation (a `Spawned`/`Destroyed` event is not what invalidates it: a prefab `instantiate` emits none); the results are attached `GameObject`s (the same handle for the same entity). A `GameObject` still pending its spawn can be **configured in the tick of the spawn**: `spawn()` returns immediately, and `add`/`set`/`remove`/`setLocalTransform`/`setParent`/`destroy` and every body command on it, or a further `spawn(parent: it)`, go into the same simulation batch addressed by a *temp reference* (`Entity::temp(id)`: generation 0, the spawn's temp id) that the world resolves against that tick's spawns before applying the command — so `final crate = spawn(); crate.add(dynamicBody); crate.add(unitBox); crate.applyImpulse(v)` in one `fixedUpdate` lands in one tick (the earlier "waits for the spawn" queue and the pending-object `StateError` are gone). A temp reference that no spawn earlier in the tick's batch carries, or one sent on the presentation lane, is `INVALID_TEMP_ID`. `World.instantiate(prefab, {transform, parent, name})` (the `PrefabRef` of the table is the prefab's asset id as text or an `AssetRef`; since M3-19a `AssetRef(source, sub: World.importedPrefabSub)` names an imported model's own hierarchy — the world keys it by the source's id, any other sub is an `ArgumentError`) builds a cooked prefab **now** and returns its root `GameObject` (ADR-0022: the instance is the root, no extra entity; resolved on return, `name` or the root's own name, at `transform` under `parent`) — exactly the shape a scene's prefab instance expands to — with the prefab's authored behaviours attached from `world.registry` like a scene's; `lastInstantiate` is the `InstantiateReport { prefabId, prefabPath, name, root, entityCount, rejectedComponents, assetCount, behaviours, warnings }`, a refusal (no project loaded, not a prefab, not in the cache, a pending or dead parent) throws `FynkelInstantiateError`/`StateError` and spawns nothing. Prefabs may nest (documents.md § Nested prefabs); the cook flattens them, so an instantiated prefab arrives as one flat tree. Tests: `components_test.dart` (`findTests`, spawn-then-add in one tick), `physics_test.dart` (a body added and pushed in the spawn tick), `prefab_test.dart`.

**The object graph as objects (ADR-0026 §5, 2026-09-25).** `GameObject.parent` is a `GameObject?` and `children` a `List<GameObject>` (sibling order), both read-your-writes: a `setParent` this tick shows at once (moved away: gone; moved in or spawned under: last), and a pending object's children are the spawns and moves under it. The world's own view as handles stays as `parentEntity` / `childEntities` (throw while pending; `World.parentOf`/`childrenOf` unchanged). Finding by type, Unity's shape: `World.findObjectOfType<T>()` / `findObjectsOfType<T>()` take a behaviour type (callback order) or a native component type (entity order, the value); `findObjectWith<T>()` / `findObjectsWith<T>()` give the objects carrying a native component, since a native component is a value that does not know its object; `GameObject.getInChildren<T>()` / `getAllInChildren<T>()` / `getInParent<T>()` look at the object itself first, then depth first in sibling order or up the parents. All skip objects inactive in the hierarchy unless `includeInactive: true`, count a behaviour whose `destroy` was asked for as gone, and include this tick's spawns; an entity no handle names yet is asked for its components without being attached unless it matches. They walk every behaviour or entity — not for every frame. Tests: `test/code_first/object_graph_test.dart`.

**Assets by name (ADR-0026 §3, 2026-09-25).** Code that knows an asset while it is written names it through the generated `Assets` class (`lib/generated/assets.g.dart`, [codegen.md](../codegen.md)): `world.instantiate(Assets.prefabs.burst)`, `scenes.transition(Assets.scenes.results)` — ids underneath, so a rename keeps working and a delete is a compile error at the next codegen. A path known only while running goes through `World.assets` (`AssetIndex`, Unity's `Resources.Load`): `find('prefabs/burst.prefab.json')` or without the extension when one file has that name (two throw `StateError`), optionally `sub:`; `pathOf(ref)`; `all` (`CookedAsset { id, path, kind }`). It reads the cooked cache's `index.json` of the last load (re-read when a cook rewrites it; a `StateError` before any load), so it sees what the last cook saw. Tests: `test/code_first/assets_test.dart`.

**Objects described in code (ADR-0026 §2, 2026-09-25).** `ObjectSpec(name, {transform, active, tags, layer, components, behaviours, children})` describes an object tree; `ObjectSpec.prefab(ref, {name, …})` an instance of a prefab asset whose `components`, `behaviours`, `tags`, `layer` and `active` apply to the instance's root on top of the prefab's, and whose `children` come after the prefab's. `World.build(spec, {parent, transform})` spawns it and returns the root: new objects are pending as with `spawn` (their tree, components and scripts readable at once), a prefab instance is made at once as with `instantiate` — under an object built in the same call, at the root and then `setParent` under it, its transform staying local. `transform:` replaces the root spec's. The whole spec is checked first (a behaviour instance already attached or listed twice, an unknown tag or layer, a sub-asset that is not `prefab`), so a mistake throws before anything spawns. Behaviours are instances (`Spinner()..speed = 2`), so a spec holding behaviours builds once: a **code prefab** is a function returning a spec, called once per copy. The same spec is what `authoring.dart` will write as a document (step 6). Tests: `test/code_first/build_test.dart`.

**The transform (Unity parity, 2026-09-24; `packages/fynkel/lib/src/world.dart` `TransformRef`, `math.dart`).** `transform` reads the way a Unity script does: `position` and `rotation` are **world** values (the parent's world pose composed with the local one; the same as before for a root object), `localPosition` / `localRotation` / `localScale` the parent-relative ones, `lossyScale` the world scale (read only), `scale` stays an alias of `localScale`, and `value` is still the whole local `Transform`. A world write is turned into the local write the command carries. Reads include this tick's writes — the object's and its ancestors' — over the parent chain as the last tick left it or as `setParent` / `spawn(parent:)` set it this tick (the entity table, once per tick; an ancestor without a handle is attached). Also: `parent` (a `TransformRef`, null for a root), `forward` / `right` / `up` (the object's axes in the world, forward −Z; setting `forward` turns the object), `eulerAngles` / `localEulerAngles` in **degrees** — the Inspector's Rotation row, applied about X, then the resulting Y, then the resulting Z (ADR-0013 amendment 2026-09-24) — `translate(v, {relativeTo: Space.self | Space.world})` (self: along the object's own axes, its scale not counted), `rotate(eulerDegrees, {relativeTo})`, `rotateAround(point, axis, degrees)`, `lookAt(point, {worldUp})` / `lookAtObject(go)`, `transformPoint` / `inverseTransformPoint` / `transformDirection` / `inverseTransformDirection`; on `Quat`, `Quat.euler(degrees)`, `eulerAngles`, `Quat.lookRotation(forward, [up])`, `inverse`; on `Vec3`, `scaled`. A child of a rotated, non-uniformly scaled parent composes scale per axis, as Unity's `lossyScale` approximates. `GameObject.worldTransform` is still the last snapshot's. Tests: `packages/fynkel/test/transform_test.dart`, `sdk_test.dart` (a reparent that keeps the world position).

**Implemented (M3-03, `packages/fynkel/lib/src/world.dart`, `diagnostics.dart`, `testing.dart`).** Scenes come from the cooked cache only: `World.loadProject(dir, {cacheDir})` opens `cacheDir` (default `World.cacheDirOf(dir)` = `<dir>/.fynkel/cache`) and loads the project's startup scene; `World.loadScene(cacheDir, sceneId)` loads one cooked scene by its asset id (a non-UUID is a `FynkelBridgeError`). Neither reads a source document, nothing in `package:fynkel` cooks — `fynkel cook`, `fynkel run` or `fynkel test` do — and a missing, stale or incomplete cache is a `FynkelLoadError` whose `FYN-R` diagnostics name the asset id and suggest `fynkel cook`. The `LoadReport` carries `sceneId`/`sceneName`/`scenePath`/`sceneKey` (the cache key of the object that was loaded), `entities`, `rejectedComponents`, `assetCount`, `cacheDir`/`cacheProfile`/`projectName`/`startupScene`, `projectDir` (when loaded through `loadProject`), the attached `behaviours` and `ResidencyStats { retained, meshes, textures, materials, bytes }` of the world's store after the load. A `package:fynkel/testing.dart` gives tests the cook step: `cookProject(dir, {cacheDir, force})` runs the `fynkel` CLI (`FYNKEL_CLI`, then PATH, then `cargo run -p fynkel_cli` in the checkout the package was resolved from) and `runFynkel(args)`; the SDK tests and the `fynkel create` starter's `test/game_test.dart` cook into a temporary cache in `setUpAll`.

### 21.3 Behaviour model

The authoritative lifecycle is defined once in **§3.2**. The SDK implements that exact contract and this section does not define a second lifecycle.

Collision, trigger, animation, asset, and input notifications should use typed events/subscriptions where possible instead of expanding the lifecycle into dozens of callbacks.

Example target:

```dart
@FynkelBehaviour(id: 'game.PlayerController')
class PlayerController extends Behaviour {
  @Expose(min: 0, step: 0.1)
  double moveSpeed = 6.0;

  @Expose()
  GameObjectRef? cameraPivot;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    final move = Input.action2D('move');
    final body = gameObject.get<RigidBody>();

    body.setPlanarVelocity(
      move.x * moveSpeed,
      move.y * moveSpeed,
    );
  }

  @override
  void afterPhysics(AfterPhysicsContext context) {
    // Optional: react to the completed fixed physics step.
  }
}
```

`Input.action2D(...)` is intentionally convenient. The static-looking facade resolves to the `InputService` belonging to the active `GameContext`; it is not mutable process-global state. Unit/headless tests can provide a deterministic test context and input service.

Generated metadata provides behaviour factories, field serialization, Inspector metadata, stable type IDs, migration hooks, and schema descriptions. Runtime reflection is not required.

**Implemented (M2-15, `packages/fynkel/lib/src/behaviour.dart` and `context.dart`).** `Behaviour` carries the §3.2 callbacks (`onCreate`, `onEnable`, `start`, `fixedUpdate(FixedUpdateContext)`, `afterPhysics(AfterPhysicsContext)` behind `wantsAfterPhysics`, `update(UpdateContext)`, `lateUpdate`, `onDisable`, `onDestroy`), `enabled`, `order`, `destroy()`, `gameObject`/`transform`/`world`/`context`. `GameObject.addBehaviour(b)` attaches; `World.frame` drives the world's behaviours at the §5.1 points: per tick `fixedUpdate` (with the non-physics events drained since the last tick — each event reaches exactly one `fixedUpdate`) → `step` → `afterPhysics` (with the tick's contacts) ; per frame `update` then `lateUpdate`. The rules as implemented: lifecycle transitions (create, enable, disable, destroy) are applied only at safe boundaries — the start of the fixed phase and of the update phase — so a behaviour attached, disabled or destroyed inside a callback changes state at the next boundary and receives no further callbacks in the current pass; `onCreate` waits for a pending spawn to resolve; `start` runs once, right before the behaviour's first active gameplay callback; a disabled behaviour receives no gameplay callbacks and re-enabling does not restart it; destroying the object (`GameObject.destroy()`, the entity dying at the tick) disables and destroys its behaviours at the next boundary; callback order is `order` (lower first), then attach sequence — never names or scene order. `GameContext` (`time`, `diagnostics`, `install<T>`/`service<T>`) is made current by the world around every callback; `Time.*` and `Diagnostics.log` resolve through `GameContext.current` and throw outside one; `world.context = TestGameContext(time: TestTime(...))` installs a hand-driven clock and recorded logs, and `context.run(() => …)` runs code under a context outside the world. `BehaviourRegistry` holds `BehaviourType { id, version, create, fields: [BehaviourField { name, type, get, set, defaultValue, min, max, step, label, tooltip }] }` — what M2-16 generates from `@FynkelBehaviour`/`@Expose` — and `instantiate(id, fields:)` builds a behaviour from authored data without reflection (`BehaviourDataError` for an unknown type/field or a rejected value; `GameObject.addBehaviourFromData` attaches one). `World.attach(entity)` gives loaded scene entities a `GameObject` so behaviours can be attached to them. `Input` waits for the M3 input contract; `RigidBody` and the other component views for their systems.

**Implemented (M2-16, `packages/fynkel_generator`).** The `BehaviourType`s are generated: a `build_runner` builder reads every `@FynkelBehaviour(id:, version:)` class and its `@Expose(label:, tooltip:, min:, max:, step:)` fields and writes `lib/generated/behaviours.g.dart` — one `<id>Type` (`gameSpinnerType`) per class with `create: Spinner.new` and a `BehaviourField` per exposed field whose `get` returns the JSON form and whose `set` converts from it (`double`/`int`/`bool`/`String`, `Vec3` as `[x, y, z]`, `Quat` as `[x, y, z, w]`, `AssetRef` as `{ asset, sub? }`, each optionally nullable; literal initializers become `defaultValue`), plus `behaviourTypes` and `registerBehaviours(registry, {replace})` — and `lib/generated/behaviours.g.json`, the same metadata with a JSON Schema per type that `fynkel codegen --project <package>` splits into `schemas/behaviours/<id>.schema.json` for the document layer (`docs/codegen.md` § Behaviour generator). Build errors name the class and member: not a `Behaviour`, abstract/private, constructor with required parameters, missing or non-namespaced `id`, duplicate id, an `@Expose` field that is final, static, private or of another type. At load, `World.loadScene`/`loadProject` read the report's `scene.behaviours` and, for each, `attach(entity).addBehaviourFromData(type, fields: data, version:)` through `world.registry`: an unregistered type is `FYN-B0001`, rejected data `FYN-B0002` (warnings via `onLoadDiagnostics`; `LoadReport.behaviours`/`behavioursAttached`). The gate lives in `packages/fynkel_generator/example/spinner_game`: the fixture project's `game.Spinner { speed: 2.5 }` becomes a `Spinner` with `speed == 2.5` that rotates its entity. **M4-04 (ADR-0018 accepted)** added the display hints — `@Expose(group, order, header, unit, kind, readOnly, hidden, showIf)` and `@FynkelBehaviour(icon, category, description)` — emitted into `x-fynkel` and the metadata's `fields[]`; the editor's Inspector reads them (`docs/editor.md` § The Inspector), the runtime ignores them.

### 21.4 Standard component catalogue for 1.0

The following component families define the expected 1.0 baseline. Components may be implemented internally by different subsystems, but they must appear coherent in Dart, scene files, schemas, and the Inspector.

#### Core and hierarchy

- `Transform`
- `Name` or built-in object name metadata
- `ActiveState`
- Parent/child hierarchy relationship
- Optional `Tag` and `Layer` metadata if real use cases justify them

`Transform` exposes local position, rotation, and scale; read-only world-space state; explicit local/world setters; parenting with keep-local/keep-world policies; direction helpers; and change observation suitable for tooling. Avoid implicit expensive matrix decompositions in common getters.

Writes have two forms with identical semantics (M2, carry-over C1): the value form (`setLocalTransform(Transform)`, property setters) for ordinary code, and the allocation-free form for per-tick hot paths — a reusable `TransformWriter` handed to `writeLocal`/`writePresentation`, or scalar `setLocalPosition(x, y, z)`/`setLocalRotation`/`setLocalScale` where unspecified parts keep their current value. Both are read-your-writes; the value types (ADR-0013) remain the only read surface. Measured: 25 moving objects cost ≈1.5 KB/frame with the writer form versus ≈5 KB with values.

#### Rendering

- `Camera`
- `MeshRenderer`
- `SkinnedMeshRenderer`
- `DirectionalLight`
- `PointLight`
- `SpotLight`
- `Environment` (one per scene: constant ambient, optional equirectangular sky — M3-04; `docs/renderer.md`)
- Optional `ReflectionProbe` only if implemented and documented before 1.0

`MeshRenderer` references mesh and material assets and exposes visibility, shadow participation, layer/mask, and bounds-related properties. Do not expose raw wgpu resources.

`Camera` should define projection, clear mode, viewport, clipping planes, render layer mask, HDR/AA settings where appropriate, and priority/order. Since M3-04 it carries exposure: `exposureMode` (`auto` meters the scene's sun/ambient, `manual` uses `exposure` in EV100). Avoid embedding Flutter view ownership directly into the camera component.

#### Physics

- `RigidBody`
- `BoxCollider`
- `SphereCollider`
- `CapsuleCollider`
- `MeshCollider` with explicit static/dynamic limitations
- `CharacterController` or `KinematicCharacterController`

Physics components expose engine concepts rather than Rapier types. Collision groups, trigger/sensor state, material/friction/bounce, mass properties, continuous collision configuration, and query participation need stable engine-level types.

**Implemented (M3-08, `packages/fynkel/lib/src/physics.dart`, `world.dart`, `behaviour.dart`, `events.dart`; `docs/physics.md` is the reference).** Two components instead of four: `RigidBody { bodyType: static | dynamic | kinematic, mass, linearDamping, angularDamping, gravityScale, ccd }` and `Collider { shape: box | sphere | capsule | mesh, size, mesh, offset, friction, restitution, isSensor, layer, mask }` — the shape is a field, not a class, so one `get<Collider>()` reaches any of them; a `Collider` alone is static geometry; one collider per entity in V1; `CharacterController` is item 9. They go through the M3-07a path (`get`/`set`/`add`/`remove`, validated at the tick with the same rules `fynkel validate` runs, `FYN-H0001`–`H0004`). A dynamic body's transform is physics-owned: a presentation write is rejected, a simulation-lane `setLocalPosition` is a teleport, and gameplay drives it with **body commands** on `GameObject` (`applyForce`, `applyImpulse`, `applyTorque`, `applyTorqueImpulse`, `setLinearVelocity`, `setAngularVelocity`, `setKinematicTarget(p, [q])`, `teleport(p, [q])`; `applyForce`/`applyImpulse`/`setLinearVelocity`/`bodyState` also on `Behaviour`) — `fynkel.cmd.Body` on the simulation lane, applied by the tick's physics step after the batch's component writes, so `add(RigidBody)` + `applyImpulse` in one `fixedUpdate` works; a body command on a pending object goes into the spawn's tick through a temp reference (M3-08b; it threw `StateError` before); the spec's `setPlanarVelocity` is `setLinearVelocity` with the y component the caller chooses. `bodyState()` (`BodyState { linearVelocity, angularVelocity, sleeping, speed }`, null without a body) is one `FYNKEL_QUERY_BODY`. **Contacts** are the physics-lane events dispatched right after each tick's step, before `afterPhysics`, to every behaviour of both objects: `onCollisionEnter/Stay/Exit(Contact)`, `onTriggerEnter/Exit(Contact)`; `Contact { a, b, point, normal (a → b), impulse, other(entity), normalFor(entity), involves, isTrigger }`; stay only for subscribed pairs (every `GameObject`). **Queries** on `World` (the `Physics` service of §21.5): `raycast`/`raycastAll(origin, direction, {maxDistance, mask, exclude, hitSensors, maxHits})`, `shapeCast(shape, size, origin, direction, {rotation, …})`, `overlap(shape, size, origin, {rotation, mask, hitSensors, maxHits})` → `PhysicsHit { entity, point, normal, distance }`, one FFI call each; `physicsQuery(List<PhysicsQueryRecord>)` batches. `world.physics` is the project's `PhysicsSettings { gravity, layers, meshColliders, mask(names) }` (also `LoadReport.physics`); `world.layerMask(['props'])` throws on an undeclared name. Tests: `packages/fynkel/test/physics_test.dart`.

**Implemented (M3-09, `packages/fynkel/lib/src/physics.dart`, `world.dart`, `behaviour.dart`; `example/player_controller.dart`; `docs/physics.md` § Character controller).** `CharacterController { radius, height, maxSlope, stepHeight, skinWidth, gravityScale, moveAction, jumpAction, speed, jumpSpeed }` (id 12) replaces the body and collider on its entity (`FYN-H0006` if both are present) and `ThirdPersonCamera { target, distance, minDistance, pivotOffset, yaw, pitch, minPitch, maxPitch, lookAction, sensitivity, mask }` (id 13) goes on the camera — `target` is the player's **name**, an authored component cannot hold an entity. Both go through the M3-07a path. Gameplay drives a character with `GameObject.move(Vec3 velocity)` (a planar velocity per tick, this tick only — the world applies it in the physics step and the character rests when nothing is sent) and `jump([speed])` (only while grounded; `speed` 0 means the component's `jumpSpeed`); `characterState()` (`CharacterState { grounded, sliding, velocity }`, null without a controller) is one `FYNKEL_QUERY_CHARACTER`; `Behaviour` forwards all three. A component with `moveAction`/`jumpAction`/`lookAction` set needs no Dart at all — the world reads the actions in the camera's yaw frame each tick and the jump edge is consumed by exactly one tick; a behaviour's own `move`/`jump` in the same tick wins. The `PlayerController` example (`@FynkelBehaviour(id: 'game.PlayerController')`: `walkSpeed`, `sprintFactor`, `jumpSpeed`, `cameraName`; reads the rig's yaw through `world.find(cameraName)?.get<ThirdPersonCamera>()`, walks with `Input.action2D('move')`, sprints on `sprint`, double-jumps on `Input.pressed('jump')` with the count refilled on landing) is the pattern the spec's §7 sketch asked for. Tests: `packages/fynkel/test/character_test.dart` (the fixture's Playground replayed from `test/platform.input.json`, commands/query/diagnostics, the example).

#### Native catalogue (M3-10)

**Implemented (M3-10, `packages/fynkel/lib/src/systems.dart`, `world.dart`; `docs/catalogue.md` is the reference).** `Rotator { axis, speed, space: MotionSpace.local | world }` (14), `Oscillate { axis, amplitude, frequency, phase, space }` (15), `Follow { target (a name), offset, smoothing, lookAt }` (16), `LookAtCamera { lockY }` (17), `LifetimeDespawn { seconds }` (18), `Emitter { prefab, rate, lifetime, maxAlive, velocity, enabled }` (19) — the §3.5/ADR-0011 catalogue, ticked by the runtime in the pre-tick slot (movers, lifetimes, emitters) and after physics (follow, billboards), authored in scenes or added from Dart through the M3-07a path like any native component; nothing to register, no `Behaviour`. `World.systemTimes()` → `List<SystemTime { system: SystemKind, entities, micros, milliseconds }>` is one `FYNKEL_QUERY_SYSTEMS` (the last step's per-phase wall time, `SystemKind.step` first; the starter HUD's `systems` line), `LoadReport.physics.prefabs` how many prefabs the scene's emitters registered, `DiagnosticCode.prefabUnavailable` (14) the once-per-emitter report for a prefab the world does not hold. The stress project's `game.Wobble` (`benchmarks/stress/wobble.dart`) is the Dart-side twin of `Oscillate` + `Rotator` for the 300-behaviour comparison. Tests: `packages/fynkel/test/catalogue_test.dart` (typed access, the once-only report, the fixture's Catalogue scene with no behaviour, the systems query).

#### Animation

**Implemented (M3-19, `packages/fynkel/lib/src/animation.dart`, `world.dart`, `behaviour.dart`; `docs/animation.md` is the reference).** `Animator { clip, speed, looping, playing }` (21) and `Skin { skin, skeleton }` (22) are native components on the M3-07a path (`get<Animator>()`, `add(const Animator(clip: …))`, `set`, `remove`); the skeleton is ordinary entities under the object (the importer's prefab, or a scene authoring the joints by hand), so a joint is a `GameObject` (`findChild('hips')`, `localTransform`, `worldTransform`). Playback: `GameObject.animation` → `AnimationControl` — `play(clip, {looping, speed})`, `crossfade(clip, seconds, {looping, speed})`, `stop()`, `pause()`, `resume()`, `speed =`, `time =` (one `fynkel.cmd.Animate` each on the simulation lane; a pending object is driven through its temp id) and `state` → `AnimatorState? { clip, time, duration, speed, fadeRemaining, playing, looping, isFading, progress }` (one `FYNKEL_QUERY_ANIMATOR`; `null` without an animator), `isPlaying`; `Behaviour.animation` forwards to its object. Events: `Behaviour.onAnimationEnded(AnimationEnded { entity, clip })` right after each tick's step, with the contact and zone callbacks; the event is also in `drainEvents()`. Rejections (`missingComponent`, `invalidPayload`) are diagnostics, `DiagnosticCode.clipUnavailable` (16) / `skinUnavailable` (17) the once-only reports for a clip or skin the world does not hold; the native layer registers from the cache a clip a behaviour plays that the scene never named. `LoadReport.animation` → `AnimationAssets { clips, skins }`. Clip references are `AssetRef(asset, sub: 'anim/<n>')`, skins `'skin/<s>'`; the reference game's `Player` takes the asset from the `Animator` the scene gave it. Deviation from the line below: there is no controller/graph asset yet — the state machine as data is a later item on the same core; V1 is clips, crossfades and the root-motion policy (none by default: clips in place, the controller moves the entity). **M3-19c (part 1)** added to the same surface: `Animator.weight`/`additive`/`rootMotion` (a second `Animator` on a child object is a layer over its ancestors' pose; `rootMotion` hands a travelling clip's planar delta and yaw to the object's `CharacterController`, else moves the transform), `animation.weight =` and `animation.setParameter(name, value)` (`setWeight`/`setParameter` ops), `AnimatorState.weight/parameter/rootDelta/rootYaw`, and `Behaviour.onAnimationMarker(AnimationMarker { entity, clip, name })` for the named markers a clip carries (a rig document's `markers`, a glTF animation's `extras`), dispatched with `onAnimationEnded`. Tests: `packages/fynkel/test/animation_test.dart`, `examples/reference_game/test/game_test.dart`.

- `Animator`
- `Skeleton`/skin data as imported runtime asset rather than a user-authored component unless necessary

`Animator` references an animation controller/graph asset or a simple clip setup. It must support parameter updates, state queries, events, speed, layer/track concepts only as needed, and root-motion policy.

#### Audio

- `AudioSource`
- `AudioListener`

`AudioSource` provides clip reference, volume, pitch, looping, spatial blend, playback state, bus routing, distance model, and voice priority. Playback handles must have explicit lifetime semantics.

**Implemented (M3-11, `packages/fynkel/lib/src/audio.dart`, `world.dart`, `behaviour.dart`, `events.dart`, `settings.dart`; `docs/audio.md` is the reference).** `AudioSource { clip, bus: AudioBus.master | music | sfx, volume, pitch, looping, playing, priority }` (23) and `AudioListener { volume }` (24) are native components on the M3-07a path (`get<AudioSource>()`, `add(const AudioSource(clip: …, playing: true))`, `set(source.copyWith(looping: true))`, `remove`); validated at the tick with the rules `fynkel validate` runs (`FYN-H0010`). Tier 1 is 2D, so spatial blend and the distance model of the line above wait for M5; the listener is a gain over everything. Playback of an object's source: `GameObject.audio` → `AudioControl` — `play()`, `stop()`, `pause()`, `resume()`, `volume =`, `pitch =` (one `fynkel.cmd.Audio` each on the simulation lane; a pending object through its temp id) and `state` → `AudioState? { clip, bus, playing, paused, time, duration, volume, pitch, looping, voice, progress }` (one `FYNKEL_QUERY_AUDIO`; `null` without a source), `isPlaying`; `Behaviour.audio` forwards to its object. **There is no playback handle for one-shots** — the explicit lifetime the line above asks for is the component's: a source's voice lives with its `playing`, and `world.audio.play(clip, {bus, volume, pitch, priority, presentation})` is fire-and-forget on no entity (`fynkel.cmd.PlaySound`; no event, no handle; `presentation: true` plays now, outside the tick — a menu click). `world.audio` (`AudioSystem`, the `Audio` service of §21.5 as a world member like `scenes`) also has `setBus(bus, volume, {muted})` (applied at submit on the presentation lane), `applySettings(SettingsDocument)` (the three volumes and the mute — `SettingsStore.apply` calls it before the rebinds, so the saved volumes reach the buses at launch), `stats` → `AudioStats { device, sampleRate, channels, voices, maxVoices, started, stolen, refused, ended, clips, callbacks, underruns, suspended, streams, starved }` (one `FYNKEL_QUERY_AUDIO_STATS`; `streams` and `starved` are M3-11 part 2's streamed music — a sidecar's `stream: true`, nothing new to call from Dart: a streamed clip plays through the same source and `play`), and the device: `open()` (false with an `audioDevice` diagnostic when the platform or the build has none; the null backend keeps counting), `isOpen`, `suspend(bool)`, `close()` — `FynkelGameView(audio: true)` drives them, a test never opens one. Events: `Behaviour.onPlaybackEnded(PlaybackEnded { entity, clip })` right after each tick's step with the contact, zone and animation callbacks, for the object's own source's non-looping end (a stop or a steal raises nothing); also in `drainEvents()`. Rejections (`missingComponent`, `invalidPayload`, `wrongLane`) are diagnostics; `DiagnosticCode.soundUnavailable` (18) is reported once per clip per source and on every `play` of a clip the world does not hold — the native layer registers from the cache the clip a `play` or a set `AudioSource` names, so any cooked audio asset plays without the scene naming it. `LoadReport.audio` → `AudioAssets { sounds, clips, streams, device, sampleRate, channels, maxVoices }`. The starter's `Mover` (`audio.play()` on take-off through the cube's source, `world.audio.play(land)` on landing through an `@Expose`d `AssetRef`) is the worked example; the reference game's `Player` (a footstep per `AnimationMarker`, the hit on a death) and `Collectible` (the pickup) and its streamed `Music` source are the acceptance. Tests: `packages/fynkel/test/audio_test.dart`, `packages/fynkel_flutter/test/game_view_test.dart`, the starter's and the reference game's `test/game_test.dart`.

#### World-space UI

- `WorldCanvas`
- Native world-UI child data such as `WorldPanel`, `WorldText`, `WorldImage`, `WorldProgress`, and `WorldButton`, either as components or a dedicated retained UI tree owned by `WorldCanvas`

Do not force Flutter widgets into the native world UI model. Projected Flutter labels are a separate feature from true depth-tested world-space UI.

#### Gameplay bridge

Dart `Behaviour` instances are registered against GameObjects but should not require one native component per behaviour field. A compact native marker/registry relationship is acceptable for scheduling and authoring identity.

### 21.5 Standard engine services and `GameContext`

Services are explicit underneath the public convenience API.

Each running world/game host has a `GameContext` containing service interfaces such as:

- `ScenesService`
- `AssetsService`
- `InputService`
- `PhysicsService`
- `AudioService`
- `TimeService`
- `DiagnosticsService`
- `ApplicationService`

Public convenience facades are allowed:

```dart
final move = Input.action2D('move');
final dt = Time.delta;
await Scenes.load('asset://scenes/level2.scene.json');
```

But those calls resolve through the active context rather than hidden singleton state.

This gives Fynkel both:

- concise game code; and
- testability/replacement of underlying services.

Tests should be able to run:

```dart
final context = TestGameContext(
  input: RecordedInput(...),
  time: FixedTestTime(...),
);
```

Avoid APIs whose behaviour depends on an unreplaceable process-global singleton.

Minimum service capabilities:

- `Scenes`: load, unload, active scene, additive operations where supported
- `Assets`: typed load, preload, release, dependency/progress status
- `Input`: actions, axes, pointer, touch/gamepad, rebinding
- `Physics`: raycast, shapecast, overlap, layer masks
- `Audio`: one-shot playback, buses, listener/global control
- `Time`: frame delta, fixed delta, time scale, frame/tick count
- `Diagnostics`: logs, markers, counters, assertions
- `Application`: platform/lifecycle and quit/restart requests where supported

**Implemented (M3-12, `packages/fynkel/lib/src/scenes.dart`, `settings.dart`, `world.dart`, `behaviour.dart`, `events.dart`, `context.dart`; `docs/scenes.md` is the reference).** The `Scenes` service is `world.scenes` (a `SceneManager`, not a static facade — the generated settings record is named `Settings`, and a world owns its scenes): `state` (`SceneState.empty | pending | ready | failed`), `current`, `loaded` (`List<LoadedScene>`), `isLoaded(id)`, `lastReport`, `error`, listeners; `load(id, {mode})` / `unload(id)` are immediate (they wrap `World.loadScene(Object sceneId, {String? cacheDir, LoadMode mode = LoadMode.replace})` — the M3-03 `loadScene(cacheDir, sceneId)` signature is gone, `cacheDir` defaults to the directory `loadProject`/the last load opened — and `World.unloadScene(id)` → `UnloadReport` / `FynkelUnloadError`), while `transition(id, {mode})` returns a `Future<LoadReport>` that the world performs at the start of the next `World.frame`, so a behaviour can ask for the next scene from inside `fixedUpdate` without pulling the world out from under the tick (`cancel()` fails a pending transition with `StateError`; a load that throws fails the future and leaves `state == failed`). `LoadMode.replace` unloads every loaded scene (last first) and destroys whatever else is alive, runtime spawns included, before loading; `LoadMode.additive` stacks and refuses an id that is already loaded (`FYN-R0011`); unloading a scene that is not loaded is `FYN-R0010`; an unload takes only the scene's still-alive entities, runtime spawns stay. Rebinds (`World.bindActions(Map<String, List<String>>)` → the `LoadReport` whose `BoundAction.rebound` marks the changed actions; an unknown action is ignored, a bad binding is `FYN-P0009`) are re-applied over the project's actions after every load. `TriggerZone { event, once }` (id 20) on a sensor collider turns a physics `TriggerEnter`/`TriggerExit` into the general-lane `ZoneEntered`/`ZoneExited { zone, other, event }` (a sealed `ZoneEvent` in `events.dart`), dispatched right after each tick's step to `onZoneEnter(ZoneEvent)`/`onZoneExit(ZoneEvent)` on the behaviours of both objects and to `world.zones` (a `ZoneEvents` hub: `on(event, callback)` / `onExit(event, callback)` / `off(event)`); `once` removes the component after its first entry. `SceneReady(tickLo, {scene})` now names its scene and `SceneUnloaded(scene, entities)` follows every unload. Settings are the `fynkel.Settings` record (`SettingsDocument` in Dart: `masterVolume`, `musicVolume`, `sfxVolume`, `muted`, `quality`, `rebinds`; lenient on missing fields, strict on kind/version/shape; canonical snake_case `toJson`; `copyWith`, `rebind`, `validate`) behind the `SettingsService` interface, whose one implementation is `SettingsStore` — `SettingsStore()` in memory, `SettingsStore.open(path, {world, onError})` on disk (`defaultPath(appName)` is the platform's application-support location; `load` tolerates a missing or broken file and reports `FYN-U0001`–`U0003` through `lastError`; `save` writes tmp + rename; `update`/`edit` save and, with a world attached, `apply` the rebinds through `bindActions`). It is a mutable `GameContext.settings` field (default a memory store; `World.useSettings(store)` attaches one), not a static facade. Tests: `packages/fynkel/test/scenes_test.dart` (the gate: a `Collector` behaviour destroys the coin its zone entered and transitions to `Results` from `fixedUpdate`; additive/unload/replace/event order/cancel; the settings document and store).

**Implemented (M3-13, `packages/fynkel/lib/src/model.dart`, `projection.dart`; `packages/fynkel_flutter/lib/src/model_binding.dart`, `projection_layer.dart`; `docs/hud.md` is the reference).** HUD state per ADR-0008: `GameModel` (a screen's model; `field(v)` registers, `fields`, `notifyAll`, `dispose`) of `ModelValue<T>` fields (`value` notifies on `!=` only, `force`, `notify`, listeners, `dispose`) — the `ValueNotifier` shape without the Flutter dependency, since `package:fynkel` imports nothing from Flutter; installed as a service (`world.context.install(HudModel())`, `service<HudModel>()`) and written by behaviours from `update`. Flutter binds with `ModelBinding<M>` (an `InheritedWidget` that notifies on instance change only), `ModelBuilder<T>(value:, builder:, child:)` (one field → one rebuild per change) and `field.listenable` (a `ValueListenable<T>`). The screen pattern (menu / HUD / pause / settings / results: a model field the tree switches on, the game's state left in the world, `setPaused` for the loop, `scenes.transition` for the level) is in `docs/hud.md`; the starter's `HudModel`, `Mover.update`, `MoverReadout` and `PauseBanner` are the worked example. World labels are the one snapshot-shaped exception: `world.projection` (`ProjectionView`: `add(target, {offset, occlusionMask})` → `ScreenAnchor { x, y, depth, inFront, onScreen, occluded, missing, noCamera, dropped, visible }`, `setViewport`, `update()` = one `fynkel_world_project` call per frame for every anchor from the frame's camera and interpolated poses, `camera`, `version`, listeners, `prune`, `maxAnchors` 64) and, over it, `ProjectionLayer(world, children: [ProjectedAnchor(target, offset, alignment, nudge, clampToEdges, occlusionMask, child) | WorldLabel(target, text, …)])` — a `Flow` repainting on the view's listener, no rebuilds per frame, unpainted and unhittable when not visible; `FynkelGameView` updates the view after each `World.frame`. Depth occlusion is opt-in per anchor by collider mask (a ray from the camera; mask 0 is never hidden); beyond 64 anchors the rest are `dropped` and `FYN-R0012` (`tooManyAnchors`, informational) is reported once per overflow. Also from this item: a `GameObject` attached this tick reads its transform through the component query before the first snapshot (`_queriedTransform`), so a partial write at the first tick (`transform.rotation = …`) keeps the authored pose instead of seeding from identity — the starter's spinning Cube had been sinking to y 0. Tests: `packages/fynkel/test/projection_test.dart`, `model_test.dart`; `packages/fynkel_flutter/test/projection_test.dart` (the gate), `model_binding_test.dart`.

### 21.6 Async, tasks, and coroutines

Use Dart's native `Future`, `Stream`, and async/await model rather than inventing an engine-specific coroutine language for ordinary asynchronous work. Provide engine-aware helpers for cancellation and scene/object lifetime.

Example:

```dart
final handle = await Assets.load<Model>('asset://models/boss.glb');
if (!gameObject.isAlive) return;
```

For gameplay timing, provide explicit scheduler helpers such as `game.delay(Duration)` or tick-based waits only if they add value. They must be cancellable when the owning scene or behaviour is destroyed. Do not encourage untracked timers that outlive worlds.

**Implemented: coroutines and Invoke (Unity parity, 2026-09-24; `packages/fynkel/lib/src/behaviour.dart`).** The gameplay-timing helpers are Unity's, on `Behaviour`, driven by the world's frame rather than Dart timers — so they are deterministic, run with the world's `GameContext` current, and never outlive the behaviour. `startCoroutine(Iterable<Object?>)` takes a `sync*` generator, runs it up to its first `yield` at once and then resumes it once per frame **after every `update`, before `lateUpdate`** (Unity's place); what it yields says when it goes on — `null` (the next frame), `WaitForSeconds(s)` (game time: the frames' summed `dt`), `WaitUntil(f)` / `WaitWhile(f)`, `const WaitForFixedUpdate()` (after the next tick's `afterPhysics`), a `Coroutine` (until it ends) or a nested generator (run in place, Unity's `yield return StartCoroutine(…)` without the extra object). `invoke(f, seconds)` and `invokeRepeating(f, delay, rate)` are coroutines too; `cancelInvoke([which])`, `isInvoking`, `stopCoroutine`, `stopAllCoroutines` (invokes stay), `Coroutine.stop()` / `isRunning`. Lifetimes as in Unity: destroying the behaviour stops everything; the object leaving the active hierarchy stops its coroutines (not its invokes); a disabled behaviour keeps both; a routine that throws stops and is reported like a callback (`World.onBehaviourError`, callback `coroutine` / `invoke`). A `Future`-based `await` helper is still not offered: a microtask runs outside the frame and its context. Tests: `packages/fynkel/test/coroutine_test.dart`.

### 21.7 Events and messaging

Support three scopes:

1. Direct typed method/service calls for normal gameplay.
2. Typed engine events for collisions, lifecycle, input, asset changes, and similar system notifications.
3. User-defined event buses/signals for decoupled gameplay where needed.

Avoid string-only global event buses as the primary model. Generated stable event IDs and typed payloads are preferable.

### 21.8 Errors and diagnostics

Development builds should fail loudly and locally for invalid handles, missing assets, illegal component combinations, schema mismatches, unsupported platform features, and bridge protocol errors. Messages must include relevant object IDs, field paths, scene names, and suggested fixes when possible.

Release builds may reduce expensive checks but must never turn invalid state into memory unsafety.

**A behaviour that throws (Unity parity, 2026-09-24; `packages/fynkel/lib/src/behaviour.dart`).** Every behaviour callback — the lifecycle ones, `fixedUpdate` / `afterPhysics` / `update` / `lateUpdate`, and the contact, zone, animation and playback callbacks — runs on its own: a throw stops only that call, the other behaviours still run the pass, and the one that threw is called again next time (a `start` that threw is not retried, and its object's update is skipped for that one frame, as in Unity). The error goes to `World.onBehaviourError(BehaviourError { behaviour, callback, error, stack, tick, message })`; the default, `World.reportBehaviourError`, writes an `exception` log record with the Dart stack on the editor channel when an editor listens (the Console's Game tab: `Player.update on "Hero" threw: …`, a click to the line), else hands the error to the current zone — a test fails on a broken script — or, in a plain Dart process's root zone, prints it. Before this, a throw left the rest of the frame's pass undone. Tests: `packages/fynkel/test/script_errors_test.dart`.

**Debug.Log (Unity parity, 2026-09-24; `packages/fynkel/lib/src/context.dart`).** `Debug.log` / `logWarning` / `logError` (any object, in or outside a callback) and `Debug.logException(error, [stack])` are Unity's: under an editor each is a `log` record at its severity with the caller's Dart stack, so the Console's Game tab shows the calling line and ↩ / a double-click opens it in the IDE (`Diagnostics.log` now carries the stack too); without one the line goes to the current context's `DiagnosticsService` prefixed by its level (`print`, or a test's `RecordingDiagnostics`). `print` in game code is a Console row as well: `GameContext.run` runs callbacks in a zone whose `print` becomes a `log` record of source `print` with its stack while the channel is on — Play in the editor runs the game in the editor's process, whose stdout no Console reads (one zone per parent zone, none forked in a steady frame; nothing changes with the channel off). Tests: `packages/fynkel/test/debug_log_test.dart`, `apps/editor/test/parity/debug_log_parity_test.dart`.

### 21.9 SDK compatibility and deprecation policy

Before 1.0, breaking changes are allowed but must use changelogs and migration notes. From 1.0 onward, adopt semantic versioning for public Dart APIs and documented project formats. Deprecate public APIs before removing them when practical. File/schema migrations need a tested upgrade path independent of source API deprecations.

### 21.10 SDK acceptance gate

The SDK is ready for 1.0 only when the reference game and at least two external pilot games can be written without direct native calls, editor-only APIs, or undocumented internal assumptions. Common tasks must have IDE-discoverable APIs, examples, tests, and actionable diagnostics. The Dart surface must remain usable without Flutter UI or the editor.


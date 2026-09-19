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
- Optional `ReflectionProbe` only if implemented and documented before 1.0

`MeshRenderer` references mesh and material assets and exposes visibility, shadow participation, layer/mask, and bounds-related properties. Do not expose raw wgpu resources.

`Camera` should define projection, clear mode, viewport, clipping planes, render layer mask, HDR/AA settings where appropriate, and priority/order. Avoid embedding Flutter view ownership directly into the camera component.

#### Physics

- `RigidBody`
- `BoxCollider`
- `SphereCollider`
- `CapsuleCollider`
- `MeshCollider` with explicit static/dynamic limitations
- `CharacterController` or `KinematicCharacterController`

Physics components expose engine concepts rather than Rapier types. Collision groups, trigger/sensor state, material/friction/bounce, mass properties, continuous collision configuration, and query participation need stable engine-level types.

#### Animation

- `Animator`
- `Skeleton`/skin data as imported runtime asset rather than a user-authored component unless necessary

`Animator` references an animation controller/graph asset or a simple clip setup. It must support parameter updates, state queries, events, speed, layer/track concepts only as needed, and root-motion policy.

#### Audio

- `AudioSource`
- `AudioListener`

`AudioSource` provides clip reference, volume, pitch, looping, spatial blend, playback state, bus routing, distance model, and voice priority. Playback handles must have explicit lifetime semantics.

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

### 21.6 Async, tasks, and coroutines

Use Dart's native `Future`, `Stream`, and async/await model rather than inventing an engine-specific coroutine language for ordinary asynchronous work. Provide engine-aware helpers for cancellation and scene/object lifetime.

Example:

```dart
final handle = await Assets.load<Model>('asset://models/boss.glb');
if (!gameObject.isAlive) return;
```

For gameplay timing, provide explicit scheduler helpers such as `game.delay(Duration)` or tick-based waits only if they add value. They must be cancellable when the owning scene or behaviour is destroyed. Do not encourage untracked timers that outlive worlds.

### 21.7 Events and messaging

Support three scopes:

1. Direct typed method/service calls for normal gameplay.
2. Typed engine events for collisions, lifecycle, input, asset changes, and similar system notifications.
3. User-defined event buses/signals for decoupled gameplay where needed.

Avoid string-only global event buses as the primary model. Generated stable event IDs and typed payloads are preferable.

### 21.8 Errors and diagnostics

Development builds should fail loudly and locally for invalid handles, missing assets, illegal component combinations, schema mismatches, unsupported platform features, and bridge protocol errors. Messages must include relevant object IDs, field paths, scene names, and suggested fixes when possible.

Release builds may reduce expensive checks but must never turn invalid state into memory unsafety.

### 21.9 SDK compatibility and deprecation policy

Before 1.0, breaking changes are allowed but must use changelogs and migration notes. From 1.0 onward, adopt semantic versioning for public Dart APIs and documented project formats. Deprecate public APIs before removing them when practical. File/schema migrations need a tested upgrade path independent of source API deprecations.

### 21.10 SDK acceptance gate

The SDK is ready for 1.0 only when the reference game and at least two external pilot games can be written without direct native calls, editor-only APIs, or undocumented internal assumptions. Common tasks must have IDE-discoverable APIs, examples, tests, and actionable diagnostics. The Dart surface must remain usable without Flutter UI or the editor.


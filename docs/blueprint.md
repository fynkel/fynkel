# Fynkel Engine — Development Blueprint

**Status:** Proposed architecture and implementation plan, version 0.4  
**Prepared:** 19 September 2026 (v0.3); revised 19 September 2026 (v0.4)  
**Audience:** Fynkel maintainers, contributors, runtime/rendering engineers, Dart/Flutter engineers, tooling engineers, and AI-assisted development workflows  
**Objective:** Build **Fynkel Engine**, a native-first, Dart-first 3D game engine with a Rust runtime, `wgpu` rendering, `hecs` behind a Fynkel-owned ECS boundary, Flutter UI/editor tooling, transparent project files, and a Unity-like workflow without making the editor mandatory.

All package names, commands, APIs, file schemas, platform targets, and performance budgets below are proposed engineering contracts until validated by milestone gates. Milestones are acceptance gates, not calendar estimates. Platform support is earned through testing, not inferred from Flutter, Rust, or dependency support.

**Related documents.** Binding decisions live in `docs/decisions/` (ADRs) and take precedence over this text where they conflict. Detailed specs for the Dart SDK and the Editor UX live in `docs/design/` and are living drafts revisited at M1 and M4 respectively. Post-1.0 speculation lives in `docs/ideas.md` and is non-binding.

### Changes in v0.4

- The engine is a **library**; a **host** owns the main loop (§2, §5.2, ADR-0007/0008). Flutter is one host among several and the engine never depends on it.
- Dart gameplay runs on the main isolate and drives `step()` synchronously; isolate/native-thread migration is a documented later option (ADR-0008).
- Fixed step uses an accumulator with render interpolation; events are pull-only; two command classes (simulation vs presentation); input edges consumed by one tick; `afterPhysics` receives its tick's physics events (ADR-0008).
- M0 is macOS/Metal + Impeller only. Windows backend proof and the Wasm spike are removed from M0 (ADR-0002).
- 1.0 platform intent is **macOS + iOS** (ADR-0009). Windows is post-1.0.
- The reference game is split into **Tier 1 (M3)** and **Tier 2 (M5)** (§1.2, ADR-0010).
- Scale rule: Dart Behaviours for the few, Fynkel-shipped data-driven native components for the many (§3.5, ADR-0011).
- Codegen, schema source of truth, asset identity, bridge tooling, and wire format are decided (ADR-0012..0015).
- Editor authoring service is a Rust crate; edit preview is a real world; Play is a separate process (ADR-0016).
- §21–24 of v0.3 moved to `docs/design/` and `docs/ideas.md`.

### Naming and brand conventions

The official product name is **Fynkel Engine**. Use **Fynkel** as the short product name in developer-facing copy. Use `fynkel` for the CLI and primary Dart package, `fynkel_*` for engine-owned Rust/Dart modules, and `fynkel.*` for built-in serialized type IDs. Shipped games may use **Made with Fynkel**.

Generic words such as “engine”, “runtime”, “renderer”, and “editor” remain lowercase when describing technical concepts.

### Scope principle for 1.0

Fynkel 1.0 is **native-first**. The product must become a good native engine before it becomes a browser development environment.

Browser/Wasm/WebGPU compatibility is kept alive only through portable contracts (§12.2). No browser work, including feasibility spikes, happens before 1.0. The browser editor, browser-only project workflow, cloud compilation, browser filesystem layer, and web collaboration are **post-1.0** work.

The project follows this rule:

> Build the smallest architecture that does not prevent Fynkel from becoming the engine we eventually want.

## 1. Product definition and scope

Build **Fynkel Engine** around three equally valid workflows:

1. **Code-first:** create worlds, entities, components, scenes, and gameplay through Dart.
2. **Editor-first:** author scenes, prefabs, materials, component fields, and assets visually.
3. **Hybrid:** author stable world structure in the editor and combine it with procedural/gameplay code.

A Fynkel game is a normal Flutter/Dart project using the Fynkel SDK. The Fynkel Editor reads and writes the same version-controlled project data used by the CLI, build system, tests, and AI agents.

The public workflow should be familiar to Unity developers: GameObjects, components, behaviours, Transform hierarchy, scenes, prefabs, materials, Inspector, Scene view, Game view, and Play/Stop. Internally, GameObjects are lightweight handles into an ECS-backed world; they are not heavyweight object trees.

### 1.1 Non-negotiable product principles

1. A complete game must build and run without the editor.
2. The Rust runtime owns performance-sensitive native state and resources.
3. Dart owns gameplay behaviours and high-level game logic.
4. Flutter owns screen-space application/game UI and the desktop editor UI.
5. Project files are readable, versioned, validated, source-control friendly, and editable without GUI automation.
6. Editor code, import tools, and development servers must not enter release game builds.
7. Dart ↔ Rust communication is batched and measured; per-property native chatter is prohibited in hot paths.
8. Public APIs and serialized formats do not expose `hecs`, Rapier, wgpu, Rust pointers, or other dependency-specific implementation details.
9. Fynkel 1.0 does not depend on the browser editor or browser-only tooling.
10. Performance and platform integration are validated before feature breadth.

### 1.2 Fynkel 1.0 reference game

The reference game is the **scope oracle** for 1.0. Every major feature proposal should answer: “Does the reference game or an external pilot project require this?”

The reference game is a small third-person 3D collect-and-interact game with this frozen baseline. The **specification** is frozen as a whole; the **milestone that must prove it** is split into two tiers (ADR-0010):

- **Tier 1 — proven at M3.** Static-mesh (capsule or rigid) player, no skinning; Flutter HUD and projected Flutter labels only; 2D audio only; everything else below.
- **Tier 2 — proven at M5.** Skinned/animated player with animation states; minimal native `WorldCanvas` for the world-space sign; spatial audio.

Items marked *(Tier 2)* below belong to the second tier.

**Project structure**

- `MainMenu` UI state.
- `Playground` gameplay scene.
- `Results`/win state.
- Saveable settings for audio, graphics quality, and input bindings.

**Player**

- One player character. *(Tier 1: static mesh; Tier 2: skinned/animated.)*
- Idle, walk, run, and jump animation states. *(Tier 2)*
- Third-person camera.
- Character collision and ground detection.
- Walk, sprint, jump, interact, pause, and camera input actions.
- The same actions driven by a Flutter touch-control overlay (virtual stick/buttons) on iOS.

**World**

- Approximately 100 static mesh renderers.
- At least 20 dynamic rigid bodies.
- 10 collectibles.
- 2 moving or physics-based hazards.
- 1 directional light with shadows.
- A small bounded number of point/spot lights.
- PBR materials and imported glTF/GLB assets.
- At least one projected Flutter world label or marker.
- At least one true world-space UI/sign element rendered by the native `WorldCanvas`. *(Tier 2)*

**Gameplay**

- Collect all 10 items.
- Trigger zones.
- Collision/trigger events.
- Win condition.
- Restart.
- Scene/state transition.
- One simple runtime-spawned object/prefab.
- One example of additive or controlled scene loading only if that feature is part of the 1.0 API.

**Flutter UI**

- Main menu.
- HUD collectible counter.
- Pause menu.
- Settings panel.
- Results/restart UI.

**Audio**

- Background music.
- Footsteps or movement audio.
- Pickup sound.
- At least one spatial world sound. *(Tier 2)*

**Tooling**

- The entire game can be built code-first before Editor V0 exists.
- The same project can later be recreated/edited through Fynkel Editor without changing runtime semantics.
- Headless tests can simulate core gameplay state without requiring a rendered window where practical.

Do not expand this reference game into a showcase project. Its purpose is to exercise the architecture, not to prove every possible engine feature.

### 1.3 Initial 1.0 capability scope

Fynkel 1.0 should target:

- GameObject/component/Behaviour model.
- Transform hierarchy.
- Dart gameplay behaviours.
- `hecs`-backed ECS behind `fynkel_ecs`.
- glTF/GLB import.
- PBR rendering.
- Bounded lighting and shadow quality.
- Physics and scene queries.
- Basic skeletal animation.
- Audio.
- Named input actions.
- Scenes.
- Simple prefabs with explicit overrides.
- Materials and custom WGSL shader contracts.
- Flutter HUD/application UI.
- Basic native world-space UI.
- Desktop Fynkel Editor.
- CLI, validation, code generation, headless testing, and AI-oriented structured tooling.
- Native builds on platforms that pass qualification gates.

The **1.0 platform intent is macOS + iOS** (ADR-0009): one graphics backend (Metal), one Flutter texture interop mechanism (CVPixelBuffer/IOSurface), one toolchain. M0 validates macOS/Metal only. iOS gets a smoke test (cube on device) in M1 and full qualification in M6. Windows/D3D12, Linux/Vulkan, and Android remain planned native targets for post-1.0 and are advertised only after they pass the same lifecycle, rendering, performance, and packaging gates. To keep the renderer from silently becoming Metal-only, `cargo check --target x86_64-pc-windows-msvc` runs periodically on the available Windows machine.

Because the performance ambition is **thousands of rendered objects** (ADR-0011), instancing and frustum culling are renderer V1 requirements at M3, not later optimizations, and a separate stress benchmark scene exists from M3 (§15.3).

### 1.4 Explicitly deferred from 1.0

Postpone:

- Browser editor.
- Browser-only authoring.
- Production browser game runtime.
- Cloud compilation/build service.
- Hosted collaboration.
- Networked multiplayer/rollback.
- Terrain.
- Large-world streaming.
- Navigation/navmesh.
- Advanced particle/VFX graph.
- Shader graph.
- Visual scripting.
- Ray tracing.
- Advanced global illumination.
- Complex prefab inheritance/variants.
- Stable native plugin ABI.
- Marketplace.
- Console support.
- AR/VR.
- Arbitrary Flutter widgets rendered into 3D textures.

A deferred feature may have an architectural interface or experiment, but it must not become a hidden requirement for shipping 1.0.

## 2. System architecture

```text
                     HOSTS (own the main loop)
 ┌──────────────────┬──────────────────┬──────────────────┬─────────────────┐
 │ Flutter game app │ Standalone host  │ Headless host    │ Fynkel Editor   │
 │ Dart behaviours  │ (winit, Rust)    │ (tests, CI,      │ (Flutter desktop│
 │ + Flutter UI     │ M0 baseline,     │  fynkel test)    │  edit-preview   │
 │ + Fynkel SDK     │ benchmarks       │                  │  world)         │
 └────────┬─────────┴────────┬─────────┴────────┬─────────┴────────┬────────┘
          │  commands / snapshots / events (batched, typed buffers) │
          └──────────────────────────┬─────────────────────────────┘
                              Versioned C ABI
                                     │
                     Rust engine LIBRARY (no loop of its own)
                        fynkel_core: step(commands) -> snapshot
                        fynkel_ecs -> hecs internally
                        transforms / physics / animation / audio
                        assets / scene runtime
                        renderer (may own a render thread)
                                     │
                                   wgpu
                     Metal (1.0) / D3D12 / Vulkan / GLES as qualified

Development only
  Fynkel CLI (Rust)            Fynkel Editor (Flutter desktop)
  codegen / importers / cooker AI/automation tools (via CLI --json)
                    │
        fynkel_document: shared authoring command service (Rust crate)
                    │
        JSON project/source documents
```

**The engine is a library; the host owns the loop** (ADR-0007). `fynkel_core` exposes `step()`; it creates no simulation thread of its own. Whoever embeds the engine — a Flutter app, a winit window, a headless test, the editor — calls `step()` on its own schedule. The engine has no dependency on Flutter. In the Flutter host, Dart gameplay runs on the main isolate and calls `step()` synchronously each host tick (ADR-0008). The renderer may run a Rust render thread that consumes immutable extracts and presents to whatever surface the host provides.

The Dart gameplay package (`fynkel`) must remain separable from `fynkel_flutter`: headless tests and the CLI use it without widgets.

### 2.1 Browser compatibility policy

Do **not** maintain a second production runtime/bridge/editor before 1.0, and do not run browser feasibility spikes before 1.0 either (ADR-0002). `wgpu` already targets WebGPU; the only pre-1.0 protection is the portability constraints in §12.2, optionally enforced by a `cargo check --target wasm32-unknown-unknown` job on the render crate once it stabilizes.

Public project formats, component schemas, asset IDs, material contracts, and high-level renderer abstractions should avoid assumptions that make future Wasm support impossible. Browser filesystem, JS bridge, browser editor, web audio, security, and cloud-build work remain post-1.0.

### 2.2 Repository layout

```text
fynkel/
  crates/
    fynkel_contracts/       # IDs, protocol records, versions, schema metadata
    fynkel_core/            # world lifecycle, time, service orchestration
    fynkel_ecs/             # Fynkel-owned ECS facade over hecs
    fynkel_scene/           # hierarchy and runtime scene instantiation
    fynkel_render/          # wgpu renderer and render extraction
    fynkel_assets/          # runtime handles, loading, residency
    fynkel_physics/         # Rapier adapter
    fynkel_animation/
    fynkel_audio/
    fynkel_world_ui/
    fynkel_document/        # source docs, validation, edit transactions
    fynkel_import/          # development-only import/cooking
    fynkel_native/          # C ABI and native host boundary

  packages/
    fynkel/                 # Flutter-independent Dart gameplay API
    fynkel_flutter/         # Flutter viewport/UI/input integration
    fynkel_codegen/         # behaviour registration and metadata
    fynkel_tools/           # project-local tool launcher/helpers

  apps/
    editor/                 # Fynkel Editor desktop app
    cli/                    # Fynkel CLI

  platform/                 # per-platform wgpu-hal texture/surface/lifecycle interop (a real subsystem)
  benchmarks/
    stress/                 # thousands-of-objects scene, tracked from M3
  hosts/
    standalone/             # winit host: M0 baseline, benchmarks
    headless/               # no-renderer host: tests, fynkel test
  schemas/
  examples/reference_game/
  tests/
    contracts/
    fixtures/
    integration/
    performance/
  docs/
    architecture/
    decisions/
    guides/
```

These are logical boundaries, not a command to create empty crates. Begin with fewer crates where that reduces friction.

Dependency direction must remain clear:

```text
contracts
   ↑
core / ecs / scene / assets
   ↑
renderer / physics / animation / audio
   ↑
native host

editor/import/build tooling  ---> may depend on contracts/document/import
runtime                      -X-> must not depend on editor UI
```

### 2.3 Dependency policy

Initial technology choices:

| Area                     | Choice                                        |
| ------------------------ | --------------------------------------------- |
| Native language          | Rust                                          |
| Renderer                 | `wgpu`                                        |
| Shader language          | WGSL                                          |
| ECS storage/query engine | `hecs`, private behind `fynkel_ecs`           |
| Math                     | `glam`                                        |
| Physics                  | Rapier                                        |
| Serialization            | `serde` / `serde_json`                        |
| Source documents         | strict JSON                                   |
| Model interchange        | glTF / GLB                                    |
| Gameplay                 | Dart                                          |
| Screen UI/editor         | Flutter                                       |
| Native bridge            | versioned C ABI, hand-written header + `ffigen` |
| Wire format              | hand-defined binary records, generated encoders |
| Dart math                | Fynkel-owned immutable `Vec3`/`Quat`/`Mat4`     |
| Dart codegen             | `build_runner`, generated files committed        |
| Component schema source  | Rust structs -> Dart bindings + JSON Schema      |
| Flutter renderer         | Impeller                                         |
| Audio                    | evaluate Kira behind a Fynkel-owned interface |

`hecs` is the default 1.0 ECS implementation because it is a small ECS library rather than a full engine framework. Fynkel still owns scheduling, public handles, component metadata, commands, snapshots, authoring identity, and the Dart-facing API.

Do not expose `hecs::Entity`, Rapier handles, `wgpu` resources, or `glam` memory layouts in public APIs or serialized documents.

Pin a tested Rust/Flutter/Dart/dependency matrix. Record:

- exact versions;
- enabled features;
- binary-size impact;
- compile-time impact;
- licenses;
- native platform support;
- upgrade notes.

Dependency upgrades are deliberate engineering changes, not automatic maintenance.

### 2.4 Licensing and repository strategy — decide before the M3 public push

Licensing is an architectural/repository decision and must be resolved before the repository goes public or any package is published. The repository stays private through M0–M2; ADR-0005 is accepted as an M3 gate item. Nothing in M0 blocks on it.

**Recommended default for discussion in ADR-0005:**

- engine/runtime/SDK/tooling code: `MIT OR Apache-2.0`;
- examples/templates: same unless asset licenses require otherwise;
- Fynkel name/logo: separate trademark/brand policy;
- third-party sample assets: explicit per-asset licenses;
- contributor policy: choose DCO or CLA before accepting external contributions.

Before the M3 public push, also decide:

- public monorepo vs split repositories;
- crate/package publishing policy;
- whether prebuilt native binaries are distributed via releases/package artifacts;
- security disclosure process;
- release signing/checksum policy;
- branch/review rules.

Do not let package layout accidentally determine licensing or governance after users already depend on it.

### 2.5 Architecture Decision Records

Every irreversible or expensive-to-change decision gets an ADR in `docs/decisions/`.

Initial ADR set:

```text
ADR-0001  Fynkel 1.0 scope and native-first policy
ADR-0002  M0 scope and Flutter/wgpu viewport composition path
ADR-0003  hecs behind fynkel_ecs
ADR-0004  Dart/Rust command-snapshot protocol and wire format
ADR-0005  license, repository, contribution, trademark policy (open)
ADR-0006  project/source document format and asset identity
ADR-0007  engine as a library; hosts own the loop
ADR-0008  Dart execution model: main isolate, synchronous step, timestep, events
ADR-0009  1.0 platform intent: macOS + iOS
ADR-0010  reference game tiers
ADR-0011  scale rule: Dart behaviours for the few, native components for the many
ADR-0012  codegen and component schema source of truth
ADR-0013  Dart math types
ADR-0014  error reporting across the ABI
ADR-0015  native runtime distribution
ADR-0016  editor architecture: authoring service, edit preview, Play process, docking
ADR-0017  AI/automation interface
```

ADRs written on 19 September 2026 (0007–0017) record the decisions of the v0.4 planning interview.

ADRs are short decision records, not replacements for this blueprint.

## 3. Public object and scripting model

Expose a Unity-familiar but Fynkel-owned API built around `GameObject`, `Component`, `Behaviour`, `Transform`, `Scene`, `Prefab`, `AssetRef<T>`, and engine services.

A `GameObject` is an ergonomic Dart handle to a runtime entity. Native components such as `Transform`, `MeshRenderer`, `Collider`, `Camera`, and `Animator` live authoritatively in Rust. Dart behaviours hold gameplay state and issue batched commands through the SDK.

Do not mirror every native component as an independently owned mutable Dart object.

### 3.1 Identity model

Keep three identities separate:

- **Authoring IDs:** persistent UUID strings for assets, entities, component instances, prefab-local objects, and documents.
- **Runtime handles:** Fynkel-owned world/entity handles with generation validation.
- **Type IDs:** stable namespaced identifiers such as `fynkel.Transform` and `game.PlayerController`.

Names and file paths are labels/locations, not identities.

Never serialize:

- raw pointers;
- `hecs::Entity`;
- Rapier handles;
- dependency-specific runtime IDs;
- temporary runtime handles.

### 3.2 Canonical Behaviour lifecycle

This is the **single authoritative lifecycle definition** for the whole blueprint. Later sections must reference it rather than redefine it.

```text
onCreate
  object and serialized fields exist;
  scene/object/asset references have been resolved

onEnable
  behaviour becomes active

start
  runs once before the first active gameplay update

fixedUpdate
  fixed-timestep gameplay/simulation intent

afterPhysics
  optional post-physics hook for behaviours that explicitly opt in

update
  presentation-rate gameplay logic

lateUpdate
  ordering-sensitive presentation/camera logic

onDisable
  behaviour becomes inactive

onDestroy
  final lifecycle notification at a safe destruction boundary
```

Typed collision, trigger, animation, and input events should use event/subscription mechanisms rather than adding a lifecycle method for every engine event.

Rules:

- `start` runs once.
- disabled behaviours do not receive gameplay update callbacks;
- destruction is deferred to a safe command boundary;
- callback ordering is deterministic within documented scheduling rules;
- behaviour ordering dependencies must be explicit, not inferred from object names or scene order.

### 3.3 Generated registration, not runtime reflection

Generate behaviour factories, field serializers, inspector metadata, stable IDs, migrations, and schemas at build/codegen time.

Proposed API:

```dart
@FynkelBehaviour(id: 'game.PlayerController')
class PlayerController extends Behaviour {
  @Expose(min: 0, step: 0.1)
  double moveSpeed = 6.0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    final move = Input.action2D('move');
    gameObject.get<RigidBody>().setPlanarVelocity(
      move.x * moveSpeed,
      move.y * moveSpeed,
    );
  }
}
```

`Input.action2D(...)` is allowed as an ergonomic static facade, but it must resolve through the **current `GameContext`/world service container**, not mutable process-global gameplay state. Tests must be able to install a deterministic test context. Internally, services remain explicit and replaceable.

The editor reads generated metadata without executing arbitrary gameplay code.

### 3.4 Code-first and generated worlds

Everything important in the editor must have a code/tool equivalent.

Dart must support:

- creating/destroying GameObjects;
- adding/removing allowed components;
- loading/unloading scenes;
- instantiating prefabs;
- setting material/component fields;
- querying objects/components;
- physics queries;
- asset loading;
- runtime spawning.

Procedurally created objects may be inspected while running. They are not automatically turned back into source code. An explicit bake/export action may create authoring data where supported.

The editor must never imply that moving a procedurally generated runtime object rewrites the Dart code that generated it.

### 3.5 Scale rule: Dart Behaviours for the few, native components for the many

Every managed gameplay language hits the same wall: thousands of per-object script callbacks per frame cost milliseconds and GC pressure regardless of how fast the ECS underneath is. Fynkel draws the line where Unity and Godot do (ADR-0011):

- **Dart `Behaviour` per object** is the primary gameplay model and is sized for the *interesting few* — on the order of hundreds of ticking behaviours.
- **Fynkel-shipped, data-driven native components** handle the *uniform many*. Physics and animation already are such systems. 1.0 adds a small catalogue of gameplay-flavoured ones, configured entirely from Dart or scene data: `Rotator`, `Oscillate`, `Follow`, `LookAtCamera`, `LifetimeDespawn`, and a simple emitter. Grow the catalogue only from reference-game or pilot needs.
- **Users never write Rust in 1.0.** Bulk logic that the catalogue cannot express is a Fynkel feature request, not a plugin.
- **Batched Dart views** (one call handing Dart a typed-data view over N components) are the post-1.0 answer for "thousands from Dart". They are added only when a real project measures the need.

## 4. ECS, hierarchy, and memory ownership

### 4.1 Use `hecs` first

Fynkel 1.0 uses `hecs` internally.

Architecture:

```text
Dart / editor / scene files
          |
     Fynkel APIs
          |
     fynkel_ecs
  handles / scheduler
  commands / metadata
  snapshots / change journals
          |
         hecs
```

`hecs` is an implementation detail, not a public contract.

Fynkel owns:

- external/runtime entity handles;
- world identity;
- component type metadata;
- scene/object identity mapping;
- command buffering;
- scheduler phases;
- snapshot extraction;
- editor/AI metadata;
- diagnostics;
- serialization contracts.

`hecs` owns the internal ECS world/query/storage mechanics.

### 4.2 Public/runtime handle indirection

Do not expose or serialize `hecs::Entity`.

A Fynkel entity handle should contain or address:

- world identity;
- Fynkel-managed runtime slot/index;
- generation.

`fynkel_ecs` maps that validated Fynkel handle to the current internal `hecs::Entity`.

Internal native systems may use `hecs::Entity` directly while operating inside the world. External/Dart/editor/network/document boundaries use only Fynkel handles/UUIDs.

This preserves the ability to replace `hecs` without changing:

- Dart games;
- scene files;
- prefabs;
- editor documents;
- plugin metadata;
- bridge protocols.

### 4.3 Custom ECS replacement gate

Do **not** write a custom ECS because it seems cleaner, smaller, or more “engine-native.”

A custom ECS may replace `hecs` only when one of these is demonstrated:

1. a required feature cannot be implemented reasonably while retaining `hecs`; or
2. repeated profiling on a frozen Fynkel workload shows a material frame-time bottleneck attributable to the ECS implementation.

For performance justification, require both:

- a meaningful relative improvement on the ECS-bound workload (target: at least ~20%); and
- a meaningful absolute frame-time win in a real Fynkel benchmark (target: at least ~0.5 ms p95 on the reference machine), unless the issue is memory/latency/correctness rather than CPU time.

These numbers are engineering gates, not promises. Update them only through an ADR backed by measurements.

A prototype replacement must preserve the existing `fynkel_ecs` API before migration is considered.

### 4.4 Scheduler ownership

Fynkel—not `hecs`—owns the logical scheduler. The single authoritative tick order is defined in **§5.1**; this section only states ownership. Do not build a general dependency graph/parallel scheduler before profiling proves it is needed.

Begin with obvious ordered phases and explicit ownership. Add parallel work behind stable system boundaries later.

### 4.5 Hierarchy is not the ECS

Parent/child hierarchy determines authoring structure and transform propagation. It does not imply component memory ownership.

Implement:

- local position/rotation/scale;
- cached world transform;
- dirty-subtree propagation;
- cycle rejection;
- explicit `setParentKeepLocal` / `setParentKeepWorld`;
- deterministic child ordering for authoring;
- robust destruction/reparenting semantics.

Adopt one coordinate convention:

> right-handed, Y-up, metres, seconds, radians in APIs, quaternion storage `[x, y, z, w]`.

The Inspector may display Euler degrees.

### 4.6 Memory ownership and correctness

Rust owns:

- ECS storage;
- native resources;
- renderer resources;
- physics state;
- authoritative native component state.

Dart may own:

- Behaviour instance state;
- immutable/local snapshots;
- staged command state;
- Flutter UI state.

No Dart object may retain an unscoped pointer into relocatable Rust storage.

Required tests include:

- stale handles;
- entity slot reuse;
- world destruction;
- component add/remove churn;
- destruction during update;
- query access safety;
- scene load/unload;
- prefab instantiation;
- large spawn/despawn batches;
- handle-to-hecs mapping integrity.

Unsafe Rust must be isolated and documented with explicit invariants.

## 5. Simulation, bridge, and scheduling contract

The bridge is one of Fynkel's highest-risk performance boundaries. Design it as a protocol, not a collection of convenience FFI calls.

### 5.1 Fixed simulation contract (authoritative tick order)

This is the **single authoritative tick definition**. §4.4 and the SDK design doc reference it rather than redefine it.

One **host frame** does the following (ADR-0008):

```text
Host frame begins (Flutter frame callback / winit redraw / test loop)
  -> capture input since last frame: held state + pending edges (pressed/released)
  -> accumulator += frame dt (clamped to a max, e.g. 250 ms)
  -> while accumulator >= fixedDt and steps < maxStepsPerFrame:
       FIXED TICK n:
         expose coherent snapshot S[n] + drained non-physics events to Dart
         input view for tick n: held state; pending edges are consumed by THIS tick only
         Dart fixedUpdate produces SIMULATION command batch C[n]
         step(C[n]):                          # one synchronous FFI call
           validate/apply C[n] at tick boundary
           native pre-tick systems
           fixed physics step
           post-physics synchronization
           animation/transform work for the tick
           deferred structural changes
           publish S[n+1]; physics events of tick n attached to S[n+1]
         Dart afterPhysics(context) (opt-in): reads S[n+1], receives tick n's
           collision/trigger events; its writes are simulation commands for tick n+1
       accumulator -= fixedDt
  -> alpha = accumulator / fixedDt
  -> Dart update(dt), lateUpdate(dt)          # presentation rate
       simulation commands  -> staged for the next fixed tick
       presentation commands -> applied before this frame's extraction
  -> render extraction: interpolate S[n]..S[n+1] by alpha, then apply presentation overrides
  -> renderer draws/presents (possibly on its own Rust thread)
```

**Two command classes** (ADR-0008):

| Class | Examples | Applied |
| --- | --- | --- |
| Simulation | `SetRigidBodyVelocity`, `SpawnPrefab`, `DestroyEntity`, `SetAnimatorParameter`, transform of physics/simulation objects | at the next fixed tick boundary |
| Presentation | camera transform, transform of non-physics objects set in `lateUpdate`, material parameters, visibility, light colour | before the current frame's render extraction |

A presentation transform command does two things: it overrides the extract for the current frame (that object is not interpolated this frame) **and** it is staged as the authoritative value for the next tick, so the object does not snap back. When that staged value is applied, the object (and its subtree) is treated as teleported: its S[n] interpolation source is set to the new value, so a one-off presentation write is not visibly interpolated back through the old state. Physics-owned transforms reject presentation commands with a diagnostic (`physics_owned`); structural or simulation-only commands on the presentation lane are rejected with `wrong_lane`; use simulation commands (forces, velocities, teleport) for them.

Extraction renders **one tick behind**: right after a tick, alpha ≈ 0 and the extract shows S[n]; it moves towards S[n+1] over the following fixed interval. This is the standard accumulator model and the price of smooth 120 Hz presentation from a 60 Hz simulation; presentation overrides are the escape hatch for objects that must reflect this frame's input (camera).

Rules:

- The fixed step defaults to 60 Hz and is configurable per project.
- **Accumulator + render interpolation** is the 1.0 model: at 120 Hz presentation, ticks run every other frame and transforms are interpolated; below 60 Hz, up to `maxStepsPerFrame` ticks catch up, then time is dropped and a `MissedDeadline` diagnostic is recorded.
- `update`/`lateUpdate` run once per host frame. Simulation writes issued there are staged for the next tick; presentation writes affect the current frame. Neither mutates native storage mid-step.
- **Input edges are consumed by exactly one fixed tick.** `pressed`/`released` transitions captured during a frame go into a pending buffer; the first fixed tick of the frame consumes them and later catch-up ticks in the same frame see only held state. If a frame runs zero ticks (e.g. 120 Hz presentation), pending edges are carried to the next tick, never dropped. `update`/`lateUpdate` see the frame's raw edges for presentation-only reactions (UI sounds, camera nudges).
- **Events are pull-only**, in two lanes. *Physics events* (collision enter/stay/exit, trigger enter/exit) generated by tick n are delivered inside `afterPhysics(context)` of tick n and again available in the next tick's drain for behaviours that do not opt into `afterPhysics`. *All other events* (animation, asset-loaded, scene-ready, lifecycle) are queued in Rust and drained at the start of the next tick, or at frame start when no tick runs. Asynchronous work on Rust threads completes into the same queue and may surface up to one frame late. No `NativePort`/callback delivery exists in 1.0.

### 5.2 Host and threading model

**The engine is a library and the host owns the loop** (ADR-0007). The engine creates no simulation thread. Hosts in 1.0:

| Host | Loop source | Dart present? |
| --- | --- | --- |
| Flutter game app | Flutter frame callback on the main isolate | yes (main isolate) |
| Standalone Rust host | winit event loop; M0 baseline and benchmarks | no |
| Rust headless host | Rust test loop; native systems and runtime contract tests only | no |
| Dart headless game host | Dart test loop; complete game simulation including Behaviours; what `fynkel test --headless` launches for a project | yes (test isolate) |
| Fynkel Editor | Flutter frame callback; edit-preview world | yes |

**Dart gameplay runs on the main Flutter isolate and calls `step()` synchronously** (ADR-0008). This is a deliberate simplicity choice for a solo-developer M1, not a permanent constraint. A dedicated gameplay isolate and/or a Rust simulation thread with a command/snapshot handshake remain possible because the protocol is already message-shaped. Migration trigger: measured Dart gameplay time on the reference game or stress benchmark that causes HUD frame drops the frame budget cannot absorb. Record the migration in a new ADR when it happens.

**Rendering is independent of Dart.** Render extraction and drawing are pure Rust and may run on a Rust render thread consuming immutable extracts. Presentation rate is set by the surface: through a Flutter `Texture`, presentation is bound to Flutter's compositor frame (display vsync); an engine-owned surface (Path C) may present uncapped. Rendering never waits on Dart beyond the extract it already holds.

Rules:

- never step the engine from a Flutter widget `build()` call;
- no synchronous disk/network I/O on frame-critical paths;
- Rust worker threads must not call Dart; Dart isolates are entered only by their own thread;
- renderer/asset workers operate on owned jobs or immutable extracts;
- moving simulation to another isolate or thread later must preserve the same command/tick semantics.

Do not create one thread per subsystem.

If Dart misses a simulation deadline:

- render the last coherent state;
- bound catch-up work;
- do not silently simulate ticks whose gameplay command phase was skipped;
- expose diagnostics for missed deadlines.

When a debugger pauses gameplay, pause or explicitly control the simulation clock instead of building an unbounded backlog.

### 5.3 Native bridge protocol

Use a stable, versioned C ABI for the native host boundary. The ABI is defined in a **hand-written C header** exported by `fynkel_native`; Dart bindings are generated from that header with `ffigen` (ADR-0004). Do not use `flutter_rust_bridge` or any tool that derives the ABI from Rust signatures: the ABI shape is a designed contract, not a by-product.

Required operation families:

```text
runtime create/destroy
world create/destroy
submit command batch
advance/step
subscribe snapshot fields
read snapshot
drain events
perform explicit query
read diagnostics/stats
resource/asset requests
```

Per-frame traffic uses reusable binary/typed buffers with a **hand-defined record layout** (ABI v1: a 24-byte buffer header, variable-size command/event records with a `kind, size` envelope so unknown kinds can be skipped, fixed-size snapshot/diagnostic records, and a string tail addressed by `(offset, len)`; see `crates/fynkel_native/include/fynkel.h`) (fixed-size little-endian records in `Uint8List`/`Float32List` views). Encoders and decoders on both sides are **generated from the Rust component schemas** (ADR-0012), so the wire layout, the Dart bindings, and the JSON Schema cannot drift independently. No FlatBuffers/Protobuf in 1.0.

Do **not** use:

- JSON per frame;
- one FFI call per property;
- Rust struct memory layouts as an ABI;
- pointers to ECS storage;
- callback reentrancy while storage is borrowed.

Define:

- byte order;
- scalar widths;
- alignment;
- buffer ownership;
- count/length validation;
- protocol version;
- schema fingerprint;
- tick number;
- request IDs;
- error/status records.

### 5.4 Commands, snapshots, and read-your-writes

Dart gameplay writes go into a command/staging buffer.

Examples:

```text
SetTransform
SetRigidBodyVelocity
SpawnPrefab
DestroyEntity
PlayAudio
SetAnimatorParameter
```

Reads normally come from a coherent snapshot. To keep ergonomic APIs useful, getters consult a local **staged-write overlay** before the last acknowledged snapshot (read-your-writes, ADR-0008):

- The overlay applies to **plain data fields** the command set directly (transform of non-physics objects, material parameters, animator parameters, audio properties).
- **Physics-driven state** (position/rotation/velocity of dynamic bodies) is read-only mid-tick: a getter returns the snapshot value even after a `setVelocity` command, because the outcome depends on the step.
- A rejected command clears its overlay entry and produces a diagnostic (§5.5); the next getter returns snapshot state.
- Documentation must state per component which fields are overlay-able.

A property getter must never hide an expensive native round trip.

Snapshots are subscription-based. Dart should receive only the objects/components/fields it asked to observe or that the engine API requires for active behaviours.

Structural command batches return mappings from temporary spawn IDs to validated Fynkel runtime handles.

### 5.5 Error and lifetime rules

Never unwind Rust panics across the ABI.

`step()` returns a **status code** and fills a **diagnostics buffer** listing per-command errors (command index, stable error code, entity/component ID, message) (ADR-0014). In debug builds the Dart SDK throws a `FynkelError` carrying those records; in release builds it logs them and continues. A Rust panic is caught at the ABI edge, converted into a fatal status, and the world is marked poisoned; the host decides whether to restart.

Use:

- status codes;
- structured error records;
- debug assertions inside Rust;
- a tested panic boundary at the ABI edge.

Destroy worlds, render resources, native handles, and subscriptions explicitly. Dart finalizers may provide leak protection but are not the primary lifetime mechanism.

### 5.6 Determinism statement

A fixed timestep is useful for stability and testing, but Fynkel 1.0 must **not** promise cross-platform deterministic simulation or rollback multiplayer.

If a future feature requires cross-platform deterministic Rapier physics, use Rapier's documented determinism requirements and validate the entire game-side input/math/state pipeline—not only the physics library.

### 5.7 Bridge acceptance targets

Before M1 closes:

- the bridge supports batched create/update/destroy traffic;
- stale handles fail safely;
- command buffers are bounded and validated;
- snapshot bandwidth is measurable;
- no hot reference-game path uses per-property FFI calls;
- protocol fixtures are versioned;
- a no-renderer test host can exercise the world/bridge contract.

## 6. Source formats: strict JSON and schemas

Use **strict JSON** for the initial authoritative scene, prefab, material, and engine project files. This deliberately refines the earlier JSON5 suggestion: one strict machine contract is the simpler starting point for schemas, agents, formatters, and round-trip tests. Comments can live in explicit `description` or `editorNotes` fields. An optional JSON5 import layer can be added later; do not maintain two authoritative formats.

Suggested extensions are `.scene.json`, `.prefab.json`, `.material.json`, and `.shader.json`. Validate structural data with JSON Schema, pinning the supported dialect. Add semantic validation for references, component uniqueness, prefab cycles, unsupported physics configurations, and shader compatibility. JSON Schema supplies a standard vocabulary, but it does not define those engine-specific semantics. [S8]

All documents carry format version and persistent ID. Use UUID-keyed maps for entities and component instances; store child ordering separately. This avoids making identity depend on array position. Store runtime handles nowhere in authoring files.

### Scene example

The following is illustrative but valid JSON. The schema file is engine-owned and resolved locally.

```json
{
  "$schema": "../../schemas/scene.schema.json",
  "formatVersion": 1,
  "id": "70d7ba8b-f7fb-4eba-920d-ec9e6b968330",
  "name": "TrainingRoom",
  "entities": {
    "f4d6d600-7931-44f5-a258-45b5a6850300": {
      "name": "Player",
      "parent": null,
      "components": {
        "818dc3dc-a9ee-4355-908f-58e20a60ce20": {
          "type": "fynkel.Transform",
          "version": 1,
          "data": {
            "position": [0, 1, 0],
            "rotation": [0, 0, 0, 1],
            "scale": [1, 1, 1]
          }
        },
        "16e1fa69-fca5-4059-956f-c0b988db2520": {
          "type": "game.PlayerController",
          "version": 1,
          "data": { "moveSpeed": 6 }
        }
      }
    }
  }
}
```

Use canonical formatting and stable key order. Preserve intentionally ordered child lists. Reject duplicate keys, non-finite numbers, malformed IDs, and ambiguous type coercion. Unknown component data must remain intact when an editor opens a project with a missing plugin; builds fail clearly instead of silently omitting it. Opening a newer incompatible document should be read-only or refused, not destructive.

### Prefabs

A prefab uses the same entity/component document model with declared root objects. An instance references the prefab asset ID plus stable instance identity. Instantiation maps prefab-local IDs to new runtime handles and remaps internal references; external asset references remain shared.

Overrides target prefab-local entity ID, component-instance ID, and stable field identifier. Support setting fields first, then adding/removing objects or components with explicit operations. Do not use array indices, display names, or a generic deep merge as the override contract. Define deletion versus null separately.

Implement basic prefabs before variants and nested prefab composition. Later additions require cycle detection, override conflicts, apply/revert controls, and regression fixtures.

### Materials and shaders

A material references a shader ID, typed parameters, textures, samplers, and render state. Start with a versioned built-in PBR shader interface. Define colour spaces, metallic/roughness ranges, alpha mode, cutoff, culling, and shadow behaviour explicitly.

A custom shader pairs WGSL with a shader-interface manifest describing resource bindings, parameter types, supported passes, and feature requirements. Validate shader/material compatibility at import time. Pack GPU uniforms using explicit layout rules. Share immutable base materials; use explicit material instances for overrides to avoid accidental global edits and excessive variants.

## 7. Project, asset pipeline, and persistence

A project is an ordinary Flutter application:

```text
my_game/
  pubspec.yaml
  pubspec.lock
  fynkel.project.json
  fynkel.lock.json
  lib/{main.dart,game/,generated/}
  assets/{scenes/,prefabs/,materials/,models/,textures/,audio/}
  schemas/
  tests/
  .fynkel/cache/             # generated; ignored by version control
  build/                    # generated; ignored by version control
  android/ ios/ windows/ macos/ linux/ web/
```

The project file defines startup scene, input actions, quality profile, component registry, and build targets. The lock file records matching runtime/tool/editor versions, schema/protocol compatibility, and asset compiler fingerprint. Platform folders exist only for enabled targets.

Source assets, metadata, scenes, scripts, and project configuration belong in version control. Asset caches and temporary preview state do not. Save-game data is a separate application persistence API with its own versions; it is not a modified authoring scene file.

### Asset identity and import

Assign stable IDs in **`.meta.json` sidecar files** stored next to each source asset and committed to version control (ADR-0006); the sidecar also holds import settings. Preserve IDs when moving assets; new copies get new IDs. glTF sub-assets need stable child identifiers as well as a parent source ID. The asset database must be reconstructible from committed files, not an indispensable hidden database.

Pipeline:

```text
Source file + import settings + importer version
  -> dependency discovery and validation
  -> normalized intermediate asset
  -> target-specific processing
  -> cached runtime chunks + dependency manifest
  -> release asset bundle
```

Begin with GLB/glTF, PNG/JPEG, selected HDR/environment inputs, and a deliberately small audio format set. glTF is an asset interchange format, not the engine's scene/prefab behavioural format. [S7]

Cache by source content, dependencies, settings, compiler version, and target profile. Include dependency hashes so changing a referenced texture invalidates the right material/model output. Distinguish stable identity from cache hash.

Use source JSON directly during early development where convenient. Cook production assets into a documented versioned container, **one file per asset** in 1.0, with explicit lengths, checksums, dependency IDs, and bounded chunk decoding. Pack files are a measured 1.x optimization if mobile load times require them. Binary scene data is an implementation choice to benchmark, not inherently faster just because it is binary. Never serialize raw Rust memory layouts.

Keep expensive decoding, mesh processing, and compression out of frame-critical execution. Upload GPU work in bounded chunks. Implement asset states, cancellation, ownership, reference tracking, eviction policy, fallback resources, and hot replacement only after a replacement validates. Keep the last valid asset when an edit fails.

Imports must validate paths, file sizes, recursion depth, decompression limits, external URI handling, and resource budgets. Opening a project or importing a model must not run arbitrary scripts.

### Headless data operations

Scene load/save, validation, prefab expansion, migration, and cooking must work without a window. Support asynchronous scene loading and explicit activation. Resolve references before lifecycle callbacks, release resources on unload, and distinguish additive loading from replacing the active scene.

Migrations are explicit, versioned, testable transformations with dry-run diffs and backups. Do not rewrite an entire project automatically when an incompatible editor opens it.

## 8. Renderer and Flutter integration

The Flutter/wgpu presentation path is the **first architectural risk Fynkel must retire**. M0 is a real go/no-go architecture gate, not a demo milestone.

### 8.1 M0 primary platform and comparison host

Use (ADR-0002):

- **M0 integration target:** macOS + Metal, Flutter with **Impeller** (Flutter's default desktop renderer; Skia is not verified).
- **Second Metal target:** iOS, smoke-tested in M1 (same CVPixelBuffer/IOSurface interop), qualified in M6.
- **Later native paths:** Windows/D3D12, Linux/Vulkan, Android GLES/Vulkan — post-1.0, each with its own interop and qualification.

Build the same minimal scene in:

1. a standalone Rust/wgpu host; and
2. a Flutter-hosted Fynkel viewport.

The standalone host is the baseline for measuring host/composition overhead.

### 8.2 Viewport paths to evaluate

Evaluate these in M0 instead of betting on one API:

**Path A — Flutter backend texture / `Texture` integration**

- Render with Rust/wgpu.
- Present through a platform-specific Flutter texture registry integration.
- Measure copies and synchronization.
- Known cost on Apple platforms: Flutter's `FlutterTexture` hands back a `CVPixelBuffer`. Zero-copy means an IOSurface-backed pixel buffer wrapped through `CVMetalTextureCache` into an `MTLTexture`, which wgpu can only render into through `wgpu-hal`'s unsafe `create_texture_from_hal` on the Metal backend. The `platform/` directory is therefore a real subsystem: per-platform `wgpu-hal` interop with documented invariants, not glue.

**Path B — Platform-native embedded surface/view**

- Let Fynkel own a native GPU surface/view embedded in the Flutter host.
- Flutter provides surrounding/overlay UI where platform composition permits it.

**Path C — Engine-owned game surface/window with Flutter UI separated**

- Fynkel owns the main game rendering surface.
- Flutter is composed as a separate native/UI layer or separate window where appropriate.
- This is an acceptable fallback if embedded texture composition is too expensive or unreliable.

**Path D — Separate game/player window**

- Especially acceptable for early Editor Play mode.
- The editor remains Flutter; the player is a separate native-hosted Flutter/Fynkel game process/window.

Do not fork Flutter in M0.

### 8.3 M0 viewport kill/fallback criteria

The preferred Flutter-hosted path fails the production gate if any of these are true on frozen reference hardware:

- It requires a full-frame GPU → CPU → GPU readback/upload every frame.
- Added host/composition overhead is consistently above the provisional M0 budget of **1.5 ms p95 at 1080p/60** compared with the standalone host. The budget is provisional: a working, readback-free Path A that lands modestly over it (for example ~2.5 ms) is **accepted** and the measured figure recorded in ADR-0002 rather than triggering a fallback. Only readback, latency, reliability, or composition failures are hard kills.
- It introduces more than one frame of unexplained presentation latency.
- Resize/DPI/minimize/restore/background/dispose behaviour is unreliable under repeat testing.
- Flutter overlays cannot compose correctly with the 3D viewport.
- Pointer/keyboard/focus routing is not controllable.
- The path prevents required GPU features or causes device/resource ownership conflicts that cannot be contained.

If Path A fails, evaluate Path B/C rather than spending months forcing `Texture` to work.

If **all practical native composition paths fail**, stop feature development and redesign the host architecture before M1. Do not bury a failed M0 behind renderer progress.

### 8.4 Platform integration contract

For every qualified target, document:

- graphics backend;
- adapter/device ownership;
- Flutter renderer relationship;
- native surface/texture ownership;
- synchronization mechanism;
- required copies;
- colour space;
- alpha/premultiplication rules;
- DPI scaling;
- resize path;
- device loss handling;
- lifecycle/background behaviour;
- resource disposal.

Flutter's `Texture` API uses platform-managed backend textures; it is not proof that an arbitrary wgpu texture can be imported without platform-specific work.

### 8.5 Renderer V1

Build a deliberately bounded forward renderer with:

- perspective and orthographic cameras;
- static/indexed meshes;
- textures/samplers;
- depth testing;
- frustum culling;
- instancing;
- PBR materials;
- environment lighting;
- one shadowed directional light;
- bounded point/spot lights;
- HDR intermediate rendering;
- tone mapping;
- selected anti-aliasing;
- alpha masking/blending;
- debug primitives;
- world-space UI pass.

Use explicit pass inputs/outputs:

```text
shadow
  -> opaque
  -> transparent/world UI
  -> post
  -> presentation
```

A small render graph may manage dependencies and transient resources. Do not start with a universal optimizing render graph.

Extract immutable render data from the simulation. Reuse allocations, cache pipelines/material bindings, batch compatible objects, and cap shader/pipeline variant explosion.

### 8.6 Renderer work after V1

Add features only when required by the reference game, pilot projects, or benchmark evidence.

Likely order:

- skeletal GPU skinning;
- LOD;
- better shadow quality;
- improved culling;
- selected post-processing;
- clustered/forward+ lighting if real light counts justify it;
- occlusion;
- indirect/GPU-driven drawing.

Do not implement TAA, SSAO, SSR, ray tracing, or GPU-driven rendering merely to match another engine's feature list.

*(R2, 2026-09-22: SSAO landed, and the rule above is why it is worth
saying what forced it. Not a feature list — the picture. Contact
darkening is the one cue the engine had no answer for: the ambient and
IBL terms light every crevice as if it were open sky, so a crate sitting
on a floor floats, and no amount of shadow-map work fixes it because the
sun is not what lights that gap. The estimate multiplies the ambient/IBL
term only — direct light is never occluded by a screen-space guess —,
it is one depth prepass plus two half-cheap full-screen passes, and the
quality profile can turn it off (`ambientOcclusion`, default true) for a
bit-identical picture. `docs/renderer.md` § Ambient occlusion.)*

*(R3a, 2026-09-22: **volumes** — how a frame looks is scene data. A
`fynkel.Volume` component (global, box or sphere, with a blend distance,
a weight and a priority) names a `.volume.json` profile of optional
overrides (present overrides, absent inherits); the renderer blends the
volumes that reach the camera once a frame on the CPU and uploads the
result. The quality profile stays the ceiling — it says what the device
affords, a volume only shapes what it allows. R2's two AO constants were
the first fields; bloom (R3b) and colour grading (R3c) are blocks in the
same profile, not new mechanisms. A general game-defined data asset — a
Unity `ScriptableObject`, `@DataAsset` on a Dart class — was considered
and deliberately deferred to the package-architecture work, where the
question of who owns a document kind gets answered once.
`docs/renderer.md` § Volumes.)*

*(R3b, 2026-09-22: **bloom** — the first new block in the volume profile
(`bloom { intensity, threshold, scatter, tint }`), off until a volume
gives it an intensity, capped by the quality profile's `bloom`. The
standard dual-filter chain (a Karis-weighted soft-knee prefilter, a
13-tap downsample, a tent upsample blended by scatter) in half-resolution
mips of one texture, added to the exposed radiance before ACES. No lens
dirt or flares: they are taste, and a game that wants them can add a
block. `docs/renderer.md` § Bloom.)*

*(R3c, 2026-09-25: **colour grading** — the profile's third block,
`colorGrading`: Unity's Tonemapping (ACES, Neutral, None), White Balance,
Color Adjustments (post-exposure, contrast, colour filter, hue shift,
saturation) and Lift Gamma Gain. Arithmetic in the post pass, folded into
two matrices on the CPU, skipped while neutral so a scene without it is
the picture it was to the bit; no quality flag, since it costs no memory.
A LUT (baked or authored elsewhere) is the next step when a game wants
one. `docs/renderer.md` § Colour grading.)*

## 9. Physics, animation, audio, and input

### Physics

Integrate Rapier behind Fynkel-owned components and handles.

1.0 baseline:

- static/dynamic/kinematic rigid bodies;
- box/sphere/capsule colliders;
- bounded MeshCollider support with explicit limitations;
- collision layers/masks;
- sensors/triggers;
- raycasts;
- shape casts;
- overlap queries;
- collision/trigger events;
- character controller;
- selected joints only when the reference game/pilots need them;
- continuous collision settings where justified.

Physics owns dynamic-body transforms. Gameplay modifies physics bodies using forces, impulses, velocities, kinematic targets, or an explicit teleport operation.

Do not allow transform hierarchy, animation, Dart scripts, and physics to silently fight over the same authoritative transform.

### Animation

Import skeletons, clips, and skins from supported glTF/GLB data.

1.0 baseline:

- clip sampling;
- looping;
- playback speed;
- simple transitions/state machine;
- basic blending;
- animation events;
- skinning;
- explicit root-motion policy.

Defer:

- advanced retargeting;
- complex IK;
- cinematic timeline;
- large node-based animation graph tooling.

### Audio

Expose Fynkel concepts rather than backend types.

1.0 baseline:

- AudioSource / AudioListener;
- 2D playback;
- basic 3D spatial attenuation/panning;
- buses;
- volume/pitch/loop;
- streamed music;
- voice limits/priority;
- pause/suspend;
- device/lifecycle handling.

Keep blocking allocation/decoding/I/O out of audio callbacks.

### Input

Use named input actions instead of hard-coded key checks.

Support:

- keyboard;
- mouse;
- pointer capture/relative movement;
- touch where a qualified target needs it;
- gamepad;
- rebinding;
- action recording/replay for tests;
- focus/pause behaviour.

Flutter UI consumes its own interactions first. Fynkel receives the remaining mapped game actions according to explicit focus rules.

Action state has two parts: **held** values (buttons down, axes) that every fixed tick sees, and **edges** (`pressed`/`released`) that exactly one fixed tick consumes (§5.1). A jump pressed once during a frame that runs three catch-up ticks triggers one jump.

The public `Input` convenience API may be static-looking, but it resolves through the active `GameContext` service, making it replaceable in tests and preventing process-global gameplay state.

## 10. UI model

**Screen-space UI:** Flutter owns menus, settings, HUD, dialogs, inventory, accessibility, and text entry. **Gameplay state reaches the HUD through Dart-side observable models** (`ValueNotifier`/`Stream`-backed classes that Behaviours write to); the engine snapshot never feeds widgets directly (ADR-0008). Raw engine fields (world positions for projected labels) are the one exception and use the snapshot subscription API. Never rebuild the whole app per frame.

**Projected world labels (Tier 1, M3):** Project selected world anchors into viewport coordinates. Include viewport transform, DPI, camera identity, clipping, and visibility. Depth occlusion requires an explicit policy; a screen overlay is not automatically hidden behind 3D geometry. Bound the number and update rate of anchored widgets; render dense crowds of labels natively if measurement requires it.

**True world-space UI (Tier 2, M5):** Implement a limited native WorldCanvas with panels, images, text, progress bars, simple layout, and basic buttons. Use maintained text shaping/font rasterization components, not a new text stack. Render using depth testing and ray-to-local-panel hit testing. Define canvas units, clipping, ordering, focus, and interaction events. Clearly document initial text/language coverage and unsupported text-entry behaviour.

Expose world UI through the same component schemas, authoring commands, and Dart API. Do not build a second general-purpose app UI framework. Flutter-to-world-texture remains an isolated experiment, not a requirement for release.

## 11. Editor and authoring service

Build Fynkel's runtime until the complete reference game is playable **without an editor**. Then implement Editor V0. Do not wait until the runtime is “finished”; after Editor V0, runtime features and their basic authoring support evolve together.

The editor is a client of the same authoring/document services used by the CLI and AI tooling. That service is the Rust crate **`fynkel_document`** (ADR-0016): the editor reaches it through the same C ABI as the runtime, the CLI links it directly, and AI agents use the CLI. There is exactly one implementation of validation, transactions, and undo data.

### 11.1 Shared authoring commands

Canonical commands include:

```text
CreateObject
DeleteObject
RenameObject
ReparentObject
AddComponent
RemoveComponent
SetField
AssignAsset
InstantiatePrefab
SetPrefabOverride
RevertPrefabOverride
CreateMaterial
SaveDocument
ImportAsset
ValidateProject
```

Every document mutation has:

- command ID;
- transaction ID;
- base document revision;
- affected persistent IDs;
- validation result;
- undo/redo data;
- structured diagnostics.

A transform-gizmo drag becomes one transaction/undo unit.

Stale revision writes must be rejected or surfaced as a conflict rather than silently overwriting newer editor/CLI/AI changes.

### 11.2 Editor V0

Required:

- desktop Flutter application;
- Scene viewport rendering a **real Rust world in edit mode** (native components only, no Behaviours ticking, gizmos via engine debug primitives);
- hierarchy;
- Inspector generated from **committed codegen metadata** (`lib/generated/`); stale metadata shows a "run `fynkel codegen`" banner — the editor never runs game code or codegen itself;
- asset browser;
- selection;
- move/rotate/scale gizmos;
- save/load;
- undo/redo;
- Play/Stop launching a **separate game process**;
- console;
- basic profiler/performance view;
- open script in external IDE;
- panel layout via a pub.dev docking/multi-split package (evaluate `docking`, `multi_split_view`; pick one in M4).

Not required for V0:

- custom code editor;
- browser editor;
- a hand-built docking framework;
- visual scripting;
- shader graph;
- collaborative editing.

### 11.3 Authoring vs play state

Maintain distinct concepts:

```text
Authoring documents
Edit-preview world
Actual play runtime
```

Edit mode does not run arbitrary gameplay behaviours by default.

Play launches the project's real game process separately (ADR-0016). The editor process never hosts game Behaviours. This is the 1.0 design, not an interim step; in-process Play may be reconsidered post-1.0 only with crash isolation evidence.

Stop discards runtime mutations unless a supported explicit “apply to source” operation is invoked.

Runtime-spawned objects can be inspected. Baking them back into authoring data is explicit.

### 11.4 Reload policy

**Assets/materials:** validate replacement first; keep the previous valid resource if import/compile fails.

**Dart code:** use supported Flutter/Dart development workflows plus Fynkel codegen/registry refresh. Structural behaviour/schema changes may require restart.

**Rust/native code:** rebuild and restart.

Flutter hot reload must not be treated as if it automatically reconstructs Fynkel native world state.

### 11.5 Editor crash/data-safety rules

- writes use atomic replacement where practical;
- autosave/recovery is separate from committed source documents;
- editor crashes must not corrupt the last explicit save;
- Play-mode crashes must not corrupt authoring documents;
- unknown plugin/component data must be preserved where possible instead of silently deleted.

## 12. Browser policy

Fynkel 1.0 does **not** include a browser editor or production browser runtime, and **no browser work — including feasibility spikes — happens before 1.0** (ADR-0002). The only pre-1.0 protection against lock-in is the list of portable contracts below.

### 12.1 Optional compile check

Once the render crate stabilizes (M3 or later), a CI job may run `cargo check --target wasm32-unknown-unknown` on it. It is a compile check, not a product; failures are recorded, not necessarily fixed.

### 12.2 Portability constraints kept before 1.0

Keep these contracts future-friendly:

- strict JSON source formats;
- stable asset IDs;
- renderer feature/capability abstraction;
- no public native pointers;
- no serialized dependency handles;
- shader/material interfaces expressed in portable data;
- bridge semantics defined above the native C ABI.

Do not add significant complexity to the native engine solely for speculative browser parity.

### 12.3 Post-1.0 browser work

After Fynkel 1.0 is stable, browser work may proceed as separate product milestones:

1. production Wasm/WebGPU runtime;
2. web-compatible Dart/Rust bridge;
3. browser Scene/editor UI;
4. local companion for filesystem/build access;
5. browser-only storage/import/export;
6. optional hosted build/compiler services;
7. collaboration/cloud features.

Each is a separate commitment. A browser UI does not imply browser compilation, and a WebSocket does not magically provide native GPU viewport streaming.

## 13. Fynkel distribution and developer workflow

Fynkel should feel like a normal Flutter/Dart development dependency plus optional tooling—not a proprietary project container.

### 13.1 Deliverables

Ship three version-matched deliverables:

1. **Fynkel Runtime SDK**
   - Dart API.
   - Flutter integration package.
   - Native runtime binaries/libraries for qualified targets.
   - Everything required to run a game.

2. **Fynkel development tools / CLI**
   - project creation;
   - validation;
   - code generation;
   - import/cooking;
   - build orchestration;
   - headless tests;
   - diagnostics;
   - AI/automation interface.

3. **Fynkel Editor**
   - separate desktop Flutter application;
   - optional;
   - development-only.

Installing the runtime package must not place the full editor/import pipeline inside the final game binary.

### 13.2 Project model

A Fynkel game remains a normal Flutter project:

```text
my_game/
  pubspec.yaml
  pubspec.lock
  fynkel.project.json
  fynkel.lock.json

  lib/
    main.dart
    game/
    generated/

  assets/
    scenes/
    prefabs/
    materials/
    models/
    textures/
    audio/

  tests/
  .fynkel/
    cache/
  build/
```

The project remains usable in VS Code, IntelliJ/Android Studio, command-line Flutter tooling, and ordinary Git workflows.

### 13.3 Proposed developer workflow

```sh
fynkel create my_game
cd my_game

fynkel doctor
fynkel codegen        # runs both generators (native schema + behaviour/build_runner); output is committed
fynkel run
fynkel test

fynkel editor .
fynkel validate
fynkel build macos
```

The exact commands may change, but the experience should remain compact and predictable.

### 13.4 Runtime package strategy

Ship prebuilt, checksummed native runtime artifacts per supported target so ordinary game developers do not need a Rust toolchain (ADR-0015). Mechanism: `fynkel_flutter` uses Flutter's **native assets build hook** (`hook/build.dart`). The hook reads the runtime version from `fynkel.lock.json`, downloads the matching artifact from the release store, verifies its checksum, and registers it as a code asset. If a Rust toolchain is present and the artifact is missing or `FYNKEL_BUILD_FROM_SOURCE=1` is set, it falls back to `cargo build`.

Maintain a source-build path for:

- Fynkel contributors;
- unsupported/custom configurations;
- development of the native runtime.

The package/build integration must fail with actionable diagnostics when a matching runtime artifact is unavailable.

### 13.5 Version matching

`fynkel.lock.json` should record or resolve:

- runtime version;
- Dart SDK package version;
- editor/tools compatibility range;
- schema/document version;
- asset compiler version;
- codegen version.

The Editor may be newer than a project only when compatibility is explicit. Never silently migrate and rewrite a project on open.

### 13.6 Release build contents

Production games should include only what the game needs:

```text
Flutter/Dart application runtime
Fynkel native runtime
compiled/cooked game assets      today: the cooked cache staged into fynkel_cache/ and bundled as Flutter assets (§19.2d item 6a)
generated behaviour registry
platform packaging
```

Exclude:

- Fynkel Editor;
- import tools;
- schema authoring UI;
- development servers;
- AI command endpoints;
- debug-only inspectors/profilers unless explicitly enabled.

## 14. AI and automation interface

AI-native does **not** mean “put a chatbot inside the editor.” It means the project and engine are controllable through stable, machine-readable contracts.

Humans, IDEs, CI, the editor, and AI agents should use the same underlying project model and authoring commands.

### 14.1 Structured CLI

The **structured `--json` CLI is the only agent interface in 1.0** (ADR-0017). An MCP server wrapping the same commands is a cheap post-1.0 addition; an in-editor agent panel is not planned. `fynkel create` writes an `AGENTS.md` into every new project describing the commands, schema locations, and the validate→build→test loop.

Proposed commands:

```sh
fynkel project describe --json
fynkel schema list --json
fynkel schema describe fynkel.Transform --json

fynkel scene inspect assets/scenes/main.scene.json --json
fynkel scene validate assets/scenes/main.scene.json --json
fynkel scene apply --patch patch.json --dry-run --json

fynkel asset import assets/models/player.glb --json
fynkel validate --json

fynkel test --headless --steps 600 --seed 42 --json   # Dart headless game host: full simulation incl. Behaviours, no renderer/GPU
fynkel run --test-profile reference
fynkel capture --scene Playground --output preview.png  # renders offscreen; requires a GPU
fynkel state dump --json                                 # world state after a headless run

fynkel build macos --json
```

All automation-facing commands must have stable exit codes and structured diagnostics.

### 14.2 Diagnostic contract

Diagnostics should contain:

- stable code;
- severity;
- human-readable message;
- document path;
- JSON/document field path where relevant;
- persistent object/component/asset ID;
- source line/column where available;
- suggested corrective action;
- related diagnostics.

An AI agent should not need to scrape colourful terminal prose to determine whether a scene is valid.

### 14.3 AI workflow target

A capable agent should be able to:

```text
inspect project
 -> inspect schemas
 -> create/modify scene/prefab/material data
 -> write Dart behaviour
 -> run code generation
 -> validate
 -> build/run reference test
 -> simulate recorded input
 -> inspect structured failures/runtime state
 -> capture image
 -> patch
 -> repeat
```

The editor is optional in this loop.

### 14.4 Safe authoring

Opening project data must not execute game code.

AI-authored scripts/plugins are untrusted executable code. Build/run operations require the same trust boundary as human-authored external code.

Provide:

- dry-run patches;
- project-root path restrictions;
- revision checks;
- explicit build/run commands;
- source-control-friendly file changes;
- deterministic test inputs where practical.

AI tooling must not bypass validation or mutate hidden editor-only state.

## 15. Performance and quality requirements

Performance is a product feature and must be measured from M0.

### 15.1 Benchmark manifest

Freeze named benchmark profiles containing:

- machine model/CPU/GPU/RAM;
- OS version;
- graphics backend;
- driver/runtime version;
- Flutter/Dart/Rust versions;
- Fynkel commit/version;
- resolution;
- quality settings;
- scene version;
- warm-up duration;
- sample duration;
- capture command.

Do not compare numbers collected from materially different profiles as if they were the same benchmark.

### 15.2 Frame-time targets

For the desktop reference profile at 1080p/60:

- frame interval: 16.67 ms;
- initial engineering target: keep the normal CPU critical path and GPU workload each with useful headroom;
- use approximately **12 ms p95** as an early total-work target where measurement semantics make that meaningful;
- track p99, stutter, missed presentation deadlines, and input latency;
- do not rely on average FPS.

CPU and GPU may overlap; do not simply add their timings and call the result frame time.

The M0 viewport composition overhead has its own stricter provisional gate in §8.

### 15.3 Separate benchmark workloads

Maintain independent benchmarks for:

- `hecs` component iteration;
- spawn/despawn churn;
- transform hierarchy updates;
- Dart/Rust bridge command throughput;
- snapshot bandwidth;
- instanced rendering;
- many materials/pipeline changes;
- skeletal animation;
- physics body counts;
- asset loading;
- Flutter-heavy HUD/editor UI;
- Editor hierarchy/Inspector scaling.

Do not combine maximum entity, triangle, light, and rigid-body counts into one misleading “supports X objects” claim.

Because the 1.0 ambition is thousands of rendered objects (ADR-0011), maintain a dedicated **`benchmarks/stress`** scene from M3 onward — separate from the reference game, which stays small — with on the order of 5,000 instanced static meshes, 500 dynamic rigid bodies, and 300 ticking Dart Behaviours. Its numbers are tracked per milestone against the benchmark manifest.

Add **Dart allocations per frame** and **Dart GC pause count/duration** to the M1 gate, not only to instrumentation: Dart runs every frame, and GC is the most likely p99 stutter source.

### 15.4 Instrumentation

Capture at minimum:

- Dart behaviour time;
- native simulation time;
- physics;
- animation;
- transform propagation;
- render extraction;
- render preparation;
- GPU pass timings where supported;
- draw/dispatch counts;
- visible/culled objects;
- bridge calls/bytes/copies;
- Dart allocations where measurable;
- native memory;
- asset residency;
- estimated GPU memory;
- Flutter frame/UI timing;
- loading/startup time.

### 15.5 Continuous quality

CI should include:

- Rust unit/integration tests;
- Dart tests;
- codegen fixtures;
- JSON/schema fixtures;
- bridge protocol golden tests;
- scene/prefab round trips;
- headless reference-game tests;
- renderer smoke tests on available GPU runners;
- formatting/linting;
- dependency/license checks;
- package/build smoke tests.

Run performance benchmarks on controlled machines, not arbitrary shared CI workers where results are too noisy.

### 15.6 Robustness testing

Fuzz or property-test:

- malformed JSON documents;
- invalid IDs/references;
- command buffers;
- bridge lengths/counts;
- imported asset metadata;
- prefab overrides.

Lifecycle stress tests repeatedly:

- create/destroy worlds;
- load/unload scenes;
- resize viewports;
- enter/exit Play mode;
- minimize/restore;
- background/foreground;
- create/destroy GPU resources.

### 15.7 Supported-platform rule

A platform is not “supported” because Flutter or wgpu lists it.

Fynkel support requires:

- build/install;
- rendering;
- input;
- lifecycle;
- file/assets;
- audio;
- release packaging;
- reference-game test;
- performance/quality budget;
- documented limitations.

Mobile qualification additionally requires sustained-device/thermal testing.

## 16. Milestones and acceptance gates

Milestones are **gates**, not time estimates. Do not progress because a date arrived; progress when the contract is proven.

For a small team, keep work-in-progress narrow. The critical path is:

```text
M0 architecture proof
  -> M1 runtime foundation
  -> M2 documents/assets/scripts
  -> M3 code-first game, Tier 1
  -> M4 Editor V0
  -> M5 game-development completeness + reference game Tier 2
  -> M6 native qualification/optimization
  -> M7 beta / 1.0
```

Browser product work is not on the 1.0 critical path.

### M0 — Architecture proof, repository bootstrap, and go/no-go

**Purpose:** retire the single highest architectural risk — compositing Rust/wgpu output under Flutter on macOS without a CPU readback — before building broad engine functionality. Everything not needed to answer that question is out of M0 (ADR-0002).

**Build**

- Repository/monorepo structure, toolchain/version lock, ADR files.
- Standalone Rust/wgpu/winit host rendering a lit cube on macOS/Metal; this is the overhead baseline.
- Flutter (Impeller) macOS app presenting the same cube through **Path A**: IOSurface-backed `CVPixelBuffer` → `CVMetalTextureCache` → `wgpu-hal` texture import.
- **Path C** (engine-owned surface with Flutter composed as a native layer on top) far enough to compare risk. Path C is the only production fallback. Path D (separate window) is prototyped only as the Editor Play mechanism and **cannot satisfy the game-host gate**, because it does not prove Flutter HUD over the Fynkel 3D world.
- Resize/DPI/minimize/background/dispose stress harness.
- Flutter overlay + pointer/keyboard focus test over the viewport.
- Frame-time instrumentation on both hosts; M0 benchmark manifest and capture script.
- Tiny versioned C ABI round-trip: header + `ffigen`, one command buffer in, one snapshot out.
- M0 architecture report and ADR-0002 measured numbers.

**Gate**

- A readback-free **Path A or Path C** composites Flutter UI over the 3D world correctly, with controllable input. Path D does not count.
- The Metal synchronization contract is demonstrated, not assumed (ADR-0002): wgpu and the imported texture share the same `MTLDevice`; IOSurface-backed `CVPixelBuffer` ring of 2–3 buffers; GPU completion/fence ownership so Flutter never samples a buffer wgpu is still rendering into; Flutter raster-thread lifetime respected; resize reallocates the ring without tearing or use-after-free.
- Measured composition overhead is recorded; ≤1.5 ms p95 is the target, modest overshoot is accepted and written down.
- Resize/DPI/lifecycle behaviour is reliable under repeat testing.
- The C ABI round-trip works from Dart with `ffigen`-generated bindings.

**Kill/fallback rule**

- If Path A fails a hard criterion (readback, >1 frame latency, unreliable lifecycle, overlay/input failure, device mismatch that cannot be contained), move to Path C.
- If every practical composition path fails, stop and redesign the host architecture before M1.

**Explicitly not in M0**

- Windows or any second graphics backend (post-1.0; periodic `cargo check` only).
- Wasm/WebGPU spike (removed).
- `fynkel_ecs`, `hecs`, physics, assets, editor, license decision (private repo until M3).

### M1 — Runtime foundation, `hecs` wrapper, and bridge

**Depends on:** M0.

**Build**

- `fynkel_core` as a library with `step()`; no engine-owned simulation thread (ADR-0007);
- `fynkel_ecs` facade over `hecs`; Fynkel runtime handles with generation, mapped to internal `hecs::Entity`;
- component registration metadata: Rust structs as source of truth, generating Dart bindings, wire encoders, and JSON Schema (ADR-0012);
- Transform/hierarchy;
- world lifecycle;
- explicit scheduler phases per §5.1, accumulator + interpolation alpha exposed to the renderer;
- command buffers and staged-write overlay;
- subscribed snapshots;
- pull-only event queue;
- status code + diagnostics buffer, `FynkelError` in debug (ADR-0014);
- Flutter host on the main isolate driving `step()` from a frame callback;
- standalone and headless (no-renderer) hosts sharing the same core;
- iOS smoke test: the M0 cube on a device through the same CVPixelBuffer interop;
- bridge diagnostics: calls, bytes, copies, Dart allocations per frame, GC pauses.

**Gate**

- Dart can create, inspect, update, and destroy objects through Fynkel APIs.
- Stale handles fail; entity reuse does not alias stale Dart handles.
- Batched updates are used in hot paths; no per-property FFI.
- Snapshot subscriptions are bounded/measurable.
- Steady-state Dart allocation per frame on the M1 test scene is zero or documented.
- World teardown is leak-safe; transform hierarchy passes cycle/reparent tests.
- `hecs` is not visible in public APIs/files.
- The cube renders on an iOS device.

**Do not**

- write a custom ECS;
- build a generic parallel scheduler;
- add a gameplay isolate or Rust sim thread;
- expose mutable native memory to Dart.

### M2 — Project documents, assets, prefabs, materials, and behaviours

**Depends on:** M1.

**Build**

- strict JSON schemas (generated for built-in components from Rust);
- scene documents;
- material documents;
- simple prefab format/override semantics;
- `.meta.json` sidecar asset identity (ADR-0006);
- document migration framework;
- glTF/GLB importer;
- texture/audio import baseline;
- content-addressed/incremental cache keys;
- `fynkel codegen`: one user-facing command wrapping **two independent generators** (ADR-0012) — (a) the native schema generator: Rust component definitions → JSON Schema + Dart native component API + wire codecs; (b) the behaviour generator: Dart `@FynkelBehaviour` classes → `build_runner` → factories, field serializers, Inspector metadata. Separate incremental caches and test fixtures;
- `fynkel_document` authoring command crate and CLI validation (ADR-0016);
- `fynkel create` including the `AGENTS.md` template (ADR-0017).

**Gate**

- documents round-trip without losing unknown/valid data;
- scene IDs remain stable through edits;
- asset moves preserve identity;
- prefab references remap correctly;
- invalid fields produce exact diagnostics;
- a compiled Dart behaviour can be instantiated from authored data;
- project can be validated/imported headlessly.

**Do not**

- hide project state in an editor database;
- implement nested prefab inheritance;
- build a shader graph;
- make editor UI the only way to modify data.

### M3 — Code-first reference game, Tier 1

**Depends on:** M2.

**Build the §1.2 reference game at Tier 1 without relying on Fynkel Editor** (ADR-0010).

Required runtime capability now includes:

- renderer V1 **with instancing and frustum culling** (thousands-of-objects ambition, ADR-0011);
- static-mesh player and third-person camera;
- physics and character controller;
- 2D audio;
- input actions, including the Flutter touch-overlay mapping for iOS;
- scenes/state transitions;
- prefab spawning;
- Flutter menus/HUD via Dart-side observable models;
- projected Flutter world labels;
- the starter native component catalogue (§3.5);
- saveable settings;
- headless gameplay tests (`fynkel test --headless`) and `fynkel state dump`;
- the `benchmarks/stress` scene.

**Gate**

- a clean checkout can build/run the reference game through documented commands;
- the game is playable end-to-end on macOS; it launches and runs on an iOS device (qualification is M6);
- no editor is required;
- critical frame paths are instrumented;
- reference-game and stress-scene performance are recorded against the benchmark manifest;
- the repository is ready to go public (license decided per ADR-0005; README, CONTRIBUTING, CI in place).

**Do not**

- add skinning, WorldCanvas, or spatial audio yet (Tier 2 → M5);
- add speculative systems to make the demo impressive;
- postpone usability problems to the editor.

### M4 — Fynkel Editor V0

**Depends on:** M2 plus stable/playable M3 integration.

**Backlog:** §19.2e (written 2026-09-21). M4 started in parallel with the open M3 items on branch `editor` in its own worktree; §19.2e fixes the rules for that (new directories only, shared-crate touches as separate commits merged to `main` first) and resolves where the authoring ABI lives (`fynkel_native`, feature `authoring`), the edit-preview sync, picking, the Play channel and the extension model (ADR-0018).

**Build** (ADR-0016)

- Flutter desktop editor using a pub.dev docking/multi-split package;
- Scene viewport rendering a real Rust world in edit mode;
- hierarchy;
- Inspector generated from committed codegen metadata;
- asset browser;
- selection;
- transform gizmos;
- save/load;
- undo/redo via `fynkel_document` transactions;
- Play/Stop launching a separate game process;
- console;
- basic profiler;
- external IDE navigation.

**Gate**

- a developer can recreate/edit the reference level visually;
- all mutations go through shared authoring commands;
- the CLI sees the same data immediately;
- Play launches the actual project runtime;
- Play crashes do not corrupt source documents;
- ordinary errors are diagnosable without editing raw UUIDs/cache files.

**Do not**

- build a custom code editor;
- build a docking framework by hand;
- host game Behaviours in the editor process;
- build browser parity;
- implement gameplay rules inside editor widgets.

### M5 — Practical game-development completeness

**Depends on:** M4.

**Build/finish**

- **Reference game Tier 2** (ADR-0010): skeletal animation and GPU skinning with the animated player; minimal native `WorldCanvas` (panels, text via a maintained shaping/rasterization stack, progress bars, buttons); spatial audio;
- animation usability;
- audio usability;
- prefab override UX;
- scene lifecycle;
- project settings;
- persistence/save example;
- world-space UI tooling;
- input rebinding;
- component/asset inspectors;
- runtime inspection;
- documentation/tutorials for the reference game;
- first external pilot project support.

**Gate**

- the full (Tier 1 + Tier 2) reference game runs and can be authored comfortably in editor-first or hybrid mode;
- at least one external pilot project reaches a playable state;
- common workflows do not require undocumented internal APIs;
- migrations and error handling survive normal project evolution.

**Do not**

- introduce large systems unused by reference/pilot projects;
- stabilize a native plugin ABI just because 1.0 is approaching.

### M6 — Native platform qualification and optimization

**Depends on:** M3–M5. Target-specific work begins earlier where necessary.

**Build**

- versioned prebuilt native artifacts delivered through the native-assets hook (ADR-0015);
- iOS qualification: touch overlay, app lifecycle (background/foreground GPU suspension), thermal/sustained testing, code signing and App Store packaging;
- clean-machine installation paths;
- packaging/export workflows;
- release-mode asset cooking;
- lifecycle hardening;
- quality profiles;
- profiler improvements;
- measured culling/batching/memory optimizations;
- crash reporting hooks/interfaces where appropriate.

**Gate**
For every platform advertised for 1.0:

- clean install/build/run succeeds;
- reference game passes lifecycle tests;
- release package excludes editor/dev-only code;
- input/audio/rendering work;
- performance budgets and known limits are documented;
- binaries/artifacts are reproducible or traceable and checksummed/signed per release policy.

**Release target set**
**macOS + iOS** (ADR-0009). Windows, Linux, and Android are not on the 1.0 marketing matrix regardless of how well they happen to work.

**Do not**

- call unqualified dependency support “Fynkel support”;
- optimize synthetic microbenchmarks while real reference/pilot bottlenecks remain.

### M7 — Beta and Fynkel 1.0

**Depends on:** M6.

**Build/finalize**

- API cleanup;
- document migrations;
- SDK deprecations/changelog;
- tutorials;
- project templates;
- troubleshooting;
- architecture docs;
- stable package publishing;
- release/update process;
- security/reporting docs;
- external pilot feedback fixes.

**Gate**

- reference game ships through the supported workflow;
- at least two external small projects/pilots can build meaningful playable content without undocumented assistance;
- release-blocking crash/data-loss bugs are resolved;
- public SDK/project formats have migration policy;
- supported platform matrix is explicit;
- benchmark report is published internally/publicly as appropriate;
- editor is optional;
- browser editor/runtime is still not required.

**1.0 means**
A small team can build and ship a native 3D game with Fynkel using Dart/Flutter and the optional editor.

It does **not** mean feature parity with Unity, Godot, or Unreal.

## 17. Workstream ownership and delivery rules

The plan must work for a small team or even one primary developer using AI tooling. “Owner” means accountable responsibility, not a requirement for a separate full-time person.

### 17.1 Responsibility areas

Maintain clear ownership for:

- architecture/contracts;
- runtime/ECS/bridge;
- renderer/platform integration;
- Dart SDK/codegen;
- documents/assets/import pipeline;
- editor/tooling;
- testing/release quality.

One person may own several areas. A code-generation or AI assistant is not the accountable owner of an architectural contract.

### 17.2 Small-team execution rule

Keep the critical path narrow.

Preferred sequence:

```text
prove architecture
 -> finish runtime slice
 -> finish data/tool slice
 -> finish complete game
 -> add editor
 -> harden
```

Parallelize only work that has a stable interface and does not create large integration debt.

Examples of safe parallel work:

- schema fixtures while ECS bridge stabilizes;
- reference-game art/content preparation;
- documentation/tests;
- isolated importer implementation.

Examples of dangerous parallel work:

- full editor before document commands stabilize;
- custom ECS while bridge/world is unfinished;
- browser product while native viewport is unresolved;
- advanced renderer while core asset/material contracts are changing.

### 17.3 Definition of done for an engine feature

A feature is not done when one demo works.

It needs, where relevant:

- public API;
- source/document schema;
- runtime implementation;
- editor representation if required by 1.0;
- validation/diagnostics;
- serialization/migration;
- asset/resource lifetime rules;
- tests;
- performance instrumentation;
- docs/example;
- headless/automation accessibility.

### 17.4 Highest risks and controls

| Risk                                     | Control                                                                               |
| ---------------------------------------- | ------------------------------------------------------------------------------------- |
| Flutter/wgpu presentation path fails     | M0 first; evaluate multiple native composition paths; explicit kill/fallback criteria |
| Bridge becomes chatty                    | command buffers, snapshots, instrumentation, no hidden native getter calls            |
| ECS becomes an engine project            | use `hecs`; custom replacement only after benchmark/feature gate                      |
| Dart/native state diverges               | authoritative ownership table, ticked commands, staged reads, snapshots               |
| Editor creates a second project model    | shared authoring commands and strict source documents                                 |
| Asset pipeline becomes opaque            | committed metadata, stable IDs, reconstructible cache                                 |
| Scope explodes                           | reference game as scope oracle; browser removed from 1.0                              |
| Performance work starts too late         | instrumentation in M0/M1, benchmark manifests                                         |
| Native dependencies leak into public API | wrapper modules and contract tests                                                    |
| Small team fragments effort              | WIP limits and milestone critical path                                                |
| AI-generated changes bypass safety       | same schemas/validation/revision/trust rules as editor/CLI                            |
| Licensing is postponed                   | repo stays private; ADR-0005 decided before the M3 public push                        |
| Dart per-object scripting hits a scale wall | §3.5 rule, native component catalogue, stress benchmark from M3                    |
| Renderer silently becomes Metal-only     | periodic `cargo check` for the Windows target; wgpu abstractions only                  |
| Dart GC causes p99 stutter               | allocation/GC gates from M1                                                            |
| Engine grows a Flutter dependency        | standalone and headless hosts share `fynkel_core`; CI builds them                     |

## 18. What not to do

Do **not**:

- attempt Unity/Godot feature parity for 1.0;
- build a custom ECS before `hecs` is proven insufficient;
- expose `hecs`, Rapier, wgpu, Rust pointers, or native memory layouts in public contracts;
- make one FFI call per property/object in hot paths;
- copy the entire ECS into Dart every frame;
- store editor-only truth outside version-controlled project documents;
- identify entities/components by array index or display name in saved data;
- implement prefab overrides as an unspecified recursive merge;
- make the editor mandatory for build/run;
- delay the first complete game until the editor exists;
- delay the editor until every runtime subsystem is “finished”;
- build the browser editor before 1.0;
- build a cloud compiler as a side effect of browser experimentation;
- run browser spikes or product work before 1.0;
- assume Flutter `Texture` means zero-copy wgpu integration (it needs `wgpu-hal` interop per platform);
- let the engine own the main loop or depend on Flutter; the host owns the loop;
- add a gameplay isolate or Rust simulation thread before a measured trigger;
- accept full-frame CPU readback as the normal viewport solution;
- fork Flutter in the first architecture milestone;
- build a custom Dart coroutine language instead of using Dart async primitives;
- build a full IDE/code editor;
- build visual scripting or shader graphs before real projects justify them;
- add a plugin ABI before internal boundaries are stable;
- assume fewer dependencies automatically means better performance;
- rewrite proven physics/audio/text/import functionality without evidence;
- advertise platform support because a dependency compiles there;
- optimize average FPS while ignoring p95/p99 stutter and latency;
- silently migrate projects without dry-run/backup/version rules;
- allow AI/editor commands to bypass validation or source-control-friendly data;
- let browser, console, XR, multiplayer, terrain, or marketplace work block the native engine.

The main discipline is: **finish coherent vertical slices before expanding breadth.**

## 19. First implementation backlog

Turn this blueprint into the repository immediately. Do not leave the plan as a standalone strategy document.

### 19.1 Repository bootstrap

Create:

- `README.md`;
- root licenses/placeholders according to ADR-0005;
- `docs/architecture/`;
- `docs/decisions/`;
- `docs/guides/`;
- `crates/`;
- `packages/`;
- `apps/`;
- `examples/reference_game/`;
- `tests/`;
- issue tracker labels/milestones for M0–M7.

Create ADR files for the initial decision set in §2.5.

### 19.2 M0 issue backlog

Track at least these issues (ADR-0002 scope):

1. Freeze toolchain matrix (Rust, Flutter/Dart with Impeller, wgpu, Xcode).
2. Freeze §1.2 reference-game spec with the Tier 1/Tier 2 split.
3. Build the standalone macOS/Metal wgpu/winit host with a lit cube.
4. Record the standalone frame/presentation baseline.
5. Build the Flutter (Impeller) macOS app with Path A: IOSurface `CVPixelBuffer` → `CVMetalTextureCache` → `wgpu-hal` import, on the same `MTLDevice` as wgpu, with a 2–3 buffer ring and GPU completion ownership.
6. Measure copies, latency, and composition overhead against the baseline.
7. Prototype Path C (engine-owned surface, Flutter as native overlay layer) far enough to compare. Path D is prototyped only as the Editor Play window.
8. Add the resize/DPI/minimize/background/dispose stress harness.
9. Add the Flutter overlay + pointer/keyboard focus test.
10. Define the C ABI v0 header and generate Dart bindings with `ffigen`.
11. Send one batched command buffer Dart → Rust and return one snapshot + event buffer Rust → Dart.
12. Write the M0 benchmark manifest and capture script.
13. Produce the M0 architecture report and fill ADR-0002 with measured numbers.

Removed from M0 (v0.4): second graphics backend, Wasm/WebGPU spike, `fynkel_ecs`/`hecs` wrapper, stale-handle design, license decision.

### 19.2b M1 issue backlog

Derived from the M1 "Build" list above; one commit per item, in this order:

1. `fynkel_contracts`: runtime handle (`world`, slot, generation), type/component ids, status and diagnostic codes, event kinds, protocol versions.
2. `fynkel_ecs`: facade over `hecs` — handle ↔ `hecs::Entity` mapping, spawn/destroy/insert/remove/get, deferred structural changes, world teardown; §4.6 tests; contract test that `hecs` is absent from public APIs.
3. `fynkel_scene`: Transform + hierarchy (§4.5) — local/world, dirty-subtree propagation, cycle rejection, `setParentKeepLocal`/`KeepWorld`, deterministic child order, subtree destruction.
4. `fynkel_core`: world lifecycle, `FrameClock` (accumulator, `maxStepsPerFrame`, alpha, `MissedDeadline`), `step()` with the §5.1 phases, two command lanes with the presentation override + staged authoritative value, pull-only two-lane event queue, subscribed snapshots, diagnostics buffer.
5. Component schema as source of truth (ADR-0012): `#[derive(Component)]` emitting schema metadata; `fynkel codegen` native pipeline → JSON Schema (`schemas/`), Dart codecs (`packages/fynkel/lib/generated/`), Rust wire codecs; schema fingerprint derived, not hand-maintained.
6. `fynkel_native` v1 over `fynkel_core`: full operation families (§5.3) incl. query, request ids, stats; Dart SDK `World`/`GameObject`/`Transform` API with staged-write overlay; `FynkelError` in debug; bridge diagnostics (calls, bytes, copies, Dart allocations, GC pauses).
7. Hosts sharing the core: `hosts/headless` (test loop, `fynkel test` seed), `hosts/standalone` rendering the core's extract (interpolated by alpha), Flutter host driving `step()` from the frame callback through the Path A viewport moved into `platform/apple`.
8. Native-assets `hook/build.dart` for `package:fynkel` (ADR-0015).
9. iOS smoke test: the cube on a device through the same interop.
10. M1 gate report: allocation-per-frame measurement, teardown leak test, bandwidth numbers appended to the benchmark capture.

**M1 carry-over (2026-09-19, from `docs/m1-gate-report.md` §5).** The M1 gate passed; these items are what the gate did not require but §5.1 and the M1 build list imply. Each is owned by the milestone named, not deferred indefinitely:

- **C1 (M2, before any scene with movers is authored):** allocation-free hot-path writers on the command batch — `setPosition(entity, x, y, z)`, `setRotation`, a reusable `TransformWriter` — so per-frame transform updates never materialise `Transform`/`Vec3`/`Quat` objects. Keeps ADR-0013 (immutable values remain the read/API type). Measured driver: ≈150 B per transform write, 3.6 KB/frame at 24 movers, ≈300 KB/frame at 2 000.
- **C2 (M3):** input capture per §5.1 — held state plus pressed/released edges consumed by exactly one fixed tick — in the hosts and the `step()` input view.
- **C3 (M3, host profiling):** live Dart allocation and GC counters in the Flutter host's bridge diagnostics (today only the offline `tool/alloc_bench.dart` probe), and an AOT (Flutter release) allocation capture in `benchmarks/m0/capture.py`. **Done (M3-16 part 2, 2026-09-22):** `DartHeapProbe` in `package:fynkel_flutter` — the app's own VM service, the `GC` timeline events' before/after heap and `getMemoryUsage` per stats window (exact bytes allocated; `getAllocationProfile` was rejected: 5.7 MB a call, allocating more than it measured) — on `FynkelFrameStats.dartHeap`; the AOT capture runs the host's `--profile` bundle (AOT with the service; a release build has none) into `alloc_aot`.
- **C4 (M5, with physics):** native system registration for the pre-tick / physics / post-physics / animation phases; `World::step()` fixes the order today with no producers.
- **C5 (M3):** subscription budget diagnostic (bytes per snapshot against a host-set ceiling) so "bounded" is enforced, not only measured.
- **C6 (first release):** publish `fynkel.lock.json` prebuilt artifacts with sha256s; until then the hook's prebuilt path is covered by unit tests only.

### 19.2c M2 issue backlog

Derived from the M2 "Build" list and gate (§16) and bound by ADR-0006 (formats), ADR-0012 (two generators), ADR-0016 (`fynkel_document`), ADR-0017 (`--json` CLI). One commit per item, in this order; §17.2 applies — data contracts first, tooling on top, behaviours last because they depend on every document kind. Rendering of meshes/materials is **M3** (renderer V1); M2 fixes the documents, validation, import, and cache, and proves them headlessly.

**Scope rules fixed here (cheaper to decide now than to migrate later):**

- Every document is one JSON object with `$schema`, `formatVersion`, `id` (UUID v4 string), `name`, and its kind-specific body. Canonical output: 2-space indent, keys in schema order, `\n` line endings, trailing newline; unknown keys are preserved byte-for-byte in value and position.
- Component instances carry `type` (namespaced type id), `version`, `data`. A missing type keeps `data` opaque; validation reports it as a warning, build as an error.
- `AssetRef` fields serialize as `{ "asset": "<uuid>", "sub": "<sub-id>" | absent }`; never a path.
- Diagnostics everywhere use the §14.2 record: `code`, `severity`, `message`, `document`, `pointer` (RFC 6901), `id`, `line`/`column` when known, `suggestion`, `related[]`. Stable code namespace `FYN-D####` (documents), `FYN-A####` (assets), `FYN-P####` (project), `FYN-G####` (codegen), `FYN-E####` (editing: authoring commands and transactions), `FYN-B####` (behaviours: the Dart SDK attaching authored behaviours at load).
- Dependencies added in M2: `serde_json` (strict, `preserve_order`, `float_roundtrip`), `jsonschema` (draft 2020-12), `uuid`, `gltf`, `image` (PNG/JPEG only), `hound` (WAV), `lewton` or `symphonia` (Ogg Vorbis, chosen at item 10), `blake3` (cache keys), `similar` (diffs). Each recorded in `docs/toolchain.md` per §2.3.

1. **C1 — allocation-free transform writers** (carry-over): `TransformWriter` (reusable, chainable, encodes straight into the batch) plus `GameObject.writeLocal/writePresentation` and scalar `setLocalPosition/Rotation/Scale`; `alloc_bench` and the Flutter demo scene switched to them. **Done:** 5 105 → 1 492 B/frame on the 25-entity scene, 0 GC events in 3 000 frames (`2026-09-19`).
2. **`fynkel_document` seed + strict JSON + diagnostics** (done 2026-09-19; `docs/documents.md`): `crates/fynkel_document` (ADR-0016) with the strict reader/writer (duplicate keys, non-finite numbers, malformed UUIDs, wrong-type coercion all rejected with pointers; canonical writer; unknown-key preservation), the document envelope, and the §14.2 `Diagnostic` record in `fynkel_contracts`. JSON Schema 2020-12 validation behind one `validate(kind, json)` entry. Round-trip corpus under `crates/fynkel_document/fixtures/` (valid, invalid-with-expected-diagnostics, unknown-data).
3. **Scene document + schema** (done 2026-09-19; `docs/documents.md` § Scene documents; codes `FYN-D0200`–`D0209`, `fixtures/scene/` corpus, `x-fynkel.authored` on component schemas): `scene.schema.json` emitted by the native schema generator from the Rust document model plus every registered component schema (`schemas/components/*`). UUID-keyed `entities`, `parent`, per-entity ordered `children`, `components` keyed by instance UUID. Semantic validation: dangling parent/child, child order ↔ parent consistency, cycles, duplicate component type per entity where the schema says `unique`, unknown type → warning with data kept. `fynkel scene inspect|validate <path> --json`.
4. **Starter native component schemas** (done 2026-09-19; `fynkel_core::components`, ids 4–7, `AssetRef` + enum field kinds in `docs/codegen.md`, fingerprint `0xa3c3bd56d1793195`): `fynkel.Camera`, `fynkel.MeshRenderer` (`mesh: AssetRef`, `material: AssetRef`), `fynkel.DirectionalLight`, `fynkel.PointLight` as Rust structs with `#[derive(Schema)]`; `AssetRef` and enum field kinds added to the schema system, Dart bindings and wire codecs regenerated. Storage only — no rendering behaviour until M3.
5. **Scene → runtime instantiation** (done 2026-09-19; `fynkel_core::load`, `fynkel_document::describe`, generated document codecs, `Command::SetComponent|RemoveComponent`; `docs/documents.md` § Scene → runtime, `docs/hosts.md`): `fynkel_core::World::load_scene(&SceneDescription)` (authoring UUID ↔ runtime handle map, queryable both ways; hierarchy and child order honoured; components applied through the same command path as the bridge). Headless host and `fynkel test --headless --scene <path>` load a document; `fynkel state dump --json` emits authoring ids, not handles. Gate: "project can be validated/imported headlessly".
6. **Material and shader documents** (done 2026-09-19; `fynkel_document::material`, hand-written `schemas/material.schema.json` + `schemas/shader.schema.json` embedded via codegen, built-in manifest `crates/fynkel_document/shaders/fynkel.pbr@1.shader.json`, codes `FYN-D0300`–`D0308` material and `D0310`–`D0312` manifest, `fixtures/material/` corpus; `docs/documents.md` § Material and shader documents): `.material.json` (shader id, typed parameters, textures as `AssetRef`, samplers, alpha mode + cutoff, cull, shadow flags, colour space per texture slot) and `.shader.json` interface manifest for the built-in versioned PBR shader (`fynkel.pbr@1`). Validation of material against manifest (unknown parameter, wrong type, range, missing required texture). `fynkel material validate --json`.
7. **Prefab documents, instances, overrides, expansion** (done 2026-09-19; `fynkel_document::prefab`, `schemas/prefab.schema.json` generated next to `scene.schema.json`, `PrefabLibrary::scan`, `derive_id` = UUID v5 in the instance namespace, codes `FYN-D0400`–`D0407`, `fixtures/prefab/` corpus with the exact `instances_ok.scene.expanded.json`; `fynkel prefab inspect|validate`, `fynkel scene … --prefabs <dir> [--expand]`, `--prefabs` on `test --headless` and `state dump`; `docs/documents.md` § Prefab documents): `.prefab.json` = scene body + `roots[]` (since ADR-0022, 2026-09-24: one `root`, and the scene entity carrying `prefab` *is* that root — formatVersion 5, documents.md § Prefab documents). Scene entity `prefab: { asset, instance }`; overrides as an explicit ordered op list — `setField {entity, component, field, value}`, `addComponent`, `removeComponent`, `addObject`, `removeObject` — with `unset` distinct from `null`. `expand(scene) -> Expanded` remaps prefab-local ids per instance (deterministic derivation from instance id) and internal references; external asset refs shared. Reject nested prefab instances and self-reference cycles (M2 "do not"). Gate: "prefab references remap correctly".
8. **Asset identity (`.meta.json`) and the asset database** (done 2026-09-19; `fynkel_document::asset` — `Meta`, `AssetDatabase::scan/get/resolve/check_references`, hand-written `schemas/meta.schema.json` embedded via codegen, codes `FYN-A0001`–`A0006`, `fixtures/assets/` corpus with the move test; `fynkel asset list|resolve|init`; `docs/documents.md` § Asset sidecars and the asset database): sidecar `{ formatVersion, id, importer, importerVersion, settings, subAssets: { "<sub-id>": uuid } }`; database rebuilt by scanning `assets/`; diagnostics for missing sidecar, duplicate id, orphaned sidecar; move = same id at a new path (test: move file+sidecar, every `AssetRef` still resolves). `fynkel asset list|resolve <uuid> --json`. Gate: "asset moves preserve identity".
9. **Document migration framework** (done 2026-09-19; `fynkel_document::migrate` — `Migration`/`Migrations::builtin()`/`migrate()`/`Migrated::write_with_backup`, `similar` unified diffs, codes `FYN-D0016`–`D0018`, envelope accepts `formatVersion` 0 as "outdated", `fixtures/migrate/` with the synthetic v0 scene and its exact v1 result; `fynkel migrate <path|dir>... [--dry-run] [--list]`; `docs/documents.md` § Migrations): per-kind ordered migrations `(from, to, fn)`, `migrate(json) -> {json, applied[], diff}`, dry-run diff output, `.bak` backup on write, refuse newer `formatVersion` (read-only diagnostic, never rewrite). `fynkel migrate <path|project> --dry-run --json`. Fixture: a synthetic v0 scene migrated to v1.
10. **glTF/GLB importer** (done 2026-09-19; `crates/fynkel_import` — `import(path, &Meta, &Limits) -> Imported`, `fynkel.gltf` v1 over the `gltf` crate without its `import` feature; `Intermediate` (`FYNI` container: meshes in the fixed 72-byte vertex layout, `fynkel.pbr@1` material documents, encoded images, the node hierarchy as a prefab document) with `to_bytes`/`from_bytes`; sub-asset ids `mesh/<n>[/<p>]`, `material/<m>`, `image/<i>`, `prefab` with uuids from the sidecar or UUID v5 of the asset id; external URIs denied unless `settings.allowExternalFiles` and then only plain relative paths; required-extension, malformed-data, cycle and limit checks (`FYN-A0100`–`A0105`); fixtures `robot.glb` + data-URI/external/strip/Draco/cycle variants, committed `robot.expected.fyni` determinism test; `fynkel import <file>... [--out <dir>] [--dry-run]` creates/updates sidecars): `crates/fynkel_import` — size, recursion, external-URI (deny by default), buffer/decompression limits; normalized intermediate (meshes with fixed vertex layout, materials mapped onto `fynkel.pbr@1`, node hierarchy as a generated prefab); stable sub-asset ids via the sidecar; determinism test (same input → same intermediate bytes).
11. **Texture and audio import baseline** (done 2026-09-19; `fynkel_import::image` (`fynkel.image` v1, `image` 0.25 PNG/JPEG only → `TextureData` RGBA8 top-row-first, `settings.colorSpace` srgb|linear declared in the sidecar, 16-bit narrowed, `sourceHasAlpha`) and `fynkel_import::audio` (`fynkel.audio` v1, `symphonia` 0.6 wav/pcm/ogg/vorbis only → `AudioData` interleaved f32 clamped to [-1, 1], sample rate/channels/codec kept, gapless); `Intermediate` gains `textures`, `clips` and an optional `prefab`; limits `max_texture_dimension` 16 384, `max_audio_frames` 10 min at 48 kHz; fixtures `textures/` (every PNG layout, JPEG, GIF for A0103) and `audio/` (8/16/24-bit WAV, stereo and mono Vorbis, truncated WAV, FLAC for A0103) with committed expected bytes; `fynkel import` unchanged, dispatch by sidecar importer): PNG/JPEG → normalized RGBA8 with declared colour space (sRGB/linear from import settings, mipmaps deferred to M3); WAV + Ogg Vorbis → PCM intermediate with metadata; limits as above.
12. **Content-addressed incremental cache + cooked container v1** (done 2026-09-19; `fynkel_import::cache` — `KeyInputs::key()` = blake3 over length-prefixed fields: cache/intermediate/container versions, asset id, kind (importer or `document/<kind>`), version, source hash, extra input files (external `.gltf` URIs), canonical settings, dependencies as sorted `(id, key)`, profile; `Cache` at `.fynkel/cache/objects/<key>.fyna` + `<key>.manifest.json` (atomic writes, no timestamps), `index.json` after each cook, `prune(keep)`. `fynkel_import::cook` — dependencies from a document's resolved `assetRef`s, Tarjan order, cycle → A0104, unresolved → A0003, blocked dependents → A0105; documents cooked as canonical `JSON`, sources through their importer into `MESH`/`JSON`/`TEXR` (embedded glTF images decoded, colour space from slot usage)/`CLIP`, `fynkel.file` as `BLOB`. `crates/fynkel_cooked` (only `crc32fast`): `FYNC` v1, 48-byte header with CRC, 8-byte-aligned payloads, table of (kind, flags, offset, length, crc32), chunk 0 = META JSON; `Container` (bytes) and `ChunkFile` (seekable) validate everything before any payload, `read(index, max_len)` bounded + CRC-verified. Tests: clean corpus `fixtures/project/assets` cooks then hits; editing `swatch.png` pixels or its sidecar `colorSpace` recooks exactly swatch + rusted.material + main.scene; document edit → itself + dependents; profile in the key; read-back with bounded decoding and checksum failure; cycles/unresolved/blocked; prune. CLI `fynkel cook [--assets] [--cache] [--profile] [--force] [--prune] [--json]`, `fynkel cooked inspect <file.fyna>... [--json]`.) key = blake3(source bytes, ordered dependency keys, settings, importer version, target profile); `.fynkel/cache/<key>` with a dependency manifest; test that editing a referenced texture invalidates exactly the dependent material/model outputs. Cooked container: one file per asset, magic + version + chunk table (offset, length, checksum) + dependency ids; bounded chunk decoding.
13. **Project model + project-wide validation** (done 2026-09-19; `fynkel_document::project` — `Project` over `fynkel.project.json` (hand-written `project.schema.json`: `startupScene`, `assets` root, `inputActions` stub, `quality` profiles, `components` registry, `targets`; `x-*` kept), `Lock` over `fynkel.lock.json` (`lock.schema.json`, no envelope: runtime/tools versions, ABI, schema fingerprint, document and importer versions, cache/container versions; `Lock::expected` + `fynkel_import::expected_lock()`, `check` = one `FYN-P0011` per differing field, never a silent migration), `validate_project` = project file → lock → asset root → database scan → startup scene/registry → every document with references, prefab expansion, material/shader checks → unregistered-component warnings; codes `FYN-P0001..P0012`; `fynkel project describe|lock [--check]`, `fynkel validate`, `fynkel schema list|describe <type|kind>`, all `--json`; exit 0/1/2 fixed; fixture project with its committed lock): `fynkel.project.json` schema (startup scene, input actions stub, quality profile, component registry, build targets) and `fynkel.lock.json` (runtime/tool versions, schema fingerprint, importer fingerprints); `fynkel project describe --json`, `fynkel schema list|describe <type> --json`, `fynkel validate --json` over every document, sidecar, and reference, exit codes fixed (0 ok, 1 diagnostics with errors, 2 usage/IO).
13a. **Scene-on-screen spike** (done 2026-09-19, M3's first item pulled forward after M2-13 so the authoring work builds on a proven path; not a backlog item — a checkpoint): `fynkel_cooked::payload` (the `MESH`/`TEXR`/`CLIP` codecs, so a loader needs nothing from the importers); `fynkel_core::Extract` gains `meshes`, `camera` (highest priority), `directional_lights`, `point_lights` resolved against records; `fynkel_render` draws `MeshRenderer` entities with their cooked mesh and the `fynkel.pbr@1` `baseColor` factor × texture (`CookedAssets` over the `fynkel cook` cache, loaded on demand, one 72-byte vertex layout, batches by (mesh, texture), magenta for a missing mesh, optional placeholder cubes, the scene's `Camera` and first `DirectionalLight`, sRGB-encoded Lambert + Blinn-Phong); `fynkel_document::runtime::{read_scene, load_project}` (project → database → startup scene with every prefab); C ABI `fynkel_world_load_scene` / `_load_project` / `_load_report` (+ `FYNKEL_STATUS_LOAD_FAILED`), Dart `World.loadScene`/`loadProject` → `LoadReport`/`FynkelLoadError`; Apple viewport ABI v2 (`set_asset_cache`, `set_placeholders`, mesh/placeholder/missing counts in the stats); Flutter host `FYNKEL_PROJECT=<dir>`; `fynkel render [--project] [--out png] [--size WxH]` headless proof. Verified: the fixture project on the macOS host from its own camera (2 cooked meshes, 0 missing, 120 Hz) and as a pixel test. Compromises in `docs/status.md` §3/§4 (scenes still read from source files, one-pass shading, no audio consumer).)
14. **Authoring commands and transactions** (done 2026-09-19; `fynkel_document::authoring` — `Editor` over one scene or prefab document with `Revision` = blake3 of the canonical text (printed by `scene|prefab inspect --json`), `Editor::apply(Request { id?, baseRevision?, commands })` runs one atomic transaction: stale `baseRevision` → `FYN-E0001` conflict and nothing touched; commands compile to reversible edits (`replace`, `insertMember`/`removeMember`, `insertItem`/`removeItem`) whose inverses are the mutation's `undo`; a failing command or any validation error the transaction introduces (`FYN-E0005`, errors in `related`) rolls the document back to the byte; `saveDocument`/`createMaterial` are side effects after validation (no undo, failure rolls back too); byte-exact `undo()`/`redo()` over the committed history; the 13 §11.1 commands + `unsetField`, JSON codec `{ "command": …, … }`; `Applied`/`Mutation` records `{ commandId, command, transactionId, baseRevision, affectedIds, diagnostics, undo }`; codes `FYN-E0001..E0011`; `fynkel scene|prefab apply <path> --patch <ops.json> [--dry-run] [--json]` writes in place or prints the diff, exit 0/1/2; tests in `crates/fynkel_document/tests/authoring.rs`): in `fynkel_document`, the command set `CreateObject … RevertPrefabOverride, CreateMaterial, SaveDocument`, each mutation `{ commandId, transactionId, baseRevision, affectedIds, diagnostics, undo }`; stale `baseRevision` → conflict, never overwrite; `fynkel scene apply --patch <ops.json> --dry-run --json`. The C ABI surface for the editor waits for M4.
15. **Behaviour SDK and lifecycle** (done 2026-09-19; `packages/fynkel/lib/src/behaviour.dart` + `context.dart` — `Behaviour` with the §3.2 callbacks, `enabled`, `order`, `wantsAfterPhysics`, `destroy()`; `GameObject.addBehaviour`/`behaviour<T>()`/`addBehaviourFromData`; `World.frame` drives `fixedUpdate` (non-physics events, each to exactly one tick) → `step` → `afterPhysics` (contacts) per tick and `update` → `lateUpdate` per frame; transitions only at safe boundaries, `start` once before the first active callback, disabled = no gameplay callbacks, destruction deferred to the boundary after the tick, order by `order` then attach sequence; `GameContext` (`TimeService`, `DiagnosticsService`, `install<T>`/`service<T>`) current around every callback with `Time.*`/`Diagnostics.log` facades, `TestGameContext`/`TestTime`/`RecordingDiagnostics` installable via `world.context`; `BehaviourRegistry`/`BehaviourType`/`BehaviourField` keyed by type id with `instantiate(id, fields:)` and `BehaviourDataError`; `World.attach(entity)` for loaded entities; `@FynkelBehaviour`/`@Expose` annotations; `test/behaviour_test.dart` covers every rule and a `Spinner` that really rotates its entity): `Behaviour` base class with the §3.2 lifecycle, `@FynkelBehaviour(id:)` / `@Expose(...)` annotations in `package:fynkel`, a behaviour registry keyed by type id, and the `World` driver calling `onCreate → onEnable → start → fixedUpdate/afterPhysics/update/lateUpdate → onDisable → onDestroy` at the §5.1 points, deterministic order, deferred destruction. Services resolve through the current `GameContext`, test context installable.
16. **Behaviour generator + `fynkel codegen` (b)** (done 2026-09-19; `packages/fynkel_generator` — a `build_runner` aggregate builder (`build`/`source_gen`/`analyzer`, auto-applied to dependents) reads every `@FynkelBehaviour` class and its `@Expose` fields and writes `lib/generated/behaviours.g.dart` (one `BehaviourType` per class with `create` and a `BehaviourField` per field whose `get`/`set` convert `double`/`int`/`bool`/`String`/`Vec3`/`Quat`/`AssetRef` (nullable too) to and from JSON, literal initializers as `defaultValue`, `behaviourTypes`, `registerBehaviours(registry)`) and `lib/generated/behaviours.g.json` (the same metadata plus a JSON Schema 2020-12 per type with `x-fynkel { kind: behaviour, id, version, dart }`); build errors name the class/member; `fynkel codegen --project <dart package> [--check]` runs (a) then (b) — `dart run build_runner build`, then the split into `schemas/behaviours/<id>.schema.json`, stale files removed, separate caches, `--check` restores and exits 2 on drift; `fynkel_document` loads a project's `schemas/` on top of the embedded set (`SchemaSet::embedded_with`/`extended_with`, `Project::schemas`, `declare_behaviour` for registry entries of kind `behaviour`) so scene validation checks behaviour data (`FYN-D0206`) instead of warning `FYN-D0205`; `describe`/`read_scene`/`load_project` hand over `AuthoredBehaviour { entity, component, type_id, version, data, pointer, known }`, the native load report lists them as `scene.behaviours[]` with the spawned handle, and `World.loadScene`/`loadProject` instantiate each through `world.registry` and attach it (`FYN-B0001` unregistered, `FYN-B0002` rejected data; warnings); gate: `packages/fynkel_generator/example/spinner_game` + the fixture project's `game.Spinner { speed: 2.5 }` on the Robot — `dart test` there asserts the compiled `Spinner` has `speed == 2.5` and rotates; `cargo test -p fynkel_cli` runs the `--check` when a Dart SDK is on PATH): `packages/fynkel_generator` (`build_runner`) → `lib/generated/behaviours.g.dart` (factories, field serializers) and `schemas/behaviours/<id>.schema.json` + Inspector metadata; `fynkel codegen` runs (a) and (b) with separate caches and `--check`. End-to-end: a scene component `game.Spinner` with authored `speed` instantiates the compiled class with that field set. Gate: "a compiled Dart behaviour can be instantiated from authored data".
17. **`fynkel create` + `AGENTS.md`** (ADR-0017): Flutter app template with `fynkel.project.json`, `fynkel.lock.json`, `assets/` tree, a starter scene + material + one behaviour, `AGENTS.md` (commands, schema locations, validate→codegen→build→test loop, do/don't). `fynkel validate` and `fynkel test --headless` pass on a fresh `fynkel create` output. (done 2026-09-19; `apps/cli/src/create.rs` + `templates/create/`: Flutter app via `flutter create --empty` + macOS/iOS runners with the sandbox off, `lib/game/game.dart` (`Game.load`), `game.Spinner` behaviour with committed generated files and schema, cube/material/scene with fresh ids and imported sidecars, `test/game_test.dart`, `AGENTS.md`; `fynkel run`; CLI works outside the checkout on the embedded schemas, scene/prefab commands pick up the enclosing project's behaviour schemas, `fynkel test --headless` / `state dump` take `--project` and default to the current project; gate = validate + headless + `codegen --check` + `flutter analyze` + `flutter test` on fresh output, in CI.)
18. **M2 gate report** (done 2026-09-19; `docs/m2-gate-report.md` — verdict GO: all seven gate criteria pass with the test names as evidence; clean-clone run (189 Rust tests, 58 Dart tests plus the starter project's 1, both `codegen --check` pipelines up to date, starter project through `flutter test`); corpus tallies (30 exact-diagnostic fixtures, 3 committed intermediates); `benchmarks/m2/capture.py` + `benchmarks/m2/results/` — the tooling capture next to the M0/M1 frame-loop capture: `validate`, scene load, `scene validate` and load at 1 000 / 10 000 synthetic entities, glTF import of the fixture and a generated 2 MiB grid, cook miss/hit/partial, `codegen --project` cold/warm, `create`; the capture found and the item fixed `read_scene` ignoring the enclosing project's behaviour schemas (`--scene`, `fynkel_world_load_scene`, `World.loadScene` now match the scene commands) and a quadratic diagnostic de-duplication in `load_scene`; `docs/status.md` rewritten; carry-over D1–D8 below): round-trip corpus results, exact-diagnostic fixtures, id-stability and move tests, headless import of the template project, timings (validate, scene load, glTF import, cache hit/miss) appended to the benchmark capture; carry-over list for M3.

Removed from M2: shader graph, nested prefabs/variants, editor UI, renderer consumption of materials (M3), mipmaps/compressed textures (M3/M6), pack files (1.x).

**M2 carry-over (2026-09-19, from `docs/m2-gate-report.md` §5).** The M2 gate passed; these are what the gate did not require but the M2 build list and §5/§6/§11 imply. Each is owned by the milestone named:

- **D1 (M3, first item — done in M3-01):** `package:fynkel_flutter` — the Path A viewport (before M3-01 Swift in `hosts/flutter_apple`'s Runner + `viewport_ffi.dart`) as a package a game depends on, and the `fynkel create` template using it; reconcile the hook lock with the project lock when artifacts are published (with C6, still open).
- **D2 (M3):** cooked scenes are what the runtime loads — cook the expanded scene, make `load_project` read only the cache; source documents stay the authoring format.
- **D3 (M3):** consumers for `inputActions` (with C2 — done in M3-07: the world's binding table), `quality` (renderer V1 — done in M3-05) and `targets` (still unread; `fynkel run -d` picks the device by hand).
- **D4 (M3):** an audio consumer for `CLIP` chunks.
- **D5 (M4 — the ABI and Dart surface done in M4-01: `crates/fynkel_authoring`, `fynkel_native` feature `authoring` + `include/fynkel_authoring.h`, `packages/fynkel_authoring`; the Inspector reading the metadata is M4 item 4):** a C ABI and Dart surface for `fynkel_document::authoring::Editor`, and the Inspector reading the generated `label`/`tooltip`/`min`/`max`/`step` metadata.
- **D6 (M3):** generate `material`/`shader`/`meta`/`project`/`lock` schemas from Rust models once renderer V1 needs a Rust `Material`; hand-written until then.
- **D7 (M4 — done in M4-01: `json::Locator` caches pointer lookups with a lazily built per-object key index for objects of 16+ members, the parser's duplicate-key check switches to a set past the same size; release, 16 000 entities with 8 000 warnings read 31 ms + validate 68 ms, was 726 ms; apply+validate on 10 000 entities 58 ms, asserted linear):** `Node::pointer` indexes object members by key instead of scanning (quadratic today for a document with thousands of diagnostics; valid documents unaffected).
- **D8 (M6):** cache garbage collection beyond `cook --prune`; mipmaps, compressed textures and per-profile transforms in the cooker.

### 19.2d M3 issue backlog

Derived from the M3 "Build" list and gate (§16), the Tier 1 reference game (§1.2, ADR-0010), renderer V1 (§8.5), §9 (physics, audio, input), §10 (HUD and projected labels), §15.3 (`benchmarks/stress`), and the carry-over lists C2–C5 (§19.2b) and D1–D6 (§19.2c). One commit per item, in this order; §17.2 applies — the asset and material contracts settle before the renderer grows (D6, D2), the runtime slice (renderer, input, physics, audio) before the game, the game before the instrumentation that judges it. Every item ends green on the same gate as M2 (`docs/status.md` §5) plus the starter project and, from item 1, a viewport a created project shows pixels in. M5 owns skinning, `WorldCanvas` and spatial audio; nothing here prepares them beyond keeping the contracts open.

**Scope rules fixed here (cheaper to decide now than to migrate later):**

- **The runtime reads only the cache.** From item 3 a host loads scenes, prefabs, meshes, textures, materials and clips from `.fynkel/cache` (expanded scene chunks, `MESH`/`TEXR`/`CLIP`/`JSON`); source documents are the authoring format and never opened by a host. Hosts never cook; `fynkel run` and the tests cook first (`fynkel cook` stays the one cooker), and a missing or stale object is a load error with the asset id, not a fallback to source.
- **Rust is the source of truth for materials.** `fynkel.pbr@1` becomes a Rust model in `fynkel_core::material` whose schema `fynkel codegen` generates like a component's (D6): factors `baseColor`, `metallic`, `roughness`, `emissive` (linear RGB × `emissiveStrength`), textures `baseColor` (sRGB), `normal` (with `normalScale`), `metallicRoughness` (glTF packing: G roughness, B metallic), `occlusion` (R, `occlusionStrength`), `emissive` (sRGB), `alphaMode` `opaque | mask (alphaCutoff) | blend`, `doubleSided`. The hand-written material/shader schemas stay only as long as no field diverges; the importer emits the Rust model's document form.
- **Physical units and exposure.** Directional light intensity is illuminance in lux, point/spot intensity is luminous intensity in candela, emissive is nits-relative; the `Camera` gains `exposure` (EV100, default from the scene's sun so a 10 000 lux sun with `exposure` unset still renders correctly — the spike's fixed `EXPOSURE_LUX` goes away). Rendering is linear, HDR `Rgba16Float` intermediate, ACES-fitted tone mapping, sRGB presentation; colours in documents are linear RGB.
- **Light and shadow budget (Tier 1).** One shadowed directional light (one 2048² map, one cascade; a second cascade only if the reference game's world needs it), at most 8 point/spot lights affecting any draw (forward, no clustering — §8.6 decides later), spot cones by inner/outer angle. Environment lighting is a constant ambient term plus an optional cooked equirectangular sky (`fynkel.sky@1` material) — no IBL prefiltering in Tier 1. *(E1, 2026-09-22: prefiltering landed after the M3 gate. A sky that is an `.hdr` is prefiltered at cook into an irradiance and a specular cube (`ENVM`) and sampled split-sum in `mesh.wgsl`; the constant ambient stays for every other sky, and the sky stayed an `Environment` field rather than becoming a `fynkel.sky@1` material — `docs/renderer.md` § Image-based lighting, § Deviations from the blueprint rule. Light culling, the other half of the E1 carry-over, landed the same day — a 64-light frame pool, the eight that reach an object shading it. R1, 2026-09-22: the *one cascade* became four. The reference game's world did not force it; the picture did — a single fit around a sixty-metre frustum is ~7 cm of world per texel, so every shadow is soft and detached. The four cascades share the same 2048² map as a 2×2 atlas, so the budget line above still holds on GPU memory — `docs/renderer.md` § Shadows.)*
- **Physics.** `rapier3d` (f32) behind `fynkel_physics`; the fixed tick is the physics tick (§5.1); dynamic bodies own their transforms (`PHYSICS_OWNED` stays the diagnostic for a Dart write to one); Dart moves bodies through `applyForce`/`applyImpulse`/`setVelocity`/`setKinematicTarget`/`teleport` commands; collision and trigger events are §5.1 physics events attached to `S[n+1]` and reach `afterPhysics`. Determinism is per platform (same binary, same inputs → same checksum), not cross-platform. Collision layers are a 32-bit mask pair named in the project file.
- **Input.** `inputActions` in the project file are the only binding source (D3): `button`, `axis1d`, `axis2d` actions with `keyboard:`, `mouse:`, `gamepad:` and `touch:overlay/<control>` bindings; the host captures held state and edges per §5.1, `Input.action2D(...)`/`Input.pressed(...)` resolve through `GameContext`; a recorded action stream (`.input.json`) replays in the headless host so gameplay tests need no window. Flutter consumes its own interactions first; the viewport widget owns focus rules.
- **Audio.** 2D only in Tier 1: `AudioSource`/`AudioListener` components, buses (`master`, `music`, `sfx`), volume/pitch/loop, streamed music from the cooked Vorbis stream, voice limit with priority, pause/suspend on lifecycle. The backend is `cpal` with Fynkel's own mixer (decided at item 11 against `kira`; the deciding factor is control over allocation in the callback), and the headless host links a null backend so every audio API runs in tests.
- **Scenes and settings.** `World.loadScene` gains `replace` (the default: unload everything the previous load spawned, then load) and `additive`; runtime prefab instantiation reads the cooked prefab and derives ids per instance exactly as the document layer does; user settings live in one `settings.json` document (schema-generated from a Rust model) under the platform's application-support directory, read at startup and written on change.
- **New identifiers.** Component ids continue from 8 in the order the items add them; event kinds from 6 (`COLLISION_STARTED`, `COLLISION_ENDED`, `TRIGGER_ENTERED`, `TRIGGER_EXITED`, `SCENE_UNLOADED`, …); diagnostic namespaces `FYN-R####` (runtime loading and residency), `FYN-I####` (input), `FYN-S####` (audio), `FYN-H####` (physics); every new ABI entry point is additive to `fynkel.h` under the same version until the M3 gate bumps it once.
- **Dependencies added in M3** (each recorded in `docs/toolchain.md` per §2.3): `rapier3d`, `cpal` (+ `symphonia` already present for streaming), ~~`gilrs` (gamepad on macOS; iOS gamepads through the Swift plugin and `GCController`)~~ — M3-07 used `GCController` on macOS as well, no crate (see item 7), and nothing else on the runtime path without an item naming it.
- **Acceptance needs content the implementer did not make** (added 2026-09-21, from item 19a's lesson: a milestone item whose fixtures, tests and feature come from one hand tests what was built, not what a game needs — the first downloaded character found two gaps in an hour). From item 19a on, an item is not done until it has passed on at least one asset or scenario from outside the repo: the Khronos glTF sample assets (`Fox`, `CesiumMan`, `RiggedFigure`, `BrainStem` — CC-BY 4.0, committed under `crates/fynkel_import/fixtures/khronos/` with the attribution file, the one allowed exception to "every binary comes from a committed script"), a Mixamo download checked locally (never committed: not script-generated, not redistributable on its own), a real sound file, a real input recording of a person playing. The verification note in `docs/status.md` §5 names it.

1. **`package:fynkel_flutter` — the viewport as a package** (D1): `packages/fynkel_flutter` as a Flutter plugin (macOS + iOS Swift classes, the `FynkelViewportTexture` that `hosts/flutter_apple` carries twice today, registered through the plugin registrant) with `viewport_ffi.dart` and a `FynkelViewport` widget (`FynkelViewportController`: world, asset cache, placeholders, host mode, resize/DPR, lifecycle, stats stream) and a `FynkelGame` widget that owns the Ticker and drives `World.frame` at the §5.1 points; `hosts/flutter_apple` becomes a thin app on the package (its Runner keeps only the app delegate); the `fynkel create` template shows the scene in `FynkelViewport` with the HUD over it; `packages/fynkel` stays widget-free (§2). Gate: the starter project shows its lit, spinning cube in its own app on macOS and the iOS smoke test still passes; `hosts/flutter_apple` numbers unchanged in the M0 capture. *(done 2026-09-19; `packages/fynkel_flutter`: Flutter plugin with a shared-darwin Swift package (`darwin/fynkel_flutter/Package.swift` + podspec; `FynkelViewportTexture` moved from the two Runners, `FynkelFlutterPlugin` on channel `fynkel_flutter/viewport` with `registerTexture`/`rebindTexture`/`stats`/`unregisterTexture` keyed by `textureId` so several viewports can live at once), `viewport_native.dart` (the ABI v2 externals), `FynkelViewportController` (state machine detached → creating → attached ⇄ resizing / failed; coalesced resizes with native resize → Swift rebind; `render(world)` returns null mid-resize; asset cache, placeholders, camera, `takeStats`/`platformStats`; ordered `detach` reusable afterwards, also when a detach lands during creation), `FynkelViewport` (LayoutBuilder × DPR → `requestSize`, `Texture`, Focus + Listener with pointer positions in physical px, placeholder/error), `FynkelGameView` (Ticker → `World.frame(dt, fixedUpdate:)` → render; dt clamp, clock re-primed on resume and on attach, lifecycle mute, per-window `FynkelFrameStats` with `MsPercentiles`, `overlay`, `setPaused`, owns a default controller); 7 tests run the real Rust viewport headlessly under `flutter test` with a mocked channel. `hosts/flutter_apple` is a thin app on the package (Runners keep only the app delegate / window; `[host]` lines unchanged, M0 capture `2026-09-19-b35f603`: 120 Hz, tick 8.33/8.34/8.34 ms, GPU 0.80/0.84/1.19 ms, raster 0.90/0.96/2.32 ms; iOS simulator 60 Hz, 0 drops). `fynkel create` depends on `fynkel_flutter` by path and `lib/main.dart` is `FynkelGameView` over `report.cacheDir` with a HUD overlay; `fynkel run` now cooks before `flutter run` (`--no-cook`); `build.rs` in `platform/apple` and `fynkel_native` pads the Mach-O header (`-headerpad_max_install_names`) so Flutter's `install_name_tool` step works from deep project paths. Gate: the starter shows its lit, spinning cube (1 mesh, 60 ticks/s, 120 Hz) in its own macOS app.)*
2. **Rust material model and generated material schema** (D6): `fynkel_core::material::PbrMaterial` (the scope rule's fields) with `#[derive(FynkelSchema)]`-style generation of `schemas/material.schema.json`, the Dart and Rust document codecs, and a `MaterialDesc` the renderer consumes directly; the importer emits the model's document form (normal, metallic-roughness, occlusion and emissive textures now imported and cooked as separate sub-assets with their colour spaces); `fynkel_document::material` validates through the generated schema with the shader manifest kept for custom WGSL contracts; the fixture and starter materials and the corpus updated by `fynkel migrate` (the first real migration step, with its fixture pair). Gate: the corpus round-trips; `codegen --check` covers the material schema; the robot's five texture slots reach the cache. *(done 2026-09-19; `fynkel_core::material::{AlphaMode, PbrMaterial}` — `#[derive(Schema)]` record with `document = "material"` (new `FieldType::Vec4` and `FieldType::OptionalAssetRef` = nil id, `RecordSchema.document`; fingerprint `0x45bf9f2f1cc8d124`): `base_color`, `metallic`, `roughness`, `emissive`, `emissive_strength`, `normal_scale`, `occlusion_strength`, `alpha_mode`, `alpha_cutoff`, `double_sided` and the five `<slot>_texture: Option<AssetRef>` (184-byte wire record, `Default`, `violations()`/`validate()`, `textures()` with each slot's colour space). `fynkel codegen` now writes `schemas/material.schema.json` from the record (envelope + `shader` + the fields with their defaults + the custom-shader `parameters`/`textures` groups; `unevaluatedProperties: false`), the Dart `PbrMaterial`/`AlphaMode`/`Vec4` codecs and `fynkel_document::generated` document codecs (`or_default` readers, `document_defaults()`). Material documents are **formatVersion 2**: PBR fields flat on the document, `parameters`/`textures` only for custom shaders (mixing is `D0307` with a `fynkel migrate` hint), `alpha_mode`/`alpha_cutoff`/`double_sided` shared, `cull`/`castShadows`/`receiveShadows` gone (shadows are the `MeshRenderer`'s); `fynkel_document::material` decodes `Material::pbr` through the generated codec, validates ranges via `violations()` (`D0303` at the field) and keeps the manifest path for custom WGSL contracts; `ResolvedMaterial` presents the PBR fields as manifest parameters/slots plus `pbr` and `doubleSided`. Migration `material/1-2/pbr-fields` (`fixtures/migrate/v1_rusted_metal.material.json` → `.migrated.json`; unknown parameters/slots fail the step) migrated every fixture, the corpus and the `fynkel create` starter. Importer (`fynkel.gltf` 2, intermediate 2, cache 2): the model's document form with all five slots, `TEXCOORD_1`/non-default samplers dropped with one `A0105` warning each, the importer decides each image's colour space and splits a dual-use image into `image/<i>` (sRGB) + `image/<i>/linear` (the robot now yields 8 sub-assets). Cook writes a `MATL` chunk next to every material's `JSON` (`fynkel_wire::material::{encode_material, decode_material}`, version 1 header + record + string tail, META extras `shader`/`alphaMode`/`textures`), and `fynkel_render::CookedAssets::material` returns the `PbrMaterial` from it (`MaterialDesc` and the JSON path removed). Gate: corpus round-trips at v2; `codegen --check` covers `material.schema.json`; the robot's cooked `Painted` material binds `base_color_texture`, `normal_texture`, `metallic_roughness_texture`, `occlusion_texture`, `emissive_texture`.)*
3. **Cooked-scene loading and runtime residency** (D2): `fynkel cook` writes the expanded scene (prefabs applied, ids derived) as the scene's `JSON` chunk and a cooked prefab chunk per prefab; `crates/fynkel_assets` (runtime handles, load from the cache by id, residency counts, eviction on scene unload, `FYN-R####` diagnostics); `load_project`/`load_scene` in the host read the cache only (the source path is gone from the ABI and `LoadReport` carries the cache key); `fynkel run` and `fynkel test --headless --project` cook first; `fynkel_render` takes meshes and textures from `fynkel_assets` instead of its own `CookedAssets`. Gate: a project whose source tree is removed after cooking still loads and renders identically; the fixture pixel test passes from the cache alone. *(done 2026-09-20; `fynkel cook` validates each scene, expands it (prefabs applied, UUID v5 ids derived) and writes the expanded scene as the object's `JSON` chunk (META `expanded: true`, `entities`, `instances`, `behaviourTypes[]`) and a cooked chunk per prefab, and `index.json` records `project { name, startupScene }`, `containerVersion` and `schemaFingerprint`; an invalid scene fails its cook (no object, status `failed` by id). `crates/fynkel_assets`: `Assets::open(cache)` (`FYN-R0001` unreadable, `R0002` stale build), `startup_scene()` (`R0009`), `scene(id)` → `CookedScene { description, behaviours, closure, key, path, diagnostics }` (`R0003` missing, `R0004` cook failed, `R0005`/`R0006`/`R0007` unreadable object/chunk/payload, `R0008` invalid), `mesh`/`texture`/`material` by `AssetRef` from the object store, `retain(closure)` → `Residency` token whose drop releases and evicts every payload no scene still holds (`ResidencyStats`); the store is shared (`Arc`), cloneable and adopted by the renderer (`ExtractRenderer::set_assets`; `CookedAssets` deleted). The C ABI is cache-only: `fynkel_world_load_project(world, cache)` and `fynkel_world_load_scene(world, cache, scene_id)`, report `{ scene { id, name, path, key, entities, rejectedComponents, assets, behaviours[] }, cache { dir, profile, project, startupScene }, residency, diagnostics }`; `fynkel_document::runtime::{read_scene, load_project}` removed (only `describe` stays). Dart: `World.loadProject(dir, {cacheDir})` / `loadScene(cacheDir, sceneId)`, `LoadReport` with `sceneKey`, `assetCount`, cache fields and `ResidencyStats`; `package:fynkel/testing.dart` (`cookProject`, `runFynkel`) so the SDK, spinner and starter tests cook into a temporary cache. CLI: `cook::prepare` (project from `--project`, the cwd or the project enclosing `--scene <path>`; `--cache`, `--no-cook`) shared by `fynkel test --headless`, `state dump`, `render` and `run` — every one cooks first and loads from the cache; a loose scene outside a project is exit 2. Headless host `HeadlessConfig { cache, scene }` over `fynkel_assets`, `run_retaining`; the Apple viewport adopts the world's store on render. Gate: `crates/fynkel_render/tests/project.rs` cooks the fixture, removes its source tree and renders identical pixels from the cache alone (residency drop evicts 1 mesh, 1 texture, 1 material; a re-render is byte-identical); `fynkel_native/tests/load.rs` loads a copied cache, refuses a tampered fingerprint (`R0002`) and a removed cache (`R0001`).)*
4. **Renderer V1 (a): PBR, HDR, lights** (done 2026-09-20; `docs/renderer.md` is the reference — units, exposure meter, passes, the 4-pipeline cap, uniform layout, per-feature pixel tests in `fynkel_render/tests/shading.rs`; `fynkel.SpotLight` (8) and `fynkel.Environment` (9) added, `fynkel.Camera` gained `exposure_mode`/`exposure` through the `camera-exposure` scene/prefab 1→2 migrations, tangents generated at cook (cache v3); **deviation:** the optional sky is a texture reference on `fynkel.Environment` (`sky`, `sky_intensity`, `sky_rotation`) rather than a `fynkel.sky@1` material — a material would have wrapped one texture reference; the shader id stays free for a procedural sky. The host's fallback sun applies only to a scene that authors no lighting at all): Cook-Torrance (GGX, Smith, Schlick) with every material map, normal mapping (tangents generated at cook when absent — closes the M2 compromise), `alphaMode` mask in the opaque pass, HDR intermediate + ACES + `Camera.exposure`, constant ambient + optional sky, up to 8 point/spot lights per draw with the `SpotLight` component added, orthographic camera; pipeline/bind-group caching and a variant cap (`docs/renderer.md` lists every variant). Gate: pixel tests per feature; the fixture project renders the rusted robot with its normal and roughness maps; the starter cube gets shading, not just a flat orange.
5. **Renderer V1 (b): shadows, transparency, MSAA, quality** (done 2026-09-20; `docs/renderer.md` § Passes — the chain with its inputs/outputs table, `src/shadow.rs` (frustum-sphere fit to 60 m, texel-snapped, 3×3 PCF, normal offset), `blend` in the transparent stage back to front per mesh, MSAA 1/4 resolved through the post pass's coverage tag (a profile's 2 runs at 1, 8 at 4), render scale 0.25–2 with the presentation pass, `src/quality.rs` reading the cache index's `project.quality` — `fynkel cook` copies the block, `fynkel_assets::ProjectInfo::quality` carries it — and switching at runtime through `ExtractRenderer::set_quality_profile` / `fynkel_apple_viewport_set_quality` / `FynkelViewportController.quality` / `fynkel render --quality`, `src/debug.rs` lines/AABBs/axes (physics shapes with the physics item); the 9-pipeline cap; `MeshRenderer.cast_shadows`/`receive_shadows` and `DirectionalLight.cast_shadows` honoured. Gate: 7 new pixel tests (shadow, per-mesh opt-outs, blend order, MSAA edges, render scale, profiles, debug lines); the M0 capture `2026-09-20-12859e1` shows the cube scene unchanged at MSAA 4 + shadow pass): the `shadow → opaque → transparent → post → presentation` pass chain with explicit inputs/outputs, one shadowed directional light (PCF), `blend` materials sorted back-to-front in the transparent pass, MSAA 1/4 and render scale from the project's `quality` profiles (the runtime reads `quality` — first consumer, D3), debug primitives (lines, AABBs, physics shapes later). Gate: shadow and transparency pixel tests; switching quality profiles at runtime through a command; the M0 capture unchanged for the cube scene.
5a. **Renderer showcase** (added 2026-09-20, done M3-05a; not in the original backlog): `examples/showcase/` — an ordinary `fynkel create` game authored so every renderer feature is visible in one view (sun shadow with per-mesh opt-outs, spot + point lights on emissive lamps, `Environment` sky, textured PBR, metals, three overlapping blend panels, a mask card, the fixture robot as a hierarchy, `game.Orbit` circling the camera, three quality profiles; Q cycles, Space pauses); `renders/<item>.png` is committed at the end of every renderer item so the history is a before/after; CI validates, runs headless, renders and tests it; `FynkelAppleViewportStats` gained the frame counts (blended, lights, casters/map, MSAA, render size) the HUD prints. Found on the way: a scene cannot instance a glTF `prefab` sub-asset (`FYN-D0401`: only prefab documents resolve; the imported hierarchy has to be re-authored in the scene) — item 12.
6. **Renderer V1 (c): instancing, frustum culling, `benchmarks/stress`** (done 2026-09-20; `docs/renderer.md` § Instancing and culling — `src/culling.rs` `Sphere` from the cooked `MeshPayload` bounds (already there since M2, no cook change) and `Frustum` from any `view_proj`; every instance tested against the camera frustum and, when the sun casts, the light's box, laid out per batch as `[shadow only][both][scene only]` so the shadow and scene draws are two contiguous ranges of one upload; `ExtractRenderer::frustum_culling` off renders pixel-identically; `FrameStats::{culled, draws, shadow_draws}` on `fynkel render --json`, `FynkelAppleViewportStats` (appended), the showcase HUD and both hosts' stats lines; `fynkel_host_standalone --cache <dir> [--scene <id>] [--quality <profile>]` runs a cooked scene without cooking; `benchmarks/stress/generate.py` (5 000 static meshes over 12 (mesh, material) pairs + 500 "Body" cubes + 300 "Behaviour" pyramids, floor, casting sun, camera over the field; git-ignored output) and `benchmarks/m3/capture.py` (generate → lock → cook → render for the counts → standalone → Flutter host, results with the gate). Gate: `benchmarks/m3/results/2026-09-20-cf2b3eb` — 5 804 entities, 5 343 instances in 15 draws ≤ 15 pairs, 458 culled, standalone 122 frames/s (prepare + encode + submit 1.4 ms p50), Flutter 120 ticks/s; the culling pixel test drops four off-screen cubes, one still casting into the view, picture identical with culling off; `renders/m3-06.png` is byte-identical to `m3-05.png`): per-(mesh, material) instance buffers rebuilt from the extract, bounding spheres cooked per mesh, frustum culling before instance upload, draw/instance/culled counts in the stats; `benchmarks/stress/` scene generator (5 000 instanced static meshes, placeholders for the 500 rigid bodies and 300 behaviours until items 8 and 10) and `benchmarks/m3/capture.py` recording it against the manifest. Gate: 5 000 meshes at 120 Hz on the reference profile with draw calls ≤ number of distinct (mesh, material) pairs; culling provably drops off-screen instances.
6a. **The bundled cache — a game runs on a device** (added 2026-09-20, done M3-06a; not in the original backlog): the showcase on a physical iPhone failed with `FYN-R0001` for the Mac's `.fynkel/cache/index.json` — `fynkel run` hands the app the project directory through `--dart-define=FYNKEL_PROJECT`, which only the simulator can see (it shares the Mac's filesystem; M1-09 never ran on the phone from a cache). A cook of a project that is an app (`pubspec.yaml`) now also stages the cache (`apps/cli/src/bundle.rs`: `index.json` and exactly the objects it names, flat, into `<project>/fynkel_cache/`, stale objects removed) after `fynkel cook` and the cook every runtime tool does first; the created project lists `fynkel_cache/` under `flutter: assets:`, git-ignores it but the keep file `fynkel_cache/README.md` (a Flutter asset directory must exist and is not recursive), and its `main.dart` takes `FYNKEL_PROJECT` when that directory exists on the device and otherwise `package:fynkel_flutter`'s `unpackBundledCache()` — `rootBundle` → `<system temp>/fynkel_cache/{index.json, objects/}` (content-addressed, warm unpack in milliseconds), the path handed to `Game.load(dir, cacheDir:)`. `fynkel run` refuses when nothing is staged; a device build after a project edit needs a cook before `flutter build`. This is §13.6's "compiled/cooked game assets" for the Tier 1 packaging; M6 decides whether a release build ships the objects as one archive instead of loose assets. Tested: `bundled_cache_test.dart` against a fake `AssetBundle`; the showcase and a fresh starter through the full suite; the showcase on macOS from the bundle with `FYNKEL_PROJECT=/nonexistent` (22 objects retained from the temp cache).)
7. **Input contract** (C2, D3) (done 2026-09-20; `docs/input.md` is the reference — `fynkel_core::input` (the grammar `parse_binding`, `Input::{bind, capture, end_tick, view}`, `World::{bind_input, capture_input, input_view}`, `end_tick` inside `step`), wire records `fynkel.rec.InputEvent`/`InputAction` and buffer kinds `InputEvents` 6 / `InputActions` 7 (fingerprint `0xe75eb93baf7a72bc`), ABI `fynkel_world_capture_input` / `fynkel_world_input_view` with the table bound from the cache index's `inputActions` at `load_project` (the load report's `input.actions`/`bindingErrors`), `fynkel validate` `FYN-P0009` through the same grammar; `package:fynkel`: `Input` facade over `GameContext.input`, `WorldInput` decoded in place, `World.frame(dt, input:)`, `InputRecording`/`InputRecorder`/`loadInputRecording`, `TestInput`; `package:fynkel_flutter`: `FynkelInputCapture`, `FynkelGameView(input:)` with the focus/pause resets, `capturePointer`, `FynkelInputChannel` + Swift `FynkelInputPlugin`, `TouchOverlay`; `fynkel test --headless --input`; the starter's `game.Mover` + `test/walk.input.json` replayed in `flutter test` and headless; **deviation:** gamepads on macOS through `GCController` like iOS, not `gilrs` — one framework, no crate; the touch overlay drove the starter on the iOS simulator by hand): `fynkel_core` input view (held state + edges consumed by exactly one tick, §5.1) fed by a `CaptureInput` batch from the host; actions from `inputActions` (`fynkel_document` already validates them) compiled into the world's binding table; `package:fynkel_flutter` captures keyboard, mouse (pointer capture, relative movement), gamepad (`gilrs` on macOS, `GCController` on iOS) and a `TouchOverlay` widget (virtual stick and buttons mapped to `touch:overlay/<control>`); `Input.action2D`/`axis`/`pressed`/`held`/`released` via `GameContext`; `fynkel test --headless --input <file.input.json>` replays a recorded stream; focus/pause rules (the viewport widget must have focus; Flutter widgets first). Gate: a headless test walks the starter cube with a recorded stream; the jump-once-across-three-catch-up-ticks case is a test; the touch overlay drives the same actions on the iOS simulator.
7a. **Typed native component access from Dart** (added 2026-09-20, done M3-07a; not in the original backlog): before physics adds `RigidBody`/`Collider`, the one generic path a behaviour uses to reach *any* native component on its object — the model was Unity-shaped (entity + native components + Dart behaviours, one registry, one schema, one validator) but from Dart only `Transform` was reachable; `fynkel_core` already had `Command::SetComponent`/`RemoveComponent` for the loader. Wire: `fynkel.cmd.SetComponent { entity, component }` followed by the component's own bytes (one command kind for every `ComponentValue`; the strings in the batch tail), `fynkel.cmd.RemoveComponent`, `FYNKEL_QUERY_COMPONENTS` answering every native component of an entity framed by `fynkel.rec.ComponentHeader { component, size }` (fingerprint `0x27ff561a8b4bc084`); `fynkel_wire` gains the generated `decode_component_value`/`encode_component_value`/`component_value_size` dispatch and the Dart codecs a `componentCodecs`/`componentIds` table plus `copyWith` on every component class. `package:fynkel`: `GameObject.get<T>()`/`has<T>()`/`set(value)`/`add(value)`/`setPresentation(value)`/`remove<T>()`/`components`, `World.componentsOf(entity)`, the same four on `Behaviour`; reads are one query per object per tick merged with the tick's staged writes (read-your-writes, no mutable mirror — §3), writes are validated by the world like the loader's, a rejection is a diagnostic and the read falls back; writes on an object not yet spawned wait for the spawn. `docs/design/dart-sdk.md` §21.2 is the reference. Gate: `components_test.dart` (a behaviour hides its mesh and adds/brightens a light across ticks, remove/rename/rejection, spawn-then-add, the presentation lane); the starter's `game_test.dart` does the same on the cube and the sun; a scratch starter whose `Mover` spawned a twin cube with the mesh read off its own object rendered both (`2 drawn in 1 draws`, blinking through `visible`).

8. **Physics — `fynkel_physics` on Rapier** (done 2026-09-20, M3-08; `docs/physics.md` is the reference; `rapier3d` 0.35 in `crates/fynkel_physics`, stepped by `World::step` at the §5.1 slot only while an entity has a body or collider; `PhysicsOwned` on dynamic bodies, `fynkel.cmd.Body` kind 8 for forces/impulses/velocities/kinematic target/teleport, `FYNKEL_QUERY_BODY`, `fynkel.rec.PhysicsQuery` → `fynkel_world_physics_query` → `fynkel.rec.PhysicsHit`, contacts with point/normal/impulse on the physics lane dispatched to `onCollisionEnter/Stay/Exit`/`onTriggerEnter/Exit`, `physics: { gravity, layers }` in the project file (`FYN-P0013`), scene rules `FYN-H0001`–`H0004`, `MESH_UNAVAILABLE`; deviations: one collider per entity, stay events only for subscribed pairs, `Physics::debug_lines` exists but no host draws it yet. Gate met: 500 bodies in the stress scene at p50 0.64 ms per tick falling and settled (checksum `fdce91f4c8a52bfc` repeated), the fixture's Crate B lands on its Floor and sleeps under the headless checksum, `physics_test.dart` sees each trigger enter/exit exactly once): `RigidBody` (static/dynamic/kinematic, mass, damping, gravity scale, CCD flag), `Collider` (box/sphere/capsule, bounded `mesh` from a cooked mesh with an explicit triangle limit, offset, friction/restitution, `isSensor`, layer/mask), the pipeline in the fixed tick at the §5.1 slot, physics-owned transforms written back after the step, collision/trigger events with contact data, queries (`raycast`, `shapeCast`, `overlap`) as batch commands answered in the query view, body commands (forces, impulses, velocities, kinematic target, teleport); components schema-generated, scene-validated, in the starter catalogue. Gate: 500 rigid bodies in the stress scene at fixed cost; the fixture scene's crate falls onto a static floor deterministically (checksum); triggers reach `afterPhysics` exactly once.
8b. **Developer ergonomics before the reference game** (added 2026-09-20, done M3-08b; not in the original backlog): five things every gameplay file was about to work around. (a) **Texture mip chains** — the cook writes the full chain into `TEXR` (box filter in linear light for sRGB maps, `TexturePayload::level(i)`, cache version 4) and `fynkel_render` uploads every level, so distant textured surfaces stop shimmering; (b) **lookups** — `World.find`/`findAll`, `GameObject.findChild('Arm/Hand')`/`findDescendant` over one `FYNKEL_QUERY_ENTITIES` (`EntityInfoRecord { entity, parent, name }`) per tick; (c) **spawn-then-configure in one tick** — a `GameObject` pending its spawn is addressed by a temp reference (`Entity::temp(id)`: generation 0, the spawn's temp id) in any later command of the same simulation batch and the world resolves it against the tick's spawns (`INVALID_TEMP_ID` otherwise, or on the presentation lane), so `spawn` + `add(RigidBody)` + `applyImpulse` is one tick and the pending-object `StateError` is gone; (d) **runtime prefab instantiate**, pulled forward from item 12 — `fynkel_world_instantiate(world, &FynkelInstantiateDesc)` / `World.instantiate(prefab, {transform, parent, name})` builds a cooked prefab now as an anchor with the prefab's roots as children (the shape a scene instance expands to), behaviours attached, closure retained, `InstantiateReport`; (e) **nested prefabs** — a prefab may contain prefab instances; `PrefabLibrary::flattened` expands them innermost first before scenes expand or the cook writes the (now flat) prefab chunk, `FYN-D0402` is a cycle. Fingerprint `0x2b369001633c9953`. Gate: `prefab_test.dart` (an instantiated crate under a named anchor, parented, refused before a project loads), `physics_test.dart` (a body added and pushed in the spawn tick moves), `components_test.dart` (`findTests`), the document corpus's `nested.prefab.json` expanding and `cycle_a`/`cycle_b` refused once each, the cook's 4×3 swatch as a 3-level chain.

9. **Character controller and third-person camera** (done 2026-09-20; `fynkel_physics::components::CharacterController` (12) — capsule radius/height, `max_slope`, `step_height`, `skin_width`, `gravity_scale`, optional `move_action`/`jump_action`/`speed`/`jump_speed` — over Rapier's `KinematicCharacterController` inside `Physics::step` (`move_characters` before the Rapier step: the queued `Move` velocity plus gravity integrated per tick, an own downward grounded probe with the slope test, `sliding` only against a steep contact normal, a resting character asks for a 1e-4 m probe instead of zero translation because Rapier's zero-translation depenetration path is unreliable for a capsule on a box), `BodyOp::Move`/`Jump` (the jump speed in `vector.y`; only while grounded), `FYNKEL_QUERY_CHARACTER` (9) → `fynkel.rec.CharacterState { grounded, sliding, velocity }`; `fynkel_core::components::ThirdPersonCamera` (13) — `target` by **name** (an authored component cannot hold an `Entity`), `distance`/`min_distance`, `pivot_offset`, `yaw`/`pitch` with `min_pitch`/`max_pitch`, `look_action`, `sensitivity`, `mask` — placed by `World::place_camera_rigs` after the physics phase with a sphere-cast pull-in from the pivot (no smoothing in V1); `World::drive_characters` reads the named actions in the camera's yaw frame **before** `Input::end_tick` so a jump consumes the edge in the tick that sees it, queued ahead of the batch's commands so an explicit `Move`/`Jump` wins; `FYN-H0005`–`H0007`; `GameObject.move`/`jump`/`characterState()` and `packages/fynkel/example/player_controller.dart` (walk, sprint, double jump in the camera's frame). Gate met: the fixture's Playground scene replayed headlessly from `test/platform.input.json` walks the Player onto the Platform with one jump, camera behind it, checksum repeated (`hosts/headless` test, `packages/fynkel/test/character_test.dart`). Deviations: name-based target; the character neither pushes dynamic bodies nor rides moving platforms; a catch-up tick repeats the frame's relative look delta; `docs/physics.md` § Character controller): `CharacterController` component over Rapier's kinematic controller (capsule, slope/step limits, ground detection exposed as held state, gravity, `move` command per tick), a `ThirdPersonCamera` native component (`Follow` from the §3.5 catalogue specialised: target, distance, pitch/yaw from a camera action, collision-aware pull-in via a shape cast) and the Dart `PlayerController` behaviour pattern in `packages/fynkel` examples; jump consumes the edge per §5.1. Gate: a headless test walks, jumps and lands on a platform from a recorded stream with a stable checksum; the reference game (item 14) is the first real consumer.
10. **Native component catalogue** (done 2026-09-20; `fynkel_core::components::{Rotator (14), Oscillate (15), Follow (16), LookAtCamera (17), LifetimeDespawn (18), Emitter (19)}` with the shared `MotionSpace { local, world }` enum, ticked by `crates/fynkel_core/src/world/catalogue.rs` — `docs/catalogue.md` is the reference. Deviations from the line above: the movers, lifetimes and emitters run in the pre-tick slot as written, but `Follow` and `LookAtCamera` run **after** physics and after the `ThirdPersonCamera` rig, on the poses the tick ends with (a light following a falling crate sits where it landed; a billboard faces where the rig put the camera); `Follow.target` is a **name** like the camera rig's (an authored component cannot hold an `Entity`); an `Emitter` whose prefab the world does not hold reports `PREFAB_UNAVAILABLE` (14) once and emits nothing until it is registered — the hosts register every prefab a loaded scene's or an instantiated prefab's emitters name (`Assets::register_prefabs`, the load report's `physics.prefabs`); `Oscillate` moves by the change in its offset so a gameplay write shifts the rest position; `LifetimeDespawn` destroys the subtree the way `Command::Destroy` does (subscriptions dropped, `Destroyed` events, Dart `onDestroy`). Every phase of `World::step` is timed: `fynkel.rec.SystemTime { system: SystemKind, entities, micros }` per phase in tick order, `FYNKEL_QUERY_SYSTEMS` (10) / `World.systemTimes()` / the starter HUD's `systems` line / `HeadlessReport::systems` (mean + worst tick) / the `fynkel test --headless` `systems` line. Documents: `FYN-H0008` for the catalogue's value rules, the fixture project's Catalogue scene (…032) and Spark prefab (…041), the six components in every registry. Stress: the 5 000 statics are native movers (1 668 `Rotator`, 1 668 `Oscillate`, 1 664 `LookAtCamera`, colliders kept), a `Follow` lantern and a 40-spark `Emitter`; `--app` puts `game.Wobble` (the same motion from Dart) on the 300 Behaviour pyramids. Gate met (`benchmarks/m3/results/2026-09-20-1263dd8.md`): standalone 121 frames/s at p99 9.03 ms; headless settled tick p50 2.34 ms with the 5 000 movers at 0.15 ms together, physics 2.0 ms (5 000 fixed colliders moving through the broad phase — up from 0.6 ms with still statics, documented in `physics.md`), transforms 0.39 ms; the `--app` build with 300 `Wobble` behaviours holds 120 fps, simulate 3.02 ms (p99 3.61) at 60 ticks/s, 0 missed; checksum `3082610c1532aa7c` repeated; 13 phases in the stats. Found on the way: the starter's `main.dart` template referenced `SystemKind`/`SystemTime` without importing them.) ~~`Rotator`, `Oscillate`, `Follow`, `LookAtCamera`, `LifetimeDespawn`, `Emitter` (simple spawn-at-rate of a prefab with lifetime) as `fynkel_core` systems in the pre-tick slot, schema-generated and scene-authorable, `Behaviour`-free; the stress scene's 300 ticking Dart behaviours added next to 5 000 native movers. Gate: the stress scene at 120 Hz on the profile with the counts §15.3 names; per-system time in the stats.~~
11. **Audio — `fynkel_audio`** — **done 2026-09-21** in two commits (M3-11; `docs/audio.md` is the reference). **Part 2** (commit 2 of 2): **streamed music** — a sidecar's `settings: { stream: true }` (importer `fynkel.audio` v2, the one setting; anything but a boolean is `IMPORT_INVALID_DATA` at `/settings/stream`) makes the import decode and validate the file as before, then keep the **source bytes** instead of the samples (`AudioData.stream`, `frames` explicit; `.fyni` v5), and the cook write a `STRM` chunk (`fynkel_cooked::KIND_STREAM`: rate, channels, frames, codec, the file; META `streamed: true`, `bytes`) where a `CLIP` would go — 1.4 MB for a 74 s Vorbis track instead of 26 MB of floats. `Assets::sound` registers whichever chunk the object has; `AudioClip::streamed` holds the bytes in an `Arc<[u8]>`. Playing one is the clip contract unchanged — the source's play/stop/pause/looping/pitch, `PlaySound`, the **tick's clock** deciding the end (from the cooked frame count, so headless and device runs still agree), `PLAYBACK_ENDED`, priorities, the buses — with the decoding on a **decoder thread per streamed voice** (`fynkel_audio::stream`: `spawn` runs `symphonia` over the bytes from the voice's start frame — `SeekMode::Accurate`, the overshoot skipped — into a 1 s `rtrb` ring at the clip's rate, sleeps 5 ms when it is full, **loops by reopening at frame 0** while `looping` is set — no gap —, and marks itself `finished` on the end, a stop, an error or an abandoned tap); the mixer reads the ring through a `StreamTap` in place of the clip table (the same resampler, smoother and buses; `Command::StartStream`, `Return::StreamReleased` so the tap is dropped on the world thread; `tests/allocation.rs` renders a streamed voice among the rest: 121 callbacks, zero allocations), plays out what is left after `finished` and reports the voice ended; a ring found empty after priming while the decoder runs is **starvation**: silence for the rest of that callback and `MixerStats::starved` +1, surfaced as `fynkel.rec.AudioStats.starved` (with `streams`, the streamed clips registered; 64 B, **fingerprint `0xab5d834fb20093b6`**, fixtures rolled, the four locks and `importers.fynkel.audio: 2` refreshed), `AudioStats.starved`/`streams` in Dart, `LoadReport.audio.streams`, the headless report's `audio.streams` and `fynkel test --headless`'s `1 sounds (1 streamed)`; a device open/close respawns each streamed voice's decoder at the voice's clock position. Tests: `stream.rs` (start position, loop wrap, stop, a bad source), `tests/engine.rs` (a streamed clip ended by the clock with `starved == 0`, looped past four lengths, stopped to silence, respawned across a device close, unregistered), `fynkel_import` (`a_streamed_sound_keeps_its_source_instead_of_samples`, `a_streamed_sound_cooks_to_a_strm_chunk`, `StreamPayload` round trip). **Acceptance on content the implementer did not make** (the rule from 19a on): the reference game's `assets/audio/` — Kenney's `footstep_concrete_000/001` (the footsteps), `pluck_001` (the pickup) and `impactMetal_heavy_000` (the hit) and Juhani Junkala's "Level 1" from *5 Action Chiptunes* (the music; all CC0, committed with the credits in the game's README) — a `fynkel.AudioListener` on the Camera, a `Music` entity looping the streamed track on the music bus from the load (on the Rust host too: `audio: 1 sources, 1 sounds (1 streamed), 1 voices started` under the route's checksum), `game.Player` playing a footstep per `step_l`/`step_r` marker of 19c (`stepLeft`/`stepRight`/`hit`/`stepVolume` exposed, the file by the marker's name, a little pitch drift with the count) and the hit on a death, `game.Collectible` its `pickup` before it destroys itself; `test/game_test.dart` 'the sounds': `report.audio.sounds 1, streams 1`, the music's state playing with the track's 74.25 s duration, one one-shot per marker over 100 walked frames, the pickup at coin 1, `started == 1 + footsteps + 1`, `starved == 0`, the hit on a fall with the music still playing; `lib/main.dart` `audio: true`; on the Mac's output device the standalone host (`--audio`, which now prints the engine's counters at the end) streamed the track for 80 s through its loop: `7497 callbacks, 0 underruns, 0 starved`. **Part 1** (commit 1 of 2; `docs/audio.md` is the reference). Backend decision (scope rule): **`cpal` 0.18.2 + Fynkel's own mixer, not `kira`** — the gate counts allocations in the callback, so the callback is ours (`crates/fynkel_audio/tests/allocation.rs`: a counting global allocator around a render with clips loading, voices starting/stopping/looping/resampling/being stolen and a full command ring pumped inside it, zero). Two halves on two threads joined by `rtrb` rings: the `Audio` engine on the world's thread (clip registry, 32 voices with priority — a new sound past the limit steals the lowest-priority voice at or below its own, the oldest among equals, or is refused —, **the clock**: `advance(dt)` per tick decides every position and end, so a headless run reports the same `PLAYBACK_ENDED` ticks as a device run; bus/mute/listener gains; `started`/`stolen`/`refused`/`ended`) and the `Mixer` inside the callback (fixed tables of 256 clips and 32 voices, linear-interpolation resampling × pitch, a 5 ms one-pole gain smoother that declicks, nothing freed on that thread: released clips and ended voices come back over the second ring). Components in `fynkel_audio::components`, listed in `ALL` the physics way: `fynkel.AudioSource` (23: `clip`, `bus` master|music|sfx, `volume`, `pitch` 0.05…8, `looping`, `playing`, `priority`) and `fynkel.AudioListener` (24: `volume`; the lowest entity with one is the ears — Tier 1 is 2D, so a gain over everything), value rules `FYN-H0010` / `INVALID_PAYLOAD`. The audio phase runs last in the tick (`SystemKind::Audio`, before publish): every source reconciled with its voice (authored or written `playing` starts/stops, changed volume/pitch/looping reach the voice, a changed clip restarts, a removed component or destroyed entity stops), then the clock, then `PLAYBACK_ENDED` (kind 11, `{ entity, clip }`) for the non-looping voices that ended with `playing` cleared the same tick (a stop or a steal raises nothing; a stolen source reads `playing: false` the same tick). Commands: `fynkel.cmd.Audio` (10, simulation lane: `play`/`stop`/`pause`/`resume`/`setVolume`/`setPitch` on a source; `MISSING_COMPONENT`/`INVALID_PAYLOAD`/`WRONG_LANE`), `fynkel.cmd.PlaySound` (11, either lane: a fire-and-forget voice on no entity — no handle, no event; `SOUND_UNAVAILABLE` (18) every time for a clip the world lacks, refused silently past the limit), `fynkel.cmd.AudioBus` (12, either lane: a bus gain and the mute — the `fynkel.Settings` volumes' path; presentation-lane sounds and bus changes apply at submit). Queries `FYNKEL_QUERY_AUDIO` (12, `fynkel.rec.AudioState`) and `FYNKEL_QUERY_AUDIO_STATS` (13, `fynkel.rec.AudioStats`: device, format, voices, counters, callbacks, underruns, suspended). Clips: the cooked `CLIP` chunk registered whole (D4 closed: the consumer exists; mono/stereo `f32`, no resampling at cook) — the loaders register what a scene's or prefab's sources name, the native world registers from the cache the clip a `PlaySound` or a set `AudioSource` names before the batch, a source with an unknown clip reports `SOUND_UNAVAILABLE` once and keeps `playing` for a later registration. Device and lifecycle: `fynkel_world_audio_open` (`Ok`, or `FYNKEL_STATUS_UNSUPPORTED` 9 + `AUDIO_DEVICE` 19 — the null backend stays), `_suspend`, `_close`; opening resends clips and mix state so playing voices continue; `FynkelGameView(audio: true)` opens after the load, suspends while hidden/paused, closes on dispose; `hosts/standalone --audio` the same on `Occluded`/close; the headless host never opens one and reports `audio: { sources, sounds, started, stolen, refused, ended, diagnostics }` (`fynkel test --headless` prints the line). The Apple dylib already linked AVFoundation/AudioToolbox/CoreAudio (the iOS build verified with the feature on). Dart: `GameObject.audio` (`play`/`stop`/`pause`/`resume`/`volume =`/`pitch =`/`state`/`isPlaying`), `Behaviour.audio` + `onPlaybackEnded`, `world.audio` (`play(clip, {bus, volume, pitch, priority, presentation})`, `setBus`, `applySettings` — `SettingsStore.apply` calls it before the rebinds —, `stats`, `open`/`isOpen`/`suspend`/`close`), `LoadReport.audio`, `DiagnosticCode.soundUnavailable`/`audioDevice`. Fingerprint `0xb223170a5c7bdaec`. Fixture project: `assets/audio/tone.wav` on the Robot (`playing: true`; one voice started at the load, ended at tick 59, headless test) and a listener on its Camera. **Gate, part 1:** the starter plays its sounds from a Dart behaviour — the Cube's `fynkel.AudioSource` holds Kenney's `jump.ogg` (CC0) and the Mover plays it on take-off with `audio.play()`; `land.ogg` on landing through `world.audio.play(land)`, an `@Expose`d `AssetRef` set in the scene; the Camera the listener; `lib/main.dart` `audio: true`; `test/game_test.dart` replays `walk.input.json` on the null backend and counts 4 voices started and ended, 1 clip registered by the first one-shot, 2 `PlaybackEnded` on the cube (the "pickup sound" of the line below is the reference game's, part 2); the headless tests assert the voice limit and bus mixing on the null backend (`crates/fynkel_core/tests/audio.rs`: 32 voices, a priority-5 thief steals the oldest lowest, a −1 sound is refused, destroy frees; `crates/fynkel_audio` unit tests: bus × master × listener × mute gains, smoothing, resampling; `packages/fynkel/test/audio_test.dart` the same through the SDK); no allocation in the callback (above). Original text: backend decision recorded (scope rule), mixer with buses and voice limits, `AudioSource`/`AudioListener` components, `AudioClip` handles from cooked `CLIP` chunks (D4), streamed music from the cooked Vorbis bytes, commands `play`/`stop`/`setVolume`/`setPitch`, events `PLAYBACK_ENDED`, lifecycle suspend/resume from the host, null backend in the headless host. Gate: the starter project plays a pickup sound from a Dart behaviour; a headless test asserts the voice limit and bus mixing on the null backend; no allocation in the audio callback (counting allocator test).
12. **Scenes, runtime prefabs, settings** (done 2026-09-20; `docs/scenes.md` is the reference — `World::load_scene_with(scene, LoadMode::{Replace, Additive})` / `unload_scene(id)` / `loaded_scenes()` / `scene_of(entity)` in the core with a scene registry (which loaded scene an entity came from): replace unloads every loaded scene (last first) and destroys whatever else is alive — runtime spawns included — before loading, additive stacks and refuses an already-loaded id (`FYN-R0011`), unload takes the scene's still-alive entities only (`FYN-R0010` when not loaded), `SCENE_READY` (5) now carries `scene: AssetRef`, `SCENE_UNLOADED` (6) `{ scene, entities }`; `fynkel.TriggerZone { event, once }` (id 20; `FYN-H0009` without a sensor) turns the physics-lane trigger contacts into general-lane `ZONE_ENTERED` (7) / `ZONE_EXITED` (8) `fynkel.ev.Zone { zone, other, event }` and `once` removes itself after the first entry — a Rapier default meant a kinematic character never overlapped a static sensor, so sensor colliders now carry `KINEMATIC_FIXED | KINEMATIC_KINEMATIC` active collision types (physics.md § Contacts); `fynkel.Settings` record (`settings.schema.json`, `FYN-U0001`–`U0003`) with rebinds re-applied over the project's actions after every load; ABI `fynkel_world_load_scene_with(…, FYNKEL_LOAD_ADDITIVE)`, `fynkel_world_unload_scene`, `fynkel_world_bind_actions(json)`; fingerprint `0x13943882aa51a5a0`. Dart: `World.loadScene(id, {cacheDir, mode})` (deviation: the M3-03 `loadScene(cacheDir, id)` order is gone, `cacheDir` remembered from the last load), `unloadScene` → `UnloadReport`, `bindActions`, `loadedScenes`; `world.scenes` = `SceneManager` (`state`, `current`, `loaded`, `load`, `unload`, `transition(to, {mode})` performed at the start of the next `World.frame`, `cancel`, listeners); `Behaviour.onZoneEnter/onZoneExit(ZoneEvent)` + `world.zones` hub; settings as `SettingsDocument` + `SettingsStore` (`open(path)` on disk with tmp+rename saves, `defaultPath(appName)`, memory by default) behind `GameContext.settings` — a mutable field rather than a static `Settings` facade because the generated record is named `Settings`. Fixture project gains `results.scene.json` and `coin.prefab.json` (sensor + `TriggerZone "collect"`). Gate: `packages/fynkel/test/scenes_test.dart` — the fixture's Playground, a coin instantiated at runtime, the character walks into it, `Collector.onZoneEnter` destroys it and `transition(Results)` from `fixedUpdate` lands within 10–30 frames (`[ready, pending, ready]`), a `SettingsStore` on disk is edited, reopened and its jump rebind applied on the next load; the native ABI test `scenes_stack_unload_replace_and_zones_fire_through_the_abi`; `crates/fynkel_core/tests/scenes.rs`. The starter installs `SettingsStore.open(settingsPath, world:)` in `Game.load` and persists Q's quality choice.) `World.loadScene` replace/additive with unload (`SCENE_UNLOADED`, residency drops), `SceneManager` in Dart (`load`, `unload`, `transition(to)` with a Dart-side state enum the HUD observes), ~~`world.instantiate(prefabRef, transform)` from the cooked prefab~~ (done in 8b: anchor + the prefab's roots, the same shape as expansion), trigger zones as sensor colliders with a `TriggerZone` catalogue component that names an event for Dart, `settings.json` (audio buses, quality profile, input rebinds) with the Rust model + schema + Dart `Settings` service in `GameContext`. Gate: a headless test loads `Playground`, collects an item spawned at runtime, transitions to `Results`, saves and reloads settings.
13. **Flutter HUD models and projected world labels** (done 2026-09-20; `docs/hud.md` is the reference — `World::project(width, height, anchors, out)` in the core (`world/projection.rs`): the frame's camera (the one `extract` picks: highest priority, ties earliest) and interpolated poses through the renderer's own `view_matrix`/`projection_matrix` (now shared with `fynkel_render`), one `fynkel.rec.ProjectedPoint { entity, x, y, depth, flags }` per `fynkel.rec.ProjectionAnchor { entity, offset, mask }`, flags `IN_FRONT`/`ON_SCREEN`/`OCCLUDED`/`MISSING`/`NO_CAMERA`/`DROPPED`; `MAX_PROJECTION_ANCHORS` 64 with `TOO_MANY_ANCHORS` (15, `FYN-R0012`) beyond; `fynkel_world_project` over `FYNKEL_BUFFER_PROJECTION_ANCHORS` (9); fingerprint `0x5b5e4210411a1a74`. `package:fynkel`: `World.projection` (`ProjectionView`/`ScreenAnchor`, one call per frame, the diagnostic once per overflow episode, informational), `GameModel`/`ModelValue<T>`. `package:fynkel_flutter`: `ProjectionLayer` (owns its children's anchors, viewport from layout, a `Flow` repainting on the view's listener — no rebuilds per frame, unpainted and unhittable when not visible), `ProjectedAnchor`/`WorldLabel`, `ModelBinding`/`ModelBuilder`/`.listenable`; `FynkelGameView` updates the view after each frame. Starter: `HudModel` installed as a context service, `Mover.update` writes it, `MoverReadout`/`PauseBanner` bind it, `WorldLabel`s on the Cube and the Crate; showcase labels. Deviations from the line below: the fields are `ModelValue<T>` (the `ValueNotifier` shape, no Flutter import in `package:fynkel`) adapted to `ValueListenable` in the Flutter package; the projection is a world-level query from the scene camera only (the viewport's fallback camera is not projected — `NO_CAMERA`), in the caller's logical pixels (the DPR is the texture's business); depth occlusion is opt-in per anchor by collider mask (a raycast from the camera eye; mask 0 is never hidden) rather than a depth read-back; the pattern is documented in `docs/hud.md` with a pointer from `docs/design/dart-sdk.md`. Found on the way: a `GameObject` attached this tick read identity before its first snapshot, so the starter's `transform.rotation = …` at the first tick sank the Cube to y 0 — the SDK now reads the transform through the component query until the snapshot arrives. Gate: `packages/fynkel_flutter/test/projection_test.dart` — a `WorldLabel` sits at the fixture crate's projected point across a scripted camera pan with zero rebuilds and is neither painted nor hit-testable once culled or behind the camera): `package:fynkel_flutter` `ProjectedAnchor`/`WorldLabel` widgets fed by a bounded per-frame projection view (viewport transform, DPR, camera identity, clipping, visibility flag; ≤ 64 anchors, a `FYN-R####` diagnostic beyond), `GameModel` base (`ValueNotifier` fields Behaviours write from `update`) with a `ModelBinding` widget; documented pattern for main menu / HUD / pause / settings / results in `docs/design/dart-sdk.md`. Gate: a label follows the starter cube on screen (widget test with a scripted camera); anchors leave the screen when culled.
14. **Reference game (a): world and core loop** (done 2026-09-21; `examples/reference_game`, its `README.md` and `AGENTS.md` are the tour — created by `fynkel create examples/reference_game`, the world generated by stdlib Python in `tools/` so the repository carries no binary of unknown origin: `kit.py` writes seven unit meshes as `assets/models/kit.glb` (imported once, `mesh/0..6`), `playground.py` writes 16 materials, the `coin`/`ball`/`burst` prefabs and `assets/scenes/playground.scene.json` with deterministic uuid5 ids — 157 entities, 113 static meshes (all colliders), 22 rigid bodies, 10 coin prefab instances, sun with shadows, 4 point lights; the route: two coins, a ramp onto a platform, a corridor swept by an `Oscillate` blade whose child sensor is a `hazard` `TriggerZone`, a lane where an `Emitter` launches a dynamic ball prefab every 4 s (a hazard sensor too), a block to jump onto, a switch pad and the gate it opens. Dart: `game.Player` (walks the `CharacterController` in the `ThirdPersonCamera`'s yaw frame — the controller also names `move`/`jump` so the Rust host walks the same route —, sprint, jump on the press while grounded, death on a hazard zone or below `fallY` = a transform write teleporting it home, `restart` → `scenes.transition`, the clock), `game.Collectible` (scores on `onZoneEnter('collect')`, `world.instantiate(burst)` with six impulsed sparks on `LifetimeDespawn`, destroys its prefab anchor via `gameObject.parent`), `game.Switch` + `game.Gate` (a hint on the model while occupied, `interact` slides the static slab up), `GameState` (`GameModel`: coins/total/won/clock/deaths/hint/paused) bound by the HUD counter, win/pause banners and a `ProjectedAnchor` prompt over the pad; `test/collect_all.input.json` from `tools/route.py` replayed by `test/game_test.dart` (all ten, no deaths, the gate open, ten bursts seen) and natively by `fynkel test --headless --input … --steps 1400`; CI regenerates the world and diffs it. Decided here (`docs/input.md` rule 5, `docs/physics.md`): a relative control's motion is consumed by exactly one tick as a rate (`x·dt` is the motion; catch-up ticks read zero, a zero-tick frame carries it; `update` reads the frame's motion through the new `frame_x`/`frame_y` — fingerprint `0x97fd26db36af3421`), and a binding takes a `*<factor>` scale (`mouse:delta*0.0025`) so pixels compose with sticks and `sensitivity` is one knob; no camera smoothing in V1 — the rig snaps. Deviations from the line below: the "physics ball" hazard is a launched prefab rather than a loose ball, the meshes are generated primitives rather than an artist's glTF, and the recording replays natively too. Gate: `flutter test` green in CI; built and played on macOS through `fynkel run` with keyboard + mouse and a gamepad): project created by `fynkel create`, `Playground` scene authored in files (≈100 static meshes from imported glTF, ≥ 20 rigid bodies, 10 collectibles, 2 hazards — one `Oscillate` platform, one physics ball — sun with shadows, a few point lights, PBR materials), capsule player with `CharacterController` + `ThirdPersonCamera`, collect/trigger/win logic in Dart behaviours, restart, one runtime-spawned prefab, HUD counter; headless gameplay test that collects all 10 from a recorded stream. Gate: playable on macOS with keyboard + gamepad via `fynkel run`; the headless test is green in CI.
15. **Reference game (b): menus, settings, audio, touch, results** (done 2026-09-21; three generated scenes — `tools/playground.py` writes `assets/scenes/menu.scene.json` (the startup scene: a podium with a slow big coin, eight ring coins on a `Rotator`, four lit pillars, the camera on a `Rotator` pivot at 0.125 rad/s, the streamed music), `playground.scene.json` and `results.scene.json` (the podium, three cones, a still camera, the music and a `Jingle` `AudioSource` playing `win.ogg` once from the load) — deterministic uuid5 ids, 29/169/13 entities; no `MainMenu` class: the flow is `Game.play()` / `finish()` / `toMenu()` over `world.scenes.transition` and a `GameState.screen` (`Screen.menu/playing/results`, set from the `SceneManager` listener) that the app's `ModelBuilder<Screen>` switches on, `GameState.lastRun` (coins, time, deaths, footsteps) snapshotted by `finish()`, `game.Player` finishing the run `winDelay` = 1.5 s after the last coin; `lib/ui/`: `MenuScreen` (Play/Settings/Quit — Quit on desktop only), `PauseMenu` (Resume/Settings/Give up/Quit to menu — Esc or the `pause` action, an on-screen pause button on touch platforms), `ResultsScreen` (a table of the run, Play again/Menu), `SettingsPanel` (master/music/effects sliders and mute through `SettingsDocument.copyWith` → `settings.update`, the quality profile through the viewport and the document, a row per button action with Rebind — the next key is captured in `FynkelGameView.onKeyEvent` before the input capture sees it, Esc cancels — Reset per action and Reset to defaults; the file is `settings.json` under the platform's app-support directory, `SettingsStore.apply` re-binds after every load so `lastReport` shows `rebound`), the HUD (`Counter`, `StatsBlock` with the audio stats, `Hint` from `Game.bindingsOf` — hidden on touch platforms —, `WinFlash`, the projected `SwitchPrompt`) under `SafeArea`; every `MenuButton` plays `click.ogg` on the presentation lane; `ExcludeFocus` keeps the buttons from stealing the viewport's focus; the music streams on every scene, footsteps/pickup/hit from M3-11; `test/game_test.dart` gains the flow (Menu → Play → the route → Results after ≈90 ticks with `lastRun` 10/10, the jingle playing once, the click, Play again, Give up → `won false`, Menu) and the settings (defaults, `bindingsOf`, a rebind written to the file and applied — Space no longer jumps, J does —, reset, a relaunch reading the file); CI runs the Menu and the Results headless and the route by `--scene`; `README` documents every command. Verified on macOS (built and driven end to end: title → settings → rebind/cancel → play → pause → give up → results → play again → quit to menu; settings written) and on the iPhone 16 Pro Max simulator (title without Quit, the overlay's stick and jump, the pause button, results) — the real-iPhone qualification stays M6. Two more Kenney CC0 clips, `win.ogg` and `click.ogg`, credited): `MainMenu` → `Playground` → `Results` transitions, pause menu, settings panel bound to `settings.json` (audio buses, quality, rebinding), background music + footsteps + pickup sound, the touch overlay on iOS, at least one projected label; `README` with the documented build/run commands. Gate: end-to-end playable on macOS; launches and plays on an iPhone (qualification stays M6).
16. **Instrumentation and captures** (§15.4, C3, C5) — **done 2026-09-22** in two commits (`docs/instrumentation.md` is the reference). **Part 1** (2026-09-21): the timers on their real paths — `World.frame` split into behaviours / native ticks / presentation (`world.frameTimes`, allocation free) and `FynkelFrameStats.behaviours/nativeStep/systems/flutterBuild/flutterRaster/totalWork`; extract / prepare / encode timed and the GPU passes stamped with `wgpu` timestamp queries (Metal stage timestamps, two alternating sets resolved a frame late because Apple's passes overlap, mapped without a wait; `gpu_frame_us` the span) with `Renderer::gpu_bytes()`; viewport ABI 3 (`extract/prepare/encode/gpu*` percentiles, `gpu_bytes`); `fynkel_core::memory::CountingAllocator` as the `#[global_allocator]` of every final binary + `task_info` resident set + `residency_bytes` in `FynkelWorldStats`, the headless report (`extract_time`, `frame_time`, `memory`) and the standalone `[1s]` line; `LoadReport.loadMs`; the C5 budget `fynkel_world_set_snapshot_budget` → `SNAPSHOT_BUDGET` (20, `FYN-R0013`) once per crossing, snapshot still whole. **Part 2** (2026-09-22): C3 as above; `FynkelFrameStats.summary()` printed as `[stats]` by the host every window and by the reference game under `FYNKEL_CAPTURE=1` (which also starts it in the Playground); `benchmarks/m3/capture.py` records both scenes — the stress scene (standalone `[1s]` incl. `sim/prepare/gpu/mem`, the Flutter host's profile bundle) and the reference game (rendered, the route headless, the Playground in the standalone host and in the game) — and checks the 12 ms p95 total-work target: `results/2026-09-22-0427213` stress **7.59 ms p95** (simulate 3.50 + render() 4.08), reference game **1.62 ms p95**, GPU stamps read back in both apps, the headless route report with the timers, the Dart heap 39–56 KB/frame recorded (a gap noted in status §3); fifteen checks green. **The allocation gap (2026-09-22, after item 17):** attributed configuration by configuration on the profile bundles — a blank Flutter app with a ticker allocates ≈ 12 KB/frame, a progress indicator +7, a per-frame `Text` +16; of the game's 57 KB/frame, 39 were the SDK's read path (`find` re-decoding the entity table every tick, `get<T>()` decoding every component of the object, the label layer repainting every frame) — and fixed in `package:fynkel`/`fynkel_flutter` without changing a contract: the entity table re-decodes only when the query's bytes differ from the retained copy, a typed read decodes one component from the object's retained query copy (still one query per object per tick), `WireBuffer.reparse()` and the bridge's per-kind query views allocate nothing in steady state, `ProjectionView` notifies only on change with `RepaintBoundary` per label. The game reads **17.6 KB/frame** (behaviours ≈ 2 KB), `tool/alloc_bench.dart` carries a `find` + `get` per tick as the regression check (docs/instrumentation.md § What a Flutter frame costs). Original text: per-frame timers for Dart behaviour time, native simulation, physics, catalogue systems, transform propagation, extraction, render preparation, GPU pass timings (Metal timestamps), draw/instance/culled counts, bridge calls/bytes, live Dart allocation and GC counters in the host's diagnostics (C3, with an AOT capture in `benchmarks/m0/capture.py`), native memory and asset residency, subscription budget diagnostic (C5); `benchmarks/m3/capture.py` records the reference game and the stress scene against the manifest; `fynkel test --headless --json` reports the same timers. Gate: numbers for both scenes in `benchmarks/m3/results/`; the 12 ms p95 total-work target checked, not assumed.
17. **Public readiness** — **done thin, 2026-09-22** (the user's call: there is no reader yet, the engine matters more than its paperwork). Done: the `fynkel create` template and its `AGENTS.md`/`README.md` at the M3 SDK (the viewport, input, audio, settings, scenes, HUD models and labels, physics, animation sections were already there; added `FynkelFrameStats` — "Where the frame goes" — with `profileDartHeap`, the `dart heap` HUD line and the `FYNKEL_CAPTURE=1` `[stats]` line, as the reference game has them); a minimal top-level `README.md` (what it is, the commands below, the layout; the reference game's README linked to one that did not exist); the gate: a clean `git clone` → `cargo install --path apps/cli` → `examples/reference_game`: `fynkel validate`, `test --headless` (the Menu and the recorded route), `render`, `flutter test` (the CLI found on `PATH`, no `FYNKEL_CLI`), `cook`, `flutter build macos --debug`, the bundle launched with `FYNKEL_CAPTURE=1` printing `[stats]` lines at 120 frames/s — no hidden assumption other than the missing README. **Deferred until there is a reader** (a "public readiness (b)" item when the repository actually goes public, after M5's Tier 2 game at the latest): ADR-0005 decided (license, contribution, trademark), `LICENSE`, the first-time-reader `README.md` (`fynkel create` in five commands, screenshots), `CONTRIBUTING.md`, issue templates, CI green on a fresh fork, the pub.dev/crates.io artifacts (C6). Original text: ADR-0005 decided (license, contribution, trademark), `LICENSE`, `README.md` rewritten for a first-time reader (what it is, `fynkel create` in five commands, the reference game), `CONTRIBUTING.md`, issue templates, CI green on a fresh fork, `docs/` index; the `fynkel create` template and `AGENTS.md` updated to the M3 SDK (viewport, input, audio, settings). Gate: a clean clone builds and runs the reference game through the documented commands only.
18. **M3 gate report** — **done short, 2026-09-22** (`docs/m3-gate-report.md`, the user's call: written short; verdict GO with two named exceptions — the repository stays private (17 thin) and the game ran on the iPhone simulator, not a device —; the clean-clone run at `6176d5e`; the C ABI deliberately **not** bumped (nothing outside the checkout links it; deferred to "public readiness (b)" with the first external consumer); carry-overs E1–E9 with owners, E1 = IBL next on this stream; `docs/status.md` header and §6 rewritten). Original text: `docs/m3-gate-report.md` against the §16 gate (build/run from a clean checkout, playable on macOS, runs on an iOS device, no editor, instrumented frame paths, reference-game and stress numbers against the manifest, repository public-ready), `docs/status.md` rewritten, carry-over list for M4/M5 (editor ABI D5, pointer index D7, cache hygiene D8, Tier 2 items).

19. **Animation — one clip core, skinning, the `Animator`** (done 2026-09-21; `docs/animation.md` is the reference — `fynkel_core::animation` (`Clip`/`Channel`/`Pose`/`Skin`: step, linear and cubic-spline sampling, lerp/slerp blending, targets by name) and `world/animation.rs` (`Animator` 21 ticked first among the pre-tick systems — a fresh animator samples its start without advancing, looping wraps, a one-shot clamps and raises `ANIMATION_ENDED` (9) once, a crossfade keeps the outgoing clip on its own clock and blends it out, `CLIP_UNAVAILABLE` 16 once per clip; `Skin` 22 resolving joints by name under its skeleton root, `SKIN_UNAVAILABLE` 17; `skin_palette` = `inverse(mesh_world) · joint_world · inverse_bind` from the joints' interpolated matrices in the extract); `fynkel_import::gltf` v4 (`SkinWeight`s as the `MESH` payload's trailing section, `SKIN`/`ANIM` chunks, `FYNI` v3, cache v4, the prefab's `fynkel.Skin`; `fixtures/rig.py`); `fynkel_render`: a second vertex buffer, the palette storage buffer at bind group 2, `vs_skinned` in `mesh.wgsl`/`shadow.wgsl`, `PIPELINE_VARIANTS` 9 → 14, `FrameStats.skinned`/`joints`, `fynkel render --steps`; wire `fynkel.cmd.Animate` (9), `fynkel.rec.AnimatorState`, `FYNKEL_QUERY_ANIMATOR` (11), fingerprint `0x18acc498ae005854`; `fynkel_native` registers a clip a behaviour plays from the cache on demand; Dart `GameObject.animation` (`AnimationControl`), `Behaviour.onAnimationEnded`, `AnimatorState`, `LoadReport.animation`; the reference game's `tools/character.py` (eleven boxes on eleven joints, idle/walk/run/jump in place) worn by the Player with the joints authored as entities and `game.Player.pose` crossfading from the controller's state; the showcase's runner and `renders/m3-19.png`. Gate met: `examples/reference_game/test/game_test.dart` 'the character is posed from the controller', `packages/fynkel/test/animation_test.dart` joint poses to the number at tick 31, `crates/fynkel_render/tests/project.rs` the posed column.) The shape as planned, in layers: **(a) the clip core, generic on purpose** — a clip is keyframe channels, each targeting *a named node under the animator and one property* (translation / rotation / scale; step, linear and cubic-spline interpolation, looping, speed), sampled into a pose; poses blend (lerp / slerp, per-channel masks), so a bone, a door and a lamp are the same thing to the sampler — the timeline/sequencer document of M5 and the data-driven state machine that follows this item both sit on this core with no second sampler; **(b) skinning** — the importer keeps glTF skins (joints by node name, inverse bind matrices, the skeleton root), animation clips (node targets by name) and `JOINTS_0`/`WEIGHTS_0` (a trailing skin section of the `MESH` payload, so static meshes keep their 72-byte layout; `fynkel_import::gltf` + intermediate/cache versions), cooked `SKIN`/`ANIM` chunks as `skin/<s>`/`anim/<a>` sub-assets, a `fynkel.Skin { skin, skeleton }` component next to the `MeshRenderer`, joint matrices computed per frame in the extract from the joints' interpolated poses and uploaded to the renderer's joint buffer (a skinned vertex path with its own pipeline variants; the 4-pipeline cap revisited in `docs/renderer.md`); **(c) the `fynkel.Animator` component** (clip, speed, loop, playing) ticked by the core in the pre-tick slot next to the catalogue — `play(clip)`, `crossfade(clip, seconds)` (a timed blend of two poses), `stop`, speed — reachable from Dart on the M3-07a path with an `ANIMATION_ENDED` event and an `AnimatorState` query; **(d) the root-motion policy, written down**: V1 extracts no root motion — the `CharacterController` (or the game) moves the entity, the animator only poses nodes under it, so clips are authored in place; **deferred to the follow-ups on the same core** (blueprint §9 "Animation"): the state machine as data (states = clips or blends, transitions on named parameters, Dart sets parameters only), the timeline/sequencer document (tracks over named objects: transforms, component fields, events), clip markers/events, IK, retargeting, a node-graph editor. Gate: the reference game's player is a rigged glTF character (generated by a committed script, like the kit) idling, walking, running and jumping from `CharacterController` state; a headless test asserts joint poses at a tick; the showcase render shows a posed character.
19a. **glTF prefab instancing** — **done 2026-09-21** (M3-19a; docs/documents.md § Prefab documents › Imported hierarchies, docs/animation.md § Instancing an imported character). As specified below with two deviations: `validate` gets the hierarchy by running the importer in memory (`fynkel_import::ImportedPrefabs`, the `SourcePrefabs` provider of `validate_project_with`) rather than a vertex-free `prefab_of` — one code path, the same ids as the cook, ~10 ms for the Mixamo file — and the runtime needed no ABI change: `Assets::prefab(id)` reads a cooked source's `prefab` chunk when the id is a source, Dart checks the sub. Acceptance passed on the fixture rig (the Rig scene's `Imported` instance with a material override, `packages/fynkel` instantiating `rig.glb#prefab` and waving it), the showcase Runner (`character.glb#prefab` + the suit override, render pixel-identical at ticks 1 and 40), the user's Mixamo rig through `playground.py --hero` (route replayed, 0 diagnostics) and — the outside content — the Khronos Fox/CesiumMan/RiggedFigure through the CLI end to end. (Original text: added 2026-09-21 after a Mixamo character went through the M3-19 path; not in the original backlog; runs after 11, before 15): the one manual step left between "download a rigged character" and "it plays in the game". Today `fynkel import` cooks a source's node hierarchy as the `prefab` sub-asset (one entity per node with the `Transform`, `MeshRenderer` and, since M3-19, `Skin`), but a scene's `prefab: { asset }` and `World.instantiate` resolve **prefab documents only** — `FYN-D0401` for a `.glb` — so the hierarchy has to be re-authored by hand (`playground.py` does it from the same table that built the model; a 65-joint Mixamo rig cannot be). (a) **Scenes:** `prefab.asset` accepts `{ asset: <source>, sub: "prefab" }`; `PrefabLibrary` gains the imported prefabs — the cook has them in the intermediate already, `validate` derives them from the sidecar + the source's node tree without reading vertex data (`fynkel_import::prefab_of(source)`, the importer's own code path, deterministic: entity ids stay UUID v5 of the asset id and `node/<i>`) — and expansion, overrides (`setField` on `node/<i>/renderer` to swap a material, on the root transform), nested prefabs, `D040x` diagnostics and `fynkel scene inspect --expand` treat it like any other prefab; a source without a hierarchy (a texture) is `D0401` with a message that says so. (b) **Runtime:** `Assets::prefab` reads the `prefab` chunk of a cooked source (`documentKind: prefab`, `sub: prefab`) next to `document/prefab`, so `World.instantiate(AssetRef(glb, sub: 'prefab'))` spawns the rig with its `Skin` and the animator's targets resolve by name — no new wire kinds, no schema change (`assetRef.sub` exists). (c) **Import:** `fynkel import --prefab` also writes the hierarchy as `<name>.prefab.json` next to the source with a fresh id for the cases that want authored, stable, editable ids. Acceptance: the fixture project's Rig scene instances `rig.glb#prefab` (the hand-authored column stays as the second, ids-by-hand form); `packages/fynkel` instantiates it at runtime and plays `wave` on the instance; the showcase's Runner is instanced from `character.glb#prefab` with a material override (its render unchanged pixel-for-pixel except the ids); a converted Mixamo rig (`docs/animation.md` gains the Blender recipe: three FBX → NLA tracks → one GLB, "In Place" clips, 4 bone influences) validates, cooks, runs headless and renders from a scene that names only the source — the scratch run on 2026-09-21 did exactly that with a hand-extracted prefab document (74 entities, 2 skinned meshes over 130 joints, walk/run/jump from Dart, `AnimationEnded` at 1.933 s).
19b. **FBX import and the rig document** — **done 2026-09-21** in two commits (M3-19b; docs/documents.md § Import › FBX and § Rig documents, docs/animation.md § FBX straight in and § Four downloads, one character). **Part (b), the rig document** (commit 2 of 2), as specified below with these resolutions: the rig's sub-assets are `prefab` (the skeleton file's hierarchy, adopted under the rig's `prefab` sub uuid as its document id and the rig's name, entity ids the source's so overrides survive the move from `<fbx>#prefab` to `<rig>#prefab`) and one `anim/<name>` per clip — **not** the skeleton's `mesh/<n>` and `skin/<s>`, which stay the source's own sub-assets (nothing is re-exported, the skin references inside the adopted hierarchy already point at the source, and the runtime closure of a rig is the rig plus its skeleton file, never the clip files — the cache key still covers them); matching by bone name at cook is `FYN-A0106` when the clip's file has no skin or the target is one of its own joints, and a dropped channel with `A0105` when the target is some other node of the clip's file; `markers` wait for 19c; `fynkel import --rig` names clips from the file stem when the file has one clip and from the clip's name otherwise (`clip_name_from`: lower-case, runs to `_`, `_2` on a collision), keeps an existing rig's id, matches every clip before writing and still writes on a mismatch so the document can be fixed; `fynkel rig inspect|validate` import in memory; `fynkel validate` checks the document and its references (`D0500`–`D0502`), the bones are the cook's and `rig validate`'s. No ABI change (fingerprint `0x18acc498ae005854`); `DocumentKind::Rig` is in every lock's `documents`. Acceptance passed on content the implementer did not make: the Khronos `Fox` through `import --rig` → `rig inspect|validate` → a scene on `<rig>#prefab` + `anim/run` → cook, headless, `state dump` (`b_Neck_04` moves), and `RiggedFigure` + `CesiumMan` refused with `A0106` at `/clips/cesiumman` (`apps/cli/tests/check.rs`); the fixture project's Column rig through cook, cache invalidation and the runtime store; the user's four Mixamo files as `hero.rig.json` with one command (65 joints, every clip 156/156), the reference game's `tools/playground.py --hero assets/models/hero/hero.rig.json` (the Player told `anim/idle` … by name through `game.Player`'s `idleAnim` … fields) validating, replaying the route headless, passing the Dart tests and rendering the X Bot in the Playground; the box character stays the committed default. **Part (a) done 2026-09-21** (commit 1 of 2). As specified below with these resolutions: the importer is the glTF importer's twin over one shared module (`crates/fynkel_import/src/graph.rs` — prefab, material header, image binding, channel checks, assembly — so `robot.expected.fyni` is byte-identical after the refactor), sub-asset ids use the file's typed ids (`mesh/<id>[/<part>]`, `material/<m>`, `image/<t>` — not `texture/`, the glTF name is kept — `skin/<s>`, `anim/<n>`, `prefab`), an animation stack that animates nothing (Mixamo's `Take 001`) is skipped silently and the clips keep their **stack names verbatim** (`"mixamo.com"` is `anim/0`; naming a clip is the rig document's job in part (b), not a heuristic in the importer), textures are bound for the base colour, normal, occlusion and emissive slots only (a roughness/metalness/opacity texture is reported and left for the material document's packed slot), external textures need `allowExternalFiles` and are cache-key inputs like a `.gltf`'s URIs, and the fixture pair is exported by `fixtures/blender_fbx.py` with `bake_space_transform` (`rig.fbx`, `robot.fbx`, `robot_external.fbx` + `paint.png`; Blender's armature convention — a −90° X turn on the armature node, +90° X on the root bone — is left as the file has it, world poses match the glTF export). Acceptance passed: `tests/fbx.rs` compares `rig.fbx`/`robot.fbx` with `rig.glb`/`robot.glb` (positions, normals, UVs, colours, weights, bind matrices, rest poses, keys, materials, the embedded texture's pixels, the prefab), a cook test, `fynkel_platform_apple` built for `aarch64-apple-ios` with `ufbx` in the workspace, and the user's four Mixamo files (Idle, Walking, Fast Run, Jumping) each imported with no diagnostics, cooked, ran headless (74 entities, 2 skins over 129 joints) and rendered posed and upright from a scene naming only the source — no Blender. Part (b), the rig document, is the next commit. (Original text: added 2026-09-21; runs after 19a): the Blender detour goes away. (a) **`fynkel.fbx` importer** in `fynkel_import` over `ufbx` (crate `ufbx` 0.11, C99 bundled, MIT/Unlicense; a CLI dependency only — the runtime never reads a source; probed 2026-09-21: builds on macOS and reads a Mixamo file as 68 nodes, 2 skinned meshes, 65 bones, the clip as the stack `mixamo.com`; built for iOS as part of acceptance even though only the CLI links it, so the workspace stays one `cargo build`): the same intermediate (`FYNI`) and sub-asset scheme as glTF — `mesh/<n>`, `material/<m>`, `texture/<t>`, `skin/<s>`, `anim/<a>`, `prefab` — unit and axis conversion to metres and Y-up at import (the prefab root carries no leftover 0.01 scale), embedded textures, a default take (`Take 001`) skipped when a named stack exists, the clip named after the file stem when the stack is Mixamo's site name, the same limits and `FYN-A01xx` diagnostics, a `.meta.json` with `importer: fynkel.fbx`. (b) **The rig document** `<name>.rig.json` — the answer to "four downloads, one character": `{ skeleton: <source ref>, clips: { idle: <source ref>, walk: … }, markers?: {…} }`, a document like a material (validated, cooked, in the asset db, `fynkel rig inspect|validate`): its sub-assets are the skeleton file's `mesh/<n>`, `skin/<s>` and `prefab` and one **named** `anim/<clip name>` per entry, matched onto the skeleton **by bone name** at cook (`FYN-A0106` names the first bone a clip has that the skeleton lacks; extra bones in the skeleton are fine, a clip's own mesh is ignored); an `Animator` names `{ asset: <rig>, sub: "anim/walk" }`, so a game reads as text and a re-export never renumbers the clips; `fynkel import Idle.fbx Walking.fbx "Fast Run.fbx" Jumping.fbx --rig hero` writes `hero.rig.json` (skeleton = the first file, clips named by stem, lower-cased, spaces to `_`). (c) **Fixtures from outside:** the Khronos `Fox` (24 joints, Survey/Walk/Run), `CesiumMan` and `RiggedFigure` under `crates/fynkel_import/fixtures/khronos/` with `ATTRIBUTION.md`; an FBX fixture exported by Blender from the fixture rig (`fixtures/rig.py` → `rig.fbx`, the export script committed, the binary regenerable by anyone with Blender, not diffed in CI) plus the same content as glTF so the two importers are compared on one model. Acceptance: the Khronos rigs import, cook, run headless and render posed (pixel tests against the glTF path); the user's four Mixamo files become `hero.rig.json` with one command, no Blender, and the reference game's opt-in hero (`tools/playground.py --hero assets/models/hero.rig.json`, git-ignored asset, the Player's clips by name) validates, replays the route and plays on macOS; the box character stays the committed default.
19c. **Animation depth: blend spaces, layers, markers, root motion** — **done 2026-09-21** in two commits (M3-19c; docs/animation.md rewritten around the four features — § Blend spaces, § Layers, § Markers, § Root motion, § The character, complete —, docs/documents.md § Blend spaces). **Part 2** (commit 2 of 2: (a) blend spaces, (e) the stress gate, the reference game's character complete). Resolutions: a blend space is a **document**, `<name>.blend.json` `{ parameter, points: [{ at, clip }], sync: "phase" }` (`DocumentKind::Blend`, `blend: 1` in every lock's `documents`, `schemas/blend.schema.json`; `fynkel validate` checks the points are strictly increasing in `at` — `FYN-D0600`; at least one by the schema —, that every clip is an `anim/*` sub-asset of a rig document or a model or nil — `D0601` —, and the entry's shape — `D0602`), cooked like a material (a generic document chunk, `documentKind: blend`, the clips' carriers its dependencies), reached at runtime through `Assets::blend`/`is_blend` and registered by `register_clip` as one space (its points' clips registered, counted once by the headless host's `animation.clips`); an `Animator` whose `clip` is the space's id **plays it like a clip** — `play`, `crossfade`, looping, speed, the state's `clip` — so nothing on the wire changed (fingerprint stays `0x68267f95a40bac58`): the world takes the two points around the parameter (clamped to the points' range, the first point's `at` until set), blends their poses by the fraction between them (a property only one side keys is taken as it is) and **phase-syncs** them — the space's duration is the lerp of the two clips', each clip is sampled at the shared phase times its own duration, and a parameter change rescales the clock (`time *= D / D_last`) so the phase is kept and the feet never jump; the space's markers are the **dominant** clip's (the nearer point), its root motion the lerp of the two, an additive space's reference the first point's clip at time 0, a crossfade out of it fades the dominant clip; `AnimatorState.duration` is the blended duration, `parameter` the value **as set** (the raw number, so a game reads back what it wrote; the clamp is the space's), `animation.setParameter(name, v)` from Dart and `SetParameter` from a native host. (e) The stress scene gains 200 rigs (`generate.py --rigs 200`: the reference game's box character skinned to its 11 joints, an animator on each — walk, run and the `locomotion.blend.json` space by turns, speeds 0.8–1.25 — and an additive wave layer on every fourth rig's `spine`; 8 406 entities) and `capture.py` the gate (item 19c: ≥ 200 skinned meshes, markers crossed, animation mean < 1 ms, p50 tick < 4 ms with everything else on; the standalone drawing ≥ 150 skinned instances at ≥ 118 frames/s). The standalone host now registers what the headless one does before the first step — gravity, mesh colliders, prefabs, clips and skins — so its rigs draw skinned and its emitters spawn (it drew every rig unskinned and no sparks before). Acceptance passed on content the implementer did not make: the Khronos `Fox` on `fox.blend.json` (Survey at 0, Walk at 1, Run at 2 by `speed`) through `validate`, `cook` and `test --headless` (the space counted once, 0 diagnostics) and in-process with `SetParameter`: at speed 1.5 every one of the 23 `b_*` joints is the lerp/slerp midpoint of Walk and Run sampled at the same phase, the phase kept across the change (`apps/cli/tests/check.rs`); the fixture Column's `column.blend.json` from Dart (duration 2 → 1.5 → 1 along the parameter with the phase kept, the hips halfway, a property only one clip keys taken as it is, an out-of-range parameter reported raw with the duration clamped, the dominant clip's markers; `packages/fynkel/test/animation_test.dart`); the reference game's Player on `player.blend.json` (idle at 0, walk at 4, run at 7 by `speed` = the controller's planar velocity — it no longer switches clips), the jump played once over it and crossfaded back on landing, the `wave` action (F, the gamepad's east button, the overlay) playing an additive wave layer on the `spine` joint once, the walk's and the run's `step_l`/`step_r` markers (authored by `tools/character.py` as glTF `extras.markers`) counted into the HUD's `steps` until audio (item 11) exists; the user's Mixamo hero through `playground.py --hero` on the same blend space (`anim/idle|walk|run`), its wave layer on the `Rig` entity and its rig document's `markers` — validate, headless, the five Dart tests, the Playground on macOS — the box character stays the committed default; the 200-rig capture `benchmarks/m3/results/2026-09-21-570c9c1`: 450 animators + skins in 0.34 ms a tick, 10 975 markers over 1 800 ticks, p50 tick 3.54 ms settled, the standalone at 122 frames/s with 183 skinned instances in view, every gate line green. **Part 1** (commit 1 of 2: (b) layers, (c) markers, (d) root motion, the whole wire change). Resolutions: **layers are not `Animator.layers[]`** — a layer is a second `fynkel.Animator` on a descendant entity (the animators of a tree run ancestors first; the child's `weight` 0…1 blends its pose over what its ancestors left, `additive: true` adds the clip's change from its first frame; the "mask" is the entity the layer sits on — its clip keys the nodes under it — so a layer has a transform, a name, a stable id, its own state, markers and events and the editor's tree shows it, with nothing new on the wire; `animation.weight =` / `SetWeight` from Dart, `AnimatorState.weight`); markers are `Clip.markers` (a rig document's `markers`, sorted and merged at cook with `D0502` for one past the clip's end; a glTF animation's `extras.markers`, importer v5, `A0104` when malformed; the `ANIM` payload's trailing marker section, cache v5, FYNI v4) crossed per tick on the unwrapped clock — once per crossing, every lap, backwards, never on a fresh tick nor on a crossfade's outgoing clip — as `fynkel.ev.AnimationMarker` (kind 10), `Behaviour.onAnimationMarker`, counted by the headless host (`animation.markers`) and `rig inspect`; root motion takes the shallowest translated target under the animator as the root, measures its planar delta and Y-twist yaw against the clip's first frame in the root's parent space (laps add the clip's travel), converts through the local matrices down from the animator, and hands it to a `CharacterController` on the same entity as a velocity into the tick's `Move` (`Physics::add_root_motion`; yaw reported, not applied — the controller faces its motion) or moves and turns the transform otherwise, the pose keeping the root at its first-frame x/z with the yaw removed; `AnimatorState` gains `weight, parameter, root_delta, root_yaw`, `fynkel.cmd.Animate` `value` + `name` with the ops `SetWeight` and `SetParameter` (the parameter is stored per animator, read by the blend space of part 2); `fynkel.Animator` gains `weight`, `additive`, `root_motion` — scene/prefab **formatVersion 3** with `scene|prefab/2-3/animator-layers`; fingerprint `0x68267f95a40bac58`. Acceptance of part 1 on content from outside: the Khronos Fox's `run` with rig-document markers through `rig inspect` (3 counted), cook (the late one dropped with `D0502`) and headless (2 `ANIMATION_MARKER` in a second); the fixture Column's `wave` markers reaching `onAnimationMarker` from Dart across a loop and backwards. (Original text: added 2026-09-21; runs after 19b, before 11 so audio gets footsteps from markers instead of timers): what a character needs beyond a crossfade, on the M3-19 core (`Pose` already blends per channel). (a) **Blend spaces** as data — `<name>.blend.json` `{ parameter: "speed", points: [{ at: 0, clip: idle }, { at: 1.4, clip: walk }, { at: 4, clip: run }], sync: "phase" }` (1D; 2D is M5): an `Animator` plays a blend document like a clip; the world blends the two neighbouring clips by the parameter, **phase-synced** (the clips advance by normalized phase so feet match — the sync group is the blend), and `animation.setParameter('speed', v)` from Dart (`fynkel.cmd.Animate` op `SetParameter`, name in the tail) — the reference game's Player sets it from the controller's planar speed and stops switching clips itself. (b) **Layers** — the `Animator` gains `layers: [{ clip, weight, mask: <bone name>, additive: false }]` (bounded, 4): each layer's pose applies to the subtree under `mask` (the joint and its descendants, by name — the runtime resolves it like a skin's root) blended by `weight` over the base, additive layers add the clip's delta from its first frame; `animation.layer(1).play(wave)`/`.weight = …` from Dart; `AnimatorState` reports the layers. (c) **Clip markers** — in the rig document (`markers: { walk: [{ at: 0.28, name: "footstep" }, { at: 0.78, name: "footstep" }] }`) or on a glTF clip via `extras`; the world raises `fynkel.ev.AnimationMarker { entity, clip, name }` when a clip's time crosses a marker (loops and negative speed included, once per crossing, the same tick), `Behaviour.onAnimationMarker(AnimationMarker)` — item 11's footsteps and item 15's pickup sound hang off these. (d) **Root motion, opt-in** — `Animator.rootMotion: true` takes the root joint's planar translation (and yaw) out of the pose and publishes the per-tick delta; a `CharacterController` on the same entity consumes it as the tick's motion (the clip drives distance, the controller keeps collision), otherwise the entity's transform moves; "In Place" clips remain the default and the documented recommendation. (e) **Pose cost** — the animation system's time in `systemTimes` per animator count; the stress scene gains 200 animated rigs (the box character) as the item's gate (p50 tick < 4 ms with everything else on). Acceptance on content from outside (rule above): the Khronos `Fox` blend Survey → Walk → Run by speed with synced feet; the user's Mixamo hero in the reference game on a blend space with an upper-body wave layer and footstep markers driving a HUD counter until audio exists; `ANIMATION_MARKER` counted headlessly; `docs/animation.md` rewritten around the four features.

**Execution order (revised 2026-09-20 after item 10, the user's call: scenes before polish).** Items 1–10 are done in their order. What remains runs **12 → 13 → 14 → 19 → 19a → 19b → 19c → 11 → 15 → 16 → 17 → 18** (revised again 2026-09-21: the user's call after the Mixamo test — finish the character pipeline and animation depth while the gaps are fresh, then audio can hang footsteps off clip markers): runtime scenes first (every game needs to load and unload), then the HUD models, then the reference game's world and loop (the first real test of the Dart game-logic API), then animation so its player is a character, then glTF prefab instancing (19a), FBX import with the rig document (19b) and animation depth — blend spaces, layers, markers, root motion — (19c), then audio (item 15's music, footsteps and pickup sound need it, and 19c's markers tell it when — audio is polish until a game wants a sound), then the reference game's menus/settings/results, instrumentation, public readiness, the gate report. The numbers stay as identifiers because the docs reference them.

Removed from M3: ~~skinning/animation~~ (pulled back in as item 19), `WorldCanvas`, spatial audio (M5); clustered lighting, LOD, occlusion, post-processing beyond tone mapping (§8.6); joints and CCD beyond a flag (only if the reference game needs them); editor panels of any kind (M4).

### 19.2e M4 issue backlog

Derived from the M4 "Build" list and gate (§16), §11 (authoring service, Editor V0, authoring vs play state, reload policy, data safety), `docs/design/editor-ux.md` (revisited here, as its header asks), ADR-0016 (architecture), ADR-0012 (the Inspector reads committed metadata), ADR-0017 (the CLI stays the agent interface) and the M2/M3 carry-over items D5 and D7. One commit per item, in this order; §17.2 applies — the authoring ABI before any panel, the panels that only *read* before the ones that *draw*, the Scene view before Play. Every item ends green on the same gate as M3 (`docs/status.md` §5) plus the editor's own `flutter test` and the acceptance below. Written 2026-09-21, at the start of M4, while M3 items 19b (part 2), 19c, 11, 15–18 are still open.

**M4 runs in parallel with the rest of M3** (the user's call, 2026-09-21; §17.2 names "full editor before document commands stabilize" as dangerous parallel work — the commands *are* stable since M2-14, the ABI over them is not, which is why item 1 comes first). The rules that keep two streams of work in one repository safe:

- *(Until 2026-09-23 — M3 closed, both streams on `main` since; the worktree and the `editor` branch removed.)* The editor is developed on branch **`editor`** in a separate git worktree (`../fynkelengine-editor`); M3 continues on `main`. The editor branch rebases onto `main` before every item's gate and merges into `main` when an item is done, so the two streams never diverge by more than one item.
- The editor lives in **new directories only**: `apps/editor/` (the Flutter desktop app), `crates/fynkel_authoring/` (the session layer over `fynkel_document`), `packages/fynkel_authoring/` (its Dart surface), `docs/editor.md`. An M4 item never edits `fynkel_import`, `fynkel_render`, `fynkel_physics`, the reference game or the showcase.
- **Shared-crate touches are separate small commits, merged to `main` first**, each announced to the other stream by name. The complete list M4 is allowed: `fynkel_document` (D7 pointer index; `DuplicateObject` and `SetDocumentField` commands; `Editor` over material documents), `fynkel_core` (`WorldConfig::edit_mode`), `fynkel_native` (the `authoring` feature and its entry points), `package:fynkel` (the editor channel sink, `@Expose` display hints), `package:fynkel_generator` (emitting those hints), `package:fynkel_flutter` (only if the viewport needs to survive re-parenting; **amended 2026-09-22 by item 6:** `FynkelViewportController.renderHandle`/`debugLines`/`readFrame`, and `platform/apple` for the additive viewport ABI 4 behind them — `fynkel_apple_viewport_debug_lines`, `fynkel_apple_viewport_read_frame`), `apps/cli` (`fynkel editor`). Anything else is a blueprint change, not a quiet edit.
- Never two streams with uncommitted edits in one file; the editor stream checks `git status` on `main` before touching a shared crate.

**Scope rules fixed here (cheaper to decide now than to migrate later):**

- **Where the authoring ABI lives.** ADR-0016 says the editor reaches `fynkel_document` "through the same C ABI as the runtime": the entry points are added to `fynkel_native` (which already links `fynkel_document`) behind a cargo feature **`authoring`** — on in the workspace and in the editor's build, off in the prebuilt runtime artifact games download (ADR-0015), so authoring code never enters a release game (§1.1 rule 6). They are declared in `include/fynkel_authoring.h` next to `fynkel.h`, prefixed `fynkel_authoring_*`, and follow every ABI v1 rule (NULL-safe, `catch_unwind`, output buffers valid until the next mutating call, JSON in the ADR-0014 diagnostic record). The Dart side is a new package **`package:fynkel_authoring`** (ffigen over that header, the same native asset as `package:fynkel`); games never depend on it.
- **One session, many documents.** The Rust crate **`fynkel_authoring`** (an rlib the CLI may link later) owns an `AuthoringSession`: the project (`Project`, `Lock`, the `SchemaSet` with the project's behaviour schemas), the `AssetDatabase` with incremental rescan, the committed behaviour metadata (`lib/generated/behaviours.g.json`, read as data), one `fynkel_document::authoring::Editor` per open document, and the edit-preview sync below. The editor UI holds no project truth (`editor-ux.md` §22.17); every panel is a view over the session, reached through the ABI.
- **Materials are documents too.** `Editor` accepts material documents; `SetDocumentField { field, value }` / `UnsetDocumentField` address a field of a document that has no entities (a material's `baseColor`, a scene's `environment`), with the same transaction, validation and undo rules. `DuplicateObject { id, name?, index? }` deep-copies a subtree with fresh ids (the Hierarchy's Duplicate, one undo step).
- **The edit-preview world is a real `World` in edit mode.** `WorldConfig::edit_mode` makes `step()` apply commands, propagate transforms and snapshot — and run *no* systems (no physics, no catalogue, no animation, no input); Behaviours never exist in the editor process. The session loads the open scene from a cook of the project (in-process, `fynkel_import`'s cooker — deterministic, milliseconds on a warm cache; cooking is not "running game code") and then keeps the world in step with the document by mapping each committed `Mutation` to world commands (`SetField` on `fynkel.Transform` → `SetTransform`, other components → `SetComponent`/`RemoveComponent`, create/delete/reparent → spawn/destroy/reparent, ids the document's own) and **falls back to a full reload** for anything it cannot map (prefab overrides, asset changes, a migration). Both paths must agree: a test compares the incremental world's `state dump` with a fresh load after every fixture transaction.
- **Preview during a drag, commit on release.** A gizmo drag or a numeric field drag writes world commands to the preview world every frame (visual only) and commits **one** transaction on release; the document never sees intermediate values, so undo is one step and no transaction is ever coalesced after the fact.
- **Picking is on the CPU, in Rust.** `fynkel_authoring::pick(ray)` tests the cooked bounding spheres, then the cooked triangles of the candidates (`MESH` payloads the session already holds) and returns authored ids by distance — deterministic and testable headless; no GPU id buffer, no dependency on colliders.
- **The editor runs no game code and no codegen, and it shells out to exactly three CLI commands:** `fynkel create` (the launcher's New project), `fynkel codegen --check --json` (the stale-metadata banner) and `fynkel run` (Play). Everything else is in-process through the session. The CLI binary is found next to the editor, then on `PATH`, then `FYNKEL_CLI`; a missing CLI is a diagnostic, not a crash.
- **Play is the project's real app in a separate process** (ADR-0016): `fynkel run` with `FYNKEL_EDITOR_CHANNEL=stdio`; `package:fynkel` then writes structured records as JSON lines on stdout (`{ "fynkel": "log" | "diagnostic" | "stats" | "state", ... }` — the §14.2 record for diagnostics, `FynkelFrameStats` for stats) that the editor's Console and Profiler read. No sockets, no shared memory in V0; the channel is one-directional (game → editor) except for the kill that Stop is. Runtime *inspection* (selecting runtime objects, "apply to source") is designed for on this channel and built after V0.
- **Undo is per document.** Each open tab is one `Editor` with its own history; ⌘Z/⇧⌘Z act on the focused document. Cross-document operations (Apply to prefab: a transaction on the prefab, then one on the scene) are two undo steps, prefab first, and say so.
- **Files the editor reads or writes directly** (everything else goes through the session): its own preferences (recent projects, layouts, IDE choice) in the platform's application-support directory; project-local editor state (open tabs, scene cameras, expanded nodes) in `.fynkel/editor.json` (already git-ignored by the template); the behaviour metadata; and autosave/recovery revisions under `.fynkel/recovery/` (§11.5 — never the source documents). Saves are atomic replacements.
- **Every editor command is a first-class command:** a stable id (`scene.duplicate`, `view.frameSelected`), a menu entry, a command-palette entry, a shortcut, and — where it mutates a document — exactly one authoring transaction. A feature is not done until all five exist and the transaction is tested.
- **Diagnostics namespace `FYN-T####`** for the authoring session and ABI (project could not open, metadata stale, CLI missing, recovery found, channel parse error); the editor UI itself emits none — it shows the session's.
- **Dependencies added in M4** (each recorded in `docs/toolchain.md`): the docking package chosen by item 2's spike (`docking` unless the spike fails it; `multi_split_view` is the fallback), `file_picker`/`desktop_drop` or their equivalents for open/drag-in, `ffigen` (already used) for the new header, `watcher` for file changes. Nothing on the runtime path.
- **Acceptance needs a hand that did not build it** (the M3-19a rule, carried into M4): from item 3 on, an item is not done until the user has driven it for a session on the reference game (`examples/reference_game`) and the fixture project, and the resulting list of surprises is closed or written into the item's done-note. The gate item (10) is a full visual rebuild of a part of the reference level by the user, not by the implementer.

1. **`fynkel_authoring` and the authoring ABI** — **done 2026-09-21** (M4-01; docs/documents.md § The authoring session and its ABI, docs/editor.md). As specified below with these notes: the session's Rust surface is `AuthoringSession::new()` + `open_project(dir)` (a session outlives its project, so the ABI has one handle per window) and every query is one `query(kind, args)` entry point (`project`, `documents`, `document`, `inspect`, `validate`, `validateProject`, `assets`, `asset`, `dependents`, `schemas`, `schema`, `metadata`, `commands`) rather than a function per read; a relative `createMaterial`/`saveDocument` path inside a transaction is the project's, and what a committed transaction wrote is rescanned at once; the document's `name` is read live from the root so `setDocumentField /name` shows before a save; the Dart package notifies through a broadcast `Stream<AuthoringEvent>` (per-document `changes`) instead of `ChangeNotifier`s — it is Flutter-independent like `package:fynkel`, the notifiers belong to the app (item 2). The runtime rule is enforced mechanically: `fynkel_native` (and `platform/apple`) build the entry points only with the cargo feature `authoring`, `package:fynkel`'s hook always passes `--no-default-features` and adds the feature only for a pubspec that lists it under `hooks.user_defines.fynkel.features` (the editor's package does, a game's never), and CI fails if a `--no-default-features` build exports a `fynkel_authoring_*` symbol. Gate: every `fynkel scene|prefab apply` CLI test unchanged (356 workspace tests green); `packages/fynkel_authoring/test/session_test.dart` drives the complete session over the ABI on a copy of the fixture project (13 transactions covering all seventeen commands, undo to the original bytes, save, save-as, the `FYN-E0001` conflict, the rusted material's `base_color`, `duplicateObject` of the rig's six entities, reload, close, dispose, every `FYN-T` error); D7's numbers above; zero authoring symbols in the game runtime. Original text: (D5, D7; shared-crate touches: `fynkel_document`, `fynkel_native`): `crates/fynkel_authoring` with `AuthoringSession::open(dir)` (project, lock check → read-only when newer, schemas, asset database, metadata), `open_document(path|id)` → a handle over `Editor`, `apply(handle, request_json)` → `Applied` JSON, `undo`/`redo`/`save`/`revision`/`is_dirty`/`close`, `validate_project` → the `ProjectReport`, asset queries (`entries`, `resolve`, `dependents`, `rescan(paths)`), `document_json(handle)` for panels to read, `component_schema(type_id)` and `behaviour_metadata()` for the Inspector; `fynkel_document` gains the D7 pointer index (`Node::pointer` through a per-object key index, invalidated on edit), `SetDocumentField`/`UnsetDocumentField`, `DuplicateObject`, and `Editor::open` over material documents; `fynkel_native` gains the `authoring` feature with `fynkel_authoring_*` entry points over the session (one buffer in, one out, JSON), `include/fynkel_authoring.h`; `packages/fynkel_authoring` with the ffigen bindings and a typed `AuthoringSession`/`DocumentHandle` Dart API (`Stream<Applied>` per document, `ChangeNotifier`s per panel concern). Gate: `fynkel scene|prefab apply` unchanged (byte-identical fixture outputs); a Dart test drives a complete edit session over the ABI on the fixture project (open, every §11.1 command, undo to the original bytes, save, conflict on a stale revision, a material's `baseColor`, `DuplicateObject` of the rig); the 16 000-entity / 8 000-warning document loads in under 100 ms (was 726 ms); apply+validate on the 10 000-entity scene stays linear; `cargo build -p fynkel_native --no-default-features` exports no `fynkel_authoring_*` symbol.
2. **The editor shell — `apps/editor`** — **done 2026-09-21** (M4-02; docs/editor.md § The shell; driven by the user on the fixture project: launcher, menus, ⌥⌘5, ⇧⌘P, Open Project). As specified below with these notes: the docking spike picked `docking` 1.16.2 (seven criteria, one widget test each, ADR-0016 § Resolutions) and only two files import it (`workspace.dart`, `main_window.dart`) so a later own docking system swaps behind `Workspace`; the menu bar is native on macOS (`PlatformMenuBar`, About/Preferences/Quit in the app menu, a menu left empty by that move is dropped) and in-window elsewhere and in tests; a lock is read-only when it names a newer runtime/ABI/tool version (the lock schema pins `formatVersion` to 1, so a v2 lock is a schema error, not read-only); the migration preview is a new session query `migrations` (own crate, no shared-crate touch) and writes nothing; the first live run found four bugs the fakes missed (an empty native menu group, the missing `files.user-selected` entitlement `file_picker` insists on, two layout overflows) — each now has a test. Gate: the fixture project and the reference game open and validate, a misspelt field is listed at `assets/scenes/main.scene.json:75:13` and opens in the IDE, a lock naming runtime 9.0.0 opens read-only with that reason, the layout survives a restart (`workspace_test.dart`), 23 commands each run from menu, shortcut and palette (`commands_test.dart`), widget tests for the launcher (11), palette (3) and console (4); 45 tests. Original text: the launcher (New project through `fynkel create` with name/title/org/directory, Open, Recent, the lock/version check with read-only mode and the migration preview through the session, `fynkel doctor`-style checks of Flutter/CLI presence); the main window with the §22.2 default workspace on the docking package chosen by a recorded spike (criteria: tabs, drag to re-dock, splits, hidden tabs keep state, a live `Texture` survives moving between tabs, layout serialisation, keyboard focus handling; the result and the pick written into ADR-0016's resolutions), layouts saved and restored per user with "Reset layout"; the menu bar and the command registry (stable ids, shortcuts, palette ⇧⌘P); the **Console** panel over the session's project diagnostics (severity, code, file, pointer/line/column, search, filter, collapse repeats, copy, click → opens the file's line in the IDE for now); preferences (IDE command, theme); `docs/editor.md` started. Gate: opens the fixture project and the reference game and lists their diagnostics with exact positions; a project whose lock is newer opens read-only with the reason; layout survives a restart; every command is reachable three ways (menu, palette, shortcut); widget tests for the launcher, the palette and the console; the docking spike report is in the ADR.
3. **Document tabs and the Hierarchy** — **done 2026-09-21** (M4-03; docs/editor.md § Document tabs and the Hierarchy; driven by the user on a copy of the fixture project and on the reference game's Playground: rename, create, save, `scene inspect --json`, the conflict path with the file edited from the terminal under a dirty tab, Save As beside it). As specified below with these notes: the tree is built once and patched from `affectedIds` through the `document` query's new `ids` form (the named entities + the ordered roots), never from the whole document; "Create with a native component" writes the component's Rust `Default` through the new `defaults` query so the editor and the runtime agree on what an empty component is; the conflict dialog's third revision is the new `revision` query (a file's revision on disk); a **scene's roots follow entity key order** (a scene has no `roots` list; the crate ignores `index` at a scene's root), so the Hierarchy offers no insertion line between scene roots and a duplicated scene root lands last — only a prefab orders its roots; ⌘Z/⇧⌘Z/⌘⌫ inside a text field act on the field (the command forwards a text `Intent`); the three queries live in `fynkel_authoring`, no shared crate touched. The user's gate found three things the fakes missed, each fixed with a test: Save As resolved an `assets/`-relative path against the project root (`FYN-E0011` in a folder that did not exist — the field is project-relative now and a new sub-folder is created), the tab kept showing only the name inside the file after a Save As (it shows "Main · main-mine" when name and file disagree), and the placeholder panels overflowed when squeezed (they scroll). Lesson for every later gate: drive it on a **copy** of the fixture project — the editor writes real files and the first save landed in the committed fixture (restored from git). Gate: every Hierarchy operation is one transaction visible to `scene inspect --json` after save (`documents_session_test.dart` on the fixture's Main scene: rename, create empty, create with PointLight, reparent with the world transform kept, duplicate, delete, undo/redo, save, inspect, reopen); a generated 10 000-entity scene opens in ~260 ms and edits in ~300 ms without rebuilding the tree and the panel builds under 60 rows; the conflict path through the real watcher (badge, refused save, Save As beside the file, the other writer's bytes untouched, a clean tab's silent reload); 86 tests in `apps/editor`, `fynkel_authoring` 11 Rust + 6 Dart, the runtime dylib exports no `fynkel_authoring_*` symbol.
   As specified: open a scene or prefab from the Console/launcher into a tab (dirty dot, ⌘S save through the session, close with a save prompt, ⌘Z/⇧⌘Z per tab, revert to saved); the Hierarchy panel as a virtualised tree of the focused document (`id`, `name`, `parent`, `children` from `document_json`, rebuilt from `Applied.affectedIds` only): select/multi-select/range, rename in place, Duplicate, Delete, Create empty / with a native component, drag-and-drop reparent with an explicit index and the "keep world transform" policy (a `SetField` on the transform in the same transaction), search and filter (name, component type, prefab state, has-error), prefab-instance badges, error badges from the document's diagnostics; the file watcher: an external change to an open document with no local edits reloads it silently, one with local edits shows the conflict with the revisions and a diff (the `Editor`'s `FYN-E0001` path, never an overwrite). Gate: every Hierarchy operation is one transaction visible to `fynkel scene inspect --json` after save; a 10 000-entity scene scrolls and selects without rebuilding the tree; the conflict path tested by editing the file from the CLI while the tab is dirty; the user's session on the reference game's Playground closed.
4. **The Inspector** — **done 2026-09-21** (M4-04; docs/editor.md § The Inspector; driven by the user in two rounds on a copy of the fixture project and a copy of the reference game: every field kind edited and saved with only the edits in the diff, a drag undone in one ⌘Z, `-1` in Spinner speed refused with the message and the value back, the asset picker and Clear, Add Component with the held type greyed out then PointLight added and removed, two entities with a mixed field, a prefab override reverted one and all, Player's behaviour fields with units and groups and Open Script into the IDE, `behaviours.g.json` broken by hand → the banner on ⇧⌘G with the fields still editable → restored → Check again). As specified below with these notes: **the hints for native components come from an editor-owned table** (`apps/editor/lib/src/inspector/native_hints.dart`, read underneath the schema's `x-fynkel`) rather than Rust `#[fynkel(...)]` attributes — `fynkel_schema_derive` and the component structs are the M3 stream's, not on the editor's shared-touch list; ADR-0018 is accepted with that note, the table only shrinks and a test fails when it names a field no schema has; the behaviour half landed as decided (`@Expose(group, order, header, unit, kind, readOnly, hidden, showIf)`, `@FynkelBehaviour(icon, category, description)`, emitted by the generator into `x-fynkel` and the metadata's `fields[]`, an unhinted class unchanged, `kind` checked against the field's type and `showIf` against a sibling boolean; the fixture's `game.Spinner` schema is the example's regenerated one); `fynkel codegen --check --json` is the CLI's new answer for the banner, run once when a project with metadata opens and on ⇧⌘G, never on a timer, next to the instant *committed drift* check (metadata schemas vs the files); validation is twofold — a precheck against the spec before any command (bounds, integrality, enum, vector length, an unknown asset `FYN-A0004`, since `apply` does not resolve assets) and the session's own refusal — and a rejection shows the crate's `related` reasons, not the `FYN-E0005` summary; components are paired **by type and position** across a multi-selection (a slot shows when every entity has it); numbers keep their JSON form (`1` stays an `int`) so the round trip is byte-exact; drag-in from the Project browser has its target but waits for item 5's drag, per-field bold inside an expanded prefab instance is item 7's. The user's first round found three things the fakes missed, each fixed with a test: a second Transform was offered (the document holds one component per type, `FYN-D0204` — the row is greyed out now and the model refuses before the crate), the refusal showed the bare summary (the related reasons are printed, for the tabs and the Inspector alike), and that message never cleared (it goes with the selection, the next accepted transaction from anywhere, or a ×); plus two display things — degrees from a float32 radian read `45.000002` (rounded to three decimals, the stored value untouched) and narrow axis fields clipped `20 m` (no suffix there; the row's label carries the unit). The rebase onto M3-11 brought the two audio components: the `defaults` query and the hint table learnt them, an audio source is offered whole as a `sound`. 116 editor tests (inspector 26 + 4 on the real session — every field of the fixture's two scenes and the reference game's 168-entity playground set to itself saves byte-identically), `fynkel_generator` 6 + example 4, `fynkel_cli` check 16. Originally: (shared-crate touches: `package:fynkel`, `package:fynkel_generator` for the display hints of ADR-0018): generated from the component schema for native components and from the committed metadata for behaviours — one widget per field kind (`number` with drag and `min`/`max`/`step`, `integer`, `boolean`, `string`/`multiline`, enum menus, `vec2`/`vec3`/`vec4`, rotation as Euler degrees over the stored quaternion, colour with a picker, `AssetRef` with a searchable picker and drag-in from the Project browser, entity references, arrays and nested objects), `label`/`tooltip`/`group`/`order`/`unit`/`readOnly` hints, the "run `fynkel codegen`" banner from `codegen --check`, Add Component (searchable over the project's component registry and the behaviours; a behaviour's defaults from its schema), Remove Component, multi-object editing with mixed-value display and one transaction over all selected, inline validation from `Applied.diagnostics` (the value returns to the last valid one, the message stays until fixed), the entity's prefab-override state per field (bold + revert, item 7 completes it), "Open script" on a behaviour (`library` from the metadata) in the configured IDE. Gate: every field type in the fixture project and the reference game is editable and round-trips byte-exactly through save; a rejected value (a `min` violation, an unknown asset) shows its diagnostic and leaves the document untouched; a drag on a number is one undo step; stale metadata shows the banner and the Inspector still shows the last committed fields.
5. **The Project browser** — **done 2026-09-21** (M4-05; docs/editor.md § The Project browser; driven by the user on a copy of the fixture project and a copy of the reference game: a texture a material uses renamed and moved with `fynkel validate` clean after each step and the same id in the moved sidecar, its delete refused with the material listed then **Delete anyway** and ⌥⌘Z bringing it back from `.fynkel/trash`, New Material / Scene / Prefab / Folder with inline rename, a colour space applied and reimported, a prefab and `robot.glb#prefab` dropped onto the Hierarchy and a texture onto a material field, Reveal in Finder, a file added from Finder picked up by the watcher). As specified below with these notes: every change is a **session operation** — the new `operate(kind, args)` next to the per-document transactions (`newDocument`, `renameAsset` moving the sidecar so the id survives and relocating an open editor, `deleteAsset` refused with one `FYN-A0004` per referencing field unless forced and then a move into `.fynkel/trash/<id>/`, never `rm`, `setImportSettings`, `reimport` running the importer in memory and refreshing `subAssets` with existing ids kept, `undoAsset`/`redoAsset` over an **asset history of its own**, ⌥⌘Z, because a document's ⌘Z must never move a file) with `assetDetail`/`assetHistory` queries, `fynkel_authoring_operate` as ABI **2** (a client built against 1 still loads); the shared touches are `AssetDatabase::update` (re-read only the touched paths: a generated 2 000-asset tree scans whole in tens of ms and one change well under a millisecond, asserted in `crates/fynkel_authoring/tests/ops.rs`) and `Editor::relocate` in `fynkel_document`; an **image is one whole `texture` asset** with no sub-assets — a material's texture slot names the png's own id — which also lets the Inspector's picker offer textures (an item 4 gap); folders are file-system work through the session with the same undo; the Asset panel shows a material's fields read-only (a material field editor is not item 5's; the session already supports it); the drop onto the Scene view waits for item 6 since there is no Scene view yet; a Flutter finding worth keeping — a translucent `GestureDetector` whose semantics node sits above a `ListView` losing rows while a popup route pops trips `'node.built': is not true`, so the panel's background detector is `excludeFromSemantics`. 140 editor tests (24 new, 6 on the real session over a copy of the fixture), `fynkel_authoring` ops tests, `fynkel_native` ABI 2 test. Originally: the asset database as folders and as a searchable flat list with type filters and sort; previews (decoded textures, material swatches from their factors, an icon per other kind — offscreen mesh thumbnails wait for item 6's world); the selected asset's Inspector (the sidecar's import settings, editable through the session; reimport; the source's sub-assets; dependents); Create Material (`CreateMaterial`) and New scene / New prefab (a `Document::new` written through the session); rename and move with the sidecar moved alongside so the id survives (a session operation with its own undo), delete refused while referenced (the dependents listed) unless forced; drag an asset onto an Inspector field, a prefab or an imported `#prefab` onto the Hierarchy or the Scene view (`InstantiatePrefab`); Reveal in Finder; the watcher rescans changed paths and the panel updates without a full scan. Gate: every operation leaves `fynkel validate` clean and the id stable across a rename (the CLI resolves the old reference); a project of 2 000 assets scans incrementally under 50 ms per change; the user's session closed.
6. **The Scene view** — **done 2026-09-22** (M4-06; docs/editor.md § The Scene view, docs/documents.md § The edit-preview world; driven by the user on the fixture project and the reference game's Playground — the steps at the end of docs/editor.md: camera, picking and cycling, the three gizmos each one ⌘Z, hidden/locked with only `.fynkel/editor.json` changing, a prefab dropped, the frame through the scene camera compared with `fynkel render`, 60 s idle with the tick still — all passed, no surprises; the gate's edits to the reference game were reverted before the commit, kept as a patch). As specified below with these notes: **the sync is a diff, not a per-`Mutation` mapping** — `fynkel_authoring::Preview::sync` diffs the edit-mode world against the description the document has *now* (`describe_with_anchors`: the runtime's description plus the map of every expanded entity to its prefab anchor) and turns only the differences into the world's own commands over as many steps as the new hierarchy is deep, so undo, redo, prefab overrides, duplicates, a prefab edited in another tab and a migration all take the same path with no special case, and a round that makes no progress falls back to the full reload the rule asks for (`SyncReport.reloaded`); the scope rule's test exists as written (`tests/preview.rs`: `Preview::dump` of the incremental world equals a fresh load after every transaction kind; the real-session test repeats it through the ABI). **Two shared touches outside the §19.2e list, recorded here as the amendment:** `platform/apple` (viewport ABI **4**, additive — `fynkel_apple_viewport_debug_lines` queues world-space segments for the renderer's existing debug pass and is cleared by each render; `fynkel_apple_viewport_read_frame` copies a rendered ring slot as RGBA8 for Save Frame — the pixel gate needs the frame the viewport drew, not a second renderer) and `package:fynkel_flutter` (`FynkelViewportController.renderHandle(worldAddress)` — the preview world is owned by `fynkel_native`, not by a Dart `World` —, `debugLines`, `readFrame`; a game never calls them). The authoring ABI is **3** (`fynkel_authoring_preview_open/_close/_world/_call`, the `"previews"` reports on every call that syncs; `fynkel_core::WorldConfig::edit_mode` is the listed touch, its own shared commit `6aec259` — a `step()` applies commands, propagates transforms and runs no system, plus `set_edit_camera` and `set_edit_hidden` for the extract). The world's asset store is the preview's own — the first preview cooks the project in process (`OpenProject::cook`, the CLI's cooker) and **Rebuild cache** (⌃⇧⌘R) cooks again on request and whenever the project's assets change. The camera through the scene camera is exact (the entity's world matrix, lens and exposure are read, not approximated) so the Scene view's frame **is** `fynkel render`'s: the fixture's Main at 320×200 differs in 0 pixels; the pixel gate on the Playground is the user's, with `Save Frame…` and `fynkel render --scene <id> --size WxH`. Not done here, by decision: the Inspector's numeric drag still commits once without previewing in the world (the preview-then-commit rule is the gizmo's; the Inspector's drag was one transaction already); mesh thumbnails wait for M5. Found and fixed on the way: `matInverseAffine` (item 3) returned the cofactor matrix un-transposed, so keep-world-transform under a rotated parent was wrong (a test with a 90° parent now); `Ray.hitPlane` (new here) treated a ray starting on the plane as a hit until the tests caught it. To report to the M3 stream (fynkel_render is not the editor's to touch): under MSAA 4 a one-pixel debug line is partially covered and the post pass reads its alpha as scene-referred, so thin lines darken with exposure — the test asserts coverage, not colour. The fixture's Robot has no `fynkel.Transform`, which is why the status line explains a missing gizmo instead of guessing. 194 editor tests (+29: scene_view 25 over fakes, scene_session 4 over the real world and the real Metal viewport), `fynkel_authoring` preview tests, `fynkel_native` ABI 3 tests, `platform/apple` line/read-back tests, `fynkel_flutter` controller tests. Originally: (shared-crate touch: `fynkel_core::WorldConfig::edit_mode`): a `FynkelViewport` per scene tab over the session's edit-preview world (the incremental sync rule, the fallback reload, a "Reload" command); the editor camera (orbit/pan/dolly/fly, focus selected (F), frame all, perspective/orthographic, saved per scene in `.fynkel/editor.json`, never a game camera); grid and origin axes and light/camera/collider outlines through the renderer's debug lines; click and marquee selection through `pick` (bounding-sphere then triangle, cycle through stacked hits on repeated clicks); selection outline as debug bounds; **move/rotate/scale gizmos** in local/world space with snapping (grid, angle, scale steps), one transaction per drag with the preview-then-commit rule, the same for a multi-selection around its pivot; hidden/locked flags (editor-only, in the editor state file, never in the document); the Scene view's own stats overlay (frame time, draws, entities). Gate: a headless test picks every entity of the fixture scenes by a ray through its centre; a gizmo drag is one undo step and the world matches a fresh load after it (the sync test of the scope rule); the Scene view of the reference game's Playground renders pixel-identical to `fynkel render` of the same scene from the same camera; nothing ticks — 60 s open on the Playground leaves every rigid body where the document put it; the user's session closed.
7. **Prefab mode** — **done 2026-09-22** (M4-07; docs/editor.md § Prefab mode, docs/documents.md § The authoring session — `instance`, `open_document("<id>#prefab")`; gate driven by the user on copies of the fixture project and the reference game's Playground — the steps at the end of docs/editor.md; one finding, the gizmo missing on a derived row, fixed and tested the same day). As specified below with these notes: **one shared touch in `fynkel_document`** — the new authoring command `unpackPrefab` and `Editor::apply_with_prefabs(request, &schemas, &PrefabLibrary)` (the plain `apply` passes an empty library; only `unpackPrefab` reads it), the unpacked entities keep the **derived ids** the runtime already used so a running preview world does not reload; **`apps/cli` `scene|prefab apply` gained `--prefabs <dir>`** (else the document's directory) so a patch that unpacks reads prefabs the way `inspect --expand` does — a deviation from the item text, which named no CLI change. **Override fields are whole keys** of a component's `data` (the crate's `setPrefabOverride` replaces the override with the same target), so an Inspector edit of `position.x` on a derived entity overrides `position`; the item's "bold" is per component field on that basis. **Apply to prefab** is exactly the two-step rule (one transaction on the prefab document — an `Applied` of its own commands, `createObject`/`deleteObject` for `addObject`/`removeObject` — then one `revertPrefabOverride` on the instance; the Dart helper `prefab_actions.dart` is shared by the Inspector, the Hierarchy and the commands). Imported hierarchies open through `open_document("<id>#prefab")` as a **read-only prefab document** (`state.sub`, `readOnly` naming the source) built from the source's `prefab` chunk or the rig's skeleton — the session's `PrefabLibrary` with open editors first — so a rig document's hierarchy works the same way; the Scene view's gizmo on a derived row (selected in the Hierarchy; a pick in the picture still answers the anchor, item 6) commits `setField` overrides on the anchor — the gate's one finding, added the same day. The `unpackPrefab` mention for `examples/reference_game/AGENTS.md` and `examples/showcase/AGENTS.md` command lists is left to the M3 stream (the editor stream does not edit them). Not done, by decision: a visual prefab-vs-instance diff and cross-level apply for nested instances (a nested instance applies to its own prefab). Specification: open a prefab document in its own tab with its own edit-preview world and Hierarchy (isolation), edit it with everything above; in a scene, prefab instances show inherited vs overridden fields (bold), Revert override (`RevertPrefabOverride`), **Apply to prefab** (a `SetField` transaction on the prefab document, then `RevertPrefabOverride` on the instance; two undo steps, the rule above), Unpack (an explicit command that copies the expansion into the scene as plain entities, one transaction); imported hierarchies (`<glb>#prefab`) open read-only with their derived `node/<i>` ids and accept overrides; a missing or broken source prefab shows as such with the diagnostic and keeps the instance's data; "Select prefab asset". Gate: apply/revert/unpack on the fixture's `coin` and `Imported` rig round-trip through `fynkel scene inspect`; a broken prefab reference opens, is shown, is not rewritten; the user's session closed. **Amended 2026-09-24 by ADR-0022** (after the M4 gate rebuild showed `ball › Ball › Danger`): a prefab has one root and the instance is that root — the Hierarchy row, Inspector and gizmo of an instance are the root's, its placement a `setField` override on the root's Transform, `World.instantiate` returns the root; scenes and prefabs formatVersion 5 with the cross-document `fynkel migrate` step (docs/documents.md § Migrations).
8. **Play / Stop and the game Console** — **done 2026-09-22** (M4-08; docs/editor.md § Play and the game Console, docs/hosts.md § The editor channel; gate driven by the user on a copy of the reference game — the steps at the end of docs/editor.md — with five findings fixed the same day, four of them the gate's own: `flutter run` was given no device and refused to choose among the five connected (now `-d macos`, and a run that dies before the game starts says *Did not start (code N)* and shows the process's own lines unasked); a Console row named a document the way the **cooked** project does, so nothing opened (`_recordDocument` tries the game's spelling then the project path); the Scene view blinked while the window was resized (the shared `FynkelViewport.resizeDebounce`, a refused frame re-requested); the banner's **Play again** did nothing, because ▶ refuses while a run is up (its own `replayProject` stops the tree first); and the banner outlived the run it was about, while a Stop read as *Exited with code -15* rather than *Stopped*). As specified below with these notes: **no `apps/cli` change was needed** — Dart's `Process.start` merges the parent environment, so `FYNKEL_EDITOR_CHANNEL` reaches the game through `fynkel run` and `flutter run` on its own; the channel is **one-directional** (ADR-0016) — Stop is a signal, never stdin; `package:fynkel_flutter` counts as the channel's stats listener whether or not the game passes `onStats`; the process tree is read from `ps` **before** the first signal, since a signalled parent reparents its children; the status line names the **game's** pid, the one a `kill -9` needs, not the CLI's. Not done, by decision: choosing a device from the editor, attaching to a game the editor did not start, and hot-reload — the banner says the change applies on the next Play. Specification: the toolbar's ▶ / ■ / state (starting, running, exited with code), Play saves dirty documents (prompt) then runs `fynkel run --project <dir>` with `FYNKEL_EDITOR_CHANNEL=stdio` and streams the process; `package:fynkel` emits the JSON-lines records when the variable is set (`Diagnostics` records, `print` through the log record, `FynkelGameView.onStats` through the stats record) and nothing otherwise; the Console gains a Game tab (tick, severity, source, entity/asset ids → select the authored entity when the id maps; filters and collapse as before); Stop kills the process tree; a crash or a `kill -9` shows the exit code and leaves every document byte-identical (test); editing during Play is allowed and marked ("changes apply on next Play"). Gate: the reference game plays from the editor on macOS and its diagnostics appear in the Console within one frame; the kill test; a project with errors refuses to Play and points at the Console.
9. **Profiler, IDE navigation, `fynkel editor`, polish** (shared-crate touch: `apps/cli`): the Profiler panel over the stats records (frame time percentiles, fixed ticks, missed deadlines, bridge bytes, the renderer's counts; the edit-preview world's own stats in edit mode); Console and Inspector navigation to `file:line` in the configured IDE (VS Code, IntelliJ/Android Studio, Cursor, a custom command); `fynkel editor [dir]` launches the app on a project; the accessibility pass (focus order, semantics labels, keyboard-only walkthrough of every panel), every string in one `l10n` table; `docs/editor.md` complete (panels, commands table with ids and shortcuts, files the editor owns, the channel format). Gate: the keyboard-only walkthrough recorded as a widget test; `fynkel editor .` on the reference game opens it in under 3 s warm.
9a. **The editor's look** — **done 2026-09-22** (docs/editor.md § The look; gate driven by the user on a copy of the reference game). As specified below with these notes: the tokens, the theme and the panel chrome are `apps/editor/lib/src/theme/` (`tokens.dart`, `theme.dart`, `chrome.dart`, `icons.dart`); the row height is the **Density** preference (Compact 18 / Default 20 / Comfortable 24), every row, field and toolbar derived from it; the icon font is `lucide_icons_flutter` 3.1.20 (docs/toolchain.md) under the names the panels already used, so `@Expose(icon: …)` short names keep working and `Icons.` appears nowhere in `lib/`. Checking the item against its own text before the gate added three things it promised and the first build did not do: **the keyboard focus ring** — one layer over the whole app (`focus_ring.dart`) that outlines what the keyboard reached, the way a browser's `:focus-visible` does (not after a click, not for a key that moves no focus, not on a text field, clipped to the scroll views around it); **the status strip** — the seven snackbars and every other one-off message (17 call sites) go through one `Notices` model shown in the status bar (the launcher's own strip before a project is open) and stay until dismissed or replaced; **no animation** — besides the spec's list, 33 dialogs and menus, the About box, the Console's tab underline, expansion tiles and the dark ↔ light switch were animated and now are not (`AnimationStyle.noAnimation`; `look_test.dart` fails on any `showDialog`/`showMenu`/`PopupMenuButton` without it and on a `SnackBar`). The gate's one bug: `Fynkel Editor <dir>` opened the project before the toolchain was detected, so the Inspector's codegen check ran without a CLI and said none was configured — the project now waits for the tools (test). The gate's judgements — numbers far from their labels, the project line above the panels, a toolbar taking the viewport's height, the Console's toggles as solid blue blocks, an Asset panel beside the Inspector, five different "which one?" dialogs — became items 9b, 9c and 9d. Friction's missing slider (its hint has a `min` and no `max`, so it is a plain number) is unchanged, the user's call. 250 editor tests (+8: `look_test.dart` 7, the startup order 1). Specification: (added 2026-09-22 at the user's request, after item 8's gate; runs **before** item 9 so the Profiler is built in the language rather than restyled into it; `apps/editor` only — no shared touch, nothing the other stream can collide with): the editor stops looking like a Material demo and starts looking like a tool. The target, in the user's references: **Unity's density, Rider's structure, Framer/Spline's finish**. One design language written down as tokens and applied from one place — today the whole 48-file UI holds seven hard-coded colours, so this is a theme layer, not a rewrite. (a) `lib/src/theme/`: the tokens as a `ThemeExtension` (surfaces window / panel / header / recessed field / popup, one hairline, three inks, one accent `#3E7BFA` used only for selection, focus and active state, diagnostics error / warning / ok kept *separate* from the accent) with dark and light palettes designed as two real sets, plus `ThemeData` whose component themes are all explicit — buttons, fields, checkboxes, menus, tooltips, banners, dialogs — so no Material default leaks through. Geometry: rows 18 / 20 / 24 px (a preference, 20 the default), toolbars 28, tabs 26, fields 20, icons 16, radius 3 on controls and 4 on panels, 4 px gutters, dividers always 1 px. Type: the platform UI face at 11.5 / 11 / 12.5 and the platform monospace at 11 for ids, paths and numbers. Motion: 90 ms hover and press, and nothing else — no ripples, no page transitions, no tab slide; a snackbar becomes a status strip. (b) The panel chrome — `tabbed_view` and `multi_split_view` themed from the same tokens instead of their stock presets. (c) The panel pass: row heights, the Inspector's label-and-fields grid, banners as thin strips, chips as segmented controls, one icon size. (d) The icon set: a Lucide-style 16 px stroke font in place of Material Symbols. Gate, driven by the user: open the editor on a copy of the reference game and judge it beside Unity and Rider — density, contrast and the accent in dark **and** light, the keyboard focus ring visible everywhere, no animation but hover, and every panel's text legible at the default row height. Not in 9a: a hand-drawn widget toolkit, a pixel copy of Unity's skin, custom-painted panel headers, or any behaviour change — if a gesture works differently afterwards, that is a bug in this item.
9b. **Readability and chrome** — **done 2026-09-23** (built 2026-09-22; gate driven by the user 2026-09-23 on a copy of the reference game — passed) (docs/editor.md § The title bar, § The Scene view's overlay, § Reading the Inspector). As specified below with these notes: **the title bar** — on macOS the window keeps a transparent title bar with its title hidden and full-size content, plus an empty `NSToolbar` in the `.unifiedCompact` style so the bar is 38 pt with the traffic lights centred in it; the window is **not movable by its background** (`isMovable = false`: otherwise AppKit drags the window from a press on any Flutter button in the bar), and the bar's empty space asks for a drag and a double-click over the `fynkel.editor/window` channel (`startDrag` → `performDrag(with:)`, `titleBarDoubleClick` honouring the system's *double-click a window's title bar to* setting; full screen drops the traffic-light inset). The project menu also lists up to five **recent projects** (a missing one disabled), and **Close Project** moved into it; the codegen state is quiet when the code is current (an icon), says *Run codegen* when stale or drifted and *Codegen check failed* when the check failed, hides when there is nothing to say, and a click checks again. **The Scene view's overlay** is a `Wrap` of frosted strips (blur 12 over the new `overlay` / `overlayEdge` tokens), not a scroll view, so the space between the groups passes clicks and scrolls to the picture; a narrow panel wraps the right side under the left rather than hiding any of it; the stats sit under the toolbar, and the bottom holds the notice, the camera-and-tool readout (the navigation keys in its tooltip) and the hidden-count chip. **The accent follows focus** through a `PanelFocus` around every panel; a toggle that is on is a soft tint of the accent (`accentSoft`; over the picture the accent at 22 %), the Console's filters included. Keys changed: `window.close` is inside the project menu (`window.project`), new `window.titleBar`, `window.drag`, `window.projectName`, `window.openProject`, `window.recent.<i>`, `window.codegen`, `window.commands`; the Project list's `project.columns`, `project.column.name` / `.kind`, `project.contents.<id>`, `project.size.<id>`; `number.unit`; the status bar lost its *⇧⌘P commands* text (the command field is in the bar). The double-click zoom and the overlay over a real scene are checked by hand in the gate; the tests cover the bar through a mocked channel (`title_bar_test.dart`, 4). Specification: (added 2026-09-22 at the user's request after 9a's gate, from the mockup "Fynkel Editor Second Pass" the user approved; `apps/editor` only): 9a gave the editor one language; 9b makes it read like a tool. (a) **One top bar**: the project line above the panels goes; the window's title bar holds the project (name, path in the tooltip, a menu to switch, reveal or close it), the open documents as tabs (close button on the active and hovered tab only, a dot when unsaved, **+** opens a document), Play / Stop centred, the codegen state and the command search on the right — on macOS the Flutter content runs under a transparent title bar (`apps/editor/macos`) so the traffic lights sit in it. (b) **The Scene view's toolbar floats over the picture** on translucent strips (tools and space on the left, view toggles and camera on the right, a status readout bottom left), so the viewport gets the panel's full height. (c) **The Inspector reads left to right**: one label column, values left-aligned right after it (the spreadsheet `alignRight` goes), the unit next to the number, the axis letter inside a vector field, a value never clipped; groups are small uppercase captions; a mask shows its layer names (`Everything`, `Nothing`, or the list), an empty value shows `—`. (d) **The accent follows focus**: a selected row is blue only in the panel that holds focus, a quiet tint elsewhere; a toggle that is on is tinted, not solid; panel-tab close and maximise buttons show on hover and on the active tab only. (e) The Project list in fixed columns (name, kind, contents, size), a selected row's secondary text legible. Gate, driven by the user: the same copy of the reference game, judged against the mockup. Not in 9b: any behaviour change.
9c. **One Inspector** — **done 2026-09-23** (built 2026-09-22; gate driven by the user 2026-09-23 on a copy of the reference game — passed) (docs/editor.md § One Inspector). As specified below with these notes: the selection is `EditorSelection` (`lib/src/shell/editor_selection.dart`) with three targets — the focused tab's entities, an asset or one of its parts (`InspectAsset(id, sub: 'mesh/0')`), a folder; coming back to a panel shows its selection again even when it did not change. **The pickers do not write it** (a deviation from (a)): choosing a texture for a material slot is not selecting the texture, and writing it would take the Inspector away from the field being edited. The pages are `AssetPage`, `SubAssetPage` and `FolderPage` (`asset_pages.dart`) built from one set of pieces (`inspector_chrome.dart`: header, facts row, uppercase captions, label/value rows on the fields' label column, and the **apply bar** pinned to the bottom while an import setting is changed — Revert, or Apply, which writes the sidecar through the session and reimports); "registered by kind" is `AssetDocumentSections.register(kind, builder)` — a document kind's sections on the asset page, the material's registered today, every other kind the generic summary — so a `@DataAsset` kind registers a builder (test). The Asset panel's file is gone and its panel id `import` is **retired** (`PanelRegistry.retired`): a saved layout that still holds it loads with it dropped; its Window menu item went with it (90 commands); the Project panel's *Show in Asset Panel* is *Show in Inspector*. **Open** is offered for scenes, prefabs and an imported model's prefab part, **not for materials and blend spaces** (a deviation from (b)): M4 has no material or blend-space editor (item 5), so their tab would be empty — the page shows their content read-only, and Open joins when an editor for them does. The lock (`inspector.lock`) pins entities in the model too, and an explicit *Show in Inspector* still moves a locked Inspector. Tests: `editor_selection_test.dart` 7, the pages and the kind registry in `project_browser_test.dart`. Specification: (added 2026-09-22 with 9b): the Inspector shows **whatever was selected last** — an entity, a project file, a part of one (`kit.glb › mesh/3`), a folder — the way Unity's does. (a) One editor-wide selection (`EditorSelection`): the Hierarchy, the Project panel and the pickers write it; the Hierarchy still owns its own multi-selection for entity work, and selecting an entity or a file replaces the whole thing. (b) The Asset panel's content moves into the Inspector as inspector pages, one per kind: a header (thumbnail, name, kind, path or id with copy), a facts row, the import settings with **Revert / Apply** (Apply reimports through the session, as the Asset panel does now), the parts (click one to inspect it, a link back to its file), the material factors, **Used by**; documents (scene, prefab, material, blend space) show an **Open** button. The Asset panel is removed and its layout key migrated. (c) A **lock** keeps the Inspector on one thing while other things are selected. (d) The pages are registered by kind, so a later data-asset document (`@DataAsset`, the packages stream) adds a page, not a panel. Gate, driven by the user: select every kind in the reference game's copy and edit an import setting through Apply and Revert. Not in 9c: data assets themselves, multi-object editing across files.
9d. **One picker** — **done 2026-09-23** (built 2026-09-22; gate driven by the user 2026-09-23 on a copy of the reference game — passed) (docs/editor.md § One picker). As specified below with these notes: `showPicker<T>` in `lib/src/picker/picker.dart`, and a **sixth** user the item did not list — Create Entity with Component (the Hierarchy's, grouped by category like Add Component, a component without defaults shown and disabled with its note). Groups: an asset slot by folder (a part by its source file), Add Component and Create With by category, Open Document by Scenes / Prefabs, the palette by menu. Every picker's keys are `<prefix>`, `<prefix>.search`, `<prefix>.<id>` (`.row.<i>` for an item without an id), `<prefix>.note.<id>`, `<prefix>.none`, `<prefix>.preview`, `<prefix>.count`, `<prefix>.empty`, `<prefix>.title`, with the prefixes `assetPicker`, `entityPicker`, `openDocument` (was `dialog.openDocument`), `addComponent` (a held type's `addComponent.held.<type>` is now `addComponent.note.<type>`), `createWith` and `palette`. It sits under the field that asked, right edges aligned and above it when there is no room below; the palette and Open Document sit at the top of the window; it opens without animation (9a). Tests: `picker_test.dart` 5 and each user's own. Specification: (added 2026-09-22 with 9b): every place that asks "which one?" opens the same popover — asset slots, entity slots, Open Document, Add Component and the command palette. A search field, results grouped (by source file, by folder, by category), **None** first where the field may be empty, a preview pane on the right (thumbnail and a few facts), ↑ ↓ ⏎ esc everywhere, anchored under the field that asked or at the top of the window for the top bar and the palette. The five current dialogs and menus go. Gate, driven by the user: each of the five on the copy, keyboard only.
9e. **Panels and navigation** — **done 2026-09-23** (gate driven by the user 2026-09-23 on a copy of the reference game — passed; its one finding, trackpad scroll and pinch not reaching the live Scene view, fixed in `package:fynkel_flutter` (`FynkelViewport` now forwards the pan-zoom gesture)) (docs/editor.md § Panel tabs, copies and the orientation gizmo). As specified below with these notes: **no shared touch** — the Scene view's copies share the document's one edit-preview world and each view sets its own camera on it right before it renders (the extract is synchronous and every viewport has its own renderer), so neither `fynkel_core` nor `fynkel_native` changed and there is nothing to announce to the M3 stream; **the tabs** show the × only under the pointer (never on the focused tab by itself) and a padlock when the copy is locked, cut at 180 px and clipped in a narrow group; **zoom** — the Scene view takes the trackpad's two-finger scroll and pinch (macOS sends both as one pan-zoom gesture, not wheel events) as dolly, besides the wheel; **the gizmo** (`lib/src/scene_view/orientation_gizmo.dart`) has a caption under it naming the view within 0.75° of an axis (`Front · Persp`, `Top · Ortho`; just the projection in between) that switches the projection on a click; a click on the end the camera already looks from goes to the other side; looking along an axis that axis's end covers the centre, and a click there is the end's — the centre switches the projection only between views; Top and Bottom keep +Z at the bottom / top of the picture; **copies** are `<panel>#<n>` (lowest free number, titled `Inspector 2`), opened beside the copy used last with its width, from the tab menu or Window ▸ New <Panel> (`window.newCopy.<id>`, ⇧⌥⌘1…4 — 94 commands); commands address the copy that had the keyboard last; **explicit requests** (Show in Inspector, Show in Project, a Console or Game row's entity) go to the copy used last that is **not locked**, a new copy when every one is — a lock is never broken from outside its panel (this replaces 9c's "Show in Inspector moves a locked Inspector"); **what comes back** — the layout keeps the copies; `.fynkel/editor.json` `panels` keeps an Inspector's lock on an asset, part or folder and a Project browser's folder, view and lock; a lock on entities and a Hierarchy's lock on a document do not come back (a project opens with no documents open); the Scene view copies' cameras are kept per document under `views`; closing a copy forgets its state; a one-frame, debug-only overflow report from `tabbed_view`'s tab-strip corner when docking rebuilds a narrow group with hidden tabs is the package's and is documented, not worked around. Specification: (added 2026-09-22 at the user's request during the 9b–9d gate; `apps/editor`, plus one shared touch for the Scene view's copies): (a) **Tabs.** A panel's tab shows its icon and title only — no maximise button anywhere, and a close × that fades in under the pointer (hover, the one animation 9a allows). A right click on a tab opens its menu: Maximise / Restore, Close, and for a panel that can be copied, New <Panel>. A double click maximises or restores. (b) **The orientation gizmo.** In the Scene view's top right, under the toolbar, in the frosted style of the overlay: the three axes turning with the camera, positive ends labelled X Y Z in the axis colours, negative ends as dots; a click on an end looks along that axis (the view looks at the pivot from that side), a click in the centre switches perspective / orthographic, a drag orbits like the right mouse button's look does around the pivot. (c) **Panel copies.** The Inspector, the Project browser, the Hierarchy and the Scene view can be opened more than once (the tab menu's New <Panel>, and a command per panel). Each copy has its own state: an Inspector its own target and padlock (a locked copy beside an unlocked one is the point), a Project browser its own folder, selection, search and padlock (a locked browser keeps its folder when something is revealed), a Hierarchy either follows the focused document or is locked to one, a Scene view its own camera. The layout remembers the copies and their locks. The first copy of each is the one commands and "Show in …" address unless another copy had the keyboard last. The Console, the Profiler and the Animation panel stay single. The Scene view's copies need the preview world to render from a camera per view instead of its one edit camera: a shared touch in `fynkel_core` edit mode / `fynkel_native` `authoring`, a separate small commit merged to `main` first and announced to the M3 stream. Gate, driven by the user on a copy of the reference game: the tab chrome and menu, the gizmo's clicks and drag, two Inspectors (one locked) with two different selections, two Project browsers in two folders, two Scene views from two sides, a restart that brings the copies and locks back.
9f. **Gizmo rendering** — **done 2026-09-23** (gate driven by the user on a copy of the reference game: "much much better"; its judgements — lines 1.5 pt read as heavy, now 1 pt; a range sphere's facets, circles now 64 segments; the level traced in green, colliders now drawn only for the selection; the Y axis through the scene, removed; an unselected camera's frustum loudest in the picture, now at a third of its alpha) (docs/renderer.md § The overlay; the Apple viewport ABI v6; `ViewportOverlay` in `package:fynkel_flutter`; widths are logical pixels × the panel's ratio, the editor's `GizmoTokens`) (ADR-0019 §1; shared touch: `fynkel_render`, `platform/apple` + `package:fynkel_flutter` for the entry point): an overlay pass after the post pass into the display-referred target — lines as screen-space quads with a width in logical pixels and an anti-aliased edge, real alpha, depth-aware (the part behind geometry drawn at 0.25 of its alpha, not dropped; an `onTop` flag for handles), flat blended triangles, and an analytic ground grid (view ray against the plane, derivative-based minor/major lines, distance and grazing fade, X/Z axis colours, reads the scene depth, never writes it). `DebugDraw` keeps its API as a client of the overlay. The editor's grid, outline and icons move onto it; the M4-06 MSAA exposure quirk goes away by construction. Gate, driven by the user on a copy of the reference game: the two screenshots of 2026-09-23 (the dotted grid under the Player, the broken light and camera arcs) retaken — clean, smooth, faded behind walls, the grid fading out without moiré; pixel tests for width, anti-aliasing, the occluded fade, the grid's fade and the exposure independence. Not in 9f: handles (9g), game gizmos (9h).
9g. **Handles and Scene tools** — **done 2026-09-23** (gate passed, driven by the user; the handle framework and the tool registry with move, rotate, scale and the transform tool on the overlay; component handles for lights, colliders and cameras behind the Inspector's Edit toggle) (ADR-0019 §2; editor only): the handle framework (shape, screen-space hit test, drag constraint — axis, plane, ring, radius, free), a tool registry (id, icon, shortcut, `handlesFor(selection)`) with move, rotate and scale rebuilt on it and a combined transform tool as the proof, the transform gizmo drawn through 9f's overlay, component handles registered per type behind the Inspector's Edit toggle — a point/spot light's range, a spot's angle, box/sphere/capsule collider sizes, a camera's field of view — every drag one transaction. Gate, driven by the user: every tool and component handle on the copy, undo per drag, the scene byte-identical after undoing everything. Not in 9g: snapping changes, custom tool plug-ins from projects (ADR-0018 §3).
9h. **Gizmos for game code** — **done 2026-09-23** (gate driven by the user on a copy of the reference game: the Switch's line to its gate, the Emitter's velocity arrow dragged and undone, a `@Gizmo.radius(when: always)` added to `game.Gate` after `fynkel codegen`, drawn and dragged, and a `Gizmos.wireSphere` from `Gate.update` in the game window under *Show Gizmos in Play* — "amazing it is working") (the runtime `Gizmos` in the game's window and Play's *Show Gizmos in Play*; `@Gizmo` through the generator with a new `List<Vec3>` field kind, drawn and dragged in the Scene view; the reference game's `Switch.target` is a `lineTo`, the Emitter's velocity a native direction hint — its Oscillate blade and trigger zones are native components without a field to draw, so they wait for the Rust field attributes) (ADR-0019 §3; shared touch: `package:fynkel`, `package:fynkel_generator`, `package:fynkel_flutter`): the runtime `Gizmos` API (line, ray, wireSphere, wireBox, wireCapsule, wireCircle, arrow, frustum; colour and matrix scopes) drawn in the game's own window in debug/profile when asked (`FynkelGameView(gizmos: true)`, `FYNKEL_GIZMOS=1` from the editor's Play), a no-op in release; the declarative `@Gizmo.radius/box/point/direction/cone/lineTo/path` annotations emitted by codegen and drawn by the editor, each also a handle (9g) that edits its field in one transaction; the reference game's behaviours annotated where it helps (the Emitter's direction, the Oscillate blade's path, trigger zones). Gate, driven by the user: a behaviour on the copy gains a `@Gizmo.radius()` field, `fynkel codegen`, the editor draws and drags it; Play with gizmos on shows a `Gizmos.wireSphere` from a behaviour in the game window.
10. **M4 gate and report**: `docs/m4-gate-report.md` against the §16 gate — the user rebuilds the reference game's start area (floor, walls, the Player prefab, the camera rig, the sun, three coins) from an empty scene with the editor only, saves, and `fynkel validate` / `fynkel test --headless` / `fynkel run` accept it; every mutation of that session is in the authoring log as commands; the CLI sees each save immediately; Play launches the real app; the kill test; no UUID, cache file or generated file edited by hand; `docs/status.md` rewritten; the carry-over list for M5 (runtime inspection over the channel, the M4 items' surprises, autosave recovery UX if not landed, custom windows per ADR-0018).

**Execution order:** 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9a → 9b → 9c → 9d → 9e → 9f → 9g → 9h → 9 → 10, with `main` rebased in before every gate. Items 1 and 6 carry the shared-crate touches with the widest blast radius (`fynkel_document`, `fynkel_native`, `fynkel_core`); they are announced to the M3 stream before they start.

**Not in M4** (§11.2 and this backlog's honesty): a code editor, a hand-built docking framework, in-process Play, runtime inspection and "apply runtime value to source" (designed for, built in M5), material preview spheres and offscreen mesh thumbnails (after item 6 the world exists for them; M5), multi-scene editing (§22.16), custom Inspector *widgets* and custom windows (ADR-0018 — declarative hints only in 1.0), browser anything, collaborative editing, Windows.

### 19.3 First vertical slice after M0

The first post-M0 slice should be:

```text
Flutter app
  -> native Fynkel viewport
  -> Rust world
  -> hecs-backed entity
  -> Transform
  -> Camera
  -> MeshRenderer
  -> one imported mesh/material
  -> batched Dart command changes transform
  -> snapshot exposes required state
  -> scene renders correctly
```

The same slice must run in the standalone host and the headless host (minus rendering) to prove the engine has no Flutter dependency.

Do not begin with physics, animation, or an editor panel before this slice is stable.

## 20. Verified technical references

Key implementation assumptions were re-checked on **19 September 2026**. Re-check exact versions and platform details when M0 freezes the toolchain and before each supported release.

- **S1 — wgpu documentation:** `https://docs.rs/wgpu/latest/wgpu/`
- **S2 — hecs documentation:** `https://docs.rs/hecs/latest/hecs/`
- **S3 — Rapier getting started:** `https://rapier.rs/docs/user_guides/rust/getting_started/`
- **S4 — Rapier determinism:** `https://rapier.rs/docs/user_guides/rust/determinism/`
- **S5 — glam documentation:** `https://docs.rs/glam/latest/glam/`
- **S6 — Kira documentation:** `https://docs.rs/kira/latest/kira/`
- **S7 — Khronos glTF:** `https://www.khronos.org/gltf/`
- **S8 — JSON Schema draft 2020-12:** `https://json-schema.org/draft/2020-12`
- **S9 — Flutter `Texture` widget:** `https://api.flutter.dev/flutter/widgets/Texture-class.html`
- **S10 — Flutter native code / FFI and build hooks:** `https://docs.flutter.dev/platform-integration/bind-native-code`
- **S11 — Flutter hot reload:** `https://docs.flutter.dev/tools/hot-reload`
- **S12 — Flutter FAQ / reflection limitations:** `https://docs.flutter.dev/resources/faq`
- **S13 — Dart JS interoperability (future browser work):** `https://dart.dev/interop/js-interop`
- **S14 — Flutter Wasm support (future browser work):** `https://docs.flutter.dev/platform-integration/web/wasm`
- **S15 — MDN WebGPU (future browser work):** `https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API`

At the time of this revision, the current docs describe `hecs` as a high-performance minimalist ECS **library, not a framework**, which is the reason it fits Fynkel's “own the engine API, reuse storage/query mechanics” approach. Current wgpu documentation continues to describe native backends including Metal, D3D12, and Vulkan plus WebGPU/Wasm support, but Fynkel platform support remains contingent on its own tests.

The architectural prescriptions, budgets, and platform priorities in this blueprint are Fynkel design decisions—not capabilities automatically guaranteed by those dependencies.


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
compiled/cooked game assets
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
- **C3 (M3, host profiling):** live Dart allocation and GC counters in the Flutter host's bridge diagnostics (today only the offline `tool/alloc_bench.dart` probe), and an AOT (Flutter release) allocation capture in `benchmarks/m0/capture.py`.
- **C4 (M5, with physics):** native system registration for the pre-tick / physics / post-physics / animation phases; `World::step()` fixes the order today with no producers.
- **C5 (M3):** subscription budget diagnostic (bytes per snapshot against a host-set ceiling) so "bounded" is enforced, not only measured.
- **C6 (first release):** publish `fynkel.lock.json` prebuilt artifacts with sha256s; until then the hook's prebuilt path is covered by unit tests only.

### 19.2c M2 issue backlog

Derived from the M2 "Build" list and gate (§16) and bound by ADR-0006 (formats), ADR-0012 (two generators), ADR-0016 (`fynkel_document`), ADR-0017 (`--json` CLI). One commit per item, in this order; §17.2 applies — data contracts first, tooling on top, behaviours last because they depend on every document kind. Rendering of meshes/materials is **M3** (renderer V1); M2 fixes the documents, validation, import, and cache, and proves them headlessly.

**Scope rules fixed here (cheaper to decide now than to migrate later):**

- Every document is one JSON object with `$schema`, `formatVersion`, `id` (UUID v4 string), `name`, and its kind-specific body. Canonical output: 2-space indent, keys in schema order, `\n` line endings, trailing newline; unknown keys are preserved byte-for-byte in value and position.
- Component instances carry `type` (namespaced type id), `version`, `data`. A missing type keeps `data` opaque; validation reports it as a warning, build as an error.
- `AssetRef` fields serialize as `{ "asset": "<uuid>", "sub": "<sub-id>" | absent }`; never a path.
- Diagnostics everywhere use the §14.2 record: `code`, `severity`, `message`, `document`, `pointer` (RFC 6901), `id`, `line`/`column` when known, `suggestion`, `related[]`. Stable code namespace `FYN-D####` (documents), `FYN-A####` (assets), `FYN-P####` (project), `FYN-G####` (codegen), `FYN-E####` (editing: authoring commands and transactions).
- Dependencies added in M2: `serde_json` (strict, `preserve_order`, `float_roundtrip`), `jsonschema` (draft 2020-12), `uuid`, `gltf`, `image` (PNG/JPEG only), `hound` (WAV), `lewton` or `symphonia` (Ogg Vorbis, chosen at item 10), `blake3` (cache keys), `similar` (diffs). Each recorded in `docs/toolchain.md` per §2.3.

1. **C1 — allocation-free transform writers** (carry-over): `TransformWriter` (reusable, chainable, encodes straight into the batch) plus `GameObject.writeLocal/writePresentation` and scalar `setLocalPosition/Rotation/Scale`; `alloc_bench` and the Flutter demo scene switched to them. **Done:** 5 105 → 1 492 B/frame on the 25-entity scene, 0 GC events in 3 000 frames (`2026-09-19`).
2. **`fynkel_document` seed + strict JSON + diagnostics** (done 2026-09-19; `docs/documents.md`): `crates/fynkel_document` (ADR-0016) with the strict reader/writer (duplicate keys, non-finite numbers, malformed UUIDs, wrong-type coercion all rejected with pointers; canonical writer; unknown-key preservation), the document envelope, and the §14.2 `Diagnostic` record in `fynkel_contracts`. JSON Schema 2020-12 validation behind one `validate(kind, json)` entry. Round-trip corpus under `crates/fynkel_document/fixtures/` (valid, invalid-with-expected-diagnostics, unknown-data).
3. **Scene document + schema** (done 2026-09-19; `docs/documents.md` § Scene documents; codes `FYN-D0200`–`D0209`, `fixtures/scene/` corpus, `x-fynkel.authored` on component schemas): `scene.schema.json` emitted by the native schema generator from the Rust document model plus every registered component schema (`schemas/components/*`). UUID-keyed `entities`, `parent`, per-entity ordered `children`, `components` keyed by instance UUID. Semantic validation: dangling parent/child, child order ↔ parent consistency, cycles, duplicate component type per entity where the schema says `unique`, unknown type → warning with data kept. `fynkel scene inspect|validate <path> --json`.
4. **Starter native component schemas** (done 2026-09-19; `fynkel_core::components`, ids 4–7, `AssetRef` + enum field kinds in `docs/codegen.md`, fingerprint `0xa3c3bd56d1793195`): `fynkel.Camera`, `fynkel.MeshRenderer` (`mesh: AssetRef`, `material: AssetRef`), `fynkel.DirectionalLight`, `fynkel.PointLight` as Rust structs with `#[derive(Schema)]`; `AssetRef` and enum field kinds added to the schema system, Dart bindings and wire codecs regenerated. Storage only — no rendering behaviour until M3.
5. **Scene → runtime instantiation** (done 2026-09-19; `fynkel_core::load`, `fynkel_document::describe`, generated document codecs, `Command::SetComponent|RemoveComponent`; `docs/documents.md` § Scene → runtime, `docs/hosts.md`): `fynkel_core::World::load_scene(&SceneDescription)` (authoring UUID ↔ runtime handle map, queryable both ways; hierarchy and child order honoured; components applied through the same command path as the bridge). Headless host and `fynkel test --headless --scene <path>` load a document; `fynkel state dump --json` emits authoring ids, not handles. Gate: "project can be validated/imported headlessly".
6. **Material and shader documents** (done 2026-09-19; `fynkel_document::material`, hand-written `schemas/material.schema.json` + `schemas/shader.schema.json` embedded via codegen, built-in manifest `crates/fynkel_document/shaders/fynkel.pbr@1.shader.json`, codes `FYN-D0300`–`D0308` material and `D0310`–`D0312` manifest, `fixtures/material/` corpus; `docs/documents.md` § Material and shader documents): `.material.json` (shader id, typed parameters, textures as `AssetRef`, samplers, alpha mode + cutoff, cull, shadow flags, colour space per texture slot) and `.shader.json` interface manifest for the built-in versioned PBR shader (`fynkel.pbr@1`). Validation of material against manifest (unknown parameter, wrong type, range, missing required texture). `fynkel material validate --json`.
7. **Prefab documents, instances, overrides, expansion** (done 2026-09-19; `fynkel_document::prefab`, `schemas/prefab.schema.json` generated next to `scene.schema.json`, `PrefabLibrary::scan`, `derive_id` = UUID v5 in the instance namespace, codes `FYN-D0400`–`D0407`, `fixtures/prefab/` corpus with the exact `instances_ok.scene.expanded.json`; `fynkel prefab inspect|validate`, `fynkel scene … --prefabs <dir> [--expand]`, `--prefabs` on `test --headless` and `state dump`; `docs/documents.md` § Prefab documents): `.prefab.json` = scene body + `roots[]`. Scene entity `prefab: { asset, instance }`; overrides as an explicit ordered op list — `setField {entity, component, field, value}`, `addComponent`, `removeComponent`, `addObject`, `removeObject` — with `unset` distinct from `null`. `expand(scene) -> Expanded` remaps prefab-local ids per instance (deterministic derivation from instance id) and internal references; external asset refs shared. Reject nested prefab instances and self-reference cycles (M2 "do not"). Gate: "prefab references remap correctly".
8. **Asset identity (`.meta.json`) and the asset database** (done 2026-09-19; `fynkel_document::asset` — `Meta`, `AssetDatabase::scan/get/resolve/check_references`, hand-written `schemas/meta.schema.json` embedded via codegen, codes `FYN-A0001`–`A0006`, `fixtures/assets/` corpus with the move test; `fynkel asset list|resolve|init`; `docs/documents.md` § Asset sidecars and the asset database): sidecar `{ formatVersion, id, importer, importerVersion, settings, subAssets: { "<sub-id>": uuid } }`; database rebuilt by scanning `assets/`; diagnostics for missing sidecar, duplicate id, orphaned sidecar; move = same id at a new path (test: move file+sidecar, every `AssetRef` still resolves). `fynkel asset list|resolve <uuid> --json`. Gate: "asset moves preserve identity".
9. **Document migration framework** (done 2026-09-19; `fynkel_document::migrate` — `Migration`/`Migrations::builtin()`/`migrate()`/`Migrated::write_with_backup`, `similar` unified diffs, codes `FYN-D0016`–`D0018`, envelope accepts `formatVersion` 0 as "outdated", `fixtures/migrate/` with the synthetic v0 scene and its exact v1 result; `fynkel migrate <path|dir>... [--dry-run] [--list]`; `docs/documents.md` § Migrations): per-kind ordered migrations `(from, to, fn)`, `migrate(json) -> {json, applied[], diff}`, dry-run diff output, `.bak` backup on write, refuse newer `formatVersion` (read-only diagnostic, never rewrite). `fynkel migrate <path|project> --dry-run --json`. Fixture: a synthetic v0 scene migrated to v1.
10. **glTF/GLB importer** (done 2026-09-19; `crates/fynkel_import` — `import(path, &Meta, &Limits) -> Imported`, `fynkel.gltf` v1 over the `gltf` crate without its `import` feature; `Intermediate` (`FYNI` container: meshes in the fixed 72-byte vertex layout, `fynkel.pbr@1` material documents, encoded images, the node hierarchy as a prefab document) with `to_bytes`/`from_bytes`; sub-asset ids `mesh/<n>[/<p>]`, `material/<m>`, `image/<i>`, `prefab` with uuids from the sidecar or UUID v5 of the asset id; external URIs denied unless `settings.allowExternalFiles` and then only plain relative paths; required-extension, malformed-data, cycle and limit checks (`FYN-A0100`–`A0105`); fixtures `robot.glb` + data-URI/external/strip/Draco/cycle variants, committed `robot.expected.fyni` determinism test; `fynkel import <file>... [--out <dir>] [--dry-run]` creates/updates sidecars): `crates/fynkel_import` — size, recursion, external-URI (deny by default), buffer/decompression limits; normalized intermediate (meshes with fixed vertex layout, materials mapped onto `fynkel.pbr@1`, node hierarchy as a generated prefab); stable sub-asset ids via the sidecar; determinism test (same input → same intermediate bytes).
11. **Texture and audio import baseline** (done 2026-09-19; `fynkel_import::image` (`fynkel.image` v1, `image` 0.25 PNG/JPEG only → `TextureData` RGBA8 top-row-first, `settings.colorSpace` srgb|linear declared in the sidecar, 16-bit narrowed, `sourceHasAlpha`) and `fynkel_import::audio` (`fynkel.audio` v1, `symphonia` 0.6 wav/pcm/ogg/vorbis only → `AudioData` interleaved f32 clamped to [-1, 1], sample rate/channels/codec kept, gapless); `Intermediate` gains `textures`, `clips` and an optional `prefab`; limits `max_texture_dimension` 16 384, `max_audio_frames` 10 min at 48 kHz; fixtures `textures/` (every PNG layout, JPEG, GIF for A0103) and `audio/` (8/16/24-bit WAV, stereo and mono Vorbis, truncated WAV, FLAC for A0103) with committed expected bytes; `fynkel import` unchanged, dispatch by sidecar importer): PNG/JPEG → normalized RGBA8 with declared colour space (sRGB/linear from import settings, mipmaps deferred to M3); WAV + Ogg Vorbis → PCM intermediate with metadata; limits as above.
12. **Content-addressed incremental cache + cooked container v1** (done 2026-09-19; `fynkel_import::cache` — `KeyInputs::key()` = blake3 over length-prefixed fields: cache/intermediate/container versions, asset id, kind (importer or `document/<kind>`), version, source hash, extra input files (external `.gltf` URIs), canonical settings, dependencies as sorted `(id, key)`, profile; `Cache` at `.fynkel/cache/objects/<key>.fyna` + `<key>.manifest.json` (atomic writes, no timestamps), `index.json` after each cook, `prune(keep)`. `fynkel_import::cook` — dependencies from a document's resolved `assetRef`s, Tarjan order, cycle → A0104, unresolved → A0003, blocked dependents → A0105; documents cooked as canonical `JSON`, sources through their importer into `MESH`/`JSON`/`TEXR` (embedded glTF images decoded, colour space from slot usage)/`CLIP`, `fynkel.file` as `BLOB`. `crates/fynkel_cooked` (only `crc32fast`): `FYNC` v1, 48-byte header with CRC, 8-byte-aligned payloads, table of (kind, flags, offset, length, crc32), chunk 0 = META JSON; `Container` (bytes) and `ChunkFile` (seekable) validate everything before any payload, `read(index, max_len)` bounded + CRC-verified. Tests: clean corpus `fixtures/project/assets` cooks then hits; editing `swatch.png` pixels or its sidecar `colorSpace` recooks exactly swatch + rusted.material + main.scene; document edit → itself + dependents; profile in the key; read-back with bounded decoding and checksum failure; cycles/unresolved/blocked; prune. CLI `fynkel cook [--assets] [--cache] [--profile] [--force] [--prune] [--json]`, `fynkel cooked inspect <file.fyna>... [--json]`.) key = blake3(source bytes, ordered dependency keys, settings, importer version, target profile); `.fynkel/cache/<key>` with a dependency manifest; test that editing a referenced texture invalidates exactly the dependent material/model outputs. Cooked container: one file per asset, magic + version + chunk table (offset, length, checksum) + dependency ids; bounded chunk decoding.
13. **Project model + project-wide validation** (done 2026-09-19; `fynkel_document::project` — `Project` over `fynkel.project.json` (hand-written `project.schema.json`: `startupScene`, `assets` root, `inputActions` stub, `quality` profiles, `components` registry, `targets`; `x-*` kept), `Lock` over `fynkel.lock.json` (`lock.schema.json`, no envelope: runtime/tools versions, ABI, schema fingerprint, document and importer versions, cache/container versions; `Lock::expected` + `fynkel_import::expected_lock()`, `check` = one `FYN-P0011` per differing field, never a silent migration), `validate_project` = project file → lock → asset root → database scan → startup scene/registry → every document with references, prefab expansion, material/shader checks → unregistered-component warnings; codes `FYN-P0001..P0012`; `fynkel project describe|lock [--check]`, `fynkel validate`, `fynkel schema list|describe <type|kind>`, all `--json`; exit 0/1/2 fixed; fixture project with its committed lock): `fynkel.project.json` schema (startup scene, input actions stub, quality profile, component registry, build targets) and `fynkel.lock.json` (runtime/tool versions, schema fingerprint, importer fingerprints); `fynkel project describe --json`, `fynkel schema list|describe <type> --json`, `fynkel validate --json` over every document, sidecar, and reference, exit codes fixed (0 ok, 1 diagnostics with errors, 2 usage/IO).
13a. **Scene-on-screen spike** (done 2026-09-19, M3's first item pulled forward after M2-13 so the authoring work builds on a proven path; not a backlog item — a checkpoint): `fynkel_cooked::payload` (the `MESH`/`TEXR`/`CLIP` codecs, so a loader needs nothing from the importers); `fynkel_core::Extract` gains `meshes`, `camera` (highest priority), `directional_lights`, `point_lights` resolved against records; `fynkel_render` draws `MeshRenderer` entities with their cooked mesh and the `fynkel.pbr@1` `baseColor` factor × texture (`CookedAssets` over the `fynkel cook` cache, loaded on demand, one 72-byte vertex layout, batches by (mesh, texture), magenta for a missing mesh, optional placeholder cubes, the scene's `Camera` and first `DirectionalLight`, sRGB-encoded Lambert + Blinn-Phong); `fynkel_document::runtime::{read_scene, load_project}` (project → database → startup scene with every prefab); C ABI `fynkel_world_load_scene` / `_load_project` / `_load_report` (+ `FYNKEL_STATUS_LOAD_FAILED`), Dart `World.loadScene`/`loadProject` → `LoadReport`/`FynkelLoadError`; Apple viewport ABI v2 (`set_asset_cache`, `set_placeholders`, mesh/placeholder/missing counts in the stats); Flutter host `FYNKEL_PROJECT=<dir>`; `fynkel render [--project] [--out png] [--size WxH]` headless proof. Verified: the fixture project on the macOS host from its own camera (2 cooked meshes, 0 missing, 120 Hz) and as a pixel test. Compromises in `docs/status.md` §3/§4 (scenes still read from source files, one-pass shading, no audio consumer).)
14. **Authoring commands and transactions** (done 2026-09-19; `fynkel_document::authoring` — `Editor` over one scene or prefab document with `Revision` = blake3 of the canonical text (printed by `scene|prefab inspect --json`), `Editor::apply(Request { id?, baseRevision?, commands })` runs one atomic transaction: stale `baseRevision` → `FYN-E0001` conflict and nothing touched; commands compile to reversible edits (`replace`, `insertMember`/`removeMember`, `insertItem`/`removeItem`) whose inverses are the mutation's `undo`; a failing command or any validation error the transaction introduces (`FYN-E0005`, errors in `related`) rolls the document back to the byte; `saveDocument`/`createMaterial` are side effects after validation (no undo, failure rolls back too); byte-exact `undo()`/`redo()` over the committed history; the 13 §11.1 commands + `unsetField`, JSON codec `{ "command": …, … }`; `Applied`/`Mutation` records `{ commandId, command, transactionId, baseRevision, affectedIds, diagnostics, undo }`; codes `FYN-E0001..E0011`; `fynkel scene|prefab apply <path> --patch <ops.json> [--dry-run] [--json]` writes in place or prints the diff, exit 0/1/2; tests in `crates/fynkel_document/tests/authoring.rs`): in `fynkel_document`, the command set `CreateObject … RevertPrefabOverride, CreateMaterial, SaveDocument`, each mutation `{ commandId, transactionId, baseRevision, affectedIds, diagnostics, undo }`; stale `baseRevision` → conflict, never overwrite; `fynkel scene apply --patch <ops.json> --dry-run --json`. The C ABI surface for the editor waits for M4.
15. **Behaviour SDK and lifecycle**: `Behaviour` base class with the §3.2 lifecycle, `@FynkelBehaviour(id:)` / `@Expose(...)` annotations in `package:fynkel`, a behaviour registry keyed by type id, and the `World` driver calling `onCreate → onEnable → start → fixedUpdate/afterPhysics/update/lateUpdate → onDisable → onDestroy` at the §5.1 points, deterministic order, deferred destruction. Services resolve through the current `GameContext`, test context installable.
16. **Behaviour generator + `fynkel codegen` (b)**: `packages/fynkel_generator` (`build_runner`) → `lib/generated/behaviours.g.dart` (factories, field serializers) and `schemas/behaviours/<id>.schema.json` + Inspector metadata; `fynkel codegen` runs (a) and (b) with separate caches and `--check`. End-to-end: a scene component `game.Spinner` with authored `speed` instantiates the compiled class with that field set. Gate: "a compiled Dart behaviour can be instantiated from authored data".
17. **`fynkel create` + `AGENTS.md`** (ADR-0017): Flutter app template with `fynkel.project.json`, `fynkel.lock.json`, `assets/` tree, a starter scene + material + one behaviour, `AGENTS.md` (commands, schema locations, validate→codegen→build→test loop, do/don't). `fynkel validate` and `fynkel test --headless` pass on a fresh `fynkel create` output.
18. **M2 gate report**: round-trip corpus results, exact-diagnostic fixtures, id-stability and move tests, headless import of the template project, timings (validate, scene load, glTF import, cache hit/miss) appended to the benchmark capture; carry-over list for M3.

Removed from M2: shader graph, nested prefabs/variants, editor UI, renderer consumption of materials (M3), mipmaps/compressed textures (M3/M6), pack files (1.x).

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


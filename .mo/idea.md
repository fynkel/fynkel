# Fynkel Engine — Development Blueprint

**Status:** Proposed architecture and implementation plan, version 0.3  
**Prepared:** 19 September 2026  
**Audience:** Fynkel maintainers, contributors, runtime/rendering engineers, Dart/Flutter engineers, tooling engineers, and AI-assisted development workflows  
**Objective:** Build **Fynkel Engine**, a native-first, Dart-first 3D game engine with a Rust runtime, `wgpu` rendering, `hecs` behind a Fynkel-owned ECS boundary, Flutter UI/editor tooling, transparent project files, and a Unity-like workflow without making the editor mandatory.

All package names, commands, APIs, file schemas, platform targets, and performance budgets below are proposed engineering contracts until validated by milestone gates. Milestones are acceptance gates, not calendar estimates. Platform support is earned through testing, not inferred from Flutter, Rust, or dependency support.

### Naming and brand conventions

The official product name is **Fynkel Engine**. Use **Fynkel** as the short product name in developer-facing copy. Use `fynkel` for the CLI and primary Dart package, `fynkel_*` for engine-owned Rust/Dart modules, and `fynkel.*` for built-in serialized type IDs. Shipped games may use **Made with Fynkel**.

Generic words such as “engine”, “runtime”, “renderer”, and “editor” remain lowercase when describing technical concepts.

### Scope principle for 1.0

Fynkel 1.0 is **native-first**. The product must become a good native engine before it becomes a browser development environment.

Browser/Wasm/WebGPU compatibility is kept alive through a small M0 feasibility spike and portable contracts, but the browser editor, browser-only project workflow, cloud compilation, browser filesystem layer, and web collaboration are **post-1.0** work.

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

The reference game is a small third-person 3D collect-and-interact game with this frozen baseline:

**Project structure**

- `MainMenu` UI state.
- `Playground` gameplay scene.
- `Results`/win state.
- Saveable settings for audio, graphics quality, and input bindings.

**Player**

- One skinned/animated player character.
- Idle, walk, run, and jump animation states.
- Third-person camera.
- Character collision and ground detection.
- Walk, sprint, jump, interact, pause, and camera input actions.

**World**

- Approximately 100 static mesh renderers.
- At least 20 dynamic rigid bodies.
- 10 collectibles.
- 2 moving or physics-based hazards.
- 1 directional light with shadows.
- A small bounded number of point/spot lights.
- PBR materials and imported glTF/GLB assets.
- At least one true world-space UI/sign element.
- At least one projected Flutter world label or marker if the projected-label system is included in 1.0.

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
- At least one spatial world sound.

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

The **minimum 1.0 platform commitment** should be made only after M0/M6 measurements. M0 uses **macOS/Metal as the primary integration target** and requires a second graphics/backend path before architecture sign-off. Windows/D3D12 is the preferred second validation path when hardware/CI is available. Linux/Vulkan and mobile targets remain planned native targets but are advertised only after they pass the same lifecycle, rendering, performance, and packaging gates.

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
Normal Flutter/Dart game project
  Dart behaviours + Flutter UI + Fynkel SDK
                    |
     FynkelBridge: commands / snapshots / events
                    |
              Versioned C ABI
                    |
              Rust runtime
       fynkel_ecs -> hecs internally
       transforms / physics / animation / audio
       assets / scene runtime / renderer
                    |
                   wgpu
       Metal / D3D12 / Vulkan / GLES as qualified

Development only
  Fynkel Editor (Flutter desktop)
  Fynkel CLI
  code generation / importers / asset compiler
  AI/automation tools
                    |
        Shared authoring command service
                    |
        JSON project/source documents
```

The runtime must remain usable without Flutter widgets except where a Flutter host is explicitly needed. The Dart gameplay package should therefore remain separable from `fynkel_flutter`.

### 2.1 Browser compatibility policy

Do **not** maintain a second production runtime/bridge/editor before 1.0.

M0 contains one deliberately small compatibility spike:

```text
Rust rendering core subset
        |
     Wasm build
        |
   wgpu / WebGPU
        |
 simple test scene
```

The spike answers only whether the core abstractions are unnecessarily native-only. It is allowed to be throwaway code. It does not create a browser product commitment.

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

  platform/                 # native texture/surface/lifecycle integrations
  experiments/
    web_spike/              # M0 compatibility experiment only
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
| Native bridge            | versioned C ABI                               |
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

### 2.4 Licensing and repository strategy — M0 decision

Licensing is an architectural/repository decision and must be resolved before public package publication.

**Recommended default for discussion in ADR-0005:**

- engine/runtime/SDK/tooling code: `MIT OR Apache-2.0`;
- examples/templates: same unless asset licenses require otherwise;
- Fynkel name/logo: separate trademark/brand policy;
- third-party sample assets: explicit per-asset licenses;
- contributor policy: choose DCO or CLA before accepting external contributions.

M0 must also decide:

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
ADR-0002  Flutter/wgpu viewport composition path
ADR-0003  hecs behind fynkel_ecs
ADR-0004  Dart/Rust command-snapshot protocol
ADR-0005  license, repository, contribution, trademark policy
ADR-0006  project/source document format
ADR-0007  runtime/editor process separation
```

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

Fynkel—not `hecs`—owns the logical scheduler.

Initial phases:

```text
Input capture
  -> Dart fixedUpdate command generation
  -> native pre-physics systems
  -> physics
  -> afterPhysics callbacks/subscriptions
  -> animation / transform work
  -> deferred structural changes
  -> update
  -> lateUpdate
  -> render extraction
```

Do not build a general dependency graph/parallel scheduler before profiling proves it is needed.

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

### 5.1 Fixed simulation contract

Initial logical fixed-tick order:

```text
Capture tick input + completed events
  -> expose coherent snapshot S[n] to Dart
  -> Dart fixedUpdate produces command batch C[n]
  -> validate/apply C[n] at tick boundary
  -> native pre-physics systems
  -> fixed physics step
  -> post-physics synchronization
  -> optional afterPhysics callbacks/events
  -> animation/transform work required for the tick
  -> deferred structural changes
  -> publish S[n+1] + events
  -> build immutable render extraction
```

Start with a configurable 60 Hz fixed step.

`update` and `lateUpdate` run at presentation rate. Authoritative writes issued there are staged for a documented safe boundary; they do not mutate physics/native storage in the middle of an in-progress simulation step.

Rendering uses completed state and interpolation where appropriate.

### 5.2 Host/threading model

For the first implementation, a host-driven batched native step is acceptable if it meets the performance gate.

Rules:

- never step the engine from a Flutter widget `build()` call;
- no synchronous disk/network I/O on frame-critical paths;
- Rust worker threads must not call arbitrary Dart gameplay callbacks;
- renderer/asset workers operate on owned jobs or immutable extracts;
- moving simulation to a native worker later must preserve the same command/tick semantics.

Do not create one thread per subsystem.

If Dart misses a simulation deadline:

- render the last coherent state;
- bound catch-up work;
- do not silently simulate ticks whose gameplay command phase was skipped;
- expose diagnostics for missed deadlines.

When a debugger pauses gameplay, pause or explicitly control the simulation clock instead of building an unbounded backlog.

### 5.3 Native bridge protocol

Use a stable, versioned C ABI for the native host boundary.

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

Per-frame traffic uses reusable binary/typed buffers.

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

Reads normally come from a coherent snapshot. To keep ergonomic APIs useful, getters may consult a local staged-write overlay before the last acknowledged snapshot.

A property getter must never hide an expensive native round trip.

Snapshots are subscription-based. Dart should receive only the objects/components/fields it asked to observe or that the engine API requires for active behaviours.

Structural command batches return mappings from temporary spawn IDs to validated Fynkel runtime handles.

### 5.5 Error and lifetime rules

Never unwind Rust panics across the ABI.

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

Assign stable IDs in metadata stored with source assets. Preserve IDs when moving assets; new copies get new IDs. glTF sub-assets need stable child identifiers as well as a parent source ID. The asset database must be reconstructible from committed files, not an indispensable hidden database.

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

Use source JSON directly during early development where convenient. Cook production assets into a documented versioned container with explicit lengths, checksums, dependency IDs, and bounded chunk decoding. Binary scene data is an implementation choice to benchmark, not inherently faster just because it is binary. Never serialize raw Rust memory layouts.

Keep expensive decoding, mesh processing, and compression out of frame-critical execution. Upload GPU work in bounded chunks. Implement asset states, cancellation, ownership, reference tracking, eviction policy, fallback resources, and hot replacement only after a replacement validates. Keep the last valid asset when an edit fails.

Imports must validate paths, file sizes, recursion depth, decompression limits, external URI handling, and resource budgets. Opening a project or importing a model must not run arbitrary scripts.

### Headless data operations

Scene load/save, validation, prefab expansion, migration, and cooking must work without a window. Support asynchronous scene loading and explicit activation. Resolve references before lifecycle callbacks, release resources on unload, and distinguish additive loading from replacing the active scene.

Migrations are explicit, versioned, testable transformations with dry-run diffs and backups. Do not rewrite an entire project automatically when an incompatible editor opens it.

## 8. Renderer and Flutter integration

The Flutter/wgpu presentation path is the **first architectural risk Fynkel must retire**. M0 is a real go/no-go architecture gate, not a demo milestone.

### 8.1 M0 primary platform and comparison host

Use:

- **Primary integration target:** macOS + Metal.
- **Preferred second validation path:** Windows + D3D12.
- **Additional later native paths:** Vulkan/GLES where platform qualification requires them.

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
- Added host/composition overhead is consistently above the provisional M0 budget of **1.5 ms p95 at 1080p/60** compared with the standalone host, unless an ADR explicitly revises the budget based on measured hardware/quality tradeoffs.
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

The public `Input` convenience API may be static-looking, but it resolves through the active `GameContext` service, making it replaceable in tests and preventing process-global gameplay state.

## 10. UI model

**Screen-space UI:** Flutter owns menus, settings, HUD, dialogs, inventory, accessibility, and text entry. Update through filtered events and small observable state, not a per-frame copy of the entire ECS or rebuild of the whole app.

**Projected world labels:** Project selected world anchors into viewport coordinates. Include viewport transform, DPI, camera identity, clipping, and visibility. Depth occlusion requires an explicit policy; a screen overlay is not automatically hidden behind 3D geometry. Bound the number and update rate of anchored widgets; render dense crowds of labels natively if measurement requires it.

**True world-space UI:** Implement a limited native WorldCanvas with panels, images, text, progress bars, simple layout, and basic buttons. Use maintained text shaping/font rasterization components, not a new text stack. Render using depth testing and ray-to-local-panel hit testing. Define canvas units, clipping, ordering, focus, and interaction events. Clearly document initial text/language coverage and unsupported text-entry behaviour.

Expose world UI through the same component schemas, authoring commands, and Dart API. Do not build a second general-purpose app UI framework. Flutter-to-world-texture remains an isolated experiment, not a requirement for release.

## 11. Editor and authoring service

Build Fynkel's runtime until the complete reference game is playable **without an editor**. Then implement Editor V0. Do not wait until the runtime is “finished”; after Editor V0, runtime features and their basic authoring support evolve together.

The editor is a client of the same authoring/document services used by the CLI and AI tooling.

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
- Scene viewport;
- hierarchy;
- generated Inspector;
- asset browser;
- selection;
- move/rotate/scale gizmos;
- save/load;
- undo/redo;
- Play/Stop;
- console;
- basic profiler/performance view;
- open script in external IDE.

Not required for V0:

- custom code editor;
- browser editor;
- full docking framework;
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

Initially, Play launches the project's real game process separately. A separate player/game window is acceptable and is safer than forcing the editor process to host arbitrary game code.

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

## 12. Web compatibility spike and post-1.0 browser policy

Fynkel 1.0 does **not** include a browser editor or production browser runtime.

The purpose of web work before 1.0 is only to avoid accidental architectural lock-in.

### 12.1 M0 WebGPU/Wasm spike

Build a strictly bounded experiment that:

- compiles a minimal subset of the Rust rendering core to Wasm;
- creates a wgpu/WebGPU device;
- renders a simple test scene/cube;
- consumes the same basic material/mesh conventions where practical;
- validates that renderer abstractions do not require native pointers or platform-only assumptions.

The spike does **not** need:

- Dart gameplay bridge;
- Flutter web editor;
- project filesystem;
- asset importer parity;
- audio;
- physics;
- production input;
- cloud build;
- browser security model;
- deployment productization.

Record unsupported assumptions and then stop browser work.

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
fynkel run
fynkel test

fynkel editor .
fynkel validate
fynkel build macos
```

The exact commands may change, but the experience should remain compact and predictable.

### 13.4 Runtime package strategy

Prefer prebuilt, signed/checksummed native runtime artifacts for supported target/toolchain combinations so ordinary game developers do not need a Rust toolchain merely to use Fynkel.

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

fynkel test --headless --steps 600 --seed 42 --json
fynkel run --test-profile reference
fynkel capture --scene Playground --output preview.png

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
  -> M3 complete code-first game
  -> M4 Editor V0
  -> M5 game-development completeness
  -> M6 native qualification/optimization
  -> M7 beta / 1.0
```

Browser product work is not on the 1.0 critical path.

### M0 — Architecture proof, repository bootstrap, and go/no-go

**Purpose:** retire the highest architectural risks before building broad engine functionality.

**Build**

- Public repository/monorepo structure.
- Toolchain/version locks.
- ADR-0001 through ADR-0007 drafts/decisions.
- License/contribution/trademark decision.
- Frozen reference-game specification.
- Standalone Rust/wgpu baseline renderer.
- Flutter-hosted viewport Path A prototype.
- At least one fallback viewport Path B/C prototype far enough to compare risk.
- Input/overlay/resize/lifecycle tests.
- Basic instrumentation.
- Tiny versioned C ABI round-trip.
- Minimal Rust/Wasm/WebGPU compatibility spike.
- Secondary native backend proof before closing the milestone.

**Primary validation**

- macOS/Metal.

**Preferred second validation**

- Windows/D3D12 when available; otherwise a documented second backend/platform chosen by ADR.

**Gate**

- A practical native Flutter/Fynkel presentation architecture passes §8 criteria.
- No production viewport path depends on full-frame CPU readback/re-upload.
- Added host/composition overhead is understood and within the approved budget.
- Overlay/input/resize/lifecycle behaviour is reliable.
- At least two native graphics/backend paths prove the renderer abstraction is not accidentally single-backend.
- The browser spike records portability blockers but does not expand scope.
- Repository governance/license/package strategy is decided.
- Reference benchmark manifest exists.

**Kill/fallback rule**

- If Flutter `Texture` fails, move to another approved native composition path.
- If every practical composition path fails, stop and redesign before M1.

**Do not**

- build the full editor;
- build a custom ECS;
- build advanced rendering;
- productize browser support;
- start five independent engine subsystems before the host boundary is proven.

### M1 — Runtime foundation, `hecs` wrapper, and bridge

**Depends on:** M0.

**Build**

- `fynkel_core`;
- `fynkel_ecs` facade over `hecs`;
- Fynkel runtime handles + mapping to internal `hecs::Entity`;
- component registration metadata;
- Transform/hierarchy;
- world lifecycle;
- explicit scheduler phases;
- command buffers;
- subscribed snapshots;
- events;
- no-renderer host tests;
- bridge diagnostics.

**Gate**

- Dart can create, inspect, update, and destroy objects through Fynkel APIs.
- Stale handles fail.
- Entity reuse does not alias stale Dart handles.
- Batched updates are used in hot paths.
- Snapshot subscriptions are bounded/measurable.
- world teardown is leak-safe.
- transform hierarchy passes cycle/reparent tests.
- `hecs` is not visible in public APIs/files.

**Do not**

- write a custom ECS;
- build a generic parallel scheduler;
- expose mutable native memory to Dart.

### M2 — Project documents, assets, prefabs, materials, and behaviours

**Depends on:** M1.

**Build**

- strict JSON schemas;
- scene documents;
- material documents;
- simple prefab format/override semantics;
- stable asset IDs/metadata;
- document migration framework;
- glTF/GLB importer;
- texture/audio import baseline;
- content-addressed/incremental cache keys;
- generated Dart behaviour registry;
- Inspector metadata;
- CLI validation;
- authoring commands independent of editor UI.

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

### M3 — Complete code-first reference game

**Depends on:** M2.

**Build the exact §1.2 reference game without relying on Fynkel Editor.**

Required runtime capability now includes:

- renderer V1;
- player/camera;
- physics;
- animation;
- audio;
- input actions;
- scenes/state transitions;
- prefab spawning;
- Flutter menus/HUD;
- world-space UI baseline;
- saveable settings;
- headless gameplay tests where applicable.

**Gate**

- a clean checkout can build/run the reference game through documented commands;
- the game is playable end-to-end;
- no editor is required;
- critical frame paths are instrumented;
- there are no hidden manual native setup steps beyond documented platform prerequisites;
- reference-game performance is recorded against the benchmark manifest.

**Do not**

- add speculative systems to make the demo impressive;
- start browser-editor product work;
- postpone usability problems to the editor.

### M4 — Fynkel Editor V0

**Depends on:** M2 plus stable/playable M3 integration.

**Build**

- Flutter desktop editor;
- Scene viewport;
- hierarchy;
- schema-generated Inspector;
- asset browser;
- selection;
- transform gizmos;
- save/load;
- undo/redo;
- Play/Stop;
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
- require full docking/multi-window framework;
- build browser parity;
- implement gameplay rules inside editor widgets.

### M5 — Practical game-development completeness

**Depends on:** M4.

**Build/finish**

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

- reference game can be authored comfortably in editor-first or hybrid mode;
- at least one external pilot project reaches a playable state;
- common workflows do not require undocumented internal APIs;
- migrations and error handling survive normal project evolution.

**Do not**

- introduce large systems unused by reference/pilot projects;
- stabilize a native plugin ABI just because 1.0 is approaching.

### M6 — Native platform qualification and optimization

**Depends on:** M3–M5. Target-specific work begins earlier where necessary.

**Build**

- versioned prebuilt native artifacts;
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

**Minimum release target set**
Must be decided by evidence. Do not add Linux/mobile to the marketing matrix until their gates pass.

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
| Licensing is postponed                   | ADR in M0 before public publishing                                                    |

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
- let the M0 web spike become a supported product by accident;
- assume Flutter `Texture` means zero-copy wgpu integration;
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

Track at least these issues:

1. Freeze toolchain matrix.
2. Decide repository/license/contribution/trademark policy.
3. Freeze §1.2 reference-game spec.
4. Build standalone macOS/Metal wgpu baseline.
5. Record standalone frame/presentation baseline.
6. Prototype Flutter `Texture`/platform-texture path.
7. Measure copies, latency, and composition overhead.
8. Prototype at least one fallback engine-owned surface/view path.
9. Add resize/DPI/minimize/background/dispose stress harness.
10. Add Flutter overlay + input/focus test.
11. Define native C ABI v0 framing.
12. Send one batched command buffer Dart → Rust and return one snapshot/event Rust → Dart.
13. Create `fynkel_ecs` wrapper with a tiny `hecs` world.
14. Validate stale-handle mapping design.
15. Validate a second native graphics/backend path.
16. Run the minimal Wasm/WebGPU compatibility spike.
17. Write M0 benchmark manifest and capture script.
18. Produce M0 architecture report and ADR decisions.

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

### 21.4 Standard component catalogue for 1.0

The following component families define the expected 1.0 baseline. Components may be implemented internally by different subsystems, but they must appear coherent in Dart, scene files, schemas, and the Inspector.

#### Core and hierarchy

- `Transform`
- `Name` or built-in object name metadata
- `ActiveState`
- Parent/child hierarchy relationship
- Optional `Tag` and `Layer` metadata if real use cases justify them

`Transform` exposes local position, rotation, and scale; read-only world-space state; explicit local/world setters; parenting with keep-local/keep-world policies; direction helpers; and change observation suitable for tooling. Avoid implicit expensive matrix decompositions in common getters.

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

## 22. Fynkel Editor UX and tooling specification

The **Fynkel Editor** should feel familiar to developers coming from Unity while remaining simpler, faster, and tightly aligned with Fynkel's code-first model. It is a visual client over the same documents, commands, schemas, runtime APIs, and validation used by the CLI and AI tooling.

### 22.1 UX principles

The editor must optimize for fast iteration rather than maximum visual complexity. The main loop should be: open project -> inspect scene -> modify object/component -> run -> observe -> stop -> continue editing.

The editor must never hide where project data lives. Every editable asset should map to a real project file or a documented generated/cache representation. When a user changes something visually, the corresponding source document must be updated through the authoring service.

Prefer keyboard-friendly workflows, searchable commands, predictable selection, and contextual inspectors. Avoid modal dialogs for routine operations.

### 22.2 Default workspace

The initial desktop layout should include:

```text
┌──────────────────────────────────────────────────────────────────┐
│ Project  Edit  Assets  Game  Debug  Window          ▶  ■  ⟳     │
├──────────────┬──────────────────────────────┬────────────────────┤
│ Hierarchy    │                              │ Inspector          │
│              │                              │                    │
│ Main Scene   │          Scene View          │ Player             │
│ ├ Player     │                              │ Transform          │
│ ├ Camera     │                              │ Position           │
│ ├ Sun        │                              │ Rotation           │
│ └ Ground     │                              │ Scale              │
│              │                              │ MeshRenderer       │
├──────────────┴──────────────────────────────┴────────────────────┤
│ Project / Assets | Console | Profiler | Animation | Import       │
└──────────────────────────────────────────────────────────────────┘
```

Allow panels to be resized and hidden early. Full arbitrary docking and multi-window layouts may arrive after the core workflow is stable; do not let a docking framework delay Editor V0.

### 22.3 Project browser and project lifecycle

The project launcher/editor shell should support:

- Create from versioned templates.
- Open an existing Flutter/Fynkel project.
- Recent projects.
- `fynkel doctor` results and missing dependency guidance.
- Fynkel SDK/runtime/tool version compatibility checks.
- Read-only mode when a project format is newer than the installed tool.
- Explicit migration preview and backup when upgrading project documents.

Never silently rewrite an entire project because the editor version changed.

### 22.4 Scene view

The Scene view is an authoring viewport, not the same thing as the Game view. It needs:

- Perspective and orthographic navigation.
- Orbit, pan, fly, focus-selection, and frame-all commands.
- Local/world transform gizmos.
- Move/rotate/scale snapping.
- Grid and origin visualization.
- Selection outlines and bounds.
- Light, camera, collider, audio, and other gizmos.
- Layer/visibility filtering.
- Debug draw overlays.
- Picking that remains responsive with large scenes.
- Camera preview where useful.

Separate scene-camera state from saved game cameras unless a user explicitly aligns one to the other.

### 22.5 Hierarchy

The hierarchy must show object parentage and scene boundaries clearly. Required capabilities:

- Multi-selection.
- Rename.
- Duplicate.
- Delete with undo.
- Drag/drop reparenting with explicit transform policy.
- Search/filter by name, component, tag, prefab state, or errors.
- Active/inactive state.
- Prefab instance indicators and override state.
- Context actions generated from the same command registry as menus/CLI where practical.

For very large scenes, use virtualization and lazy expansion. Do not rebuild thousands of Flutter widgets on every transform update.

### 22.6 Inspector generation

The Inspector should primarily be generated from component and behaviour schemas. Field metadata should drive labels, ranges, units, enum menus, asset pickers, object references, multiline fields, colour pickers, and read-only state.

Custom inspectors are an extension point, not the only way to make a component usable.

Inspector rules:

- Support multi-object editing where field semantics allow it.
- Show mixed values explicitly.
- Every edit participates in one transaction/undo history.
- Numeric drags and sliders coalesce into one undo operation.
- Invalid values show immediate validation without destroying the last valid runtime value.
- Asset/object reference fields support drag/drop and searchable pickers.
- Runtime-only values are visually distinguished from authored values.

### 22.7 Asset browser and import tooling

The asset browser should mirror the project's source assets rather than exposing an opaque editor database. Provide:

- Folder/tree and searchable flat views.
- Type filters.
- Thumbnails/previews for supported assets.
- Drag/drop into scenes and inspectors.
- Rename/move with stable asset-ID preservation.
- Duplicate with new asset identity.
- Import diagnostics and reimport.
- Source file -> imported artifact dependency visibility.
- References/dependents inspection.

Import settings are stored in committed metadata. The UI edits those settings and asks the shared import service to rebuild.

### 22.8 Prefab editing UX

Provide a focused prefab mode rather than treating prefabs as invisible scene serialization tricks. Users must be able to:

- Open a prefab asset in isolation.
- See prefab-local hierarchy and IDs.
- Instantiate into scenes.
- Distinguish inherited values from overrides.
- Revert an override.
- Apply a supported instance change back to the source prefab.
- Inspect missing/broken source prefab states.

Do not implement nested prefabs, variants, or complex inheritance until their data semantics are proven and well tested.

### 22.9 Material and shader tooling

Material editing should be schema-driven from the material/shader interface. V1 requires:

- PBR material Inspector.
- Texture slots with colour-space information.
- Render-state settings that are safe for users to change.
- Live preview sphere/plane/model.
- Validation of unsupported parameter combinations.
- Shader compilation errors with file and line information.

A visual shader graph is post-1.0 unless a real project justifies it.

### 22.10 Play mode and Game view

Keep authoring state and play state separate. Required behaviour:

- Play launches or connects to the actual game runtime.
- Stop returns to authoring without accidental persistence of runtime mutations.
- Pause and single-step may be added once simulation stepping is reliable.
- The Game view can attach to the player's rendered output where technically supported, or use a separate game window initially.
- Runtime selection/inspection is allowed but visually marked as runtime state.
- Applying a runtime value back to an authored field is an explicit command.

Do not make the editor process the only supported runtime host.

### 22.11 Console and diagnostics

The Console needs structured logs, not plain terminal dumping. Each message should carry severity, timestamp/tick, source system, object/asset IDs where relevant, and optional source location for Dart/shader code.

Support search, filtering, collapse/repetition counts, copy, clear, and navigation to the related object or asset.

Validation errors should appear both in the Console and inline where possible.

### 22.12 Profiler tooling

The first profiler should expose measured engine data before sophisticated visualization:

- Frame time and percentile history.
- Dart script time.
- Native simulation time.
- Physics time.
- Render extraction/preparation time.
- GPU pass timings where supported.
- Draw/dispatch counts.
- Visible/culled objects.
- Asset memory and GPU resource estimates.
- Command-buffer traffic across Dart/Rust.
- Flutter frame/UI timing where relevant.

Later add timeline/flame views, allocations, asset residency visualization, and capture comparison. Never report precision the underlying platform cannot actually measure.

### 22.13 Search and command palette

Add a command palette early. It should expose scene actions, project actions, asset creation, build/run commands, panel navigation, and extension commands.

This improves both keyboard workflows and discoverability and allows editor actions to share stable command IDs with automation.

### 22.14 IDE integration

Do not build a full Dart code editor initially. Provide:

- Open script in the user's configured IDE.
- Create Behaviour/component templates.
- Navigate from errors and components to source files.
- Generated-code locations and "do not edit" markers.
- Debug-launch configuration generation where feasible.
- Optional language-server/editor integrations later.

### 22.15 Undo, redo, autosave, and crash recovery

Undo/redo belongs to the authoring command layer, not individual widgets. Every mutation has enough information to invert or replay it. Group continuous interactions into transactions.

Autosave should write recoverable temporary revisions, not constantly overwrite committed source files. After a crash, offer recovery by comparing the recovery revision against the last explicit save.

Use atomic file replacement for writes where the platform permits it. Never leave a partially written scene as the only copy.

### 22.16 Multi-scene and large-project UX

After the single-scene workflow is stable, support additive/open multiple scenes, clear ownership of objects by scene, per-scene dirty state, scene activation, and cross-scene references only under documented rules.

For large projects, prioritize virtualization, indexed search, incremental asset scanning, cancellation, and background import work. Do not block the UI thread on full-project parsing/imports.

### 22.17 Future browser-editor parity constraints

Browser editing is **post-1.0** and is not part of the Editor V0 or Fynkel 1.0 acceptance gate.

However, desktop editor architecture should avoid unnecessary lock-in:

- authoring commands remain UI-independent;
- documents remain strict JSON;
- Inspector generation remains schema-driven;
- panels should not own project truth;
- project operations remain available through service interfaces;
- editor command IDs should be stable enough for future alternate clients.

Do not add browser-specific storage, companion-server, security, or JS/Wasm complexity to the desktop editor before 1.0.

When browser work begins after 1.0, it should preserve the same conceptual workspace where practical while clearly exposing browser-specific limitations.

### 22.18 Accessibility and internationalization

Use Flutter semantics/accessibility APIs for standard controls. Keyboard navigation, visible focus, scalable text where practical, and sufficient contrast should be treated as normal editor quality requirements. Keep editor strings localizable even if 1.0 ships only in English.

### 22.19 Editor acceptance gate

The **desktop** editor is 1.0-ready when a developer unfamiliar with Fynkel internals can:

- create/open the reference project;
- construct and edit the reference level;
- attach/configure behaviours;
- author materials;
- manage assets;
- run/stop the real game;
- diagnose common errors;
- inspect basic performance;
- undo ordinary mistakes;
- recover from crashes/autosave state;
- continue working through normal Dart/Flutter IDE tooling.

They must not need to manually edit internal UUIDs, cache files, `hecs` data, Rust handles, or generated registries.

Every source-changing editor mutation maps to a shared authoring command or a clearly documented editor-local preference.

Browser-editor parity is not part of this gate.

## 23. Fynkel extensibility and plugin architecture

Fynkel must be extensible without forcing users to fork the engine, but a broad/stable plugin platform is **not a prerequisite for 1.0**.

The 1.0 priority is to establish clean internal boundaries and first-party extension points that can later become stable public plugin contracts.

### 23.1 Extension layers

Treat these as separate mechanisms:

| Layer                                                  | Typical use                            | 1.0 status                 |
| ------------------------------------------------------ | -------------------------------------- | -------------------------- |
| Dart package                                           | reusable gameplay libraries/behaviours | Supported                  |
| Generated custom Behaviour metadata                    | user gameplay types                    | Supported                  |
| Custom data/components through documented registration | project/game extensions                | Supported where safe       |
| Editor command/Inspector extensions                    | project tooling                        | Limited/experimental       |
| Importer extensions                                    | new asset formats/processors           | First-party/internal first |
| Build extensions                                       | project-specific build/codegen steps   | Limited                    |
| Native Rust plugin                                     | high-performance engine extension      | Post-1.0 stable API        |
| Renderer plugin/pass API                               | custom render pipeline integration     | Post-1.0 stable API        |

This prevents “plugin system” from becoming one enormous ABI commitment.

### 23.2 1.0 extensibility contract

For 1.0, guarantee primarily:

- normal Dart/pub packages can provide reusable gameplay code;
- custom `Behaviour` classes register through Fynkel code generation;
- serialized custom behaviour fields are schema-described;
- projects can define reusable higher-level systems without modifying Fynkel source;
- editor Inspector generation can render normal exposed fields automatically;
- command-line validation understands registered project types;
- unknown extension data is preserved safely when possible.

Do not make a game developer write a native Rust plugin for ordinary gameplay.

### 23.3 Plugin manifest direction

A future plugin/package manifest should include:

```json
{
  "id": "com.example.fynkel.terrain",
  "version": "1.2.0",
  "fynkel": ">=1.3.0 <2.0.0",
  "capabilities": ["editor", "importer"],
  "entrypoints": {
    "dart": "package:example_terrain/plugin.dart"
  }
}
```

The final schema may differ.

A manifest must declare:

- stable plugin ID;
- version;
- compatible Fynkel range;
- capabilities;
- required dependencies;
- platform support;
- execution/trust requirements;
- optional editor/runtime/build entrypoints.

### 23.4 Dart gameplay extensions

Dart packages are the preferred early ecosystem layer.

They can provide:

- behaviours;
- controllers;
- game systems;
- helper libraries;
- state machines;
- camera systems;
- gameplay AI;
- reusable UI;
- test helpers.

They operate through public Fynkel APIs and must not depend on private native layouts.

### 23.5 Custom component/type registration

Fynkel should distinguish:

1. built-in native components;
2. Dart Behaviour state;
3. future custom native/data components.

Before exposing general custom native component storage, define:

- stable type IDs;
- serialization schema;
- default values;
- migration hooks;
- editor metadata;
- lifecycle;
- threading/access rules.

Do not expose `hecs` component insertion directly as the public extension API.

### 23.6 Editor extensions

After Editor V0 is stable, experimental editor extension points may include:

- custom Inspector widgets;
- panels;
- menu/command palette commands;
- asset previews;
- project validation;
- tooling actions.

Editor extensions use the shared authoring command service rather than mutating source files behind the editor's back.

The stable editor plugin SDK can arrive after 1.0 once real extension patterns are known.

### 23.7 Importer extensions

Importer plugins may eventually provide:

- recognized file extensions/MIME signatures;
- import settings schema;
- dependency discovery;
- deterministic import/cook operation;
- preview metadata;
- diagnostics;
- target-specific cooking.

First build Fynkel's own glTF/texture/audio import pipeline. Public importer APIs should be extracted from working internal contracts, not invented beforehand.

### 23.8 Native Rust plugins — post-1.0 stable API

A stable native plugin ABI is deliberately post-1.0.

Reasons:

- runtime internals will still change rapidly before 1.0;
- freezing ECS/renderer/threading details too early would damage optimization freedom;
- unsafe third-party native code introduces crash/security/support complexity.

When implemented, expose:

- opaque Fynkel handles;
- versioned function tables;
- explicit alloc/free ownership;
- thread constraints;
- logging/diagnostics hooks;
- capability negotiation;
- registration functions.

Never expose direct ownership of the wgpu device, `hecs::World`, Rapier world, or raw internal pointers as the first native plugin ABI.

### 23.9 Scheduler/system extensions

A future system extension API should register logical requirements such as:

```text
phase: PrePhysics
reads: Transform, Velocity
writes: CustomComponent
ordering: after game.SomeSystem
```

Fynkel decides the actual execution/thread scheduling.

Do not let plugins assume a specific worker thread or `hecs` archetype/storage layout.

### 23.10 Rendering extensions

Rendering extensibility should start high-level:

- custom material/shader packages;
- custom post effects through approved interfaces;
- render-feature registration;
- debug rendering.

Delay arbitrary render-graph/device access until the renderer is stable enough to support it without freezing internals.

### 23.11 Build extensions

Future build extensions may contribute:

- validation;
- code generation;
- asset cooking;
- packaging steps.

They must:

- declare inputs/outputs;
- support structured diagnostics;
- avoid writing arbitrary undocumented project state;
- be reproducible enough for CI.

### 23.12 Dependency resolution and lockfiles

When a plugin ecosystem exists:

- record exact resolved plugin versions;
- detect incompatible Fynkel ranges;
- expose transitive dependencies;
- support reproducible restoration;
- surface conflicts before build/run.

Do not allow an editor to silently upgrade project extensions.

### 23.13 Security and trust

Code plugins are executable code.

The editor/tooling should surface when a project introduces:

- a native plugin;
- an editor extension;
- an importer/build extension;
- a script/package requiring execution.

Trust decisions must be explicit.

Signed packages may be introduced later, but signatures do not replace sandboxing/review/permission design.

### 23.14 Plugin testing

A future stable plugin SDK requires conformance tests for:

- load/unload;
- version rejection;
- state cleanup;
- project migration;
- error containment;
- editor reload;
- build/package integration;
- unsupported platform behaviour.

Native plugin failures must not corrupt source documents.

### 23.15 Plugin distribution

Early ecosystem distribution should use the normal Dart/Rust/package ecosystems where practical.

A dedicated Fynkel registry/marketplace is post-1.0 and must not be needed to build a normal Fynkel project.

### 23.16 Extensibility acceptance rule

For **Fynkel 1.0**, extensibility is sufficient when external developers can build reusable gameplay packages and registered behaviours without forking the engine.

A **stable public plugin SDK** is ready only later, after at least several real external extension use cases validate the contracts.

Do not block 1.0 on a marketplace-grade plugin platform.

## 24. Fynkel post-1.0 roadmap

Post-1.0 work should be driven by shipped games, profiler data, platform demand, and ecosystem feedback. This roadmap defines likely directions and sequencing; it is not a commitment to implement every item. Protect 1.0 stability before pursuing breadth.

### 24.1 Roadmap principles

After 1.0:

1. Protect shipped projects and migration quality before adding breadth.
2. Improve workflows proven by real projects before speculative systems.
3. Keep advanced systems modular so small games do not pay for unused features.
4. Benchmark every major renderer/runtime change against frozen workloads.
5. Prefer established libraries/formats unless a measured limitation justifies replacement.
6. Keep CLI/editor/automation parity for new authoring features.
7. Revisit `hecs` only through the replacement gate in §4.3.
8. Begin browser product work only after the native engine is demonstrably stable enough that it will not starve core development.
9. Make stable plugin APIs from proven internal extension contracts rather than guesses.

### 24.2 Phase A — 1.x stability, profiling, and workflow depth

The first releases after 1.0 should concentrate on what shipped projects reveal:

- crash/data-loss fixes;
- faster startup;
- incremental import improvements;
- clearer diagnostics;
- project migration quality;
- editor scaling;
- profiler capture/timeline improvements;
- resource/memory leak tooling;
- SDK ergonomics;
- templates/tutorials;
- headless test depth;
- supported-platform hardening;
- additional native platforms that pass qualification;
- stable plugin APIs only where repeated external use cases exist.

Also benchmark `hecs` with real shipped workloads.

Do not start a custom ECS project unless §4.3's gate is satisfied.

Avoid turning Fynkel 1.1 into another pre-1.0 scope explosion.

### 24.3 Rendering roadmap

Likely renderer improvements, introduced from measured need:

**Near-term 1.x:**

- Better batching and instancing.
- Hierarchical/frustum culling improvements.
- LOD groups.
- Improved shadow quality/cascades.
- Bloom, SSAO, tone-mapping choices, and selected post effects.
- GPU skinning optimization.
- Pipeline/material cache improvements.
- RenderDoc/graphics-debugging workflow documentation.

**Later:**

- Forward+ or clustered lighting if scene/light counts justify it.
- GPU-driven visibility and indirect drawing.
- Occlusion culling.
- Temporal anti-aliasing/upscaling where platform quality is sufficient.
- Reflection/probe improvements.
- Large-scene streaming integration.
- Experimental ray tracing only on targets and use cases that justify the maintenance cost.

Do not add effects merely to match engine feature checklists. Image quality, performance, and platform support must be measured together.

### 24.4 Terrain and large worlds

Terrain should arrive as a modular system rather than a special case inside the core ECS. Likely capabilities:

- Heightfield terrain assets.
- Chunked streaming.
- LOD.
- Splat/material layers.
- Collision integration.
- Vegetation/instance scattering.
- Editor sculpt/paint tools.

Large-world support may later add world partitioning, origin rebasing or high-precision coordinates, asynchronous scene/chunk streaming, HLOD, and background asset residency policies.

Do not choose a large-world coordinate architecture until target game scale is known.

### 24.5 Navigation and AI systems

Navigation can become a first-party optional module:

- Navmesh generation/import.
- Runtime path queries.
- Dynamic obstacles.
- Off-mesh links.
- Area costs.
- Editor visualization.
- Incremental/baked workflows as required.

Keep high-level game AI such as behaviour trees, GOAP, or utility AI outside the mandatory core. They can be packages/plugins unless several projects demonstrate a need for a first-party implementation.

### 24.6 Particle and VFX systems

A VFX roadmap can progress from CPU-authored/GPU-rendered particles to more advanced GPU simulation:

- Emitters.
- Curves/gradients.
- Sprite/mesh particles.
- Collision options.
- Trails.
- GPU simulation.
- VFX graph only if node-based authoring proves worthwhile.

The initial engine should not have delayed 1.0 waiting for a visual VFX graph.

### 24.7 Advanced animation

Possible post-1.0 additions:

- Blend trees.
- Layered/additive animation.
- Masks.
- Better animation graphs/state-machine tooling.
- IK constraints.
- Retargeting.
- Animation compression and streaming.
- Timeline/cinematic sequencing.

Animation authoring features should continue to use explicit assets and schemas so they remain editable by tools and AI.

### 24.8 Networking and multiplayer

Do not make one networking model part of the mandatory runtime. Start with transport-agnostic interfaces and sample integrations.

Potential first-party modules later:

- Client/server transport abstraction.
- Replication primitives.
- Snapshot interpolation.
- RPC/message schema generation.
- Dedicated/headless server target.
- Interest management.

Rollback/prediction requires deterministic simulation policies far beyond simply using a fixed timestep. Treat competitive rollback networking as a specialized later project, not an assumed engine feature.

### 24.9 Browser and cloud development

Browser support begins as a **post-1.0 product track**. The M0 web spike exists only to keep the architecture compatible enough to revisit this work.

Recommended sequence:

**Phase 1 — Production web runtime**

- qualify Rust/Wasm/wgpu WebGPU rendering;
- define the Dart/web bridge;
- define browser input/audio/lifecycle;
- establish asset/package size budgets;
- test named browser/version combinations.

**Phase 2 — Connected browser editor**

- Flutter web editor client;
- shared authoring commands/schemas;
- authenticated local companion for project filesystem and native build/run;
- clear trust, origin, path, and execution boundaries.

**Phase 3 — Browser-only authoring**

- browser project storage/import/export;
- scene/material/prefab editing without a local companion;
- browser-safe preview runtime.

**Phase 4 — Optional hosted services**

- remote builds;
- hosted project storage;
- shareable previews;
- collaboration.

A cloud compiler/build service is a separate security, operations, pricing, and reliability commitment. Never make it a hidden requirement for local/open development.

Browser work must not force the native 1.x runtime to accept significant complexity without a measured benefit.

### 24.10 Collaboration and source-control tooling

Potential improvements:

- Document-aware diff viewers.
- Scene/prefab merge assistance using stable IDs.
- Locking/check-out workflows for teams that prefer them.
- Change review inside the editor.
- Git integration for status and history without replacing Git.
- Multi-user live editing only after revision/transaction semantics and conflict handling are mature.

Text/source-control friendliness remains the baseline even if richer collaboration tools exist.

### 24.11 Visual scripting

Visual scripting is optional and post-1.0. If implemented, it should compile or lower into the same gameplay/service APIs used by Dart rather than creating a second unrelated engine model.

Requirements before starting:

- Proven audience demand.
- Stable underlying SDK.
- Debugging/stack trace design.
- Versionable graph format.
- Source-control-friendly IDs.
- Interop with Dart behaviours.

Do not allow visual scripting to make ordinary engine features inaccessible from code.

### 24.12 Shader graph and material authoring

A shader graph may eventually generate WGSL plus the standard shader interface manifest. The generated form must remain inspectable/debuggable and cacheable.

The graph is an authoring convenience; the renderer should still consume the same compiled shader/material contracts as hand-authored shaders.

### 24.13 AR, VR, and spatial computing

Treat XR as a platform module with explicit demand. Work includes:

- OpenXR or platform-specific runtime evaluation.
- Stereo/multiview rendering.
- Head/controller tracking.
- Spatial input.
- Performance profiles.
- True world-space UI improvements.

Do not claim XR support because wgpu can render to a GPU. Runtime/session/input integration is separate work.

### 24.14 Console support

Console support should be pursued only through legitimate platform developer programs and SDK agreements. It requires platform-specific rendering, input, packaging, certification, crash/performance tooling, and potentially restricted repositories under NDA.

Keep console integrations behind platform interfaces so restricted code does not leak into the public engine repository.

### 24.15 Mobile-specific depth

Post-1.0 mobile work may include:

- Dynamic resolution.
- Device quality tiers.
- Thermal-aware settings guidance.
- Touch/gesture templates.
- Better background/resume behavior.
- Package-size optimization.
- Mobile GPU profiling and shader/pipeline tuning.

Do not optimize desktop workloads and assume the same settings are appropriate on mobile.

### 24.16 Ecosystem and templates

Grow reusable first-party/sample packages rather than bloating the engine core:

- Third-person controller.
- First-person controller.
- Orbit camera.
- Save-system example.
- Inventory/UI sample.
- Basic networking examples.
- Terrain/navigation plugins when mature.
- Genre starter templates.

Templates should be production-quality examples of engine architecture, not abandoned demos.

### 24.17 AI-native evolution

Continue treating machine-readable engine control as infrastructure rather than a chatbot feature. Possible additions:

- Semantic project/scene inspection APIs.
- Structured runtime state queries.
- Automated screenshot comparison.
- Replayable test scenarios.
- Asset-generation/import hooks with provenance metadata.
- Safe project patch planning and dry-run validation.
- Editor command execution through the same stable command IDs.

AI integrations should never bypass validation, trust boundaries, source control, or the normal engine APIs.

### 24.18 Version 2.0 criteria

Do not create a 2.0 merely for marketing. A major version is justified when an architectural improvement requires breaking public SDK, plugin, bridge, or source-format contracts and migration cannot reasonably preserve compatibility.

Before a 2.0, provide migration tools, compatibility documentation, benchmark evidence for the change, and a long enough preview period for real projects to test it.

### 24.19 Long-term success criteria

Fynkel is successful if independent developers/teams can ship complete games while choosing code-first, editor-first, or hybrid workflows; project data remains transparent and durable; the native runtime performs competitively on clearly defined workloads; developers can extend common gameplay without engine forks; and the SDK/editor/CLI/AI tooling all operate on the same underlying contracts.

Later browser, cloud, plugin, console, networking, terrain, XR, and visual tooling are successful only if they extend that foundation rather than becoming separate incompatible products.

Feature count is not the primary metric.

The stronger long-term metrics are:

- games shipped;
- time from project creation to playable result;
- iteration/build times;
- crash/data-loss rate;
- upgrade/migration success;
- measured performance;
- documentation success;
- external package/plugin reuse;
- ability of both humans and automation to diagnose and modify projects safely.

Review the roadmap after substantial releases using real project evidence, issue trends, profiling data, and direct developer feedback.

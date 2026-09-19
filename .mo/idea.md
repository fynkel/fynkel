# Fynkel Engine — Development Blueprint

**Status:** Proposed architecture and implementation plan, version 0.2  
**Prepared:** 18 September 2026  
**Brand updated:** 19 September 2026  
**Audience:** Runtime, rendering, Flutter/Dart, editor, tooling, and quality engineers  
**Objective:** Build **Fynkel Engine**, a Dart-first 3D game engine with a Unity-like authoring workflow, a Rust runtime, and optional Flutter desktop/browser editors.

All package names, commands, APIs, file schemas, and performance budgets below are proposed contracts, not descriptions of an already implemented product. Milestones are acceptance gates, not calendar estimates. Platform support is earned through testing, not inferred from dependency support.

### Naming and brand conventions

The official product name is **Fynkel Engine**. Use **Fynkel** as the short product name in normal developer-facing copy. Use `fynkel` for the CLI command and primary Dart package, `fynkel_*` for engine-owned Rust/Dart modules where a qualified package name is useful, and `fynkel.*` for built-in serialized type IDs. A shipped-game attribution may use **Made with Fynkel**. Generic words such as “engine”, “runtime”, “editor”, and “renderer” remain lowercase when they describe a technical concept rather than the product name.

## 1. Product definition and scope

Build **Fynkel Engine** with three equally valid workflows: code-first, editor-first, and hybrid. A game is a normal Flutter project with the **Fynkel SDK** added. The editor reads and writes the same version-controlled project files that developers, command-line tools, and AI agents use.

The public workflow should be familiar to Unity users: GameObjects, components, behaviours, a Transform hierarchy, scenes, prefabs, materials, an Inspector, and Play/Stop. Internally, GameObjects are handles into an entity-component system (ECS); they are not heavyweight object trees containing the simulation.

The first release should support small and medium 3D games and interactive applications. The reference game is a compact third-person collect-and-interact game with a menu, player movement, collisions, collectibles, animation, sound, a Flutter HUD, and a win/restart loop. Use that game to make scope decisions.

**Non-negotiable principles**

1. A complete game must build and run without the editor. The Rust core must also support no-renderer tests; a game using Dart behaviours still requires a Dart execution host.
2. Rust owns native simulation and engine resources. Dart owns gameplay behaviours and Flutter application UI. Explicit contracts govern shared state.
3. Project data is readable, versioned, validated, and editable without GUI automation.
4. Runtime, authoring tools, and editor applications are separate deliverables. Editor code must not enter production game builds.
5. Performance, native rendering integration, web compatibility, and resource lifetime are tested from the first milestone.

**Initial release scope:** hierarchy and components; Dart behaviours; glTF import; PBR rendering; bounded lighting/shadow quality; physics and scene queries; basic skeletal animation; audio; input; scenes and simple prefabs; Flutter HUD; native world-space panels/text; desktop editor; CLI; native release exports; connected browser-editor mode.

**Deferred scope:** console support, networked multiplayer/rollback, terrain systems, advanced global illumination, ray tracing, shader graphs, visual scripting, complex prefab inheritance, marketplace, hosted collaboration, large-scale streaming worlds, and arbitrary Flutter widgets rendered into 3D textures.

## 2. System architecture

```text
Normal Flutter game project
  Dart behaviours + Flutter UI + Fynkel SDK
                    |
      FynkelBridge: commands / snapshots / events
             /                         \
      Native C ABI              Browser JS/Wasm bridge
             |                         |
             +------- Rust runtime ----+
                      ECS / transforms
                      physics / animation / audio
                      asset runtime / renderer
                                |
                               wgpu

Development only
  Fynkel Editor / Fynkel CLI / AI tools
                    |
        Versioned authoring command service
                    |
      Project documents / importers / asset compiler
                    |
       Compiled assets + generated Dart registry
```

`wgpu` supplies graphics abstraction, not a complete game renderer or a ready-made Flutter integration. Its documented native and browser backends support the overall direction, but every target needs its own verified integration path. [S1]

### Repository layout

```text
fynkel/
  crates/
    fynkel_contracts/       # IDs, wire records, versions, schema metadata
    fynkel_core/            # world lifecycle, time, services
    fynkel_ecs/             # entity allocation, storage, queries
    fynkel_scene/           # hierarchy, runtime scene instantiation
    fynkel_render/          # wgpu renderer and render extraction
    fynkel_assets/          # runtime handles, loading, residency
    fynkel_physics/         # Rapier adapter
    fynkel_animation/
    fynkel_audio/
    fynkel_world_ui/
    fynkel_document/        # source documents, validation, edit transactions
    fynkel_import/          # development-only import and cooking
    fynkel_native/          # C ABI and native host boundary
    fynkel_web/             # Wasm exports and browser adaptation
  packages/
    fynkel/                 # Flutter-independent Dart gameplay API
    fynkel_flutter/         # viewport, Flutter UI/input integration
    fynkel_codegen/         # Dart behaviour registration and metadata
    fynkel_tools/           # project-local tool launcher
  apps/
    editor/                 # Fynkel Editor: Flutter desktop and web UI
    cli/                    # Fynkel CLI: native project/build/authoring tools
  platform/                 # platform-specific texture and lifecycle plugins
  schemas/                  # engine-owned schema source and fixtures
  examples/reference_game/
  tests/{contracts,fixtures,integration,performance}/
  docs/{architecture,decisions,guides}/
```

These are logical boundaries. Begin with fewer crates if appropriate; do not create empty crates solely to match the diagram. Keep dependencies one-directional: runtime must not depend on editor/import tools; contracts must not depend on Flutter, wgpu, or a particular ECS implementation.

### Dependency policy

Use Rust, `wgpu`, WGSL shaders, `glam`, `serde`/`serde_json`, Rapier, and a thin C ABI. Evaluate Kira behind an audio interface, including real-device and browser backend tests before commitment. glTF/GLB is the first 3D interchange format. Rapier, glam, Kira, and glTF cover existing functionality that should not be rebuilt merely to avoid dependencies. [S2–S5]

Do not use `bevy_ecs` in the production design. Implement a deliberately small engine-owned ECS, but measure the tradeoff. `bevy_ecs` is independently usable; avoiding it is not proof of a faster or smaller engine. Use `hecs` as an optional development-only comparison because it explicitly targets a minimalist ECS library. [S6–S7]

Pin a tested Rust/Flutter/Dart/dependency matrix. Record compiled size, compile time, enabled features, platform support, licenses, and upgrade burden. Review updates; do not blindly track every latest release. Do not expose dependency-specific handles or types in public scene files or the Dart SDK.

## 3. Public object and scripting model

Expose `GameObject`, `Component`, `Behaviour`, `Transform`, `Scene`, `Prefab`, and `AssetRef<T>`.

A GameObject is an ergonomic facade over an entity. Native components such as Transform, MeshRenderer, Collider, and Animator live in Rust. Dart behaviours hold ordinary gameplay state and call SDK operations. Do not turn every native component into a separate mutable Dart-owned copy.

Distinguish three kinds of identity:

- **Authoring IDs:** persistent UUID strings for assets, entities, and component instances. Names and paths are labels, not identities.
- **Runtime handles:** world identity plus entity index and generation. A reused slot must never validate an old handle.
- **Type IDs:** stable namespaced component/behaviour identifiers, with versioned field metadata. Do not serialize Rust runtime type IDs or unqualified Dart class names.

Use two 32-bit words or another explicitly safe encoding for runtime handles at the browser bridge. Never assume a packed 64-bit handle can be round-tripped through an ordinary JavaScript number without a deliberate representation.

### Behaviour lifecycle

Define `onCreate`, `onEnable`, `start`, `fixedUpdate`, `update`, `lateUpdate`, `onDisable`, and `onDestroy`, plus collision/trigger callbacks. A newly instantiated scene must resolve all object and asset references before start callbacks. `start` runs once before the first update while active. Disabling pauses update callbacks; destruction is deferred to a safe boundary. Publish exact event ordering and cover it with tests.

Permit multiple behaviours per object. Native component metadata declares whether a type is unique. Avoid universal per-object ticking: static objects need no behaviour allocation or update call; behaviours opt into the callbacks they need.

### Generated registration, not runtime class discovery

Flutter does not support `dart:mirrors`; generate behaviour factories, field serializers, inspector metadata, and registries at build time. [S8]

For example, a proposed `@FynkelBehaviour(id: 'game.PlayerController')` annotation and `@Expose()` field generate a factory and schema. Rust-owned component definitions generate the equivalent schema and Dart bindings from their owning source. Merge both into one versioned project registry, rejecting duplicate type IDs and schema mismatches.

The editor reads this metadata without executing gameplay. Play compiles and launches the project's own Dart program. A prebuilt editor must not assume it can dynamically load an arbitrary Dart class merely because its filename appears in a scene.

### Code-first and generated worlds

Support creating objects, attaching components, loading scenes, and instantiating prefabs entirely through Dart. Support typed bulk operations as well as convenient per-object methods.

Procedurally created runtime objects are inspectable but are not automatically editable source code. An explicit bake/export operation can save an authoring snapshot. The editor must never pretend moving a runtime object rewrites the Dart generator that produced it.

## 4. ECS, hierarchy, and memory ownership

Start with generational entity allocation, typed dense component arrays plus sparse entity lookup, read/write queries, resources, change tracking, deferred structural commands, and an explicitly ordered scheduler.

Do not initially implement a general parallel scheduler or a full archetype/chunk engine. Begin with clear single-writer ownership. Benchmark sparse-set joins and realistic mutation patterns before changing storage. Keep storage private so a future change does not alter project files or public handles.

Required correctness tests include stale handles, entity reuse, component destruction, query aliasing, mutation during iteration, world shutdown, asset references, and large spawn/despawn batches. Minimize and isolate unsafe code. Any unsafe storage code requires documented invariants and targeted validation.

Keep hierarchy and ECS conceptually distinct. Parent/child relationships determine transform propagation and authoring structure, not memory ownership of arbitrary components. Cache world transforms and update dirty subtrees. Reject cycles. Define whether reparenting preserves world or local transform; make both explicit operations.

Adopt one coordinate convention: right-handed, Y up, metres, seconds, radians in APIs, quaternion storage `[x,y,z,w]`, and documented matrix layout. The Inspector can display Euler degrees. Normalize asset imports into that convention and test camera, collider, skinning, and picking agreement. Restrict unsupported non-uniform scale on dynamic physics bodies instead of silently producing incorrect results.

Rust owns ECS storage, native resources, and authoritative native component state. Dart may read immutable snapshots and keep local behaviour state. No SDK object may retain an unscoped pointer into relocatable ECS storage.

## 5. Simulation, bridge, and scheduling contract

### Logical tick order

```text
Capture tick input and completed events
  -> read snapshot S[n]
  -> Dart fixedUpdate builds command batch C[n]
  -> validate/apply C[n] at tick boundary
  -> native pre-physics systems
  -> fixed physics step
  -> transform synchronization and required animation work
  -> deferred structural changes
  -> publish S[n+1], events, and render extraction
```

Start with a configurable 60 Hz simulation. Rendering uses the latest completed state and interpolation where needed. `update`/`lateUpdate` are presentation-rate callbacks; authoritative simulation writes from them are queued for a documented next tick, not applied to arbitrary in-progress physics.

For V0, a host-driven batched native step is acceptable if measured inside its CPU budget. Keep it outside widget `build()` and prohibit synchronous I/O. A native simulation worker is an optimization with the same tick contract, not an excuse for undefined command timing.

On native targets, render/asset workers may operate on immutable extracts and owned jobs. Do not call Dart from arbitrary Rust worker threads. Native completions must use an approved host messaging mechanism. Begin with browser single-thread correctness; additional workers are optional acceleration, not a prerequisite. Flutter web's Dart isolate support differs from native Flutter, so the web path cannot simply reuse an isolate-based native design. [S9]

If Dart misses a deadline, keep rendering the last coherent state; never silently advance a tick that requires missing gameplay commands. Bound catch-up steps and outstanding work. During a debugger pause, pause the simulation. Report dropped wall-clock time rather than allowing an unbounded backlog. A fixed timestep alone is not a promise of cross-platform deterministic physics; Rapier's determinism requires additional configuration and application-wide conditions. [S10]

### Bridge contract

Use a stable, versioned C ABI for native and matching semantic operations through JS/Wasm for web. Define create/destroy world, submit commands, advance simulation, subscribe/read snapshots, drain events, request queries, and retrieve diagnostics. Flutter texture/lifecycle registration belongs in the platform integration layer.

Per-frame traffic uses reusable typed/binary buffers, not JSON and not a message per property. Define byte order, scalar widths, alignment, buffer lifetime, count/length validation, request IDs, protocol version, schema fingerprint, tick number, and error records. Do not pass Rust struct layouts, `glam` layouts, or WGSL uniform layouts across boundaries by assumption.

Read subscriptions should cover only needed components, fields, and objects; do not mirror the entire world into Dart every frame. Writes enter a staging buffer. Define read-your-writes behaviour: getters may consult that staging overlay, then the last acknowledged snapshot. They must not conceal an expensive native query on every property access.

Structural batches return mappings from temporary creation IDs to runtime handles. Batch application has documented transaction or partial-failure semantics; authoring transactions are all-or-nothing. Physics queries that must affect the current tick use an explicitly budgeted query phase, not an accidental one-frame-late answer.

Never unwind Rust panics across the ABI. Use status/error records and a tested panic boundary. Avoid callback reentrancy while storage is borrowed. Destroy worlds and GPU resources explicitly; finalizers are leak protection, not the primary lifetime mechanism.

## 6. Source formats: strict JSON and schemas

Use **strict JSON** for the initial authoritative scene, prefab, material, and engine project files. This deliberately refines the earlier JSON5 suggestion: one strict machine contract is the simpler starting point for schemas, agents, formatters, and round-trip tests. Comments can live in explicit `description` or `editorNotes` fields. An optional JSON5 import layer can be added later; do not maintain two authoritative formats.

Suggested extensions are `.scene.json`, `.prefab.json`, `.material.json`, and `.shader.json`. Validate structural data with JSON Schema, pinning the supported dialect. Add semantic validation for references, component uniqueness, prefab cycles, unsupported physics configurations, and shader compatibility. JSON Schema supplies a standard vocabulary, but it does not define those engine-specific semantics. [S11]

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

Begin with GLB/glTF, PNG/JPEG, selected HDR/environment inputs, and a deliberately small audio format set. glTF is an asset interchange format, not the engine's scene/prefab behavioural format. [S5]

Cache by source content, dependencies, settings, compiler version, and target profile. Include dependency hashes so changing a referenced texture invalidates the right material/model output. Distinguish stable identity from cache hash.

Use source JSON directly during early development where convenient. Cook production assets into a documented versioned container with explicit lengths, checksums, dependency IDs, and bounded chunk decoding. Binary scene data is an implementation choice to benchmark, not inherently faster just because it is binary. Never serialize raw Rust memory layouts.

Keep expensive decoding, mesh processing, and compression out of frame-critical execution. Upload GPU work in bounded chunks. Implement asset states, cancellation, ownership, reference tracking, eviction policy, fallback resources, and hot replacement only after a replacement validates. Keep the last valid asset when an edit fails.

Imports must validate paths, file sizes, recursion depth, decompression limits, external URI handling, and resource budgets. Opening a project or importing a model must not run arbitrary scripts.

### Headless data operations

Scene load/save, validation, prefab expansion, migration, and cooking must work without a window. Support asynchronous scene loading and explicit activation. Resolve references before lifecycle callbacks, release resources on unload, and distinguish additive loading from replacing the active scene.

Migrations are explicit, versioned, testable transformations with dry-run diffs and backups. Do not rewrite an entire project automatically when an incompatible editor opens it.

## 8. Renderer and Flutter integration

### Integration prototype first

Implement two test hosts: a minimal standalone Rust renderer host and a Flutter-hosted viewport. Compare equivalent scenes to isolate embedding overhead.

Flutter's Texture widget uses platform-managed texture registries; it does not promise direct import of every arbitrary wgpu texture. A separate platform plugin may be needed even when ordinary native functions are exposed through FFI. [S12–S13]

For each target, document the graphics API, GPU adapter/device relationship, surface/texture ownership, synchronization primitive, colour space, alpha convention, and copies. Test resize, DPI changes, clipping, overlapping Flutter UI, minimize/restore, app backgrounding, device loss, and resource disposal.

Prefer supported GPU-resident sharing or a measured GPU copy. A production path that reads a full frame back to CPU and uploads it again every frame fails the intended performance design unless a deliberately limited fallback is approved. Do not promise zero-copy before measuring it. Do not fork Flutter in the first milestone.

### Renderer V1

Build forward rendering with perspective/orthographic cameras, static meshes, indexed drawing, textures/samplers, depth testing, frustum culling, instancing, PBR, one initial shadowed directional light, bounded additional lights, environment lighting, HDR intermediate colour, tone mapping, selected anti-aliasing, alpha masking/blending, and debug primitives.

Use explicit pass inputs and outputs from the beginning: shadow, opaque, transparent/world UI, post-processing, presentation. A small render graph should manage dependencies and resource lifetime; do not initially build a general-purpose optimizing graph or force a depth prepass on every target.

Extract immutable render data from simulation. Reuse allocations, batch compatible objects, cache pipelines/material bindings, and place bounds on pipeline variants. Baseline rendering must not require advanced GPU features on every target. Use capability profiles and explicit quality fallbacks.

### Later rendering work

Add skeletal skinning and practical animation with the reference game, then measured LOD, clustered lighting, occlusion, indirect drawing, and additional post-processing. Make each advanced feature earn its cost through benchmark improvement or a concrete game requirement. Do not implement TAA, SSAO, SSR, and GPU-driven rendering as an undifferentiated checklist.

## 9. Physics, animation, audio, and input

**Physics:** Integrate Rapier through engine-owned components and handles. Support static/dynamic/kinematic bodies, primitive colliders, collision layers, sensors, raycasts/shape casts, collision events, and a character controller. Add common joints and selected continuous collision detection as needed. Physics owns dynamic-body transforms; gameplay drives them using forces, impulses, kinematic targets, or explicit teleport operations. Avoid competing writes from parenting, animation, and physics.

**Animation:** Import skeletons, clips, and skins. Implement sampling, looping, transitions, basic blending, events, and skinning. Define root-motion ownership and the relationship to physics before implementing it. Add a simple state-machine authoring model before a large animation-graph editor. Defer retargeting, advanced IK, and cinematic tooling.

**Audio:** Provide engine-level sources/listeners, 2D playback, basic spatial attenuation/panning, buses, volume, looping, and streamed music. Limit voices. Keep blocking work out of audio callbacks. Test interruption, device change, mute/suspend, browser activation, and resource disposal. Native and browser audio may use different backends behind the same API.

**Input:** Define named actions with keyboard/mouse/touch/gamepad bindings rather than hard-coded key checks. Support focus, pointer capture, relative mouse movement, remapping, pause, and input recording for tests. Flutter consumes UI interactions first; only appropriate unconsumed actions reach gameplay. Test pointer-lock and focus behaviour on web explicitly.

## 10. UI model

**Screen-space UI:** Flutter owns menus, settings, HUD, dialogs, inventory, accessibility, and text entry. Update through filtered events and small observable state, not a per-frame copy of the entire ECS or rebuild of the whole app.

**Projected world labels:** Project selected world anchors into viewport coordinates. Include viewport transform, DPI, camera identity, clipping, and visibility. Depth occlusion requires an explicit policy; a screen overlay is not automatically hidden behind 3D geometry. Bound the number and update rate of anchored widgets; render dense crowds of labels natively if measurement requires it.

**True world-space UI:** Implement a limited native WorldCanvas with panels, images, text, progress bars, simple layout, and basic buttons. Use maintained text shaping/font rasterization components, not a new text stack. Render using depth testing and ray-to-local-panel hit testing. Define canvas units, clipping, ordering, focus, and interaction events. Clearly document initial text/language coverage and unsupported text-entry behaviour.

Expose world UI through the same component schemas, authoring commands, and Dart API. Do not build a second general-purpose app UI framework. Flutter-to-world-texture remains an isolated experiment, not a requirement for release.

## 11. Editor and authoring service

Build the runtime first until the reference game is playable through code, then build Editor V0. Design documents and edit commands before the editor. Thereafter, engine features and their basic authoring tools should ship together.

The editor is a client of a shared authoring service. Commands include create/delete object, add/remove component, set field, reparent, assign asset, instantiate prefab, apply/revert override, save document, and request preview/build.

Each edit has a transaction ID, base document revision, validation, affected IDs, and undo information. Group a gizmo drag into one undo step. Reject stale writes or show a merge conflict instead of silently overwriting edits from another editor or agent. Apply validated document transactions atomically.

Editor V0 includes a Scene viewport, hierarchy, schema-generated Inspector, asset browser, selection, transform gizmos, save/load, undo/redo, Play/Stop, logs, and a basic performance panel. It does not need a custom code editor; open Dart files in the developer's existing IDE.

Maintain separate authoring state, edit-preview runtime, and play runtime. Edit mode never runs arbitrary game behaviours by default. Play builds/launches the actual project with its pinned runtime in a separate process initially. A separate game window is acceptable; embedding a crash-isolated player viewport is later work. Stop discards runtime mutations unless the user explicitly applies a supported change back to source.

The Scene viewport uses an editor camera and edit-preview world. The Game view uses the project's cameras and Flutter UI. A minimal split avoids pretending the editor itself is the deployed game.

### Reload policy

Material/asset reload: validate, replace safely, preserve the last valid version on failure. Dart reload: integrate supported development tooling and an explicit engine refresh/restart policy. Rust/native changes: rebuild and restart. Structural script/schema changes may require regeneration and restart. Flutter hot reload is a debug capability; it does not automatically rebuild native code or reconstruct the engine's scene lifecycle. [S14]

## 12. Browser editor strategy

Treat three separate capabilities as separate milestones: editor UI in a browser, the 3D runtime in a browser, and compilation of newly edited Dart game code. One does not imply the others.

### Mode A — Connected browser editor: first supported mode

A local `fynkel serve` process serves the Flutter web editor and a restricted authoring/build API. The browser can present a WebGPU edit-preview world using the Wasm runtime. Project files and compiler tools remain in the local companion.

Play launches a native player separately or a locally compiled web player. A WebSocket carries commands/events; it does not by itself transport native GPU frames. Native viewport streaming would be a separate feature with its own encoding, latency, and security work.

Bind the companion to loopback by default. Use per-session authentication, origin checks, request size limits, project-root path validation including symlinks, and explicit project trust for build or script execution. Do not expose an unauthenticated shell or filesystem service. Serve UI and API together where practical to simplify browser-origin constraints.

### Mode B — Browser-only authoring: later, narrower initially

Use Flutter web, Rust/Wasm, and WebGPU with JS interop. Dart's web interop path differs from native FFI. Flutter web supports Wasm builds, but its compilation/rendering path and the Rust WebGPU runtime must both be tested on named browser versions. [S15–S16]

Use an abstract ProjectStore: browser storage and import/export for browser-only mode; ordinary filesystem operations through the companion in connected mode. The origin-private filesystem can store browser-local project data, but quotas and persistence/export behaviour require explicit handling. Native `dart:io` is not the browser storage implementation. [S9, S17]

Start with scene editing and precompiled game templates. Editing arbitrary Dart code requires a separately designed compiler path: the local companion initially; a sandboxed hosted build service only when deliberately implemented. A precompiled Flutter editor must not promise to execute arbitrary new Dart source by inspection.

Require WebGPU for the first supported browser-runtime profile. Feature-detect availability, limits, and device loss; provide a clear unsupported message or a non-3D editing mode. WebGPU has secure-context requirements and varying availability, so maintain a tested compatibility matrix rather than marketing universal parity. [S18]

Do not depend on threads/shared memory for baseline correctness. Multi-threaded web acceleration can require additional deployment headers and browser capabilities. Flutter's Wasm documentation describes these hosting requirements. [S16]

## 13. Fynkel distribution and developer workflow

Ship three independently installable but version-matched Fynkel deliverables: the **Fynkel Runtime SDK**, **Fynkel development tools/CLI**, and the optional **Fynkel Editor** application. The browser editor is another client of the tools/runtime, not a fourth engine.

The proposed workflow is:

```sh
fynkel create my_game
fynkel doctor
fynkel run
fynkel editor .
fynkel serve .
fynkel validate --json
fynkel test --headless --json
fynkel build windows
```

Support adding the Fynkel SDK to an existing Flutter app without regenerating the app. Installing the runtime dependency must not install or bundle the editor. A project-local tool launcher may locate or install matching development binaries, with explicit version and integrity checks.

Prefer prebuilt Rust runtimes for supported targets so ordinary game developers do not need a Rust compiler. Keep a source-build path for contributors and unsupported configurations. Normal Flutter/platform SDK requirements still apply. Flutter's native build hooks can build or bundle native assets, including downloaded precompiled binaries, but the viewport's Flutter Plugin API integration is a separate packaging concern. [S13]

Use a compatibility handshake between the Fynkel Editor, tools, Dart API, native library, and schemas. A source version mismatch should produce actionable diagnostics; a binary/protocol mismatch must never proceed silently. Keep the first release train aligned before attempting broad cross-version editor compatibility.

The build pipeline resolves locked dependencies, generates script registration, validates documents, cooks assets, compiles target code, packages the game, and runs smoke tests. Document whether raw `flutter run/build` automatically invokes preparation or requires an explicit `fynkel prepare`; never let stale caches silently succeed. Initial templates may require preparation, but the tool must detect stale outputs and explain recovery.

Release builds exclude editor UI, importers, developer daemons, debug command endpoints, unnecessary source assets, and unused optional systems. Keep debug symbols separately for diagnostics.

## 14. AI and automation interface

Provide the same validated capabilities to humans, scripts, and agents. Do not make GUI clicking the authoritative automation API.

Required proposed commands:

```sh
fynkel schema describe fynkel.Transform --json
fynkel scene inspect assets/scenes/main.scene.json --json
fynkel scene apply --patch changes.json --dry-run --json
fynkel scene apply --patch changes.json --expected-revision 42 --json
fynkel validate --json
fynkel test --headless --steps 600 --seed 42 --json
fynkel capture --scene main --output preview.png
fynkel build web --json
```

Machine output needs a version, stable diagnostic codes, severity, document path, JSON pointer/field path, entity/component ID, message, and suggested repair where known. Keep machine output separate from human logs. Define exit codes and cancellation/timeouts.

An automated workflow should be able to create a level, validate it, run scripted inputs, assert gameplay state, capture an image, and produce a release build. Screenshot tests require a graphics-capable host; no-renderer tests do not verify visuals.

Respect revision conflicts, dry runs, permission boundaries, and explicit trust. AI-authored code is untrusted code like any other external code. Optional agent-protocol adapters should delegate to the same command service rather than introducing a separate capability surface.

## 15. Performance and quality requirements

These are proposed engineering targets, not measured performance claims. M0 must freeze named hardware, OS, driver, browser, resolution, scene contents, and repeatable measurement commands before the team treats a number as a release gate.

For the desktop reference game, propose a 1080p, 60 Hz profile. CPU critical-path work and GPU work including composition should each remain below roughly 12 ms at the 95th percentile, leaving headroom within the 16.67 ms frame interval. CPU and GPU work may overlap; do not add their times as though the pipeline were wholly serial. Track missed presentation deadlines and the 99th percentile separately.

Set scene-specific sub-budgets after profiling. Initial bridge target: bounded batched traffic and less than 0.5 ms p95 encoding/copy/dispatch overhead on the agreed desktop benchmark, excluding native simulation. This is a target to validate, not a promise that all APIs, platforms, or scene sizes meet it.

Maintain separate tests for dense transform updates, mixed ECS queries and churn, instanced rendering, many material changes, animated characters, physics stress, heavy Flutter UI, large project editing, cold startup, and asset streaming. Do not combine unrelated maximum counts into one advertised capacity figure.

Measure bridge bytes/calls/copies, script time and allocations, native allocations, ECS/query cost, physics and animation time, render preparation, GPU pass times where available, Flutter composition, VRAM/native/Dart memory, load times, package size, and input-to-visible latency. Device memory must include Flutter as well as the 3D engine.

Add trace spans and counters in M0. Record standalone versus embedded performance. Avoid per-frame CPU readback, unbounded queues, allocation churn, blocking I/O, unnecessary full-world synchronization, and uncontrolled pipeline compilation. Instrument each; do not rely on code review alone.

Mobile targets require sustained on-device thermal/power and lifecycle testing, not a short simulator FPS result. Browser profiles require their own budgets and feature limits. Pause unnecessary work in hidden/inactive views according to the documented game policy.

### Continuous integration and release quality

Every change runs formatting/linting, unit tests, schema/ABI fixtures, and an appropriate build matrix. Nightly or dedicated hardware jobs run rendering, performance, and lifecycle tests.

Use property/fuzz tests for ECS operations, document parsing, binary buffers, prefab expansion, and malformed assets. Test unsafe storage paths with applicable Rust tooling. Test repeated create/destroy, scene load/unload, resize/suspend/resume, and Play/Stop for leaks and crashes.

Maintain visual golden tests with documented tolerances and fixed inputs; do not require byte-identical images across unrelated GPU drivers. Test CLI/editor equivalence, code-first builds, missing assets/plugins, migrations, and release packages. Generated registries and binding fixtures must be reproducible.

Investigate repeatable regressions rather than trusting a single noisy run. Release gating includes error recovery and diagnostic quality, not only FPS.

## 16. Milestones and acceptance gates

### M0 — Feasibility, platform paths, and contracts

**Owners:** Architecture lead; rendering/platform; Dart SDK/tooling.

**Build:** Standalone wgpu test host; Flutter native viewport prototype; browser WebGPU/Wasm viewport prototype; overlay/input/resize tests; initial bridge encode/decode; lifecycle and performance instrumentation; architecture decision records.

**Freeze:** Coordinates, ID model, ownership, tick semantics, bridge outline, initial source format, release target order, hardware benchmarks, and scope of browser mode.

**Gate:** Equivalent scenes render in standalone and Flutter hosts on a primary desktop target; validate a second graphics backend before claiming portability. Browser prototype proves canvas composition and input. Record a mobile surface/lifecycle risk spike. All unavoidable copies and synchronization are documented. No unexplained resource leak through repeated creation/resizing.

**Do not:** Begin a full editor, custom ECS scheduler, or advanced renderer before the surface integration has an acceptable path.

### M1 — Runtime world, ECS, and bridge

**Depends on:** M0. **Owners:** Runtime/ECS; SDK.

**Build:** Generational entities; basic storage/queries; transform hierarchy; command buffers; immutable snapshots; time/input contract; typed handles; world lifecycle; no-renderer test host.

**Gate:** Dart creates, updates, queries, and destroys objects; stale handles fail safely; native and web bridge fixtures agree; hierarchy tests pass; bounded queues and repeatable ECS benchmarks exist. Rendering can continue from completed state without accessing mutating storage.

**Do not:** Expose ECS internals or raw pointers; implement generalized parallel scheduling before correctness and measurement.

### M2 — Documents, assets, schemas, and behaviour registration

**Depends on:** M1. **Owners:** Tools/document; SDK; asset pipeline.

**Build:** Scene/material/basic prefab schemas; UUID asset metadata; authoring command service; strict validation; JSON formatting; glTF import; incremental cache; generated Dart registry; headless load/save/cook; migration fixtures.

**Gate:** A scene created through code/document commands round-trips without losing IDs or supported data. Prefabs instantiate with correct reference remapping. Missing asset/type errors identify exact locations. A generated behaviour loads in a compiled game. Editor-independent build preparation works from a clean checkout.

**Do not:** Maintain separate editor-only formats, serialize raw Rust structs, or execute project scripts on open.

### M3 — Complete code-first reference game

**Depends on:** M2. **Owners:** Runtime/rendering/physics/SDK; reference-game owner.

**Build:** Renderer V1 subset; camera and lighting; physics character movement; interaction/collectibles; basic audio; Flutter menu/HUD; scene transition; win/restart flow; packaged desktop game.

**Gate:** A new developer builds and plays the whole game without opening the Fynkel Editor. Native resources are released correctly; baseline performance and input latency are recorded. The game has actual gameplay and a completion/restart loop, not only a rendering demo.

**Do not:** Wait for all engine subsystems or graphics effects to finish before proving a game can ship.

### M4 — Editor V0

**Depends on:** M2 and playable M3 integration. **Owners:** Editor/document; runtime support.

**Build:** Scene viewport, hierarchy, generated Inspector, selection/gizmos, asset browser, save/load, undo/redo, Play/Stop, console, basic profiler. Use real schemas and authoring commands.

**Gate:** A developer recreates the reference level visually, attaches a Dart behaviour, saves it, and runs the same project through the CLI. Undo/redo preserves valid state. A play-process crash does not corrupt the authoring document. Play mutations are discarded by default.

**Do not:** Put gameplay in editor widgets or depend on the editor to generate hidden essential state.

### M5 — Practical game and authoring completeness

**Depends on:** M4. **Owners:** Runtime systems; editor counterparts.

**Build:** Skeletal animation and transitions; improved audio; common physics options; world-space UI subset; projected labels; practical prefab overrides; scene lifecycle; persistence example; tools for each supported feature.

**Gate:** The reference game uses animated characters, reusable prefab instances, sound, a depth-tested interactive panel, and scene transitions. Each feature has a code example, CLI/serialization contract, and basic editor support.

**Do not:** Add systems that the reference game or early adopters do not exercise.

### M6 — Connected browser editor

**Depends on:** M2 and M4; shell/interop work can begin earlier. **Owners:** Web/platform; editor/tools.

**Build:** Flutter web editor, authenticated local companion, browser edit-preview renderer, project file access through the companion, compile/run commands, and a documented browser support profile.

**Gate:** The same project can be edited in desktop and browser clients without format conversion. Conflicting revisions are detected. The browser editor can launch a native player or a locally compiled web player. Security tests reject unauthorized origins and project-root escape attempts.

**Do not:** Describe the local-companion mode as browser-only or confuse WebSocket control with viewport streaming.

### M7 — Supported platform builds and optimization

**Depends on:** M3–M5; target-specific work begins at M0. **Owners:** Platform/rendering; tooling; quality.

**Build:** Versioned runtime binaries; verified package integration; supported desktop exports; Android/iOS integration and device tests as release scope permits; quality profiles; practical memory/performance improvements; signed/checksummed artifact pipeline.

**Gate:** Clean-machine install/build/run works on every advertised target. No editor binaries ship in the game. Lifecycle, rendering, asset, input, audio, and sustained-performance tests pass. Unsupported targets fail clearly, not partially.

**Do not:** Advertise every Flutter/wgpu platform merely because a dependency lists it.

### M8 — Browser-only authoring and web-player qualification

**Depends on:** M5–M7 as applicable. **Owners:** Web/runtime; tools.

**Build:** Browser project storage/import/export; precompiled template preview; complete browser runtime validation; asset limits; capability diagnostics. Add a hosted compiler only under a separate approved security/product scope.

**Gate:** Browser-only mode can edit, save/export, and preview its supported projects without a local install. Newly edited Dart source is explicitly routed to a compiler service/companion when required. No feature silently assumes native files, FFI, or threads.

**Do not:** Block a useful native/connected-editor release on full cloud IDE functionality.

### M9 — Beta and 1.0 readiness

**Depends on:** M5–M7, plus whichever M8 features are advertised. **Owners:** All leads; quality/release owner.

**Build:** Migration policy, API cleanup, release documentation, tutorials, troubleshooting, templates, benchmark reports, issue triage, and independent pilot projects.

**Gate:** Independent developers build games without undocumented support from the engine authors. At least two additional small pilot projects exercise workflows beyond the reference game. No known release-blocking data-loss or crash defects remain. Code-first, hybrid, editor-first, and advertised web workflows have automated coverage. Platform support and known limitations are published accurately.

**Do not:** Declare 1.0 based only on a polished demo or a long feature list.

## 17. Workstream ownership and delivery rules

Assign accountable owners to runtime/ECS, rendering/platform integration, Dart SDK/code generation, documents/assets/editor, and testing/releases. These are responsibilities, not mandatory separate hires. One architecture owner resolves cross-boundary contracts and prevents duplicate sources of truth.

The critical path is M0 integration -> M1 world/bridge -> M2 data/scripts -> M3 playable game -> M4 editor. Browser experiments begin in M0; browser editor work can proceed once document and bridge contracts are stable. Do not have every engineer implement a different large subsystem before the integration path is proven.

Each milestone produces a runnable build, tests, documentation, measured performance notes, known limitations, and a short demo script. Estimate calendar dates only after M0, actual team capacity, and target hardware are known. Keep acceptance criteria stable when schedules change; reduce scope rather than quietly removing correctness gates.

### Definition of done for a feature

A feature is complete only when its runtime API, source schema/migration, generated bindings or registry, code example, relevant authoring support, diagnostics, tests, resource lifetime, performance impact, and target compatibility are documented. Mark genuinely code-only or platform-specific features explicitly.

### Highest risks and controls

| Risk                                                                  | Control / decision gate                                                                                     |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Flutter/wgpu interoperability is expensive or unsupported on a target | M0 platform spike, copy/synchronization accounting, standalone comparison; do not claim support prematurely |
| Custom ECS becomes a project within the project                       | Minimal storage/scheduler, benchmark against a small library, bounded scope, review before storage redesign |
| Native and Dart state diverge                                         | Tick-numbered commands, ownership table, immutable snapshots, read-your-writes contract, conformance tests  |
| Editor cannot inspect or run user behaviours                          | Generated metadata and factories; actual project compilation and a separate player host                     |
| Project files become fragile or unmergeable                           | Stable IDs, strict schemas, canonical formatting, explicit prefab overrides, revisions and transactions     |
| Browser editor implies an unplanned cloud compiler                    | Connected mode first; separate editing, rendering, and compilation milestones                               |
| Performance work arrives too late                                     | M0 instrumentation, reference game, fixed benchmarks, regression gates                                      |
| Project loading becomes arbitrary code execution                      | Data-only inspection, explicit trust, restricted companion, bounded parsers/importers                       |
| Version mismatch corrupts data or crashes                             | Lockfiles, protocol/schema handshake, tested migration, refuse incompatible writes                          |

## 18. What not to do

Do not build a Unity clone feature-for-feature. Reuse a familiar workflow while keeping a smaller, coherent engine.

Do not wait until a supposedly finished runtime exists before considering editor operations. Document formats, schema metadata, undoable transactions, and script registration are early architecture, not late polish.

Do not equate fewer dependencies with better performance. Avoid Bevy in this implementation as requested, but do not replace proven physics, image, audio, or text libraries without evidence.

Do not assume FFI, shared textures, hot reload, browser file access, native concurrency, and arbitrary Dart compilation automatically work together.

Do not make every entity a Flutter widget, every component a Dart object, every property access a bridge call, or every tick a JSON message.

Do not write a custom physics engine, custom compiler, general text renderer, visual scripting system, shader graph, marketplace, or multiplayer backend in the first release.

Do not make unmeasured claims of Godot/Unity performance parity. Compare defined workloads, image quality, hardware, frame pacing, memory, and shipped-game behaviour.

Do not sacrifice source-data safety for fast previews. Lost scenes, broken IDs, and incompatible upgrades can invalidate otherwise strong rendering work.

## 19. First implementation backlog

Start with nine concrete deliverables: repository/toolchain locks; architecture decision records; standalone renderer host; Flutter viewport adapter; browser canvas prototype; versioned binary command round-trip; resize/dispose/suspend test harness; instrumentation and benchmark manifests; and the smallest code-created world exercising a camera and mesh.

Once M0 passes, implement generational entities and transforms, then document/schema/registry infrastructure, then the playable reference game. Start the editor immediately after that code-first path works. Continue with engine features and authoring support together.

**Delivery objective:** a developer can create a normal Flutter project, author a game in Dart or the optional editor, validate it through automation, and produce a stable, measured release build without learning the engine's internal Rust implementation.

## 20. Verified technical references

References were checked on 18 September 2026. Re-check platform and toolchain details when freezing M0 and before release. The architectural prescriptions and budgets in this plan are design decisions, not claims that dependencies implement the entire proposed system.

- **S1 — wgpu documentation:** `https://docs.rs/wgpu/latest/wgpu/`
- **S2 — Rapier getting started:** `https://rapier.rs/docs/user_guides/rust/getting_started/`
- **S3 — glam documentation:** `https://docs.rs/glam/latest/glam/`
- **S4 — Kira documentation:** `https://docs.rs/kira/latest/kira/`
- **S5 — Khronos glTF:** `https://www.khronos.org/gltf/`
- **S6 — Bevy ECS standalone documentation:** `https://docs.rs/bevy_ecs/latest/bevy_ecs/`
- **S7 — hecs documentation:** `https://docs.rs/hecs/latest/hecs/`
- **S8 — Flutter FAQ, reflection/code generation:** `https://docs.flutter.dev/resources/faq`
- **S9 — Flutter web FAQ, I/O and concurrency:** `https://docs.flutter.dev/platform-integration/web/faq`
- **S10 — Rapier determinism:** `https://rapier.rs/docs/user_guides/rust/determinism/`
- **S11 — JSON Schema draft 2020-12:** `https://json-schema.org/draft/2020-12`
- **S12 — Flutter Texture widget:** `https://api.flutter.dev/flutter/widgets/Texture-class.html`
- **S13 — Flutter native FFI and build hooks:** `https://docs.flutter.dev/platform-integration/bind-native-code`
- **S14 — Flutter hot reload:** `https://docs.flutter.dev/tools/hot-reload`
- **S15 — Dart JavaScript interoperability:** `https://dart.dev/interop/js-interop`
- **S16 — Flutter WebAssembly support:** `https://docs.flutter.dev/platform-integration/web/wasm`
- **S17 — MDN origin-private filesystem:** `https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system`
- **S18 — MDN WebGPU:** `https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API`

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

Dart behaviours provide the main game-scripting experience. Define a small lifecycle rather than dozens of callbacks:

```text
onCreate      object and fields exist; scene references are resolved
onEnable      behaviour becomes active
start         runs once before the first gameplay update
fixedUpdate   fixed-rate simulation logic
update        variable-rate gameplay logic
lateUpdate    ordering-sensitive presentation logic
afterPhysics  optional post-physics callback if needed
onDisable     behaviour becomes inactive
onDestroy     final cleanup notification
```

Collision, trigger, animation, and input events should be typed events or subscriptions rather than adding a callback method for every possible engine event.

Example API target:

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
    body.setPlanarVelocity(move.x * moveSpeed, move.y * moveSpeed);
  }
}
```

Generated metadata should provide behaviour factories, field serialization, Inspector metadata, stable type IDs, migration hooks, and schema descriptions. Do not use runtime reflection as a requirement.

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

### 21.5 Standard engine services

Provide focused services rather than global static APIs that cannot be tested. Static convenience facades may delegate to the current `GameContext`, but the underlying service boundaries should be explicit.

Minimum services:

- `Scenes`: load, unload, additive load, active scene, lookup by ID
- `Assets`: typed load, preload, release, dependency status, async progress
- `Input`: actions, axes, pointer, touch, gamepad, rebinding
- `Physics`: raycast, shapecast, overlap, layer masks
- `Audio`: one-shot playback, global buses, listener control
- `Time`: frame delta, fixed delta, scale, frame/tick count
- `Diagnostics`: logs, markers, counters, assertions
- `Application`: platform, lifecycle, quit/restart requests where supported

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

### 22.17 Browser editor UX

The browser editor should preserve the same conceptual workspace and authoring commands. Browser-specific differences such as file permissions, companion connectivity, capability limits, and build availability must be visible rather than hidden.

If connected to a local companion, show connection identity, project root, trust state, and whether a command runs in-browser or locally. Never present local execution as if it were isolated cloud execution.

### 22.18 Accessibility and internationalization

Use Flutter semantics/accessibility APIs for standard controls. Keyboard navigation, visible focus, scalable text where practical, and sufficient contrast should be treated as normal editor quality requirements. Keep editor strings localizable even if 1.0 ships only in English.

### 22.19 Editor acceptance gate

The editor is 1.0-ready when a developer unfamiliar with engine internals can create or open the reference project, construct a level, attach/configure behaviours, author materials, run the game, diagnose common errors, inspect performance, and recover from ordinary mistakes without manually editing internal IDs or cache files. Every editor mutation must map to a shared authoring command or a clearly documented editor-local preference.

## 23. Fynkel extensibility and plugin architecture

Extensibility is required for a healthy ecosystem, but it should be introduced only after the engine's contracts are stable enough to extend. The plugin system must not expose arbitrary internal pointers or freeze the ECS/renderer implementation permanently.

Treat extensibility as multiple capability layers rather than one universal plugin mechanism.

### 23.1 Extension categories

Define these categories separately:

| Extension type        | Typical use                                                 | Execution environment       |
| --------------------- | ----------------------------------------------------------- | --------------------------- |
| Dart package          | Gameplay libraries, reusable behaviours, high-level systems | Game Dart runtime           |
| Editor extension      | Panels, inspectors, asset tools, commands                   | Editor Dart/Flutter runtime |
| Importer extension    | New source asset formats and preprocessing                  | Development tool process    |
| Native engine plugin  | Performance-sensitive native systems                        | Rust/native runtime         |
| Build extension       | Packaging/codegen/validation steps                          | Development/build process   |
| Shader/render feature | Custom material/shader functionality                        | Renderer-defined contract   |

A plugin may provide more than one extension category through one manifest, but each capability needs explicit permissions and lifecycle rules.

### 23.2 Plugin/package manifest

Use a committed manifest with stable plugin ID, semantic version, engine compatibility range, dependencies, capabilities, and entry points.

Conceptual example:

```json
{
  "id": "com.example.terrain",
  "version": "1.2.0",
  "fynkel": ">=1.1.0 <2.0.0",
  "capabilities": [
    "dartPackage",
    "editorExtension",
    "nativeRuntime",
    "assetImporter"
  ],
  "dependencies": {
    "com.fynkel.navigation": "^1.0.0"
  }
}
```

Do not infer compatibility from filenames or dynamically inspect untrusted libraries without a manifest/handshake.

### 23.3 Dart gameplay extensions

The simplest extension mechanism is an ordinary Dart package that depends on the Fynkel SDK. It may provide Behaviours, data models, helper systems, editor metadata, and assets.

Generated registration should allow packages to contribute stable behaviour/component schemas without modifying a central engine source file manually.

For most third-party gameplay libraries, this should be the preferred model.

### 23.4 Custom component types

Support two classes of third-party component:

**Dart-authored data/behaviour components:** suitable for gameplay state and authoring metadata where native high-throughput iteration is unnecessary.

**Native registered components:** supplied by approved native plugins for systems that need Rust-side storage and scheduling.

Every custom component needs a stable type ID, versioned schema, serializer/migration path, Inspector metadata, and unknown-component preservation semantics.

Do not allow plugins to reuse engine-reserved type IDs.

### 23.5 Editor extensions

Editor extensions should use a documented Flutter/Dart API that permits:

- New panels/windows.
- Custom inspectors/property editors.
- Asset previewers.
- Project/scene commands.
- Menu/command-palette actions.
- Gizmos/overlays through defined extension APIs.
- Validation rules.
- Import/build status UI.

Extensions must use the authoring service for project mutations. Directly editing project files behind the editor's back should be discouraged because it bypasses revisions, validation, undo, and collaboration/conflict checks.

Keep editor extension UI isolated from engine-private widgets. Public extension APIs need compatibility guarantees separate from internal editor implementation.

### 23.6 Importer extensions

Importers run during development/build, not inside the shipped game unless explicitly required. An importer declares accepted file types, importer/version ID, settings schema, output artifact types, dependency discovery, and deterministic cache-key inputs.

Importer execution must have cancellation, bounded error reporting, and a documented trust model. A third-party importer is executable code and must not be treated as safe merely because it processes an asset.

### 23.7 Native Rust plugins

Native plugins are the highest-risk extension mechanism and should not be required for common game development.

A stable C ABI boundary is preferable to exposing Rust ABI details across independently built plugin binaries. Rust ABI is not a long-term binary compatibility contract.

The native plugin ABI should expose opaque handles, versioned function tables, explicit ownership, allocator rules, thread constraints, logging, registration functions, and capability negotiation.

If stable dynamic native plugins prove too costly initially, support **source-linked Rust extensions** first: the plugin contributes a Rust crate compiled as part of the game's native engine build. This avoids premature ABI promises while still enabling advanced extensions.

Do not permit arbitrary native plugins in browser-only builds unless they have a separate Wasm-compatible implementation.

### 23.8 Scheduler/system extensions

Native extension systems should register into named phases such as:

```text
PreFixed
FixedSimulation
PostPhysics
PostFixed
VariableUpdate
LateUpdate
RenderExtract
```

Dependencies/ordering constraints must be explicit and cycle-checked. Do not let plugins inject undocumented work at arbitrary points inside renderer internals.

Dart extension systems should use a similar high-level scheduling contract without exposing native thread ownership.

### 23.9 Rendering extensions

Keep renderer extensibility narrower than gameplay extensibility. Initially support:

- Custom shaders through the documented shader/material interface.
- Custom material definitions.
- Optional render features inserted at a small number of named extension points once the render graph API is stable.

Do not expose raw internal render-graph nodes or wgpu device ownership as the first plugin API. Renderer internals are likely to evolve significantly after 1.0.

Advanced native render extensions can become a post-1.0 experimental API before receiving compatibility guarantees.

### 23.10 Build extensions

Build extensions may add validation, generated source, cooked assets, platform configuration, and packaging steps. They need declared inputs/outputs so builds remain incremental and reproducible.

A build extension must not silently modify unrelated project files. Build steps should operate in staging/generated areas unless explicitly performing a migration accepted by the user.

### 23.11 Plugin dependency resolution and lockfiles

Project dependency resolution must produce a lockfile containing exact plugin/package versions and relevant native artifact hashes. A project reopened later should resolve to the same tested versions unless the developer intentionally upgrades.

Detect incompatible engine/plugin version ranges before opening in write mode or building.

Avoid a custom dependency solver where Dart's package system already solves Dart-package dependencies. The engine manifest should coordinate non-Dart capabilities and engine compatibility around the existing package ecosystem rather than replacing it unnecessarily.

### 23.12 Security and trust

Differentiate data-only packages from executable extensions. Native plugins, editor extensions, importers, and build extensions can execute code with developer-machine privileges.

The editor should show when a project introduces new executable extensions and require trust before executing them. Browser/local-companion mode must apply the same trust model.

Do not load unknown native binaries merely to inspect project metadata.

### 23.13 Plugin testing

Provide an extension test harness with:

- API/ABI compatibility checks.
- Schema validation tests.
- Example project fixtures.
- Editor extension smoke tests.
- Headless runtime tests.
- Importer determinism/cache tests.
- Cross-version compatibility tests for supported ranges.

The engine CI should maintain a small set of representative first-party/sample plugins as compatibility canaries.

### 23.14 Plugin distribution and registry

Do not make a marketplace or proprietary registry a prerequisite for plugins. The first distribution path should use normal source/package hosting plus manifests and lockfiles.

A curated registry or marketplace can be added later for discovery, signing, compatibility metadata, reviews, paid assets, or hosted binaries. The engine must remain capable of consuming packages outside that registry.

### 23.15 Extensibility acceptance gate

The plugin architecture is ready for stable support when an external team can implement at least one Dart gameplay package, one editor extension, and one custom importer using only documented public APIs; projects preserve unknown extension data safely; version mismatches fail clearly; and no extension requires private repository imports. Native dynamic ABI support should remain experimental until multi-version compatibility has been proven.

## 24. Fynkel post-1.0 roadmap

Post-1.0 work should be driven by shipped games, profiler data, platform demand, and ecosystem feedback. This roadmap defines likely directions and sequencing; it is not a commitment to implement every item. Protect 1.0 stability before pursuing breadth.

### 24.1 Roadmap principles

After 1.0:

1. Maintain backward compatibility and migration quality before major feature expansion.
2. Improve the workflows already used by real projects before adding speculative systems.
3. Keep advanced systems modular so a small mobile game does not pay for features it does not use.
4. Benchmark each major renderer/runtime change against frozen reference workloads.
5. Prefer interoperable formats and established libraries over proprietary reinvention.
6. Keep AI/CLI/editor parity for new authoring capabilities.

### 24.2 Phase A — 1.x stability, profiling, and workflow depth

The first releases after 1.0 should concentrate on reliability:

- Crash and data-loss fixes.
- Faster startup and incremental import.
- Better error messages and migration tooling.
- Editor performance on larger projects.
- Profiler timeline/capture comparison.
- Memory/resource leak tooling.
- Improved documentation and templates.
- SDK ergonomics based on real game feedback.
- More complete headless testing and automation.
- Stable plugin APIs where evidence supports them.

Avoid immediately turning 1.1 into a second 1.0-sized feature wave.

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

After connected browser editing is stable, possible expansion includes:

- Browser-only scene/material/prefab authoring.
- Hosted project storage integrations.
- Remote build workers.
- Shareable read-only scene previews.
- Web collaboration sessions.
- Hosted CI/testing.

A cloud compiler/build service is a separate product/security/operations commitment. Do not make it a hidden architectural dependency of the open/local engine workflow.

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

The engine should be considered successful if independent teams can ship complete games while choosing code-first, editor-first, or hybrid workflows; the runtime remains competitive on defined workloads; project data remains transparent and durable; extensions do not require forks; browser/automation workflows share the same project model; and new engine capabilities remain accessible equally to humans, IDEs, CI systems, and AI agents.

The roadmap should be reviewed after every substantial release using telemetry that users explicitly opt into where applicable, public issue trends, profiler evidence from real projects, and direct developer feedback. Feature count alone is not the objective; reliable iteration speed and shipped games are.

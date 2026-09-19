# Fynkel ideas backlog (post-1.0, non-binding)

These sections were moved out of the blueprint. They are speculation, not commitments. Nothing here may become a hidden 1.0 requirement.

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

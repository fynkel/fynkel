> **Living design draft** — extracted from blueprint v0.3 §22. Revisit at the start of M4. Where this conflicts with an ADR, the ADR wins. Known v0.4 impacts: authoring service is `fynkel_document` (Rust); Scene view is a real edit-mode world; Play is a separate process; docking via a pub.dev package; Inspector from committed codegen metadata (ADR-0016). §22.17 (browser parity) is non-binding.

> **Revisited 2026-09-21 at the start of M4.** The backlog that implements this spec is `docs/blueprint.md` §19.2e; the resolutions it needed are in ADR-0016 (authoring ABI location, edit-preview sync, Play channel, docking spike) and ADR-0018 (custom inspectors are declarative hints in 1.0 — §22.6's "extension point" means that, not project code in the editor). §22.2's "full arbitrary docking may arrive later" is superseded: docking comes from a package from item 2 on. §22.7's thumbnails for meshes and §22.9's live material preview wait for the Scene view's world (M5). §22.16 (multi-scene) is M5.

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


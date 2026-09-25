# Roadmap (from 2026-09-23)

**Status:** Agreed with the user on 2026-09-23 ("ok let's start this"),
after a full review of the foundation. It orders the work from here to
1.0 and names what comes after. `status.md` §6 stays the running log of
what is done; this file is the order. The order puts the fixes that grow
more expensive with every imported asset and every line of game code
ahead of new features.

## Phase 0 — Live Play mode (done)

ADR-0021 P7: L1–L5 plus create/duplicate while playing and whole-object
Apply to Scene (`docs/design/live-play-mode.md`).

## Phase 1 — Foundation fixes

1. **Stable sub-asset ids.** A model's meshes, materials, images, skins,
   animations and nodes are keyed by name, not by position in the file,
   so reordering them in the modelling tool keeps every reference.
   Existing sidecars and documents are migrated. Design:
   `docs/design/stable-sub-asset-ids.md`. **Done 2026-09-23.**
   - **Material remapping** (added 2026-09-24, next to item 1 because
     it extends the same sidecar): a model's embedded materials remapped
     to project materials in its import settings, and Extract Materials.
     Design: `docs/design/material-remapping.md`. **Done 2026-09-24.**
2. **`SetActive` / `activeInHierarchy`.** An inactive object and its
   subtree are not drawn, not simulated and run no behaviours
   (`onDisable`/`onEnable` fire); a checkbox in the Inspector header, a
   greyed row in the Hierarchy. Design: `docs/design/set-active.md`.
   **Done 2026-09-24.**
3. **Editor and cook scale.** The quadratic reference prune on refresh
   (`fynkel_authoring/src/project.rs`), `by_path` as a map, a
   size+mtime check before the cook re-reads and re-hashes a source,
   cook inputs read one at a time instead of all held at once. Measured on
   a generated 10k-asset project (`benchmarks/scale/`).
   **Done 2026-09-24.**
4. **Automatic sidecars.** A file dropped into `assets/` from Finder gets
   its sidecar on the editor's next scan instead of `FYN-A0001`.
   **Done 2026-09-24.**
5. **Housekeeping.** The editor's tests in CI; the ADR index in
   `docs/README.md` (0019–0021); a `cargo check --target
   wasm32-unknown-unknown` job over core, scene, physics and render
   (blueprint §12.1).
   **Done 2026-09-24.**

## Phase 2 — Close M4 (the editor)

- **Scripts in the editor:** the Project browser shows `lib/` (a
  *Scripts* root; double-click opens the IDE); **Create → Behaviour**
  from a template (codegen, then open in the IDE). **Done 2026-09-24**
  (ADR-0016 amended: the editor runs `fynkel codegen --project-only` for
  it).
- **Profiler panel** over the frame timings the engine already measures.
  **Done 2026-09-24.**
- **IDE navigation** from the Console and components to source.
  **Done 2026-09-24.**
- a11y/l10n basics, docs, the **M4 gate report**. **Done 2026-09-25:
  M4 GO** (`docs/m4-gate-report.md`), after the Unity parity pass.

## Phase 2b — Code first (ADR-0026)

Added 2026-09-25 at the user's request: building a game from Dart as
fully as from the editor, ahead of the extensibility work, because the
user ranks the code path above the editor. The checklist is
`docs/code-first.md`; the order is ADR-0026 § Order; the acceptance test
is `examples/code_only/`. **Done 2026-09-25** through step 8's `createMesh`
(`examples/code_only/` passing both ways); code-built animation clips
wait for the Phase 5 Animator, as the ADR orders.

## Phase 3 — Extensibility and the data model

Three ADRs, then their implementation:

1. **Editor extensions in Dart** (reopens ADR-0018 now that ADR-0021
   compiles the game into the editor): custom Inspectors, property
   drawers, editor windows, menu items; `editor/` code never ships in a
   game build. **ADR-0028 accepted 2026-09-25**: a package's
   editor part (`lib/editor/editor.dart`'s `registerEditor`, through
   `package:fynkel_editor_api`), so the package work (item 3) builds on
   it. Step 1 (the loop: menu items, discovery, reload, `FYN-P0015`,
   Preferences › Extensions) done 2026-09-25; step 2 (`EditorContext`:
   scene, selection, edits as one undo step, assets) done 2026-09-25;
   step 5 (windows and Hierarchy/Project/component context entries)
   done 2026-09-25; step 3 (custom Inspectors: `inspector<T>`,
   `InspectorContext`, `code_only`'s coin kind page) done 2026-09-25; step 4 (property
   drawers: `drawer<T>` for records, `namedDrawer` with
   `@Expose(drawer:)`, `code_only`'s jump height) done 2026-09-25; step 6 (Play access:
   `context.play` with the running `World`, Pause, Step and Stop;
   `code_only`'s Level Tools collect the coins and respawn the player)
   done 2026-09-25; step 7 (acceptance: `code_only`'s `Sparks` record
   drawer, the plain editor editing every field of `code_only`, a
   release build with no editor code) done 2026-09-25 — only the user's
   hot-reload check remains.
2. **`@DataAsset`** (Unity's ScriptableObject): a Dart class becomes an
   asset type with a schema, an Inspector page, references by id, a
   cooked form. **ADR-0027 accepted 2026-09-25; built first**, ahead of
   item 1 (its order is the ADR's § Order).
3. **Package architecture:** physics, audio and animation as optional
   engine modules; game packages that ship behaviours, data assets,
   prefabs and editor extensions; published artifacts (E2). **ADR-0029
   accepted 2026-09-25**: every Fynkel package is a Dart package found
   through the package graph; modules switch off in Project Settings;
   publishing is rehearsed from a local store and waits for ADR-0005. Step 1 (the package
   graph in `fynkel_document`, `fynkel.package.json`, `FYN-P0019`) done
   2026-09-25; step 2 (`registerPackages`, schemas from every package,
   `FYN-P0016`/`P0017`) done 2026-09-25; step 3 (package assets mounted
   at `Packages/<name>/`, the Project browser's Packages root, Copy into
   Project, Import Package Assets…, read-only when pub keeps the package;
   the primitives stay built in) done 2026-09-25; step 4 (the example
   package `examples/packages/pickups`: `Pickup`, `PickupKind`, the pickup
   prefab and material, an editor part; code_only's gem is its prefab)
   done 2026-09-25; step 5 (modules: `fynkel_animation`, the cargo
   features and `modules.strip`, `FYN-P0018`/`R0014`, `world.modules`,
   Project ▸ Modules…, the sizes in the ADR; code_only plays with
   animation off and stripped) done 2026-09-25.

## Phase 4 — Rendering resumes

The pending R capture, R3c colour grading, contact shadows, shader assets
(a cooked record for custom-shader materials), texture mips and GPU
compression (ASTC/BC).

## Phase 5 — M5: a real game

Tier-2 reference game and a pilot project; an **Animator state machine**;
physics joints and several colliders per object; async scene loading
and objects that survive a load; a **Game UI designer** (a `.ui.json`
widget tree edited in the Hierarchy and Inspector, bound to `GameModel`,
live preview, custom widgets through the Phase 3 extensions); WorldCanvas,
spatial audio, rebinding, runtime inspection.

## Phase 6 — M6: shipping

A physical iOS device and a second machine (E3); Export (release cook,
signed macOS app, iOS archive); the simulation off the UI thread (E4),
dirty-only transform propagation, query-based extract, a 50k-entity
stress target; public readiness (license, ADR-0005, ABI discipline).

## Phase 7 — Beta and 1.0

## After 1.0, in order

1. Web runtime (games playable in the browser: Wasm + WebGPU, a Dart↔JS
   bridge; blueprint §12.3 step 1).
2. Windows, then Android.
3. Particles/VFX, navmesh/AI, LOD, streaming.
4. Browser editor — only if the web runtime lands; a product decision.
5. General Flutter app builder — only if the Game UI designer proves
   itself.

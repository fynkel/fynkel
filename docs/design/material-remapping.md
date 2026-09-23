# Material remapping — design note

**Status:** Accepted (2026-09-24; the user took the three
recommendations: remaps change the model's hierarchy only, Extract
Materials writes to `Materials/` next to the model, the importer versions
move). Roadmap Phase 1, next to item 1
(stable sub-asset ids), because it adds to the sidecar format that item
just settled.
**Related:** ADR-0006 (asset identity), `docs/design/stable-sub-asset-ids.md`
(material keys are names), `docs/documents.md` § Import,
`crates/fynkel_import/src/graph.rs` (`renderer_component`),
`crates/fynkel_import/src/cook.rs` (`prepare`), the Inspector's asset
panel (`docs/editor.md` § The Project browser).

## The problem

An imported model brings its own materials: `robot.glb` has
`material/Painted` and `material/Glass`, and every `MeshRenderer` in the
model's hierarchy (`<model>#prefab`) points at them. Those materials are
**read-only** — they are whatever the file says, rebuilt on every
reimport.

A game almost never wants that. The artist's `Painted` should be the
project's `metal_red.material.json`, shared with twenty other models, with
the game's textures and tuning. Today the only way is a `setField`
override on the material of every renderer, on every instance: twenty
placed robots with two parts each is forty overrides, and a new instance
starts wrong. Unity solves this in the model's import settings (the
**Materials** tab: *remap* an embedded material to a project material, or
*extract* them all as editable files). So do we.

## Decision

### 1. A remap table in the model's sidecar

```json
"settings": {
  "materials": {
    "material/Painted": { "asset": "<uuid of metal_red.material.json>" },
    "material/Glass": { "asset": "<uuid of glass.material.json>" }
  }
}
```

- Keyed by the embedded material's **key** — a name since item 1, so a
  remap survives the artist reordering materials in Blender.
- The value is an ordinary `assetRef` to a **material document**, so
  `AssetDatabase::check_references`, rename/move and "Referenced by" all
  see it with no new code.
- `fynkel.gltf` and `fynkel.fbx` both read it (the rule lives in
  `graph.rs`, shared by the two importers).

### 2. What a remap changes

- **The model's hierarchy only.** Every `MeshRenderer` the importer
  writes into `<model>#prefab` whose material is a remapped key gets the
  project material instead. Instances of the model — in scenes, in
  prefabs, from `world.instantiate` — pick it up. An instance's own
  overrides still win, as for any prefab field.
- **Not explicit references.** A scene that names
  `{ asset: <model>, sub: "material/Painted" }` itself keeps that exact
  material: it asked for it by name (Unity does the same).
- **The embedded material stays a sub-asset.** It is still imported,
  cooked and referenceable, so nothing pointing at it breaks when a remap
  is added or removed.

### 3. Cook and runtime

- The remap is part of the sidecar's `settings`, which are already in the
  cook key: changing a remap recooks the model once.
- **New:** a model's remap targets become its **runtime dependencies**
  (the cache index's `dependencies`; today a source has none). The runtime
  loads the project material with the model, so
  `world.instantiate(model.prefab)` from Dart finds it resident like any
  prefab reference.
- They are **not key inputs**: the cooked model holds only the target's
  id, which the settings already cover, so editing the project material
  recooks the material alone. (A key dependency would also be a cycle: a
  material extracted from the model points back at the model's images.)
- `fynkel.gltf` 6 → 7, `fynkel.fbx` 2 → 3: an older build would ignore
  the table and silently draw the embedded materials; the version bump
  makes the lock say so (`FYN-P0011`) instead. No document or cooked
  format changes.

### 4. Validation

- A remap to something that is not a material document (another kind of
  asset, or a sub-asset such as the model's own `material/Glass`):
  `FYN-A0007` on the sidecar, the model does not cook. An entry that is
  not an `assetRef` is `A0006`. A nil reference means "the embedded one".
- A remap to an asset that does not exist: the existing `A0004`.
- A remap for a key the file no longer has (the artist renamed or deleted
  the material): a warning naming the model's current material keys, the
  same shape as the "no longer produced" warning — never an error, since
  the model still imports correctly.

### 5. The editor

The Inspector's asset panel for a model gains a **Materials** section,
one row per embedded material:

- the embedded material's name and swatch, and a material field
  ("None (use embedded)" or a project material), draggable and pickable
  like any `assetRef` field; it edits the settings draft, so **Apply and
  Reimport**, **Revert** and the asset history's undo work as for every
  other import setting;
- **Extract Materials** — writes each embedded material that is not
  remapped yet out as an editable `<name>.material.json` (a fresh id, the
  same values) into a
  `Materials/` folder next to the model, and remaps every one to its
  copy, as one undoable asset-history step. A name already taken in
  that folder gets ` 2`, ` 3`, … (`Painted 2.material.json`), so models
  sharing a folder never overwrite each other's copies. The copies keep pointing at
  the model's images (`{ asset: <model>, sub: "image/paint" }`), so no
  texture is duplicated — Unity's separate *Extract Textures* is not
  needed.

The CLI gets the same: `fynkel asset remap <model> <key> <material|none>`
and `fynkel asset extract-materials <model> [--to <dir>]`, for agents
and scripts.

### 6. What this does not do

- No remap by material **name across the project** (Unity's "search and
  remap" of every model at once). One model at a time; a project-wide
  pass can come later as a CLI command over the same table.
- No per-mesh material override in the sidecar; that is what prefab
  overrides are for.
- LOD groups, and anything else the model's hierarchy does not already
  carry, are separate work (Phase 4).

## Steps

1. **M1 — Import and cook.** The `materials` setting in both importers
   (through `graph.rs`), the dependencies, the versions, validation.
   Tests: a remapped model's prefab renders the project material; an
   instance override still wins; an explicit `material/…` reference is
   untouched; a missing key warns; a non-material target fails; editing
   the project material recooks it alone and the model loads it.
   **Done** (`crates/fynkel_import/tests/remap.rs`,
   `material_remaps_validate_against_the_database`).
2. **M2 — Editor and CLI.** The Materials section, Extract Materials, the
   two CLI commands. Tests: remap, revert and undo through the asset
   history; extract writes valid documents and remaps them in one step.
   **Done** (`extractMaterials` in `fynkel_authoring` over
   `fynkel_import::extract`, `remaps_are_settings_and_extract_materials_is_one_step`,
   `asset_remap_and_extract_materials_edit_the_models_sidecar`, the editor's
   `a model: materials remapped from the Materials section…`).
3. **M3 — Docs.** `documents.md` § Import (the setting), `editor.md`,
   the template's `AGENTS.md`. **Done.**

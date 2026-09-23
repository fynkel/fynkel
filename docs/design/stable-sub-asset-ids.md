# Stable sub-asset ids — design note

**Status:** Accepted (2026-09-23; the user took all three recommendations:
renames break loudly, `~2`/`#3` markers, explicit migration). Roadmap Phase 1 item 1
(`docs/roadmap.md`).
**Related:** ADR-0006 (asset identity: sidecars, references by id),
`docs/documents.md` § Prefab documents (overrides address prefab-local
ids), `crates/fynkel_import/src/{gltf,fbx,graph}.rs`,
`crates/fynkel_document/src/rig.rs` (rig clips are already `anim/<name>`).

## The problem

A reference to a part of a model is `{ "asset": <model>, "sub": "<key>" }`.
The sidecar maps each key to the part's own uuid. Today the importers make
the key from the part's **position in the file**:

| part | key today | source |
| --- | --- | --- |
| mesh | `mesh/<i>`, `mesh/<i>/<prim>` | `gltf.rs:391`, `fbx.rs:217` |
| material | `material/<i>` | `gltf.rs:365`, `fbx.rs:172` |
| image | `image/<i>` | `gltf.rs:350` |
| skin | `skin/<i>` | `gltf.rs:729`, `fbx.rs:559` |
| animation | `anim/<i>` | `gltf.rs:879`, `fbx.rs:677` |
| imported-prefab entity/component ids | uuid v5 of `node/<i>`, `node/<i>/transform`, … | `graph.rs:330–420` |

Reorder the meshes in Blender and re-export: `mesh/5` is now a different
mesh. Every scene that drew the ramp now draws whatever moved into slot 5,
**with no error**, because the key still resolves. The same happens to
materials, clips (a rig's `walk` plays `run`), and to prefab overrides on
an imported model's nodes (a `setField` on `node/12` lands on another
bone). The uuid in the sidecar does not help: it is attached to the key,
so it follows the position too.

The reference game already references `mesh/0`–`mesh/6`, `anim/0`–`anim/4`
and `skin/0` about 230 times.

## Decision

### 1. Keys come from names

| part | new key | example (reference game) |
| --- | --- | --- |
| mesh | `mesh/<mesh name>`; several primitives: `mesh/<mesh name>/<prim>` | `mesh/ramp` (was `mesh/5`) |
| material | `material/<name>` | `material/Beta_Joints_MAT1` |
| image | `image/<name>`, else the URI's file stem | |
| skin | `skin/<name>` | `skin/Skeleton` |
| animation | `anim/<name>` | `anim/walk` (was `anim/1`) |
| node (ids only, never a `sub`) | `node/<name path from the root>` | `node/Hero/mixamorig_Hips/…` |

- **Unnamed parts** keep their position, marked so it cannot collide with
  a name: `mesh/#3`.
- **Duplicate names** among parts of the same kind (or among siblings, for
  nodes): the first in file order gets the plain name, later ones `~2`,
  `~3`: `mesh/Cube`, `mesh/Cube~2`.
- **Characters** a key does not allow (the sidecar's `subId` pattern:
  letters, digits, `_ . / space -`) become `_`: Mixamo's `mixamorig:Hips`
  → `mixamorig_Hips`. The key only has to be stable, not reversible.
  The pattern gains `#` and `~` for the two markers above; the 128-character
  limit stays (a longer name is cut and gets a `~` hash suffix). Node paths
  are never sidecar keys, so they have no length limit.
- Primitives keep their index inside a mesh: they have no names, and
  reordering primitives inside one mesh is rare.

What this buys: **reordering** parts in the file keeps every reference.
**Renaming** a part in the modelling tool breaks its references — as in
Unity — but loudly: the key disappears, the existing "sidecar sub-asset(s)
no longer produced" warning fires, and every reference to it is
`FYN-A…` unknown sub, never a silent wrong part. The warning names the
new keys of the same kind that appeared in the same import, since one
vanished and one appeared is almost always a rename (an automatic remap is
deliberately not done: a guess that is wrong is the silent failure this
note removes).

### 2. Importer versions

`fynkel.gltf` 5 → 6, `fynkel.fbx` 1 → 2. The cache key already includes
the importer version, so every model recooks once.

### 3. Migration: `fynkel migrate` rewrites keys, keeps uuids

A sidecar written by the old importer version is migrated together with
every document that points into it, as one project operation:

1. Import the model's file **once** and derive both key tables from the
   same parse: old key (position) → new key (name), for every part and
   every node.
2. **Sidecar:** each `subAssets` entry is renamed old key → new key with
   **the same uuid**, so references by uuid (and the cooked data) keep
   pointing at the same part. `importerVersion` is bumped.
3. **Documents** (scenes, prefabs, materials, rigs, blends, volumes —
   anything `AssetDatabase::check_refs` walks): every `{ asset: <model>,
   sub: <old> }` becomes `{ asset, sub: <new> }`. Prefab instances of
   the model's hierarchy (`sub: "prefab"`) get their override targets
   (`entity`, `component`, `parent`) rewritten from the old node-derived
   uuids to the new ones.
4. Written with the usual backups (`Migrated::write_with_backup`); the
   editor offers it through the existing migration preview when it opens a
   project with old sidecars. `fynkel validate` reports an unmigrated
   sidecar (a new `FYN-A…` warning) with the command to run.

The migration maps positions **in the file as it is now**. That is exactly
what the old keys meant: they always referred to the current file's
positions. So a model that was already reordered before the migration
keeps whatever it shows today — the migration never makes anything worse,
it only stops it drifting further.

A model whose file is missing or unreadable is left as it is and
reported; its documents are untouched.

### 4. What does not change

- Asset uuids, document ids, `sub: "prefab"`, rig clip keys (`anim/<clip>`,
  already names), `packed` and other non-importer keys.
- The reference format (`assetRef`) and `AssetDatabase::resolve`.
- Instances in scenes that were **unpacked** from a model: they own their
  own ids already.

## Steps

1. **S1 — Keys.** `fynkel_import::subkey` (naming, sanitising, `#`, `~`),
   used by both importers and `graph.rs`; the schema pattern. An import
   through a sidecar at `formatVersion` 2 uses name keys; today's sidecars
   (1) keep position keys, so nothing changes until S2, and every import
   reports the renames (`Imported::renames`: sub keys and the prefab's
   derived ids) the migration needs. Tests: reorder meshes/materials/clips/nodes in a glTF and an
   FBX fixture, reimport, every key and uuid stays on the same part;
   duplicates, unnamed, bad characters, long names.
   **Done** (2026-09-23): `crates/fynkel_import/src/subkey.rs`,
   `tests/subkeys.rs` (reordered glTF, GLB and FBX fixtures).
2. **S2 — Migration.** Sidecar `formatVersion` 2 becomes current and the
   importer versions move (`fynkel.gltf` 5 → 6, `fynkel.fbx` 1 → 2) —
   together, since only then does an import's output change. The old→new
   table from one parse; sidecar and
   document rewrite in `fynkel_document`/`fynkel_import`; `fynkel migrate`
   runs it; the `FYN-A…` warning. Tests: the reference game's
   fixtures before/after, overrides on an imported prefab survive.
3. **S3 — Apply.** Migrate the reference game, the showcase, the starter
   template and the fixtures; the editor's migration preview; docs
   (`documents.md`, ADR-0006 consequence line).

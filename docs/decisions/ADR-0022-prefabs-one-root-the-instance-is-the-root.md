# ADR-0022: Prefabs — one root, and the instance *is* the root

**Status:** Accepted (2026-09-24, the user: "ok do that"; proposed the same day after the M4 gate rebuild showed `ball › Ball › Danger`)  
**Date:** 2026-09-24  
**Related:** ADR-0006 (document format, override targets by prefab-local id — unchanged), ADR-0020 (precedent: a scene + prefab `formatVersion` bump with a migration), ADR-0021 (Play in the editor), `docs/documents.md` § Prefab documents, `docs/scenes.md` § Runtime prefabs, `docs/hosts.md` § Prefabs at runtime, `docs/editor.md` § Prefab mode

## Context

Today a prefab document lists any number of `roots[]`. A scene places it
with an **anchor**: an ordinary scene entity that carries
`prefab: { asset, instance, overrides[] }` and its own components
(normally a Transform). When the scene is expanded, the prefab's roots
become the anchor's **children**. The runtime does the same:
`World.instantiate` spawns a new anchor entity and puts the prefab under
it.

So every instance is two levels. The user's gate rebuild showed it:
dropping the Ball prefab made `ball › Ball › Danger`. The object you
select (`ball`) has no mesh, no body and, until 2026-09-24, no Transform.
Everything the prefab is made of sits one level down, on a row you cannot
rename, and the Inspector shows "Prefab instance — no overrides" with
nothing else. Anyone coming from Unity reads this as a bug:

- **Unity:** a prefab has exactly one root GameObject. The instance in the
  scene *is* that root. Its Transform, mesh and body are shown on it; a
  changed value is an override (bold). Its position and rotation are
  simply overrides of the root's Transform. Renaming the instance renames
  the root. `Instantiate(prefab)` returns the root.

The two-level model also leaks into game code. `Collectible` destroys
`gameObject.parent`, because the coin's root has an anchor above it.
Emitters put the lifetime component on the anchor. The editor's preview
maps every pick to the outermost anchor.

Only a few prefabs in the repository use more than one root:

- `burst.prefab.json` has 6 roots, the six sparks;
- an intentionally invalid fixture;
- imported models whose file has several top-level nodes (`robot.glb` has 2).

Every other prefab has one root already. The change costs less now than
after release.

## Decision

1. **A prefab has exactly one root.** The prefab document's `roots` array
   is replaced by **`root: <entity id>`**. The root has no parent, and
   every other entity descends from it. Validation extends D0400 ("the
   root must be the prefab's only parentless entity"). Scenes and prefabs
   move to **`formatVersion` 5**.

2. **The instance is the root.** In a scene (or in a prefab, when nested)
   the entity that carries `prefab: { asset, instance, overrides }` **is**
   the prefab's root:
   - **Id.** It keeps its own scene id. The prefab's other entities are
     still `derive_id(instance, local)`, and component ids the same, so
     every existing derived id and override target stays valid except the
     old root's.
   - **Components.** They come from the prefab's root. An instance entity
     has **no components of its own**. A change is an override on the
     root's prefab-local ids, as for any other entity of the prefab.
     `addComponent` on the root adds one; `setField` on its Transform
     moves it. Placing an instance therefore writes a `setField` on the
     root Transform's `position` (and `rotation`/`scale` when they
     differ), which is what Unity stores.
   - **`name` and `active`.** The instance's own `name` and `active` win
     over the prefab root's (Unity: renaming an instance is allowed).
   - **Children.** The prefab root's children come first. Children the
     scene adds under the instance come after them, as today (Unity's
     "added GameObject").

3. **Imported models get one root, the way Unity's default import does:**
   - when the file has one top-level node, that node is the root and
     keeps its `node/<path>` id;
   - when it has several, a new root entity named after the file is
     added, with id `derive_sub_id(asset, "node")` and an identity
     Transform, and the top-level nodes become its children. Their ids
     don't change.

   The glTF and FBX importers bump their versions so every cache re-cooks.
   Rig documents follow their skeleton model, as today. A *Preserve
   Hierarchy* import setting (always wrap) can come later if someone needs
   it.

4. **Runtime.** `World::instantiate` / `World.instantiate` (Rust, ABI,
   Dart) spawn the prefab's root under `parent`, with the given transform
   **as the root's Transform**, and return **the root**. No extra entity
   is created. The ABI report names it `root`, not `anchor`. Emitters put
   `LifetimeDespawn` on the root. A cooked prefab has one root.

5. **Unpack** turns the instance into a plain entity with the same id: the
   root's components with the overrides applied, and derived children.
   **Making a prefab from the Hierarchy** keeps the object's id: the object
   becomes the instance, and its Transform stays in the prefab root, with
   no override. So scene references by id and by name survive.

6. **Migration, `fynkel migrate`, which reads the whole project.** An
   instance can't be migrated without its prefab, so this is a
   cross-document step like `keys_migration`, not a per-document one:
   - **A prefab with several roots:** a new root is added (named after
     the prefab, id `uuid_v5(prefab id, "root")`, identity Transform),
     and the old roots become its children. `addObject` overrides whose
     `parent` was null get the new root as their parent.
   - **An anchor:** its Transform is composed with the prefab root's
     Transform, and the result is written as `setField` overrides on the
     root Transform (only the keys that differ). Its other components
     become `addComponent` overrides on the root. The Runner's Animator
     and the Rig scene's Animator take this path. Its authored children
     stay as added children.
   - **When the migration refuses:** if a composition can't be expressed
     as position/rotation/scale, the migration refuses that anchor with a
     diagnostic and a pointer and changes nothing. That happens with
     non-uniform scale under a rotation.
   - **Scope:** all of this also runs on nested anchors inside prefabs.
     `.bak` copies are kept as for every migration.

7. **The editor follows from 2–5.**
   - **Hierarchy:** the instance row is the root. The prefab badge is on
     it, and its children are the derived rows.
   - **Inspector:** the instance shows the root's components, with
     overrides in bold and Revert/Apply per field. The prefab section
     (Open, Select asset, Unpack, Apply/Revert all) stays.
   - **Gizmos:** a gizmo drag on the instance writes overrides on the root
     Transform. The preview picks the instance itself.
   - **Prefab drop:** dropping a prefab writes the drop point as a
     position override.

## What changes for a developer

| Before | After |
| --- | --- |
| `ball › Ball › Danger` | `ball › Danger` (the instance *is* Ball, renamed) |
| The instance holds a Transform; the prefab root has its own | One Transform: the prefab root's, overridden by the instance |
| `world.instantiate(p)` returns a new parent; the prefab is under it | It returns the prefab's root |
| Game code destroys `gameObject.parent` to remove an instance | `gameObject.destroy()` (the reference game's `Collectible` changes) |
| A prefab may have several roots | One root; `burst` gets a `Burst` root over its six sparks |
| `robot.glb#prefab` has two roots | One `robot` root over them |

## Consequences

- **Good:**
  - What you see is one object per thing, as in Unity.
  - The Inspector on an instance is useful.
  - Positions are ordinary overrides, so "Revert position" and "Apply
    position to prefab" work like any field.
  - Game code no longer reasons about anchors.
  - One less entity per instance at runtime.
- **Cost:**
  - A `formatVersion` bump for scenes and prefabs.
  - A cross-document migration with fixture pairs.
  - Importer version bumps (every cache re-cooks once).
  - An ABI report field renamed.
  - Every place that reads `roots` or an anchor changes: the expansion,
    cook, runtime load, emitter, authoring commands (`instantiatePrefab`,
    `unpackPrefab`, `attach`/`detach`), the `instance` query, the
    editor's Hierarchy, Inspector, Scene view and apply-to-scene, the
    playground and stress generators, and the docs listed above.
- **Ids:** the one id that changes is the old prefab root's derived id in
  each instance. It becomes the instance's id. Nothing in the repository
  refers to it by id (entity fields refer by name, and `remap_strings`
  only rewrites prefab-local ids).
- **Your local files:** your uncommitted playground edits and the local
  `hero.prefab.json` (not in git) are migrated by `fynkel migrate` like
  everything else, and nothing is committed for you.

## Plan (one commit or small group each, gate green after every step)

1. **Documents** (`fynkel_document`):
   - format v5 (`root`), validation and the new expansion;
   - the authoring commands;
   - the cross-document migration with fixture pairs, including prefab
     fixtures;
   - the schemas regenerated through codegen.
2. **Import:** one root for imported hierarchies (importer versions), and
   the cook.
3. **Runtime:** `instantiate` returns the root (Rust, ABI, Dart), the
   emitter, the reference game's `Collectible`, and the runtime docs.
4. **Authoring session and editor:**
   - the `instance` query, unpack, making a prefab and apply-to-scene;
   - the Hierarchy, Inspector and Scene view;
   - the editor tests.
5. **Content:**
   - `fynkel migrate` over the reference game, the showcase, the fixtures
     and the benchmarks;
   - `playground.py` and `benchmarks/stress/generate.py` write v5;
   - the docs, and the M4 gate rebuild on a fresh copy.

## Amendment (2026-09-25): prefab variants and Unpack Completely

**Status:** Accepted — proposed by the assistant, confirmed by the user 2026-09-25.

Unity has three prefab workflows that this ADR did not cover: **Prefab
Variants**, **Unpack** vs **Unpack Completely**, and **making a prefab
from an object that already holds prefab instances**. With "the instance
is the root" in place, all three follow from rules already here.

1. **A variant is a prefab whose root is an instance of its base.** The
   variant's `root` entity carries `prefab: { asset: <base>, instance,
   overrides[] }` and has no components of its own (D0408, as for every
   instance). It may have plain children after the base's (Unity's added
   GameObjects). This is the nested case of decision 2, so:
   - **Ids.** The flattening already resolves it. The root keeps the
     variant root's id, and the other entities are `derive_id(instance,
     local)`. A chain (a variant of a variant) resolves innermost first.
     `derive_id` is unchanged, and so is every derived id.
   - **Cycles.** A variant that leads back to itself is `D0402`, like any
     other cycle.
   - **What follows.** Everything reads the flat prefab: describe, the
     preview, the `instance` query, the cook and `World::instantiate`.
     There is no variant-specific code downstream.
   - **Override addressing.** A variant's overrides address the base's
     flattened ids, like any instance's. A scene instance of a variant
     addresses the variant's flattened ids.
   - **Creating one.** `newDocument { kind: "prefab", variantOf, overrides?,
     instance? }`. The root is named like the base's root.
     `Prefab::variant_of()`, `assetDetail.variantOf` and the `instance`
     query's `prefab.variantOf` say that a prefab is a variant, and of
     what.

2. **Apply from an instance goes where the value comes from.** The new
   command `applyPrefabOverride { override }` is sent to the prefab
   document an instance comes from.
   - **Plain edit.** An op on the prefab's own entities and components
     becomes the equivalent plain edit.
   - **Override.** An op on something the prefab takes from another prefab
     becomes an override of that instance in the prefab. That other prefab
     is a variant's base, or a nested instance. The op is translated to
     that prefab's ids.
   - **Removing what the prefab added.** Removing an object or component
     that the prefab's own override added drops that override.
   - **Two transactions.** The editor then reverts the override on the
     instance, as for any Apply.
   - **Apply to the base.** Unity's "Apply to Base" is the same command
     sent further down the chain, one prefab at a time. It is not a
     separate mode.

3. **Unpack is one level by default. `completely: true` is Unity's
   Unpack Completely.**
   - **One level.** The instances nested in the prefab stay instances. An
     instance of a variant becomes an instance of its base. The outer
     overrides that land inside a nested instance move into its list.
   - **Completely.** Nothing below the entity is an instance any more.
     Every entity keeps the id the runtime already used. This is M4-07's
     unpack.
   - **Ids under one level.** Entities inside a kept instance get ids
     derived from the new instance id, `derive_id(instance, J's instance)`.
     `derive_id` is a hash, so the old ids cannot be carried over.
   - **Both.** The scene looks the same after either, and each is one undo
     step.

4. **A prefab can hold instances.** `instantiatePrefab` works in a prefab
   document, with an explicit `id`, `instance` and a whole `overrides[]`
   list. It is refused with `D0402` when the prefab being placed is, or
   contains, the document itself. So "make a prefab from an object that
   holds instances" writes nested instances rather than copies, which is
   what Unity does.

**No `formatVersion` bump.** Nothing new is written:
- A variant is a prefab document whose root carries the `prefab` block
  that decision 2 already allows on any prefab entity.
- `unpackPrefab.completely`, `instantiatePrefab.overrides` and
  `applyPrefabOverride` are authoring commands, not document keys.

A reader that predates this amendment already flattens a variant
correctly: `PrefabLibrary::flattened` handles an instance at a prefab's
root. So no document needs migrating, and no older build misreads a newer
file.

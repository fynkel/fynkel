# ADR-0027: Data assets — a Dart class as an asset type

**Status:** Accepted — proposed by the assistant 2026-09-25, confirmed by the user 2026-09-25 ("Okay, I accept that")  
**Date:** 2026-09-25  
**Related:** ADR-0006 (asset identity: references by id), ADR-0012 (codegen, the behaviour generator), ADR-0016 / ADR-0021 (the editor never runs game code / Play compiles the game into the editor), ADR-0018 (declarative Inspector hints and their amendments), ADR-0025 (physics materials, the last document kind added), ADR-0026 (code-first building), `docs/roadmap.md` Phase 3 item 2

## Context

Unity's `ScriptableObject` is how a game keeps data outside scenes: a
C# class with serialized fields becomes an asset type. `[CreateAssetMenu]`
puts it in the Create menu, each asset is edited in the Inspector, and
scripts reference it by dragging it onto a field. At runtime every
reference to one asset shares a single loaded object. Weapons, enemy
stats, item tables, wave lists, dialogue lines and level settings all
live this way. They are tuned in one place and shared by every prefab
that points at them.

Fynkel has no equivalent. A game today has three options for such data:
a behaviour's exposed fields, repeated on every prefab; Dart constants,
which the editor cannot see or edit; or a hand-written JSON file with no
schema, which nothing validates. R3a deferred a general game-defined data
asset "to the package-architecture work, where the question of who owns
a document kind gets answered once" (blueprint §8, 2026-09-22). This ADR
answers that question and is written ahead of the package ADR.

What exists already (mapped 2026-09-25):

- **The document pipeline handles any JSON kind.** Reference tracking
  ("Used by"), cook dependencies, the residency closure and the
  generated `Assets` constants handle every document kind without
  per-kind code.
- **What does not exist:**
  - The document kinds are a closed Rust enum (`DocumentKind`), with one
    schema per kind. Only components and behaviours have schemas keyed
    by type id.
  - Dart cannot read an asset's cooked content. `world.assets` answers
    ids and paths only.
  - `@Expose` has one list type (`List<Vec3>`) and no nested values.
  - Validate checks that a referenced asset exists, but not that it is
    of the right kind.

## Decision

1. **One engine-owned document kind; game-owned types.** A new
   `DocumentKind::Data` (kind name `data`, suffix `.data.json`, the
   hand-written envelope `schemas/data.schema.json`) holds
   `{ "$schema", "formatVersion": 1, "id", "name", "type": "game.Weapon",
   "version": 1, "data": { … } }`. This is the same `{type, version,
   data}` shape a component instance has. The engine owns the kind. A
   type belongs to whoever declares its Dart class, a game or a package,
   and is named by a namespaced type id like a behaviour's (`game.`,
   later a package's prefix). Adding a data type never touches Rust,
   because one kind covers every type. This is the answer R3a deferred.
   The package ADR only decides the namespaces.

2. **Declared as a Dart class, the way a behaviour is declared.**

   ```dart
   @FynkelDataAsset(id: 'game.Weapon', menu: 'Game/Weapon', icon: 'sword',
       description: 'What a weapon does')
   class Weapon extends DataAsset {
     Weapon({this.damage = 1, this.cooldown = 0.5, this.projectile, this.sounds = const []});
     @Expose(min: 0) int damage;
     @Expose(unit: 's') double cooldown;
     @Expose(kind: 'asset(prefab)') AssetRef? projectile;
     @Expose() List<AssetRef> sounds;
   }
   ```

   - **Fields.** `@Expose` has the same vocabulary and hints as on a
     behaviour (ADR-0018). The two scene references, `GameObject?` and
     behaviour fields, are a codegen error on a data asset, because an
     asset cannot point into a scene (Unity's rule too).
   - **The generator.** `fynkel codegen`'s behaviour generator (ADR-0012)
     emits:
     - the type's field table and registration;
     - its JSON Schema, to `schemas/data/<id>.schema.json`, with
       `x-fynkel.kind: "data"`;
     - its metadata (`menu`, `icon`, `description`) in `behaviours.g.json`.

     `SchemaSet` loads these schemas by type id, as it loads behaviour
     schemas. The project's registry (`fynkel.project.json` →
     `components`) gains entries of kind `data`, written by the same
     authoring op that registers behaviours.
   - **`version`.** It works as a behaviour's does.

3. **One shared instance per asset, mutable for the run, never saved
   from the game.** At runtime a data asset is an ordinary Dart object.
   The world makes one instance per asset id the first time something
   needs it and keeps it for the world's life, across scene loads, so
   every reference shares it (Unity's model).
   - **Game code** may change its fields. The change lasts for the run
     and is never written back, in a build or in the editor's Play. This
     avoids Unity's well-known trap where Play-mode changes to a
     ScriptableObject silently persist.
   - **The Inspector** edits the document itself, so its edits are
     saved. During Play they are also applied to the running instance
     (item 7).

4. **References are typed by the class.**
   - **Declaring one.** A field typed by a data class (`@Expose() Weapon?
     weapon;` on a behaviour or on another data asset) is stored as
     `{ "asset": id }`. Its schema type is `data`, and `x-fynkel.data`
     names the type.
   - **At load**, after the behaviours of a load are attached, the world
     resolves the field to the shared instance. This is the same second
     pass object references use (ADR-0018 amendment). A missing asset
     leaves the field null with a new load warning, `FYN-B0004`.
   - **Validate** reports a referenced document of the wrong type, and
     the cook refuses it. The check is general: any `asset(<kind>)` hint
     or data-typed field whose target has another kind is `FYN-A0007`
     (`ASSET_WRONG_KIND`), which today only material remaps raise.
   - **Code.** `world.data<Weapon>(ref)` gives the instance for an
     `AssetRef`, for example `world.data<Weapon>(Assets.data.sword)`.

5. **Lists, and records inside them.**
   - **Lists.** `@Expose` gains `List<T>` for every exposed scalar type,
     `Vec3`, `Quat`, enums, `AssetRef` and data classes. Behaviours get
     them too, since they share the generator.
   - **Records.** A class annotated `@FynkelRecord` is a nested value
     with `@Expose` fields (Unity's `[Serializable]` class, such as a
     wave of `{enemy, count, delay}`). It is stored as a JSON object,
     alone or in a list, and has no identity of its own.

   This is what makes item tables, wave lists and dialogue possible.
   The Inspector draws a list with add, remove and reorder, and a record
   as a foldout.

6. **The runtime reads cooked JSON through one new ABI call.**
   - **Cook.** A data document is validated against its type's schema
     and cooked as the generic JSON chunk (volumes and physics materials
     already work this way), so a scene's closure retains it.
   - **ABI.** `fynkel_world_asset_json(world, id)` returns the cooked
     JSON of a document asset the store holds. Data assets are its first
     user; any document kind can be read through it.
   - **Unchanged.** No wire record changes, so the schema fingerprint
     stays the same, and the ABI stays version 1: the call is an
     addition. The lock's `documents` gains `data: 1`.

7. **The editor shows a data asset like any other asset.**
   - **Project browser and Create menu.** A Data kind in the Project
     browser, and a Create entry for each type's `menu` path read from
     the committed metadata. The editor still never runs game code for
     authoring (ADR-0016).
   - **Inspector.** A data asset's Inspector page is one registered
     `AssetDocumentSections` builder. It draws the type's fields from its
     schema with the same field specs and widgets a script's Inspector
     uses, and saves each edit as a document transaction (`AssetEdits`).
     To make that possible, the component Inspector's field editor is
     refactored so it no longer needs an entity slot.
   - **Pickers.** A data-typed field's picker lists only documents of
     that type.
   - **Play.** Because ADR-0021 compiles the game into the editor, an
     Inspector edit during Play is also applied to the running instance,
     as Unity does.

8. **From code, first-class (ADR-0026).**
   - **Without a file.** `Weapon(damage: 3)` is a data asset with no
     document. A live-built behaviour field can hold it directly.
   - **Written to a file.** `ProjectWriter.writeData('data/sword',
     weapon)` writes the document (id stable across runs, untouched when
     unchanged) and remembers the instance, so a scene or prefab written
     afterwards stores that id for every field that holds it.
   - **Refused.** A written scene that refers to an instance that was
     never written is an `ArgumentError`.
   - **Constants.** `Assets.data.sword` comes from the folder-based
     generator as it stands.

## Alternatives considered

- **One `DocumentKind` per data type** (`.weapon.json`). This matches
  how volumes and physics materials were added, but every game type
  would then change the Rust enum, the validator and the cook. The kind
  list is closed and belongs to the engine; the type is the open part.
- **Data as JSON with no Dart class** (tables the game parses itself).
  There would be no schema, no Inspector, no typed references and no
  compile-time names. That is what games have to do today, and why this
  ADR exists.
- **Immutable value classes** (`final` fields, `copyWith`). These are
  safer for shared data, but they break from behaviours, which have
  mutable exposed fields and share one generator and one Inspector path.
  They also break from Unity, which is the reference. Rule 3 keeps the
  danger small: runtime writes are never saved.
- **Data assets as hidden entities with components.** This would reuse
  the scene path, but an asset outlives scenes, is referenced across
  them, and has no transform or lifecycle. It would be a fake object
  with a fake place in the world.
- **Waiting for the package ADR.** The kind-ownership question is the
  only part of this that touches packages, and rule 1 answers it
  independently. Waiting would leave games without the tool that most
  Unity projects rely on first.

## Consequences

- Game data gets one home, with a schema, an Inspector, references by
  id, "Used by", cooking and residency. Most of it reuses machinery that
  already handles any document kind.
- The shared generator grows lists, records and data-typed fields, so
  behaviours gain them too. `List<Vec3>` stops being the only list.
- **Validate becomes stricter.** A reference to an asset of the wrong
  kind, which today passes and fails only at runtime, becomes a validate
  error. Existing projects that hold such a reference will see it
  reported.
- **Dart can read cooked JSON.** One new ABI call opens every document
  kind's content to Dart, which later tools may use.
- **Runtime writes are never saved,** in Play too. A Unity user who
  relies on Play-mode changes persisting will not find them persisted.
  The docs say so.
- **What is left for the editor-extension ADR (Phase 3 item 1).** A data
  asset's page is generic. Custom Inspectors for data types are that
  ADR's first use case.

## Order

1. List fields in `@Expose` (behaviours and the Inspector), with the
   general wrong-kind check in validate.
2. The `data` document kind in Rust: the envelope schema, per-type
   schemas by type id, validate, the cook, the lock, `fynkel document new
   --type`, and the authoring session's `newDocument`.
3. The generator: `@FynkelDataAsset`, `DataAsset`, `schemas/data/`, and
   the registry entries.
4. The runtime: `fynkel_world_asset_json`, `world.data<T>`, and
   data-typed fields resolved at load (`FYN-B0004`).
5. Code first: `ProjectWriter.writeData` and written references.
6. The editor: the Data kind, the Create menu, the Inspector page, typed
   pickers, and live Play edits.
7. `@FynkelRecord` records, alone and in lists.
8. Acceptance. `examples/code_only` moves its coins' value, colour and
   sound into a data asset written from Dart, and a second coin type (a
   big coin worth five) is a second asset of the same type. The headless
   test covers both ways. On a copy of the project, the editor creates a
   Weapon-style asset, edits it and assigns it to a script field, and
   the Play result follows the edit.

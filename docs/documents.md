# Authoring documents

Status: M2 (blueprint §6, §7, §14.2; ADR-0006, ADR-0016). Implemented in
`crates/fynkel_document`; `fynkel document …` in the CLI. Kind-specific
bodies (scene, prefab, material, shader, project, meta) are added by the
following M2 items and documented here as they land.

## Envelope

Every document is one JSON object:

```json
{
  "$schema": "https://fynkel.dev/schemas/scene.schema.json",
  "formatVersion": 1,
  "id": "70d7ba8b-f7fb-4eba-920d-ec9e6b968330",
  "name": "TrainingRoom",
  ...kind-specific body...
}
```

| Field | Rule |
| --- | --- |
| `$schema` | optional string; should name `<kind>.schema.json` (warning `FYN-D0014` otherwise). Either the canonical URL or a relative path is accepted. |
| `formatVersion` | integer ≥ 0. Newer than the tool supports → `FYN-D0013`, the document is **read-only**: it is refused, never rewritten. Older than the current version → `FYN-D0016`, the document is **outdated**: it can be read and migrated (`fynkel migrate`), not validated or loaded. |
| `id` | canonical UUID text: lower-case, hyphenated, 36 characters, not nil. It is the document's persistent identity; names and paths are labels. |
| `name` | string ≤ 256 characters. |

Kinds and file names: `.scene.json`, `.prefab.json`, `.material.json`,
`.shader.json`, `.meta.json`, and the fixed `fynkel.project.json`. The kind
is taken from the file name (`--kind` overrides).

## Strict JSON

The reader (`fynkel_document::json`) is a purpose-built parser that keeps a
source span for every value and the original lexeme of every number, so
diagnostics can point at a line/column for any JSON pointer and unknown data
survives a rewrite byte-for-byte. It rejects, with codes:

| Code | Rejects |
| --- | --- |
| `FYN-D0001` | syntax: comments, trailing commas, single quotes, leading zeros, unescaped control characters, lone surrogates, a BOM, content after the root |
| `FYN-D0002` | duplicate keys (anchored at the second key) |
| `FYN-D0003` | `NaN`/`Infinity`, numbers outside `f64`, integers beyond 2^53 (they would not survive a Dart `double`) |
| `FYN-D0004` | a root that is not an object |
| `FYN-D0005` | nesting deeper than 128 levels |

A syntax error is reported alone: nothing after it is meaningful.

## Canonical form

`Document::write` (and `fynkel document format`) emits: two-space indent,
one member/item per line, `{}`/`[]` for empties, minimal string escaping
(non-ASCII kept raw), number lexemes exactly as parsed (`1.0` stays `1.0`;
programmatic floats use the shortest round-trip form with a fraction), LF
line endings, trailing newline. Key order is the document's own order,
except that the envelope keys are hoisted to the front in the order above.
Unknown keys keep their value and position; canonical text is a fixed point.

## Validation

`Document::validate(&SchemaSet)` runs the typed envelope check (exact
messages, `FYN-D001x`) and then JSON Schema 2020-12 validation against
`schemas/<kind>.schema.json`, every `*.schema.json` under the directory being
registered by its `$id` so `$ref`s resolve locally (nothing is fetched).
Schema violations are `FYN-D0100` with the instance pointer and its source
position; unexpected properties get one diagnostic each, anchored at the key.
A kind without a schema yet validates against the shared
`schemas/envelope.schema.json` with a `FYN-D0101` warning. Semantic checks
(references, uniqueness, cycles) are per kind and documented with each.

`schemas/envelope.schema.json` also hosts the shared `$defs`: `uuid`,
`typeId` (`fynkel.Transform`, `game.PlayerController`), `assetRef`
(`{ "asset": "<uuid>", "sub": "<sub-id>"? }` — identity, never a path).

## Diagnostics

Every authoring diagnostic is the §14.2 record
(`fynkel_contracts::Diagnostic`): `code`, `severity` (`error` / `warning` /
`info`), `message`, `document`, `pointer` (RFC 6901), `id`, `line`,
`column`, `suggestion`, `related`. Absent fields are omitted in JSON. Code
namespaces: `FYN-D` documents, `FYN-A` assets, `FYN-P` project, `FYN-G`
codegen, `FYN-E` editing (authoring commands), `FYN-B` behaviours (the Dart
SDK attaching authored behaviours at load), `FYN-H` physics (scene rules the
component schemas cannot express, [physics.md](physics.md)), `FYN-R` runtime loading (the
cooked cache: `R0001` unreadable, `R0002` stale — cooked for another schema
fingerprint, `R0003` asset not in the cache, `R0004` its cook failed,
`R0005` object unreadable, `R0006` chunk missing, `R0007` payload
malformed, `R0008` cooked scene invalid for this build, `R0009` no startup
scene); append only.

Human form: `error[FYN-D0012] assets/scenes/main.scene.json:3:3 /id: id is not a canonical UUID: upper-case hex (use lower-case …)`.

## Scene documents

`.scene.json` bodies are validated by `schemas/scene.schema.json`, which
`fynkel codegen` generates from the envelope plus every registered component
schema (`docs/codegen.md`); the component `data` of each authored type is
bound with an `if type == … then data $ref` clause, so a bad Transform is
reported at its exact field. Body:

```json
"entities": {
  "<entity uuid>": {
    "name": "Player",
    "parent": null | "<entity uuid>",
    "children": ["<entity uuid>", ...],
    "components": {
      "<component uuid>": { "type": "fynkel.Transform", "version": 1, "data": { ... } }
    }
  }
}
```

`fynkel_document::Scene::from_document` builds a lenient view (malformed
entries are skipped and reported, never fatal) and `Scene::validate` adds the
semantic rules on top of the schema pass:

| code | rule |
|---|---|
| `FYN-D0200` | `parent` names an entity that does not exist |
| `FYN-D0201` | a `children` entry names an entity that does not exist |
| `FYN-D0202` | `children` ↔ `parent` disagree (reported on both sides) |
| `FYN-D0203` | the `parent` chain has a cycle |
| `FYN-D0204` | two components of the same type on one entity |
| `FYN-D0205` | unknown component type (warning; `data` is kept opaque) — not raised for a behaviour type the schema set knows (`schemas/behaviours/<id>.schema.json` in the project, or a registry entry of kind `behaviour`) |
| `FYN-D0206` | component `data` violates the type's schema (per field; behaviour schemas too) |
| `FYN-D0207` | type is not authorable (e.g. `fynkel.Name` — set `name` instead) |
| `FYN-D0208` | the same child listed twice |
| `FYN-D0209` | malformed entity/component entry skipped by the view |
| `FYN-H0001` … `H0004` | physics rules (M3-08, [physics.md](physics.md)): a `fynkel.RigidBody` value the runtime would reject, a `fynkel.Collider` value (size per shape, friction, restitution, `mesh` missing), a `shape: mesh` collider on a dynamic or kinematic body, a dynamic body without a collider (warning) |
| `FYN-H0005` … `H0007` | character rules (M3-09): a `fynkel.CharacterController` value the runtime would reject (capsule, slope, negative limits), a `fynkel.RigidBody`/`fynkel.Collider` next to a `fynkel.CharacterController` (its own capsule replaces them; one diagnostic per ignored component), a `fynkel.ThirdPersonCamera` value (distance, pitch range, sensitivity). The corpus fixture `scene/physics_rules.scene.json` lists `H0001`–`H0007` |

Component schemas carry `x-fynkel.authored`; `#[schema(authored = false)]`
marks runtime-only records. `fixtures/scene/` holds one document per rule
with its expected diagnostics.

Component `data` follows the field types of `docs/codegen.md`: vectors are
number arrays, enums are the lowerCamel variant name (`"projection":
"perspective"`), asset references are `{ "asset": "<uuid>", "sub": "mesh/0" }`
with `sub` omitted for the asset itself — never a path. Built-in authored
components today: `fynkel.Transform`, `fynkel.MeshRenderer`, `fynkel.Camera`,
`fynkel.DirectionalLight`, `fynkel.PointLight`, `fynkel.SpotLight`,
`fynkel.Environment` (`fixtures/valid/training_room.scene.json` uses all of
them). Scenes and prefabs are **formatVersion 2** since M3-04: `fynkel.Camera`
gained `exposure_mode` (`auto` | `manual`) and `exposure` (EV100; the
camera-exposure migration below fills the defaults in), `SpotLight`
(candela, range, inner/outer cone angles in radians) and `Environment`
(constant ambient in lux, optional equirectangular `sky` texture reference
with `sky_intensity` in nits and `sky_rotation`; the first one in the scene
counts) were added; `docs/renderer.md` says how the renderer uses each.

A component of any other type is a **behaviour**: a Dart class of the game
(`@FynkelBehaviour(id: 'game.Spinner')`) whose `@Expose` fields are its
`data`. `fynkel codegen --project` generates `schemas/behaviours/<id>.schema.json`
for each (`docs/codegen.md` § Behaviour generator); a project's `schemas/`
directory is loaded on top of the embedded schemas (`SchemaSet::embedded_with`,
`Project::schemas`), and every `components` registry entry of kind
`behaviour` declares its type known even without a schema. The runtime never
instantiates these; the Dart SDK does, from the load report (below).
`fixtures/project` has `game.Spinner` on the Robot with `speed: 2.5`, and the
generated schema under `fixtures/project/schemas/behaviours/`.

## Scene → runtime

`fynkel_document::describe(&Scene, &SchemaSet)` turns a validated scene into
the typed `fynkel_core::SceneDescription` (entities in document order, each
with its `ComponentValue`s) plus `behaviours: Vec<AuthoredBehaviour {
entity, component, type_id, version, data, pointer, known }>` — every
non-native component, for the SDK; a type the schema set does not know as a
behaviour is still handed over but warned as `FYN-D0205`. Undecodable native
data is `FYN-D0206`. `World::load_scene(&description)` then
spawns roots in key order and children in listed order, applies every
non-Transform component through `Command::SetComponent` (so a value the
bridge would reject is rejected here too, command index `u32::MAX` in
`World::diagnostics()`), propagates transforms and emits `SceneReady`. The
returned `LoadedScene` maps authoring UUID ↔ runtime handle both ways;
structural errors (`LoadError`) load nothing. The typed ↔ JSON codecs are
generated (`crates/fynkel_document/src/generated.rs`, `DocumentCodec`), as
are the schemas `SchemaSet::embedded()` compiles in, so a shipped runtime
validates without a `schemas/` directory. `docs/hosts.md` covers
`fynkel test --headless` and `fynkel state dump`.

`describe` is what the cooker runs on a scene (after validating and
expanding it) to check it and to record its behaviour types, and what the
runtime runs on the expanded chunk it reads back. **A host never opens a
scene file** (M3-03, D2): source documents are the authoring format, the
cooked cache is what runs.

### Cooked scenes and the runtime store (`fynkel_assets`)

`fynkel cook` writes each scene's `JSON` chunk **expanded** — prefab
instances applied, their entity and component ids derived, overrides in
place, no `prefab` blocks left — with META
`{ documentKind: "scene", entities, instances, expanded: true,
behaviourTypes: [{ type, version }] }` (the behaviour types the project's
schemas and registry knew, so the runtime declares them known without the
project), and each prefab's chunk as its template (`entities` in META).
An invalid scene is a failed cook, never an object.

`crates/fynkel_assets` is the runtime's only door to content:

- `CacheIndex::open(cache_dir)` reads `index.json` and refuses a cache
  cooked for another schema fingerprint (`FYN-R0002`; a missing or
  malformed index is `FYN-R0001`, "run `fynkel cook`"). It knows the
  profile, the project (`name`, `startupScene`, `quality` — the profiles
  the renderer switches between, D3) and, per asset, the
  authoring path (for messages), kind, status, key, object file and
  dependencies; `closure(id)` is an asset and everything it depends on.
- `Assets` is the shared, thread-safe **residency store** a world and its
  renderer both hold (clone = same store). `mesh(&AssetRef)`,
  `texture(&AssetRef)` and `material(&AssetRef)` load on first use from
  the object whose chunk `kind` and `sub` match (bounded reads) and stay
  resident while something retains their asset; `retain(ids)` hands out a
  `Residency` token — a loaded scene holds one for its closure — whose
  release (or drop) evicts every payload of an asset no other token
  retains and bumps an eviction counter the renderer compares to drop its
  GPU copies. Every miss is a `FYN-R` diagnostic naming the asset id:
  not in the cache (`R0003`), cook failed (`R0004`), object unreadable
  (`R0005`), no such chunk or sub-asset (`R0006`), payload malformed
  (`R0007`) — never a fallback to a source file.
- `Assets::scene(id)` reads the expanded chunk, declares the recorded
  behaviour types, validates and describes it →
  `CookedScene { id, name, key, path, description, behaviours,
  diagnostics, closure }`; a chunk this build cannot describe is
  `FYN-R0008` (recook); a prefab or other non-scene asset is `R0006`.
  `startup_scene()` is the index's, or `FYN-R0009` for a cache cooked
  without a project (`fynkel cook --assets`).

Over the C ABI (`fynkel_native::load`): `fynkel_world_load_project(world,
cache_utf8, len)` loads the startup scene the cache's index names and
`fynkel_world_load_scene(world, cache_utf8, len, scene_id_utf8, len)` any
cooked scene by id; both return `FYNKEL_STATUS_OK`,
`FYNKEL_STATUS_LOAD_FAILED` (the world is unchanged) or
`FYNKEL_STATUS_INVALID_ARGUMENT` (NULL, non-UTF-8, non-UUID). The world
keeps one `Assets` store per cache directory and retains each loaded
scene's closure until it is destroyed; a viewport rendering the world
adopts that store (`fynkel_apple_viewport_render`), so residency and GPU
copies agree without the host pointing the viewport at anything.
`fynkel_world_load_report` is the JSON of the last call: `{ ok, scene: {
id, name, path, key, entities, rejectedComponents, assets, behaviours: [{
entity, authoringId, component, type, version, known, data, pointer }] } |
null, cache: { dir, profile, project, startupScene } | null, residency: {
retained, meshes, textures, materials, bytes }, diagnostics: [...] }` —
`entity` is the spawned handle, `assets` the closure size. In Dart,
`World.loadProject(dir, {cacheDir})` (default `<dir>/.fynkel/cache`) and
`World.loadScene(cacheDir, sceneId)` return a `LoadReport` (`sceneKey`,
`cacheDir`, `cacheProfile`, `startupScene`, `assetCount`, `residency`;
warnings to `onLoadDiagnostics`) or throw `FynkelLoadError`; on success
the SDK instantiates every listed behaviour through `world.registry`
(`registerBehaviours(world.registry)` from the generated
`behaviours.g.dart` first) and attaches it to `attach(entity)`. A type the
registry lacks is `FYN-B0001`, data the type rejects is `FYN-B0002` — both
warnings in the report (`behavioursAttached` counts the rest); the entity
and its native components load either way. Tests cook first:
`package:fynkel/testing.dart`'s `cookProject(dir, {cacheDir})` runs the
`fynkel` CLI (`FYNKEL_CLI`, `fynkel` on `PATH`, or `cargo run -p
fynkel_cli` inside a source checkout).

### Cooked assets in the renderer

`fynkel_render` draws from the same `Assets` store (`set_assets`; a store
it already holds is a no-op, so re-pointing per frame is free): every
extracted `MeshRenderer` with its mesh and its `fynkel.pbr@1` material
(the `PbrMaterial` record decoded from the cooked `MATL` chunk — every
factor and all five maps shade, `docs/renderer.md`), from the scene's
`Camera` (highest `priority`, perspective or orthographic, its exposure)
with its first `DirectionalLight` as the sun, the eight nearest point and
spot lights and its `Environment`; a mesh missing from the cache draws as
a magenta cube, a missing map as its neutral fallback, and entities
without a `MeshRenderer` as tinted placeholder cubes when
`ExtractRenderer::placeholders` is on (the default; the Flutter host turns
it off with a project). At the start of each `prepare` the renderer drops
the GPU copies of assets the store evicted since its last frame (a scene
unload) — a material's bind group goes with any of its maps — and reloads
what the extract still references. The cooked vertex layout is the GPU
layout (`fynkel_cooked::payload::Vertex`, 72 bytes; the tangent the cook
generated when the source had none is what normal mapping uses). A
viewport drawing a world that loaded from a cache uses the world's store;
`fynkel_apple_viewport_set_asset_cache` remains for viewports drawing a
world that did not (the demo scene). `fynkel render` does the whole path
headlessly, from the cache alone.

## Prefab documents

`.prefab.json` (`schemas/prefab.schema.json`, generated from the same
document model as the scene schema) is a scene body plus `roots[]`: the
parentless entities, in order — validation checks both directions
(`FYN-D0400`). A scene entity becomes an **instance anchor** by carrying
`prefab: { asset, instance, overrides[] }`: `asset` is the prefab document's
`id` as an `AssetRef`, `instance` a UUID v4 that is stable for the anchor's
lifetime, `overrides` an ordered op list addressed by **prefab-local** ids:

| op | fields | note |
| --- | --- | --- |
| `setField` | `entity`, `component`, `field`, `value` | `null` stores null |
| `unsetField` | `entity`, `component`, `field` | removes the key (the schema default applies) |
| `addComponent` | `entity`, `component`, `type`, `version`, `data` | new instance id, checked for uniqueness |
| `removeComponent` | `entity`, `component` | |
| `addObject` | `entity`, `parent`, `name`, `components` | `parent` is a prefab-local entity or absent (root) |
| `removeObject` | `entity` | removes the subtree |

Array indices, display names and deep merges are not override targets.
Two anchors sharing an `instance` id is `D0405`.

**Nested prefabs** (M3-08b): a prefab entity may carry `prefab` itself. The
library is *flattened* before anything expands
(`PrefabLibrary::flattened(schemas)`): every prefab's own instances are
expanded into plain entities, innermost first, with the same derivation rule
(the inner instance id is the namespace), so a scene instantiating the outer
prefab gets the whole tree remapped once more by its own instance id. A
chain that leads back to a prefab being flattened (a cycle) is `D0402`,
anchored at the closing instance's `prefab/asset` in the prefab document;
the prefabs involved stay unflattened and a scene instantiating one is
refused for that instance (`D0402` at the scene's instance). `fynkel
validate --project` flattens the library once and reports every prefab's
problems where they are; a single scene's expansion only carries the
problems of the prefabs it reaches. The cook writes each prefab's chunk
**flat** (`documentKind: prefab`, info `entities`, `roots`, `instances`,
`expanded`, `behaviourTypes`), which is what `fynkel_assets::Assets::prefab`
describes for runtime instantiation (hosts.md § Prefabs at runtime).

`fynkel_document::expand(&scene, &library, &schemas) -> Expanded` replaces
every anchor's instance by the prefab's entities: each prefab-local entity
and component id becomes `derive_id(instance, local)` — UUID v5 with the
instance as namespace, so expansion is deterministic and two instances of the
same prefab never collide (`D0406` if a derived id meets an authored one) —
and every reference inside the prefab (`parent`, `children`, component data
strings equal to a local id) is remapped; `AssetRef`s stay shared. Instance
roots are appended to the anchor's `children` after its authored children,
the new entities follow the anchor in document order, and the `prefab` key is
gone from the result, which is a plain scene (`Expanded::scene()`).
Overrides apply in order; a target that does not exist (or no longer exists)
is `D0403`, an op whose result would not validate against the component
schema (or a duplicate unique component) is `D0404`, a malformed op is
`D0407`. An unknown prefab is `D0401`; the whole instance is skipped and the
anchor kept. `PrefabLibrary::scan(dir)` collects every `*.prefab.json`
under a directory (a later path with the same id wins, with a warning).
The headless host expands before `describe`, so derived ids reach the
runtime and `fynkel state dump` prints them.

## Authoring commands and transactions

`fynkel_document::authoring` (§11.1, ADR-0016) is the one implementation of
document mutation the editor, the CLI and agents share; the C ABI for it
waits for M4. An `Editor` holds one scene or prefab document (`Editor::open`
refuses other kinds, read-only newer documents and outdated ones with
`FYN-E0007`; a document that already has errors opens, and those baseline
errors are tolerated). Its `Revision` is the blake3 hash of the canonical
text (`Document::write`), so two tools that read the same bytes agree on it
without shared state — `fynkel scene inspect --json` and `fynkel prefab
inspect --json` print it as `revision`.

`Editor::apply(Request { id?, baseRevision?, commands }, &schemas)` runs one
**transaction** atomically:

1. When `baseRevision` is given and differs from the editor's revision the
   whole request is a conflict (`FYN-E0001`): nothing is read, nothing is
   applied, the caller re-reads and rebases. Two writers never overwrite each
   other silently.
2. Each command compiles to primitive, reversible **edits** on the JSON tree
   (`replace` at a pointer, `insertMember`/`removeMember` at an index of an
   object, `insertItem`/`removeItem` of an array). Applying an edit yields
   its exact inverse; the command's `undo` is those inverses in reverse. A
   command that cannot run (unknown entity `E0002`, unknown component
   `E0003`, malformed `E0004`, cycle `E0006`, not an instance `E0008`,
   duplicate id `E0010`) stops the transaction and everything already applied
   is rolled back.
3. The document is validated (`Scene::validate` / `Prefab::validate`). Any
   error the transaction *introduces* (not in the baseline) rejects it whole
   with `FYN-E0005`, the errors in `related`, and the document back to the
   byte. Warnings (an unregistered component type, say) are reported and
   never fatal.
4. Side effects run last, once the document is known good: `saveDocument`
   writes the document (to `path` or where it was read from) and
   `createMaterial` writes a new material document (never overwriting;
   validated against the built-in shader library first). A failed side
   effect (`FYN-E0011`) rolls the transaction back too.

The result (`Applied`, `--json` shape) is `{ ok, transactionId, baseRevision,
revision, mutations[], diagnostics[] }`; each mutation is the §11.1 record
`{ commandId, command, transactionId, baseRevision, affectedIds[], ok,
diagnostics[], undo[] }`. `affectedIds` lists the entities, components,
instances and documents the command touched or created. The editor keeps the
committed transactions as its history: `undo()` replays the inverses of the
last transaction and `redo()` its forward edits, both byte-exact (key order,
positions and number lexemes come back as they were); `FYN-E0009` when there
is nothing to undo or redo. `is_dirty()` compares the revision with the one
last opened or saved; a saved file stays saved when its transaction is
undone. A transform-gizmo drag is one transaction, so one undo step.

The commands (`Command`, JSON form `{ "command": "<name>", ... }`, ids are
new UUIDs when omitted, `field` is a key of the component's `data` or a JSON
pointer into it such as `/position/1`):

| command | fields | effect |
| --- | --- | --- |
| `createObject` | `id?`, `name`, `parent?`, `index?`, `components[]` | new entity `{ name, parent, children: [], components }`, appended to (or inserted at `index` of) the parent's `children`, or the prefab's `roots` |
| `deleteObject` | `id` | removes the entity and its subtree, detaches it from its parent / `roots` |
| `renameObject` | `id`, `name` | |
| `reparentObject` | `id`, `parent?`, `index?` | moves the entity (`FYN-E0006` if `parent` is the entity or below it); `null` makes it a root |
| `addComponent` | `entity`, `component: { id?, type, version?, data? }` | `version` 1 and empty `data` by default; `FYN-E0010` if the id exists |
| `removeComponent` | `entity`, `component` | |
| `setField` | `entity`, `component`, `field`, `value` | replaces the value, or adds the member / appends to an array end |
| `unsetField` | `entity`, `component`, `field` | removes the member (distinct from setting `null`) |
| `assignAsset` | `entity`, `component`, `field`, `asset: { asset, sub? }` | `setField` with an `assetRef` |
| `instantiatePrefab` | `id?`, `name`, `parent?`, `index?`, `prefab: { asset, sub? }`, `instance?`, `components[]` | `createObject` plus `prefab: { asset, instance, overrides: [] }`; scene documents only |
| `setPrefabOverride` | `entity`, `override` (a prefab-model op object) | replaces the override with the same target (field for `setField`/`unsetField`, component for `addComponent`/`removeComponent`, object for `addObject`/`removeObject`), else appends |
| `revertPrefabOverride` | `entity`, `index?` | removes one override, or all of them |
| `createMaterial` | `id?`, `name`, `shader?`, `path` | writes a new `.material.json` (default shader `fynkel.pbr@1`); no undo |
| `saveDocument` | `path?` | writes the document; no undo |

`ImportAsset` and `ValidateProject` from §11.1 are `fynkel import` and
`fynkel validate` — they do not mutate a document, so they are not commands
here.

A **patch file** for `fynkel scene apply` / `fynkel prefab apply` is either
an array of commands or `{ "transactionId"?, "baseRevision"?, "commands":
[...] }`, each command optionally carrying a `commandId`. `apply` reads the
document, runs the one transaction and writes the file back; `--dry-run`
prints the unified diff of the canonical text instead (with `--json`, as
`diff`, next to `dryRun` and `written`). Exit 0 when the transaction committed
(and, unless `--dry-run`, the file was written), 1 when it was refused
(conflict, invalid command, rejected by validation), 2 when the document,
the patch or the arguments cannot be read.

## Material and shader documents

`.shader.json` is a shader's **interface manifest** (`schemas/shader.schema.json`,
hand-written): `shaderId` (`<name>@<version>`, the version bumps on any
interface change), `source` (`{ "builtin": name }` or `{ "wgsl": assetRef }`),
`passes` (`opaque | mask | blend | shadow`), `features`, typed `parameters`
(`float int bool vec2 vec3 vec4 color3 color4`, each with a `default` and
optional per-component `min`/`max`) and `textures` slots (`colorSpace`
`srgb | linear`, `required`, fallback texel `white | black | normal | gray`).
The built-in PBR shader `fynkel.pbr@1` ships its manifest inside the engine
(`crates/fynkel_document/shaders/fynkel.pbr@1.shader.json`, glTF metallic-
roughness: `baseColor`, `metallic`, `roughness`, `emissive`,
`emissiveStrength`, `normalScale`, `occlusionStrength`; slots `baseColor` and
`emissive` sRGB, `metallicRoughness`, `normal`, `occlusion` linear). Bindings
and uniform layout are the renderer's business and arrive with it (M3).

`.material.json` (`schemas/material.schema.json`, **formatVersion 2**,
generated by `fynkel codegen` from `fynkel_core::material::PbrMaterial`)
names a `shader` and sets values. For `fynkel.pbr@1` the values are the
**PBR model's fields, flat on the document** — the same record the
renderer consumes, so the schema, the Dart/Rust codecs and the wire form
come from one `#[derive(Schema)]` (`document = "material"`; every field is
optional in the document and takes the Rust `Default`, which the schema
carries as `default`):

| Field | Type | Default |
| --- | --- | --- |
| `base_color` | `[r, g, b, a]` 0..1 | `[1, 1, 1, 1]` |
| `metallic`, `roughness` | 0..1 | `0`, `0.5` |
| `emissive` | `[r, g, b]` 0..1 | black |
| `emissive_strength`, `normal_scale` | ≥ 0 | `1`, `1` |
| `occlusion_strength` | 0..1 | `1` |
| `alpha_mode` | `opaque | mask | blend` | `opaque` |
| `alpha_cutoff` | 0..1 (for `mask`) | `0.5` |
| `double_sided` | bool | `false` |
| `base_color_texture`, `normal_texture`, `metallic_roughness_texture`, `occlusion_texture`, `emissive_texture` | `assetRef` or `null` | unbound |

A material for a custom shader (`shaderId` from a project manifest) keeps
the manifest-driven form instead: `parameters` (name → value, anything
unset takes the manifest default) and `textures` (slot → `{ texture:
assetRef, colorSpace?, uvSet?, sampler?{filter, mipmaps, wrap, anisotropy}
}`), plus the shared `alpha_mode`, `alpha_cutoff` and `double_sided`. The
two groups are exclusive: PBR fields on a custom material, or
`parameters`/`textures` on a PBR one, are `FYN-D0307` (the schema itself
lists both so one bad field is one diagnostic, not a cascade). Shadow
casting/receiving is the `fynkel.MeshRenderer`'s (`cast_shadows`,
`receive_shadows`), not the material's — a `blend` material never casts,
and the sun is the only light that shadows (`DirectionalLight.cast_shadows`).
`alpha_mode` decides the pass: `opaque` and `mask` (texels under
`alpha_cutoff` discarded) draw in the opaque pass, `blend` in the
transparent pass sorted back to front per mesh and without depth writes
(`docs/renderer.md`). `fynkel document new material`
yields an untextured `fynkel.pbr@1` material carrying only `shader`.

Texture colour spaces are fixed per slot: `base_color_texture` and
`emissive_texture` sample sRGB, the other three linear. The renderer takes
the record straight from the cooked `MATL` chunk (below), never from JSON.

`fynkel_document::material` holds the typed views (`ShaderManifest`,
`Material`), `ShaderLibrary::builtin()` (add project manifests with `add`),
and the rules JSON Schema cannot express. `Material::pbr` is the decoded
`PbrMaterial` for `fynkel.pbr@1` (`PbrMaterial::violations()` reports the
ranges: `D0303` at the field's pointer). Material vs manifest (custom
shaders): unknown shader `FYN-D0300`, unknown parameter `D0301`, wrong type
(number / bool / vector length, whole numbers for `int`) `D0302`, out of
range `D0303`, unknown slot `D0304`, required slot unbound `D0305`,
`alpha_mode` naming a pass the shader lacks `D0306`, malformed entry or a
field from the other form (`parameters` on a PBR material, `metallic` on a
custom one) `D0307` (the suggestion names `fynkel migrate` when the shape
is the formatVersion 1 one), colour-space override that disagrees with the
slot `D0308` (warning). Manifest rules: default not of its type or outside
its range `D0310`, `min > max` `D0311`, malformed entry `D0312`.
`Material::resolve(&library)` fills every default into a `ResolvedMaterial`
(what `inspect` prints: a PBR material's fields are presented as the
manifest's parameters and slots, `authored` marking the ones the document
set; `pbr` and `doubleSided` alongside).

## Migrations

Each kind has a current `formatVersion` (`DocumentKind::supported_format_version`:
material, scene and prefab 2, every other kind 1). `fynkel_document::migrate` brings older documents
up: a `Migration` is `(kind, from, name, apply: fn(&mut Node))` and always
steps `from → from + 1`; `Migrations::builtin()` is the ordered registry
(`Migrations::path(kind, from, to)` is the chain). `migrate(&doc, &registry,
&schemas) -> Migrated` applies the chain, sets `formatVersion` after each
step, validates the result against the current schema and returns the
document plus `applied[]` (`scene/0-1/entities-keyed-by-id`, ...) and a
unified diff of the canonical text (`similar`). Anything that goes wrong
leaves the document as it was: no step for a version `FYN-D0017`, a step
that cannot handle the document or a result the schema rejects `D0018`, a
newer document `D0013`. `Migrated::write_with_backup(path)` copies the
original to `<file>.bak` first and refuses when nothing changed or errors
were reported; the diff and the `.bak` are the two ways to see what a
migration did.

Built-in steps (`fynkel migrate --list`):

- **material 1 → 2** `material/1-2/pbr-fields` (M3-02, the first real
  step; `fixtures/migrate/v1_rusted_metal.material.json` →
  `.migrated.json`): a `fynkel.pbr@1` material's `parameters` map becomes
  the flat fields (`baseColor` → `base_color`, `emissiveStrength` →
  `emissive_strength`, …) and its `textures` map the `<slot>_texture`
  references (`baseColor` → `base_color_texture`, `metallicRoughness` →
  `metallic_roughness_texture`, …; the binding's sampler/`uvSet` are
  dropped — the PBR model fixes them); for every shader `alphaMode` →
  `alpha_mode`, `alphaCutoff` → `alpha_cutoff`, `cull: none` →
  `double_sided: true`, and `castShadows`/`receiveShadows` are removed
  (they live on `fynkel.MeshRenderer`). A parameter or slot the PBR model
  does not have makes the step fail (`D0018`) rather than drop data.
- **scene 0 → 1** `scene/0-1/entities-keyed-by-id` (synthetic;
  `fixtures/migrate/v0_training.scene.json` → `.migrated.json`, which now
  continues to v2): v0 kept `entities` and `components` as arrays of
  objects carrying their `id`; v1 keys them by that id in the same order.
- **scene 1 → 2** `scene/1-2/camera-exposure` and **prefab 1 → 2**
  `prefab/1-2/camera-exposure` (M3-04;
  `fixtures/migrate/v1_lookout.scene.json` → `.migrated.json`): every
  `fynkel.Camera` gets the fields the record gained — `exposure_mode:
  "auto"` and `exposure: 12.0` — when it has neither; other components
  are untouched. The step is the pattern for any component that gains a
  required field: the schema-derived component schemas require every
  field, so a document written before the field exists is outdated
  (`D0016`) until migrated.

New steps follow the same shape when a format changes: add the
`Migration`, bump the kind's version, keep the fixture pair. Until a
document is migrated every command refuses it with `FYN-D0016`.

## Asset sidecars and the asset database

A **source asset** (a `.glb`, `.png`, `.wav`, ... under `assets/`) has no
envelope of its own, so its identity lives in a sidecar next to it:
`<file>.meta.json` (`schemas/meta.schema.json`, hand-written; ADR-0006).
Besides the envelope (`name` defaults to the file name) it holds
`importer` (namespaced id chosen from the extension: `fynkel.gltf`,
`fynkel.image`, `fynkel.audio`, `fynkel.file` for anything else),
`importerVersion`, importer-defined `settings`, and `subAssets` — sub-asset
id (`mesh/Body`, `material/0`; the `sub` of an `assetRef`) → that
sub-asset's own stable uuid, written on first import and kept across
re-imports. Documents (`.scene.json`, `.prefab.json`, `.material.json`,
`.shader.json`) are their own identity and have no sidecar.
`fynkel asset init <file>` writes a fresh sidecar (new v4 id); moving an
asset means moving the file **and** its sidecar — the id, and every
`assetRef` to it, survive because references never hold a path. A copy
needs a new sidecar: two files sharing an id is an error.

`fynkel_document::AssetDatabase::scan(dir, schemas)` is the asset database:
a walk of the tree (hidden files skipped, path order), nothing persisted,
so it can never disagree with the committed files. Each entry is
`{ id, path, name, kind: document(kind) | source{importer, version},
subAssets }`; `get(id)`, `by_path`, and `resolve(&AssetRef)` (the asset by
id, then `sub` against its declared sub-assets; a sub-asset's own uuid
resolves to the same place). `check_references(&Document)` finds every
`assetRef`-shaped object in a document (nil ids mean "unset" and are
skipped) and reports the ones that do not resolve, anchored at the
reference — the project-wide check in `fynkel validate` (M2-13) builds on
it. Codes: source file without a sidecar `FYN-A0001`, sidecar without a
source `A0002`, duplicate id (across documents, sidecars and sub-assets;
the first path wins) `A0003`, unknown asset `A0004`, unknown sub-asset
`A0005`, malformed sidecar entry `A0006`.

## Import

`fynkel_import` (blueprint §6 "Asset identity and import", ADR-0006) turns a
source file, its sidecar's `settings` and the importer's version into a
**normalized intermediate**: the one shape the cache (M2-12), the cooked
container and the renderer's loaders read, whatever the source format was.
It is development-only, never runs in a shipped game, and serializes to
bytes deterministically — the same source, settings and sidecar give the
same bytes on every machine, so it can be hashed, cached and diffed.

### glTF/GLB (`fynkel.gltf`, version 3)

The importer reads glTF 2.0 `.gltf`/`.glb` through
the `gltf` crate (no `import` feature: buffers and images are resolved by
Fynkel, never by the crate) and produces, per file:

| Sub-asset id | What | Notes |
| --- | --- | --- |
| `mesh/<n>` (one primitive) or `mesh/<n>/<p>` | one mesh per **triangle** primitive | fixed 72-byte vertex: position, normal, tangent(4), uv0, uv1, colour(4); `u32` indices; bounds. Missing normals → unwelded with flat face normals (per spec); missing tangents → zero in the intermediate (the cook generates them, below); missing colour → white. Points/lines/strips/fans are skipped with `FYN-A0105`. |
| `material/<m>` | a `fynkel.pbr@1` material document (the current formatVersion) | `base_color`, `metallic`, `roughness`, `emissive`, `emissive_strength` (`KHR_materials_emissive_strength`), `normal_scale`, `occlusion_strength`, `alpha_mode`/`alpha_cutoff`, `double_sided`; the five `<slot>_texture` references bound to `{ asset: <this asset>, sub: "image/<i>" }` (or `image/<i>/linear`, below). A slot that samples `TEXCOORD_1` or a non-default sampler (nearest filtering, clamp/mirror wrap) is imported without that setting and warned about once (`FYN-A0105`): the PBR model has one UV set and one sampler. Validates like a hand-written one. |
| `image/<i>` | the encoded bytes (PNG/JPEG) with their MIME type and **colour space** | sRGB when a colour slot (`base_color`, `emissive`) uses it, linear when only data slots (`normal`, `metallic_roughness`, `occlusion`) do, sRGB when unused |
| `image/<i>/linear` | a linear copy of a **dual-use** image | when one image feeds both a colour and a data slot it is imported twice; the data slots point at the linear copy, so every texture reaches the cache in the colour space its slot needs |
| `prefab` | the default scene's node hierarchy as a prefab document (the current formatVersion; importer version 3 = prefab formatVersion 2) | one entity per node with a `fynkel.Transform` (decomposed TRS, matrices included); a node with a single-primitive mesh carries a `fynkel.MeshRenderer` (`mesh` and `material` are `assetRef`s into this asset; a primitive without a material gets the nil ref = engine default); a multi-primitive mesh gets one child entity per primitive. Entity and component ids are UUID v5 of the asset id and `node/<i>`, `node/<i>/transform`, `node/<i>/renderer`, `node/<i>/primitive/<p>[/transform|/renderer]`. |

Skins, morph targets, animations and cameras are skipped with `A0105`
warnings (static meshes are M2's scope; animation import is a later
milestone). Every sub-asset's uuid comes from the sidecar's `subAssets`
when present, else UUID v5 of the asset id and the sub-asset id — so a
fresh checkout and a re-import derive the same ids, and ids assigned once
survive re-imports; the sidecar is rewritten with any new ones (`fynkel
import`), and a sidecar id the file no longer produces is reported (`A0105`,
references to it will stop resolving).

**Boundaries** (ADR-0006, §6): buffers and images from the GLB `BIN` chunk
or `data:` URIs only, unless the sidecar's settings say
`"allowExternalFiles": true` — and then only plain relative paths (no
`..`, no absolute paths, no schemes) inside the source's directory
(`FYN-A0102` otherwise). A file that *requires* an extension the importer
does not implement (Draco, meshopt, ...) is refused with `A0103`;
malformed data (buffer views past their buffer, attribute counts that do
not match, index out of range, a node graph with a cycle or a node with two
parents) is `A0104`; the `Limits` — file 256 MiB, buffers 256 MiB total,
100 000 nodes, depth 64, 16 M vertices per primitive, 256 images of 64 MiB —
are `A0101`; a file that is not glTF at all is `A0100`. Any error means no
intermediate is produced.

### Textures (`fynkel.image`, version 1)

`.png` / `.jpg` / `.jpeg` → one `TextureData`: RGBA8, row-major, top row
first, `width × height × 4` bytes, no resizing; the **cook** appends the
full mip chain (M3-08b: `TEXR` holds every level down to 1×1, a 2×2 box
filter applied in linear light for sRGB textures — alpha linear —, an odd
trailing row or column dropped; `mipLevels` in the chunk info). Grey, grey+alpha, RGB, RGBA, palette and
16-bit sources all normalize to 8-bit RGBA (16-bit is narrowed;
`sourceHasAlpha` records whether the file had an alpha channel). The
colour space is **declared, not detected**: the sidecar's
`settings.colorSpace` (`"srgb"`, the default, for colour; `"linear"` for
normal, roughness, metallic, occlusion and other data maps) — the same
`colorSpace` a material's texture binding can state, so the two can be
checked against each other later. A value that is neither is `FYN-A0104`
anchored at `/settings/colorSpace` of the sidecar. Only PNG and JPEG are
compiled in (`image` with `default-features = false`); anything else is
`A0103`. Limits: `max_image_bytes` (64 MiB of file), `max_texture_dimension`
(16 384 per side, enforced before decoding) → `A0101`. A standalone
texture has no sub-assets: an `assetRef` to it is `{ asset }` alone.

### Audio (`fynkel.audio`, version 1)

`.wav` (PCM 8/16/24/32-bit and float) / `.ogg` (Vorbis) → one `AudioData`:
interleaved `f32` samples in `[-1, 1]` (lossy overshoot clamped), the
sample rate and channel count as the file had them, and the source codec
(`pcm`, `vorbis`). No resampling, no channel mixing, no normalization,
no tags — the runtime's mixer (a later milestone) decides. Decoding is
`symphonia` with only `wav`/`pcm`/`ogg`/`vorbis` compiled in and no SIMD
features, so a Vorbis decode is bit-identical across machines; anything
else (FLAC, MP3, AAC, ...) is `A0103`. A stream that ends early or has a
corrupt packet is `A0104` — a game asset with a hole in it is fixed at
the source, not skipped over. Limit: `max_audio_frames` (10 minutes at
48 kHz) → `A0101`. No settings yet; no sub-assets.

### The intermediate's bytes

`.fyni`: `FYNI`, version, a JSON header (source, importer, meshes /
materials / images / textures / clips with `[offset, length]` into the
blob area, the prefab document when there is one, the sub-asset ids) and
4-byte-aligned blobs (vertices, `u32` indices, encoded image bytes, RGBA8
texels, `f32` samples). `Intermediate::to_bytes`/`from_bytes` round-trip;
the committed `fixtures/robot.expected.fyni`,
`fixtures/textures/swatch.expected.fyni` and
`fixtures/audio/tone.expected.fyni` pin the format, the normalization and
every id derivation (`FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_import`
regenerates them when a change is intended).

## Cache and cooked assets

`fynkel cook` (`fynkel_import::cook_project`) turns the whole asset
database of a project into one **cooked container per asset** in a
**content-addressed cache** (blueprint §6, §19.2c item 12) and records the
project (name, startup scene) in the index; `--assets <dir>` cooks a bare
asset root. Nothing in the cache is needed to rebuild it — the committed
sources, sidecars and documents are the truth (ADR-0006) — so
`.fynkel/cache/` is ignored by version control and `rm -rf` is a valid
repair. The cache is, on the other hand, **all the runtime reads**
(M3-03): `fynkel run`, `fynkel test --headless`, `fynkel state dump` and
`fynkel render` cook first (`--no-cook` skips it), hosts never cook, and a
missing or stale object is a `FYN-R` load error naming the asset, never a
fallback to source.

**Keys.** `fynkel_import::cache::KeyInputs::key()` is BLAKE3 over
length-prefixed fields: the cache format version, the intermediate and
container versions, the asset id, the kind (`fynkel.gltf`, `fynkel.image`,
…, or `document/<kind>`), its version (importer version or
`formatVersion`), the BLAKE3 of the source bytes, extra input files (a
`.gltf`'s non-`data:` `buffers[].uri` / `images[].uri`, hashed, sorted; a
missing file hashes as "missing"), the sidecar's `settings` as canonical
JSON, every dependency as `(id, key)` sorted by id, and the target profile.
Same inputs → same key → the object is already there ("cached"); anything
else is a new key ("cooked"). Objects are never edited in place; an old
key's object stays until pruned.

**Dependencies** are what a document references: every `assetRef` in it
(nil ids skipped), resolved against the database to the *asset* that owns
the target (a sub-asset reference depends on the whole source). Sources
depend on nothing but their own files. Because a dependency's key is part
of the dependent's key, editing a texture recooks the material that uses
it and the scene that uses that material — and nothing else
(`tests/cook.rs` pins that down). Assets are cooked in dependency order
(Tarjan's SCCs); a cycle fails its members (`FYN-A0104`), an unresolved
reference fails the document (`FYN-A0003`), and a dependent of a failed
asset is skipped (`FYN-A0105`, "not cooked because … failed"). The rest of
the database still cooks.

**Layout.** `<cache>/objects/<key>.fyna` next to `<key>.manifest.json`
(both written to a temporary name and renamed, the object first), and
`<cache>/index.json` after every cook: `{ fynkel: "cache-index",
cacheVersion, containerVersion, schemaFingerprint, profile, project: {
name, startupScene, quality: { default, profiles: [{ name, shadows,
shadowResolution, msaa, renderScale, anisotropy, textureMaxSize }] } |
null } | null, assets: [{ id, path, name, kind, status, key, file,
dependencies }] }` — how the runtime finds "the current cooked file
for this asset" without recomputing keys, and whether this build may read
the cache at all (`schemaFingerprint` is the build's `fynkel schema`
fingerprint; it is also part of every document's key through the key's
`settings`, so a schema change recooks every document). The manifest records the key's inputs
(`sourceHash`, `extraInputs`, `settings`, `dependencies: [{ id, key }]`)
and the chunk table (`kind`, `name`, `length`, `crc32`), no timestamps.
`Cache::prune(keep)` deletes every object not in `keep`; `fynkel cook
--prune` keeps the current profile's keys only.

**What is cooked.** A document: validated (schema + references + scene
semantics; errors fail it) and stored as its canonical text in one `JSON`
chunk — a scene **expanded** (prefabs applied, ids derived; see "Cooked
scenes and the runtime store" above), a prefab as its template. A source
with an importer: run through `fynkel_import::import`, then meshes →
`MESH` (a mesh whose vertices all carry a zero tangent gets one generated
per vertex from positions and `uv0` — Lengyel accumulation, Gram-Schmidt
against the normal, handedness in `w`, `fynkel_import::tangents`; the
chunk info says `"tangents": "authored" | "generated" | "none"`, the last
when there are no UVs either and the renderer falls back to the vertex
normal; cache version 3), materials → `JSON` (`documentKind: material`) **plus** a `MATL`
chunk of the same name/sub (the decoded `PbrMaterial`, what the renderer
reads; a material document cooks the same pair), embedded glTF images →
decoded `TEXR` in the colour space the importer assigned them (dual-use
images arrive as two sub-assets, one per colour space), the asset's own
texture → `TEXR`, clips → `CLIP`, the prefab → `JSON`. A `fynkel.file` source: its bytes in one
`BLOB` chunk. A sidecar whose `subAssets` fall behind the import cooks with
a warning (the container carries the derived ids) — `fynkel import`
updates the sidecar; the cooker never writes outside the cache.

### The cooked container (`fynkel_cooked`, `.fyna`, version 1)

A dependency-free crate (only `crc32fast`) so a shipped runtime can read
containers without the importers. Little-endian throughout.

- **Header** (48 bytes): `FYNC`, `u32` version (1), `u32` header length,
  `u32` chunk count, `u64` table offset, `u64` file length, `u32` flags,
  `u32` CRC-32 of the first 36 header bytes, `u64` reserved.
- **Payloads**, each 8-byte aligned, then the **table**: per chunk
  `kind[4]`, `u32` flags, `u64` offset, `u64` length, `u32` CRC-32 of the
  payload, `u32` reserved. At most 65 536 chunks; chunk 0 is always `META`.
- **Reading is bounded**: `Container::parse` (bytes in memory) and
  `ChunkFile::open` (seekable stream; only the header, the table and the
  requested chunks are read) validate the header, the table and every
  offset/length against the file length before any payload is touched;
  `read(index, max_len)` refuses a chunk larger than the caller's budget
  (`ChunkTooLarge`) and verifies its CRC (`Checksum`); `verify()` checks
  every payload.

Chunk kinds and payloads:

| Kind | Payload |
| --- | --- |
| `META` | JSON: `{ fynkel: "cooked", version, id, name, path, kind, importer, importerVersion, profile, key, dependencies: [uuid], subAssets: { sub: uuid }, chunks: [{ index, kind, name, sub?, id?, … }] }` — per-chunk extras: `documentKind`; `vertices`, `indices`, `material`; `width`, `height`, `colorSpace`, `sourceHasAlpha`, `mipLevels`; `sampleRate`, `channels`, `frames`, `codec`; `entities`; `extension` |
| `JSON` | A document's canonical text (scene, prefab, material, …) |
| `MATL` | `u32` payload version (1), `u32` record size (`PbrMaterial::SIZE`, 184), `u32` string-tail length, `u32` reserved, then the `fynkel.PbrMaterial` wire record (ADR-0012 layout: `Vec4` 16 bytes, optional asset refs as a nil id when unbound) and its string tail (the `sub` ids). `fynkel_wire::material::{encode_material, decode_material}`. META extras: `shader`, `alphaMode`, `textures` (the bound slot names). |
| `MESH` | `u32` vertex count, `u32` index count, `[f32; 3]` bounds min, `[f32; 3]` bounds max, vertices (72 bytes each, the intermediate's layout), `u32` indices |
| `TEXR` | `u32` width, `u32` height, `u32` format (1 = RGBA8 sRGB, 2 = RGBA8 linear), `u32` mip levels (the full chain, `full_mip_count(w, h)`), then every level's `w × h × 4` texels concatenated, level 0 first, top row first (`TexturePayload::level(i)` / `base()`) |
| `CLIP` | `u32` sample rate, `u32` channels, `u64` frames, interleaved `f32` samples |
| `BLOB` | The source file's bytes |

`fynkel_cooked::payload::{decode_mesh, decode_texture, decode_clip}` (and
the `encode_*` counterparts, `Vertex`, `MeshPayload`, `TexturePayload`,
`ClipPayload`) read the typed payloads back (length-checked, indices
bounds-checked; `None` on a malformed payload); `fynkel_import::cook`
re-exports them.

## Project and lock

`fynkel.project.json` (`fynkel_document::project`, `project.schema.json`,
hand-written like the material and shader schemas) is the one file that
makes a directory a project. It is a document (envelope, strict JSON,
`formatVersion` 1, `DocumentKind::Project`) and holds:

| Member | Meaning |
| --- | --- |
| `startupScene` | `{ asset }` — required; must resolve to a scene document in the asset database |
| `assets` | The asset root, relative to the project directory (default `assets`; no `..`, no absolute path) |
| `inputActions` | The game's actions (§5.1, [input.md](input.md)): `name → { type: button \| axis1d \| axis2d, description?, bindings: ["<device>:<control>", …] }` — `keyboard:WASD`, `keyboard:Space`, `mouse:delta`, `gamepad:leftStick`, `touch:overlay/jump`, …; every binding is checked against the runtime's grammar (`FYN-P0009`). The cook copies the block into the cache index; the runtime compiles it into the world's binding table when the project loads (M3-07) |
| `physics` | `{ gravity: [x, y, z], layers: [name, …] }` ([physics.md](physics.md), M3-08): the world's gravity (default `[0, -9.81, 0]`) and up to 32 unique collision layer names (bit `i` of a collider's `layer`/`mask`; default `["default"]`) — `FYN-P0013` on more than 32, an empty or duplicate name, or a non-finite gravity. The cook copies the block into the cache index; the world applies it when the project loads |
| `quality` | `{ default, profiles: { name → { shadows, shadowResolution, msaa, renderScale, anisotropy, textureMaxSize } } }`; `default` must name a profile; unset fields take the schema defaults (shadows on, 2048, MSAA 4, scale 1, anisotropy 8, 4096). The cook copies the block into the cache index and the renderer reads it there (`docs/renderer.md` — MSAA runs at 1 or 4, the other values are clamped); `textureMaxSize` is for the cook |
| `components` | The component registry: `typeId → { kind: native \| behaviour, version?, description? }`. Absent means every built-in is allowed. A `native` entry this build does not have is an error; a scene or prefab using a type the registry leaves out is a warning (once per document and type) |
| `targets` | `macos \| ios \| android \| windows \| linux \| web → { enabled (default true), profile (default dev) }`; all disabled is a warning |
| `x-*` | Tool extensions, kept verbatim |

`fynkel.lock.json` (`lock.schema.json`, not a document: no envelope, no id)
records what this build is: `runtime { version, abiVersion }`, `tools {
version }`, `schemaFingerprint`, `documents { kind → formatVersion }`,
`importers { id → version }` and `cache { version, container }`. `Lock::
expected(importers, cacheVersion, containerVersion)` is the lock this build
writes (`fynkel_import::expected_lock()` fills the importer and cache
versions in, since `fynkel_document` cannot see them); `Lock::check` reports
every differing field as `FYN-P0011` at its pointer. A mismatch is always an
error — nothing migrates silently; `fynkel project lock` rewrites the lock
once the upgrade is meant. The lock is canonical JSON (sorted maps), so it
diffs cleanly in version control.

`validate_project(dir, schemas, expectedLock)` is the whole-project check
behind `fynkel validate`: the project file (schema, then semantics), the lock
against the build, the asset root, the asset database scan (every sidecar),
the startup scene and the native registry entries, then every document under
the asset root — references checked against the database, scenes validated
and expanded against every prefab in the project, prefabs, materials against
the built-in shaders plus every manifest in the project, shader manifests —
and the registry check. The report counts documents, sources and references;
a missing project file is `FYN-P0001` and the one case the CLI exits 2 on.

Codes: `FYN-P0001` no project file, `P0002` startup scene unset/unresolved/
not a scene, `P0003` unknown native component in the registry, `P0004`
(warning) unregistered component used, `P0005` quality default not a
profile, `P0006` (warning) no enabled target, `P0007` asset root escapes the
project or is missing, `P0008` malformed entry, `P0009` an input binding
the runtime grammar rejects (at `/inputActions/<name>/bindings/<i>`,
[input.md](input.md)), `P0010` no lock, `P0011` lock mismatch, `P0012`
malformed lock, `P0013` a `physics` block the runtime cannot apply (layers,
gravity).

## New projects: `fynkel create`

`fynkel create <dir> [--name <package>] [--title <text>] [--org
<reverse.domain>] [--no-flutter] [--force] [--json]` (`apps/cli/src/create.rs`,
blueprint §13.2/§13.3, ADR-0017, §19.2c item 17) writes a complete game
project — the blueprint's project model, filled in:

```text
my_game/
  pubspec.yaml                 Flutter app on package:fynkel + package:fynkel_flutter (path dependencies into the checkout) + fynkel_generator (dev)
  fynkel.project.json          startup scene, assets root, input actions (move, jump, pause), quality profiles, registry (game.Spinner, game.Mover declared)
  fynkel.lock.json             `expected_lock()` for the tools that created it
  AGENTS.md  README.md         layout, every command, the validate → codegen → build → test loop, do/don't
  lib/main.dart                the app: `FynkelGameView` over the project's cooked cache (loop + viewport) with a HUD overlay; FYNKEL_PROJECT on this machine, the bundled cache on a device — `FynkelInputCapture` feeds the world (keys, mouse, gamepad), `TouchOverlay.fromActions` on phones, the `pause` action pauses the loop
  fynkel_cache/README.md       keep file: the cook stages the cache here and pubspec.yaml bundles the directory as assets (the rest git-ignored)
  lib/game/game.dart           `Game.load(dir)`: World, `registerBehaviours(world.registry)`, `loadProject`; `projectDirectory()`
  lib/game/spinner.dart        `@FynkelBehaviour(id: 'game.Spinner')` with `@Expose` speed/axis
  lib/game/mover.dart          `game.Mover`: walks the cube with `Input.action2D('move')`, hops on `Input.pressed('jump')` (input.md)
  lib/generated/               behaviours.g.dart + .g.json, exactly what `fynkel codegen --project .` emits
  schemas/behaviours/          game.Spinner.schema.json, game.Mover.schema.json
  assets/scenes/main.scene.json    Camera, Sun (fynkel.DirectionalLight, 10 000 lux) and a Cube with MeshRenderer + game.Spinner { speed: 1.5 } + game.Mover
  assets/materials/starter.material.json
  assets/models/cube.glb (+ .meta.json)   a unit cube (24 vertices, normals, UVs); sidecar with the imported sub-asset ids
  assets/textures/swatch.png (+ .meta.json)
  assets/prefabs/  assets/audio/           empty (.gitkeep)
  test/game_test.dart          `flutter test`: the scene loads, `behavioursAttached == 2`, the spinner's authored speed, it spins; `test/walk.input.json` replayed through the Mover walks the cube (one jump per press across a 50 ms frame)
  test/walk.input.json         a recorded input stream (input.md § Recordings); also `fynkel test --headless --input test/walk.input.json`
  macos/  ios/                 platform runners from `flutter create --empty` (skipped with --no-flutter or without `flutter` on PATH)
```

Every file is a template under `apps/cli/templates/create/` embedded into
the binary; `{{placeholders}}` are the package name (the directory's name,
`lower_snake_case`, or `--name`), the title (`My Game`, or `--title`), the
SDK package paths (relative when the project lives inside the checkout,
absolute otherwise; `--root <repo>` names the checkout when the CLI cannot
find it from the working directory or its own location) and fresh v4 ids for
the project, documents, entities and components. The two sources are run
through `fynkel_import::import` so their sidecars carry sub-asset ids as
`fynkel import` would leave them. After `flutter create`, the macOS
entitlements get `com.apple.security.app-sandbox = false` (a dev build
reads the project directory; the Fynkel Flutter host runs the same way).
The output passes `fynkel validate`, `fynkel test --headless`, `fynkel
codegen --project . --check`, `flutter analyze` and `flutter test` (CI runs
all five on a fresh project; the test cooks the project first through
`package:fynkel/testing.dart`, `FYNKEL_CLI` naming the CLI) and renders
with `fynkel render`.

`fynkel run [--project <dir>] [--no-cook] [-- <flutter run args>]`
validates the project, cooks it into `.fynkel/cache` (a warm cache costs
milliseconds, `--no-cook` skips it) — the app loads its scene, meshes and
textures from that cache and nothing else — then runs `flutter run
--dart-define=FYNKEL_PROJECT=<abs dir>` in it, which is how `lib/main.dart`
finds the project from a bundle on the same machine.

**The bundled cache.** A device cannot see that directory (an iPhone is
not the Mac that holds the project; the iOS simulator is), so a cook of a
project that is an app (`pubspec.yaml` beside `fynkel.project.json`) also
*stages* the cache: `fynkel cook`, and the cook every runtime tool does
first, mirror `.fynkel/cache/index.json` and exactly the `.fyna` objects
the index names into `<project>/fynkel_cache/` — flat, since a Flutter
asset directory is not recursive (`staged N object(s) into ./fynkel_cache
… (C copied, R removed)`; stale objects are removed, present ones kept). The
created project's `pubspec.yaml` lists `fynkel_cache/` under `assets:`,
its `.gitignore` ignores everything in it but the keep file
`fynkel_cache/README.md` (so a clean checkout still has the asset
directory and `flutter build` does not fail), and `lib/main.dart` loads
`FYNKEL_PROJECT` when that directory exists on the device and otherwise
unpacks the bundle through `package:fynkel_flutter`'s
`unpackBundledCache()` into the app's temp directory and loads that
(`Game.load(dir, cacheDir: …)`). `fynkel run` refuses with exit 1 when the
project is not a Flutter app (no `pubspec.yaml` — the bare stress project;
the message names `fynkel create` / `generate.py --app`) or when
`fynkel_cache/index.json` is missing (a `--no-cook` before any cook), and
a device build after editing the project needs a cook (any `fynkel` runtime
command) before `flutter build`/`flutter run` for the app to carry the
change; a project that is never cooked bundles nothing and the app says so
(`… and the app bundles no cache: run \`fynkel cook\`, then build again`).

Outside the repository the CLI validates against the schema set embedded in
the build (identical to `schemas/` by `fynkel codegen --check`); `--schemas
<dir>` overrides either. Scene and prefab commands run on a document inside
a project (`fynkel.project.json` in an ancestor directory) with that
project's behaviour schemas and registry, so `game.*` components validate
instead of warning `FYN-D0205`. The runtime tools — `fynkel test
--headless`, `fynkel state dump`, `fynkel render`, `fynkel run` — read a
cooked cache only, so they cook the project first (`cook::prepare`):
`--project <dir>` (default: the current directory when it holds a project
file), `--scene <path|id>` (a `.scene.json` inside a project resolves that
project and the scene's id; a loose scene outside any project is refused
with exit 2, since nothing could cook it), `--cache <dir>` (default
`<project>/.fynkel/cache`; alone, with no project around, it is read as it
is) and `--no-cook`.

## CLI

```sh
fynkel document validate <path>... [--json] [--schemas <dir>] [--kind <kind>]
fynkel document format <path>... [--check] [--json]
fynkel document new <kind> <name> [--out <path>] [--json]
fynkel scene inspect <path> [--json] [--schemas <dir>] [--prefabs <dir>] [--expand]
fynkel scene validate <path>... [--json] [--schemas <dir>] [--prefabs <dir>]
fynkel scene apply <path> --patch <ops.json> [--dry-run] [--json] [--schemas <dir>]
fynkel prefab inspect <path> [--json] [--schemas <dir>]
fynkel prefab validate <path>... [--json] [--schemas <dir>]
fynkel prefab apply <path> --patch <ops.json> [--dry-run] [--json] [--schemas <dir>]
fynkel material inspect <path> [--json] [--schemas <dir>] [--shader <manifest>]...
fynkel material validate <path>... [--json] [--schemas <dir>] [--shader <manifest>]...
fynkel shader inspect <path> | --builtin <shaderId> [--json]
fynkel shader validate <path>... [--json] [--schemas <dir>]
fynkel asset list [--assets <dir>] [--json] [--schemas <dir>]
fynkel asset resolve <uuid>[#<sub>]... [--assets <dir>] [--json] [--schemas <dir>]
fynkel asset init <file>... [--json]
fynkel migrate <path|dir>... [--dry-run] [--json] [--schemas <dir>]
fynkel migrate --list [--json]
fynkel import <file>... [--out <dir>] [--dry-run] [--json] [--schemas <dir>]
fynkel cook [--project <dir>] [--cache <dir>] [--profile <name>] [--force] [--prune] [--json]
fynkel cook --assets <dir> [--cache <dir>] [--schemas <dir>] ...
fynkel cooked inspect <file.fyna>... [--json]
fynkel project describe [--project <dir>] [--json] [--schemas <dir>]
fynkel project lock [--project <dir>] [--check] [--json] [--schemas <dir>]
fynkel validate [--project <dir>] [--json] [--schemas <dir>]
fynkel codegen [--check] [--root <repo>] [--project <dart package>]
fynkel render [--project <dir>] [--scene <path|id>] [--out <file.png>] [--size <W>x<H>] [--cache <dir>] [--no-cook] [--placeholders] [--quality <profile>] [--json]
fynkel schema list [--json]
fynkel schema describe <type|kind> [--json]
fynkel test --headless [--project <dir>] [--scene <path|id>] [--cache <dir>] [--no-cook] [--input <file.input.json>] [--steps N] [--seed S] [--json]
fynkel state dump [--project <dir>] [--scene <path|id>] [--cache <dir>] [--no-cook] [--steps N] [--json]
fynkel create <dir> [--name <package>] [--title <text>] [--org <domain>] [--no-flutter] [--force] [--json]
fynkel run [--project <dir>] [--no-cook] [-- <flutter run args>]
```

`scene inspect` prints the entity tree (or `{ kind, id, name, formatVersion,
revision, entityCount, instanceCount, componentCount, componentTypes,
roots[], expanded }` with `--json`) followed by the diagnostics; anchors show their
prefab, instance id and override count, `--expand` shows the expanded scene
instead. Scenes with instances resolve prefabs from `--prefabs <dir>`
(default: the scene's own directory) and report override problems in
`validate`. `prefab inspect` prints roots and the tree. `scene apply` and
`prefab apply` run one authoring transaction from a patch file (see
[Authoring commands and transactions](#authoring-commands-and-transactions)). `material inspect` prints the resolved material (every
manifest parameter with its value, `(default)` where the material did not set
it; every slot with its binding or fallback); `--shader` adds custom manifests
to the built-in library (a manifest with errors is reported and left out).
`shader inspect --builtin fynkel.pbr@1` shows the built-in interface.
`asset list` scans `assets/` (or `--assets <dir>`) and prints every asset
with its id, kind and sub-assets (`--json`: `{ ok, root, assets[],
diagnostics }`); `asset resolve` prints the file (and sub-asset) behind each
id (`--json` adds `resolved[]`); `asset init` writes sidecars and prints
`id  importer  sidecar` per file (an existing sidecar is kept, with a
warning). `migrate` takes files or directories (every document under them,
sidecars included); `--dry-run` prints each document's diff and writes
nothing, otherwise changed files are rewritten with the original kept as
`<file>.bak` (`--json`: `{ ok, dryRun, documents: [{ path, kind, from, to,
applied[], diff, written, backup? }], diagnostics }`); `--list` shows the
current versions and known steps. `import` runs each file through its
sidecar's importer and writes `<out>/<asset id>.fyni` (`--out` defaults to
`.fynkel/cache/import`, ignored by version control); a file without a
sidecar gets one, a sidecar missing sub-asset ids is updated (existing ids
kept); `--dry-run` prints the summary and writes nothing (`--json`: `{ ok,
dryRun, files: [{ file, ok, id, importer, sidecar, sidecarCreated,
sidecarWritten, summary, intermediate, bytes }], diagnostics }`). `cook`
reads the project (`--project`, default the current directory), scans its
asset root with its behaviour schemas and cooks every asset into the cache
(`--cache`, default `<project>/.fynkel/cache`) for `--profile` (default
`dev`), recording the project's name and startup scene in the index;
`--assets <dir>` cooks a bare asset root instead (no project record, so
the runtime can load its scenes by id only);
each asset prints as `cooked|cached|failed  path (kind) key bytes`;
`--force` cooks even when the key is present, `--prune` deletes every
object the run did not produce (other profiles' objects included)
(`--json`: `{ ok, profile, cooked, cached, failed, cache, pruned, assets:
[{ id, path, kind, status, key, file, dependencies, chunks, bytes }],
diagnostics }`). `cooked inspect` prints a container's header, its chunk
table with checksums verified, and the META summary (`--json`: `{ ok,
files: [{ file, ok, version, bytes, verified, error, chunks[], meta }],
diagnostics }`); a damaged container is an error. `project describe` prints
the project file resolved against its asset database and lock (`--json`:
`{ ok, id, name, path, formatVersion, assets, assetCount, startupScene: {
asset, path, name }, inputActions[], quality, components[], targets[],
lock, diagnostics }`); `project lock` writes `fynkel.lock.json` for this
build (unchanged files are left alone; `--check` only compares and exits 1
on a mismatch; `--json`: `{ ok, lock, path, written, check, diagnostics
}`). `validate` runs the whole-project check (`--json`: `{ ok, project: {
id, name, path }, lock, documents, sources, references, errors, warnings,
diagnostics }`). `schema list` prints every document kind and every
component/command/event record with its wire size and the fingerprint
(`--json`: `{ ok, fingerprint, abiVersion, documents[], records: [{ kind,
id, name, rust, doc, wireSize, authored, schema, fields: [{ name, type,
offset, size, doc }] }] }`); `schema describe <fynkel.Transform>` prints one
record with field offsets, `schema describe <scene>` a document kind's
schema id and envelope. `--project` defaults to the current directory.
`render` cooks the project, loads its startup scene (or `--scene`) from
the cache through `fynkel_assets`, steps the world once and draws the
extract offscreen from the same store to a PNG, 1280×720 by default
(`--json`: `{ ok, out, size, project, scene: { id, name, key, entities,
rejectedComponents }, cache, instances, culled, draws, shadowDraws,
meshes, placeholders, missing, transparent, batches, sceneCamera,
sceneLight, lights, lightsDropped, sky, ev100, shadowCasters, shadowMap,
quality: { profile, shadows,
shadowResolution, msaa, renderScale, anisotropy, renderSize },
assetErrors, diagnostics }`; `docs/renderer.md` names the counts).
`--quality <profile>` renders with one of the project's quality profiles
instead of its default (exit 2 naming the profiles when it does not
exist); exit 1 on a project/scene/asset error, 2 when no GPU adapter is
available. `codegen` regenerates the native schemas and
codecs (pipeline (a)); with `--project <dart package>` it also runs the
behaviour generator in that package and splits its schemas into
`schemas/behaviours/` (pipeline (b), `docs/codegen.md`); `--check` compares
and exits 2 on drift without touching the tree.

Exit codes, everywhere: 0 ok (warnings allowed), 1 diagnostics with errors
(or drift for `format --check`), 2 usage, or a project that cannot be opened
at all (no `fynkel.project.json`, unreadable schemas). `--json` prints
`{ "ok": bool, "diagnostics": [ ...records... ] }` (`format` adds
`"drift": [paths]`).

## Fixtures

`crates/fynkel_document/fixtures/`: `valid/` (parse, validate, canonical
fixed point), `unknown/` (unknown keys and lexemes byte-for-byte, hoisting
expectations in `*.expected.json`), `invalid/` (exact code/pointer/line/
column expectations in `*.expected.json`), `scene/` and `material/` (semantic
expectations per kind; `material/` also holds custom manifests the material
fixtures reference), `project/` (a complete project: `fynkel.project.json`,
the lock this build writes — `FYNKEL_UPDATE_FIXTURES=1 cargo test -p
fynkel_document --test project` refreshes it — and an asset tree with a
scene, a prefab instance, a glTF mesh, a texture and a material; since
M3-08 the scene also has a static Floor collider and a dynamic Crate B that
lands on it, with `physics.layers` `["default", "props"]`; since M3-09 a
second scene, Playground (`assets/scenes/playground.scene.json`, id
`…0031`: Floor, a 0.8 m Platform, a Player with a
`fynkel.CharacterController` driven by the `move`/`jump` actions, a
Camera with a `fynkel.ThirdPersonCamera` targeting it by name and turned
by the `look` action, Sun) with `test/platform.input.json` (120 frames:
W held, Space at frame 20, W released at 75) — the headless gate of item
9 — so the main scene's counts are unchanged). Every kind added later
extends this corpus.

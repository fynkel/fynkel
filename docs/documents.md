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
SDK attaching authored behaviours at load); append only.

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

Component schemas carry `x-fynkel.authored`; `#[schema(authored = false)]`
marks runtime-only records. `fixtures/scene/` holds one document per rule
with its expected diagnostics.

Component `data` follows the field types of `docs/codegen.md`: vectors are
number arrays, enums are the lowerCamel variant name (`"projection":
"perspective"`), asset references are `{ "asset": "<uuid>", "sub": "mesh/0" }`
with `sub` omitted for the asset itself — never a path. Built-in authored
components today: `fynkel.Transform`, `fynkel.MeshRenderer`, `fynkel.Camera`,
`fynkel.DirectionalLight`, `fynkel.PointLight` (`fixtures/valid/training_room.scene.json`
uses all of them).

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
`fynkel test --headless --scene` and `fynkel state dump`.

The file-to-description paths every host shares live in
`fynkel_document::runtime`:

- `read_scene(path, prefabs)` — read, validate, expand prefab instances
  (from a directory scanned for `*.prefab.json`), describe →
  `DescribedScene { description, behaviours, diagnostics }`. `Err` carries
  every diagnostic when any is an error. Embedded schemas only: no project,
  so every behaviour type is an unknown one (warned, still handed over).
- `load_project(dir)` — `fynkel.project.json` → the project's schemas
  (`<dir>/schemas` + registry declarations) → asset database scan →
  `startupScene` resolved → the scene read with **every prefab in the
  database** available → `LoadedProject { project_name, dir, assets, cache
  (<dir>/.fynkel/cache), scene_path, scene, behaviours, warnings }`. The
  lock is not checked here (`fynkel project lock --check` is).

Both read source documents: the cooked scene chunk is the unexpanded
canonical text and no loader reads it yet (M3).

Over the C ABI (`fynkel_native::load`): `fynkel_world_load_scene(world,
path_utf8, len)` and `fynkel_world_load_project(world, dir_utf8, len)`
return `FYNKEL_STATUS_OK`, `FYNKEL_STATUS_LOAD_FAILED` (the world is
unchanged) or `FYNKEL_STATUS_INVALID_ARGUMENT`; `fynkel_world_load_report`
is the JSON of the last call: `{ ok, scene: { id, name, path, entities,
rejectedComponents, behaviours: [{ entity, authoringId, component, type,
version, known, data, pointer }] } | null, project: { name, dir, assets,
cache } | null, diagnostics: [...] }` — `entity` is the spawned handle. In
Dart, `World.loadScene(path)` / `World.loadProject(dir)` return a
`LoadReport` (warnings to `onLoadDiagnostics`) or throw `FynkelLoadError`;
on success the SDK instantiates every listed behaviour through
`world.registry` (`registerBehaviours(world.registry)` from the generated
`behaviours.g.dart` first) and attaches it to `attach(entity)`. A type the
registry lacks is `FYN-B0001`, data the type rejects is `FYN-B0002` — both
warnings in the report (`behavioursAttached` counts the rest); the entity
and its native components load either way.

### Cooked assets in the renderer

`fynkel_render::CookedAssets::open(cache_dir)` reads the cache's
`index.json` and serves `mesh(&AssetRef)`, `texture(&AssetRef)` and
`material(&AssetRef)` from the containers on demand (`ChunkFile`, bounded
reads, the chunk whose `kind` and `sub` match the reference). The renderer
draws every extracted `MeshRenderer` with its mesh and the `baseColor`
factor × texture of its `fynkel.pbr@1` material (`MaterialDesc`: base
colour, metallic, roughness, emissive, base colour texture — read straight
from the cooked material JSON), from the scene's `Camera` (highest
`priority`) with its first `DirectionalLight`; a mesh missing from the cache
draws as a magenta cube, a missing texture as white, and entities without
a `MeshRenderer` as tinted placeholder cubes when
`ExtractRenderer::placeholders` is on (the default; the Flutter host turns
it off with a project). The cooked vertex layout is the GPU layout
(`fynkel_cooked::payload::Vertex`, 72 bytes). Hosts point a viewport at a
cache with `fynkel_apple_viewport_set_asset_cache`; `fynkel render` does the
whole path headlessly.

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
Prefab entities may not carry `prefab` themselves (`D0402`, no nesting in
M2); two anchors sharing an `instance` id is `D0405`.

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

`.material.json` (`schemas/material.schema.json`) names a `shader` and sets
values: `parameters` (anything unset takes the manifest default), `textures`
(slot → `{ texture: assetRef, colorSpace?, uvSet?, sampler?{filter, mipmaps,
wrap, anisotropy} }`), `alphaMode` (+ `alphaCutoff` for `mask`), `cull`,
`castShadows`, `receiveShadows`. `fynkel document new material` yields an
untextured `fynkel.pbr@1` material.

`fynkel_document::material` holds the typed views (`ShaderManifest`,
`Material`), `ShaderLibrary::builtin()` (add project manifests with `add`),
and the rules JSON Schema cannot express. Material vs manifest: unknown shader
`FYN-D0300`, unknown parameter `D0301`, wrong type (number / bool / vector
length, whole numbers for `int`) `D0302`, out of range `D0303`, unknown slot
`D0304`, required slot unbound `D0305`, `alphaMode` naming a pass the shader
lacks `D0306`, malformed entry `D0307`, colour-space override that disagrees
with the slot `D0308` (warning). Manifest rules: default not of its type or
outside its range `D0310`, `min > max` `D0311`, malformed entry `D0312`.
`Material::resolve(&library)` fills every default into a `ResolvedMaterial`
(what `inspect` prints and the renderer will consume).

## Migrations

Each kind has a current `formatVersion` (`DocumentKind::supported_format_version`,
1 for all of them today). `fynkel_document::migrate` brings older documents
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

The one built-in step is the synthetic **scene 0 → 1**
(`fixtures/migrate/v0_training.scene.json` → `.migrated.json`): v0 kept
`entities` and `components` as arrays of objects carrying their `id`; v1
keys them by that id in the same order. Real steps follow the same shape
when a format changes: add the `Migration`, bump the kind's version, keep
the fixture pair.

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

### glTF/GLB (`fynkel.gltf`, version 1)

The importer reads glTF 2.0 `.gltf`/`.glb` through
the `gltf` crate (no `import` feature: buffers and images are resolved by
Fynkel, never by the crate) and produces, per file:

| Sub-asset id | What | Notes |
| --- | --- | --- |
| `mesh/<n>` (one primitive) or `mesh/<n>/<p>` | one mesh per **triangle** primitive | fixed 72-byte vertex: position, normal, tangent(4), uv0, uv1, colour(4); `u32` indices; bounds. Missing normals → unwelded with flat face normals (per spec); missing tangents → zero (M3 computes them); missing colour → white. Points/lines/strips/fans are skipped with `FYN-A0105`. |
| `material/<m>` | a `fynkel.pbr@1` material document | `baseColor`, `metallic`, `roughness`, `emissive`, `emissiveStrength` (`KHR_materials_emissive_strength`), `normalScale`, `occlusionStrength`; the five texture slots bound to `{ asset: <this asset>, sub: "image/<i>" }` with `uvSet` (0/1) and the sampler (`filter` from `magFilter`, `mipmaps` from `minFilter`, `wrap` from `wrapS` — a different `wrapT` is warned about once per texture); `alphaMode`/`alphaCutoff`; `doubleSided` → `cull: none`. Validates against the manifest like a hand-written one. |
| `image/<i>` | the encoded bytes (PNG/JPEG) with their MIME type | decoding is the texture importer's job (M2-11) |
| `prefab` | the default scene's node hierarchy as a prefab document | one entity per node with a `fynkel.Transform` (decomposed TRS, matrices included); a node with a single-primitive mesh carries a `fynkel.MeshRenderer` (`mesh` and `material` are `assetRef`s into this asset; a primitive without a material gets the nil ref = engine default); a multi-primitive mesh gets one child entity per primitive. Entity and component ids are UUID v5 of the asset id and `node/<i>`, `node/<i>/transform`, `node/<i>/renderer`, `node/<i>/primitive/<p>[/transform|/renderer]`. |

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
first, `width × height × 4` bytes, no mipmaps (M3 generates them at load
or cook time), no resizing. Grey, grey+alpha, RGB, RGBA, palette and
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

`fynkel cook` (`fynkel_import::cook`) turns the whole asset database into
one **cooked container per asset** in a **content-addressed cache**
(blueprint §6, §19.2c item 12). Nothing in the cache is needed to rebuild
it — the committed sources, sidecars and documents are the truth (ADR-0006)
— so `.fynkel/cache/` is ignored by version control and `rm -rf` is a
valid repair.

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
cacheVersion, profile, assets: [{ id, path, kind, status, key, file,
dependencies }] }` — how a loader finds "the current cooked file for this
asset" without recomputing keys. The manifest records the key's inputs
(`sourceHash`, `extraInputs`, `settings`, `dependencies: [{ id, key }]`)
and the chunk table (`kind`, `name`, `length`, `crc32`), no timestamps.
`Cache::prune(keep)` deletes every object not in `keep`; `fynkel cook
--prune` keeps the current profile's keys only.

**What is cooked.** A document: validated (schema + references; errors
fail it) and stored as its canonical text in one `JSON` chunk. A source
with an importer: run through `fynkel_import::import`, then meshes →
`MESH`, materials → `JSON` (`documentKind: material`), embedded glTF images
→ decoded `TEXR` (colour space from the material slots that use them:
`baseColor`/`emissive` → sRGB, data slots → linear, unused → sRGB, both →
sRGB with an `FYN-A0105` warning), the asset's own texture → `TEXR`, clips
→ `CLIP`, the prefab → `JSON`. A `fynkel.file` source: its bytes in one
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
| `MESH` | `u32` vertex count, `u32` index count, `[f32; 3]` bounds min, `[f32; 3]` bounds max, vertices (72 bytes each, the intermediate's layout), `u32` indices |
| `TEXR` | `u32` width, `u32` height, `u32` format (1 = RGBA8 sRGB, 2 = RGBA8 linear), `u32` mip levels (1), then `width × height × 4` texels, top row first |
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
| `inputActions` | A stub for §5.1: `name → { type: button \| axis1d \| axis2d, description?, bindings: ["keyboard:…", "mouse:…", "gamepad:…", "touch:…"] }`; nothing consumes it yet |
| `quality` | `{ default, profiles: { name → { shadows, shadowResolution, msaa, renderScale, anisotropy, textureMaxSize } } }`; `default` must name a profile; unset fields take the schema defaults |
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
project or is missing, `P0008` malformed entry, `P0010` no lock, `P0011`
lock mismatch, `P0012` malformed lock.

## New projects: `fynkel create`

`fynkel create <dir> [--name <package>] [--title <text>] [--org
<reverse.domain>] [--no-flutter] [--force] [--json]` (`apps/cli/src/create.rs`,
blueprint §13.2/§13.3, ADR-0017, §19.2c item 17) writes a complete game
project — the blueprint's project model, filled in:

```text
my_game/
  pubspec.yaml                 Flutter app on package:fynkel (path dependency into the checkout) + fynkel_generator (dev)
  fynkel.project.json          startup scene, assets root, input-action stub, quality profiles, registry (game.Spinner declared)
  fynkel.lock.json             `expected_lock()` for the tools that created it
  AGENTS.md  README.md         layout, every command, the validate → codegen → build → test loop, do/don't
  lib/main.dart                the host: a Ticker drives `Game.frame`; shows the simulation as text (viewport widget: M3)
  lib/game/game.dart           `Game.load(dir)`: World, `registerBehaviours(world.registry)`, `loadProject`; `projectDirectory()`
  lib/game/spinner.dart        `@FynkelBehaviour(id: 'game.Spinner')` with `@Expose` speed/axis
  lib/generated/               behaviours.g.dart + .g.json, exactly what `fynkel codegen --project .` emits
  schemas/behaviours/          game.Spinner.schema.json
  assets/scenes/main.scene.json    Camera, Sun (fynkel.DirectionalLight, 10 000 lux) and a Cube with MeshRenderer + game.Spinner { speed: 1.5 }
  assets/materials/starter.material.json
  assets/models/cube.glb (+ .meta.json)   a unit cube (24 vertices, normals, UVs); sidecar with the imported sub-asset ids
  assets/textures/swatch.png (+ .meta.json)
  assets/prefabs/  assets/audio/           empty (.gitkeep)
  test/game_test.dart          `flutter test`: the scene loads, `behavioursAttached == 1`, the spinner's authored speed, it spins
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
all five on a fresh project) and renders with `fynkel cook && fynkel render`.

`fynkel run [--project <dir>] [-- <flutter run args>]` validates the
project, then runs `flutter run --dart-define=FYNKEL_PROJECT=<abs dir>` in
it, which is how `lib/main.dart` finds the project from a bundle.

Outside the repository the CLI validates against the schema set embedded in
the build (identical to `schemas/` by `fynkel codegen --check`); `--schemas
<dir>` overrides either. Scene and prefab commands run on a document inside
a project (`fynkel.project.json` in an ancestor directory) with that
project's behaviour schemas and registry, so `game.*` components validate
instead of warning `FYN-D0205`. `fynkel test --headless` and `fynkel state
dump` take `--project <dir>` (the startup scene through `load_project`,
prefabs and behaviour schemas included) and default to the current
directory when it holds a project file.

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
fynkel cook [--assets <dir>] [--cache <dir>] [--profile <name>] [--force] [--prune] [--json] [--schemas <dir>]
fynkel cooked inspect <file.fyna>... [--json]
fynkel project describe [--project <dir>] [--json] [--schemas <dir>]
fynkel project lock [--project <dir>] [--check] [--json] [--schemas <dir>]
fynkel validate [--project <dir>] [--json] [--schemas <dir>]
fynkel codegen [--check] [--root <repo>] [--project <dart package>]
fynkel render [--project <dir>] [--out <file.png>] [--size <W>x<H>] [--cache <dir>] [--placeholders] [--json]
fynkel schema list [--json]
fynkel schema describe <type|kind> [--json]
fynkel test --headless [--project <dir> | --scene <path> [--prefabs <dir>]] [--steps N] [--seed S] [--json]
fynkel state dump [--project <dir> | --scene <path> [--prefabs <dir>]] [--steps N] [--json]
fynkel create <dir> [--name <package>] [--title <text>] [--org <domain>] [--no-flutter] [--force] [--json]
fynkel run [--project <dir>] [-- <flutter run args>]
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
scans `assets/` (or `--assets <dir>`) and cooks every asset into the cache
(`--cache`, default `.fynkel/cache`) for `--profile` (default `dev`);
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
`render` loads the project's startup scene, steps the world once and draws
the extract offscreen with the cooked cache (`--cache` defaults to
`<project>/.fynkel/cache`; run `fynkel cook` first) to a PNG, 1280×720 by
default (`--json`: `{ ok, out, size, project, scene, cache, instances,
meshes, placeholders, missing, batches, sceneCamera, sceneLight,
assetErrors, diagnostics }`); exit 1 on a project/scene/asset error, 2 when
no GPU adapter is available. `codegen` regenerates the native schemas and
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
scene, a prefab instance, a glTF mesh, a texture and a material). Every kind
added later extends this corpus.

# Authoring documents

Status: M2 (blueprint §6, §7, §14.2; ADR-0006, ADR-0016). Implemented in
`crates/fynkel_document`; `fynkel document …` in the CLI. Kind-specific
bodies (scene, prefab, material, shader, rig, project, meta) are added by the
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
scene, `R0010` unload of a scene the world does not hold, `R0011` an
additive load of a scene loaded already — M3-12, `R0012` a projection
request with more than 64 anchors, the rest dropped — informational,
M3-13, [hud.md](hud.md)), `FYN-U` the user's
settings document ([scenes.md](scenes.md) § Settings: `U0001` not a
settings document or another format version, `U0002` a field or rebind
of the wrong shape or out of range, `U0003` the file could not be read or
written); append only.

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
    "active": false,
    "parent": null | "<entity uuid>",
    "children": ["<entity uuid>", ...],
    "components": {
      "<component uuid>": { "type": "fynkel.Transform", "version": 1, "data": { ... } }
    }
  }
}
```

`active` is the entity's own flag (Unity's `activeSelf`). It is optional:
absent means `true`, and the editor writes it only when it is `false`.
An entity is **active in the hierarchy** when it and every ancestor are.
An inactive subtree keeps its ids, components and transforms but is not
drawn, simulated or updated
([design/set-active.md](design/set-active.md)). A value that is not a
boolean is `FYN-D0209` at `…/active` and reads as `true`.

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
| `FYN-H0005` … `H0007` | character rules (M3-09): a `fynkel.CharacterController` value the runtime would reject (capsule, slope, negative limits), a `fynkel.RigidBody`/`fynkel.Collider` next to a `fynkel.CharacterController` (its own capsule replaces them; one diagnostic per ignored component), a `fynkel.ThirdPersonCamera` value (distance, pitch range, sensitivity). The corpus fixture `scene/physics_rules.scene.json` lists `H0001`–`H0008` |
| `FYN-H0009` | (warning, M3-12, [scenes.md](scenes.md)) a `fynkel.TriggerZone` on an entity without a `fynkel.Collider` whose `is_sensor` is true: it never fires. A blank `event` is a value error (`H0008`, the zone's own `validate()`) |
| `FYN-H0008` | catalogue rules (M3-10): a `fynkel.Rotator`/`Oscillate`/`Follow`/`LifetimeDespawn`/`Emitter` value the runtime would reject (a zero or non-finite axis, a negative amplitude, frequency, smoothing, seconds, rate or lifetime, a non-finite offset or velocity; since M3-19 a `fynkel.Animator` with a non-finite speed, `docs/animation.md`) — the same `validate()` the tick runs (`docs/catalogue.md`) |
| `FYN-H0010` | audio rules (M3-11, [audio.md](audio.md)): a `fynkel.AudioSource` or `fynkel.AudioListener` value the runtime would reject with `INVALID_PAYLOAD` — a `volume` outside `0..=1`, a `pitch` outside `0.05..=8` (the components' own `validate()`) |
| `FYN-H0011` | volume rules (R3a, [renderer.md](renderer.md) § Volumes): a `fynkel.Volume` value the runtime would reject — a `weight` outside `0..=1`, a `size` or `radius` that is not finite and positive, a negative `blend_distance` |
| `FYN-H0012` | layers and tags (ADR-0020): a `fynkel.Layer` whose `layer` the project's `layers` does not declare, or a `fynkel.Tags` with a bit past the project's `tags` — at the component's field, with the project's list in the message; a `layer` of 32 or more is `H0012` too, from the component's own `validate()` (the tick's `INVALID_PAYLOAD`) |

Component schemas carry `x-fynkel.authored`; `#[schema(authored = false)]`
marks runtime-only records. `fixtures/scene/` holds one document per rule
with its expected diagnostics.

Component `data` follows the field types of `docs/codegen.md`: vectors are
number arrays, enums are the lowerCamel variant name (`"projection":
"perspective"`), asset references are `{ "asset": "<uuid>", "sub": "mesh/Cube" }`
with `sub` omitted for the asset itself — never a path. Built-in authored
components today: `fynkel.Transform`, `fynkel.MeshRenderer`, `fynkel.Camera`,
`fynkel.DirectionalLight`, `fynkel.PointLight`, `fynkel.SpotLight`,
`fynkel.Environment` (`fixtures/valid/training_room.scene.json` uses all of
them). Scenes and prefabs are **formatVersion 5** since ADR-0022 (a
prefab has one `root` and an instance *is* that root, § Prefab
documents). FormatVersion 4 (ADR-0020, tags and layers): every object may carry `fynkel.Layer { layer }` (the index
of one of the project's `layers`; absent is 0, `default`) and
`fynkel.Tags { bits }` (bit `i` is the project's `tags[i]`; absent is no
tag) — optional authored components like any other, always allowed by a
component registry; `fynkel.Collider` and `fynkel.CharacterController`
lost `layer` (the collider collides as its object's layer) and
`fynkel.Camera`, `fynkel.PointLight` and `fynkel.SpotLight` gained
`culling_mask` (the object-layers migration below). FormatVersion 3
(M3-19c): `fynkel.Animator` gained `weight`, `additive` and
`root_motion` (the animator-layers migration below fills the defaults
in; [animation.md](animation.md)). FormatVersion 2 (M3-04) added
`fynkel.Camera`'s `exposure_mode` (`auto` | `manual`) and `exposure`
(EV100; the camera-exposure migration), `SpotLight` (candela, range,
inner/outer cone angles in radians) and `Environment` (constant ambient
in lux, optional equirectangular `sky` texture reference with
`sky_intensity` in nits and `sky_rotation`; the first one in the scene
counts); `docs/renderer.md` says how the renderer uses each.

A component of any other type is a **behaviour**: a Dart class of the game
(`@FynkelBehaviour(id: 'game.Spinner')`) whose `@Expose` fields are its
`data`. `fynkel codegen --project` generates `schemas/behaviours/<id>.schema.json`
for each (`docs/codegen.md` § Behaviour generator; `codegen --check --json`
answers `{ok, check, native: {status}, project: {status, stale?, error?}}`
with `status` one of `written | upToDate | stale | error` — what the editor's
Inspector banner reads, exit code 2 on drift); a project's `schemas/`
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
document model as the scene schema) is a scene body plus **`root`**: the
id of its one parentless entity, which every other entity descends from
(ADR-0022; a missing root, a second parentless entity or an entity not
under the root is `FYN-D0400`). A scene entity becomes a **prefab
instance** by carrying `prefab: { asset, instance, overrides[] }`, and the
instance **is the prefab's root**, as in Unity: it keeps its own id,
`name` and `active` (they win over the root's), and it has **no components
of its own** (`FYN-D0408`) — its components are the root's, and a change
to them is an override like any other. Placing an instance is a
`setField` on the root Transform's `position` (and `rotation`/`scale`
when they differ). `asset` is the prefab document's `id` as an `AssetRef`
— or, since M3-19a, an imported model's node hierarchy as `{ asset:
<source>, sub: "prefab" }` (below) — `instance` a UUID v4 that is stable
for the instance's lifetime, `overrides` an ordered op list addressed by
**prefab-local** ids:

| op | fields | note |
| --- | --- | --- |
| `setField` | `entity`, `component`, `field`, `value` | `null` stores null |
| `unsetField` | `entity`, `component`, `field` | removes the key (the schema default applies) |
| `addComponent` | `entity`, `component`, `type`, `version`, `data` | new instance id, checked for uniqueness |
| `removeComponent` | `entity`, `component` | |
| `addObject` | `entity`, `parent`, `name`, `components` | `parent` is a prefab-local entity (the root included) |
| `removeObject` | `entity` | removes the subtree |
| `setActive` | `entity`, `value` | sets the entity's own `active` flag (a boolean) |

Array indices, display names and deep merges are not override targets.
Two instances sharing an `instance` id is `D0405`.

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
every instance by the prefab's entities: the root becomes the instance
entity itself (its id, name and active; the root's components with the
overrides applied), every other prefab-local entity and component id
becomes `derive_id(instance, local)` — UUID v5 with the instance as
namespace, so expansion is deterministic and two instances of the same
prefab never collide (`D0406` if a derived id meets an authored one) — and
every reference inside the prefab (`parent`, `children`, component data
strings equal to a local id; the root's id becomes the instance's) is
remapped; `AssetRef`s stay shared. The root's children come first in the
instance's `children`, the children the scene added after them; the new
entities follow the instance in document order, and the `prefab` key is
gone from the result, which is a plain scene (`Expanded::scene()`).
Overrides apply in order; a target that does not exist (or no longer exists)
is `D0403`, an op whose result would not validate against the component
schema (or a duplicate unique component) is `D0404`, a malformed op is
`D0407`. An unknown prefab is `D0401`; the whole instance is skipped and the
instance entity kept (without components). `PrefabLibrary::scan(dir)` collects every `*.prefab.json`
under a directory (a later path with the same id wins, with a warning).
The headless host expands before `describe`, so derived ids reach the
runtime and `fynkel state dump` prints them.

**Imported hierarchies** (M3-19a): a source the glTF importer read has its
node tree as the `prefab` sub-asset (§ Import › glTF/GLB: one entity per
node with `Transform`, `MeshRenderer` and `Skin`, ids UUID v5 of the asset
id and `node/<name path>`, components `node/<path>/transform|renderer|skin`;
one root, as a prefab has: the file's one top-level node, or — when it has
several — a new root named after the file, id `derive_sub_id(asset,
"node")` with an identity Transform `node/transform`, over them), and
`prefab: { asset: <source id>, sub: "prefab" }` instances it like a prefab
document — the same expansion, derivation, overrides (`setField` on
`node/<path>/renderer` swaps a material, on the root's transform turns the
model), nesting (a prefab document may instance an import), `D040x`
diagnostics and `fynkel scene inspect --expand`. The library keys such
prefabs by `(source, "prefab")` (`PrefabLibrary::add_imported`,
`resolve(&AssetRef)`); the sub is required — a `.glb` without one is
`D0401` with the hint, as is a sub that is not `prefab` or a source
without a hierarchy (a texture: its sidecar lists no `prefab`, so the
reference is also `A0004`). Where the hierarchy comes from: `fynkel
validate`, `fynkel run` and `fynkel scene` import the source in memory
through `fynkel_import::ImportedPrefabs` (the `SourcePrefabs` provider
`validate_project_with` takes; a bare `validate_project` has none and
reports `D0401`) — the importer's own code path, so the ids are the ones
the cook writes; the cook takes the `prefab` chunk of the cooked source
(sources cook first, cached or not) and expands the scenes against it;
`fynkel scene inspect|validate --prefabs <dir>` scans that directory for
sources with sidecars as well as `.prefab.json` files. `fynkel import
--prefab <file.glb>` writes the same hierarchy as an editable
`<stem>.prefab.json` next to the source with a fresh document id (never
over an existing file; a source without a hierarchy is `A0105`) for the
cases that want authored, stable, hand-edited ids. The fixture project's
Rig scene holds both forms: the hand-authored column and `Imported`, an
instance of the Column rig document's `prefab` (the same hierarchy as
`rig.glb#prefab`, § Rig documents) with its Prop's material overridden.

## Authoring commands and transactions

`fynkel_document::authoring` (§11.1, ADR-0016) is the one implementation of
document mutation the editor, the CLI and agents share; the editor reaches
it through the authoring session and its C ABI (below, M4-01). An `Editor`
holds one scene, prefab or **material** document (`Editor::open` refuses
other kinds, read-only newer documents and outdated ones with `FYN-E0007`; a
document that already has errors opens, and those baseline errors are
tolerated; a material is validated against the shader library —
`Editor::open_with(document, schemas, shaders)` — and an entity command on
it is `FYN-E0007` pointing at `setDocumentField`). Its `Revision` is the blake3 hash of the canonical
text (`Document::write`), so two tools that read the same bytes agree on it
without shared state — `fynkel scene inspect --json` and `fynkel prefab
inspect --json` print it as `revision`.

`Editor::apply(Request { id?, baseRevision?, commands }, &schemas)` runs one
**transaction** atomically (`Editor::apply_with_prefabs(request, &schemas,
&PrefabLibrary)` is the same with the prefabs the document can see, which
only `unpackPrefab` reads — `apply` passes an empty library):

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
   duplicate id `E0010`, a protected document field `E0012`) stops the
   transaction and everything already applied is rolled back.
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
| `createObject` | `id?`, `name`, `parent?`, `index?`, `components[]` | new entity `{ name, parent, children: [], components }`, appended to (or inserted at `index` of) the parent's `children`; a parentless one only in a scene, its `index` its place among the roots (the entity goes before the root now at that position in `entities`, whose key order is the roots' order) (a prefab's entities descend from its `root`) |
| `deleteObject` | `id` | removes the entity and its subtree, detaches it from its parent; a prefab's `root` cannot be deleted |
| `renameObject` | `id`, `name` | |
| `setObjectActive` | `id`, `active` | `false` writes `"active": false` right after the name; `true` removes the key (nothing to write when it is absent) |
| `reparentObject` | `id`, `parent?`, `index?` | moves the entity (`FYN-E0006` if `parent` is the entity or below it); `null` makes it a root; `index` counts the new parent's list without the entity — for a scene root, the roots without it: the entity's member moves to just before that root in `entities` (the Hierarchy's reorder, as Unity's, 2026-09-24) |
| `addComponent` | `entity`, `component: { id?, type, version?, data? }` | `version` 1 and empty `data` by default; `FYN-E0010` if the id exists |
| `removeComponent` | `entity`, `component` | |
| `setField` | `entity`, `component`, `field`, `value` | replaces the value, or adds the member / appends to an array end |
| `unsetField` | `entity`, `component`, `field` | removes the member (distinct from setting `null`) |
| `assignAsset` | `entity`, `component`, `field`, `asset: { asset, sub? }` | `setField` with an `assetRef` |
| `instantiatePrefab` | `id?`, `name`, `parent?`, `index?`, `prefab: { asset, sub? }`, `instance?`, `components[]` | an entity with `prefab: { asset, instance, overrides }` — the instance is the prefab's root (ADR-0022), so `components[]` become overrides on the root (`setField` for a key that differs from the prefab, `addComponent` for a type the root lacks); scene documents only |
| `setPrefabOverride` | `entity`, `override` (a prefab-model op object) | replaces the override with the same target (field for `setField`/`unsetField`, component for `addComponent`/`removeComponent`, object for `addObject`/`removeObject`, the entity's flag for `setActive`), else appends |
| `revertPrefabOverride` | `entity`, `index?` | removes one override, or all of them |
| `unpackPrefab` | `entity` | replaces the instance by its expansion, in place: the instance keeps its id and loses `prefab`, gaining the root's components with the overrides applied and the root's children (under the ids the runtime derives) before its added ones; the expanded entities are inserted right after it in `entities` — the document then reads exactly like `expand`'s; needs the prefab library (`Editor::apply_with_prefabs`), an instance that does not expand is refused (`FYN-E0004` with the `D040x` in `related`), not an instance `FYN-E0008`; one undo step (M4-07) |
| `duplicateObject` | `id`, `newId?`, `name?`, `index?` | deep-copies the subtree with fresh entity, component and prefab-instance ids (every reference inside the copy remapped), placed right after the original (a scene root's copy: right after it in `entities`, since 2026-09-24) or at `index`; the copy's root is the first affected id; a taken `newId` is `FYN-E0010`; one undo step (M4-01) |
| `pasteObjects` | `entities` (object of entity JSON by id: whole subtrees, as the `document` query's `ids` returns them), `parent?`, `index?` | the editor's Paste (2026-09-24): copies the entities with fresh entity, component and prefab-instance ids, every reference among them remapped as `duplicateObject` does; the entities whose parent is not among them are the pasted roots, in order, under `parent` at `index` (or at the end); `children` naming an entity not among them are dropped; the affected ids start with the pasted roots; works across documents; one undo step |
| `setDocumentField` | `pointer`, `value` | sets a field of the document itself by RFC 6901 pointer — a material's `/base_color`, a scene's `/name`; the envelope's identity (`/$schema`, `/kind`, `/formatVersion`, `/id`) and a scene's `/entities` / `/roots` are refused with `FYN-E0012` (M4-01) |
| `unsetDocumentField` | `pointer` | removes a document field (`/name` cannot be unset, `FYN-E0012`) (M4-01) |
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

### The authoring session and its ABI (M4-01)

`crates/fynkel_authoring` (`AuthoringSession`, blueprint §19.2e item 1) is
the layer the editor holds a project through — one session per editor
window, never running game code and never reading a cooked cache
(ADR-0016/0018):

- **`open_project(dir)`** reads `fynkel.project.json` with its diagnostics,
  the embedded schemas plus the project's `schemas/behaviours/*.json`,
  `fynkel.lock.json` — a lock written by a **newer** Fynkel opens the project
  **read-only** (`OpenProject::read_only` carries the reason; every edit,
  undo and save is then `FYN-E0007`), an older one is a warning — the asset
  database (a full scan, every asset reference indexed for `dependents`)
  and `lib/generated/behaviours.g.json` when present (its absence is a
  `FYN-T0006` warning that says to run `fynkel codegen`).
- **`open_document(path | id)`** opens a scene, prefab or material by path
  (absolute or relative to the project) or by asset id into an `Editor`
  behind a small integer **handle** (idempotent; handles are never reused
  in a session). `apply`, `undo`, `redo`, `save(path?)`, `reload`,
  `close_document` act on a handle; `state_json` is what a tab strip shows
  (`path`, `relativePath`, `sub`, `kind`, `id`, `name`, `formatVersion`,
  `revision`, `dirty`, `canUndo`, `canRedo`, `history` — the transactions
  left to undo — and `readOnly`). The name is read live from the root, so
  `setDocumentField /name` shows at once. **`open_document("<id or
  path>#prefab")`** (M4-07) opens an **imported hierarchy** — the prefab the
  importer derives from a model source (`hero.glb`) or a rig document's
  skeleton, the one an instance `{ asset, sub: "prefab" }` expands — as a
  read-only prefab document: `kind` `prefab`, `sub` `"prefab"`, `path` the
  source file, `relativePath` with `#prefab` appended, `readOnly` naming
  the source; every transaction on it is `FYN-E0007`. A texture's or any
  other sub is `FYN-D0401`. Its entities carry the derived `node/<path>` ids
  the runtime uses, so an override written against the instance addresses
  the same ids the tab shows.
- **Writes inside a transaction** (`createMaterial`, `saveDocument`) take a
  relative path from the **project directory**, not the process's working
  directory, and the paths a committed transaction wrote are rescanned so
  the asset database sees the new material or the saved document without a
  second call. `save` and a save-as rescan the written path the same way (a
  save-as keeps the id, so the database reports `FYN-A0003` until one copy
  goes).
- **The authoring log** (M4 gate, 2026-09-24): every change the session
  makes is one JSON line in `<project>/.fynkel/authoring.log` (git-ignored
  with the rest of `.fynkel/`; `fynkel_authoring::log`). Each line has
  `v` 1, `at` (Unix ms), `session` (new on every `open_project`) and an
  `op`: `open`; `apply` — a **committed** transaction's `document`
  (relative to the project), `transactionId`, `baseRevision`, `revision`,
  its `commands` in the JSON form `fynkel scene apply` reads, and
  `affectedIds` per command; `undo` / `redo` — the `transactionId`
  replayed and its command names; `save` — the `document` written;
  `asset` — an asset operation's `kind` and `args` that succeeded. A
  refused transaction changed nothing and is not written; a line that
  cannot be written is dropped rather than failing the edit. Replaying a
  log's `apply`/`undo`/`redo`/`save` lines on another copy writes the same
  bytes (`every_change_of_a_session_is_in_the_authoring_log_and_replays`).
- **Queries** (`query(kind, args)`, all read-only): `project`, `documents`,
  `document` (state + the whole document as JSON; with `ids: [...]` the
  named entities — `null` for one the document no longer has — plus
  `roots`, the root ids in sibling order: a prefab's `root`, a scene's
  null-parent entities in key order — what the Hierarchy re-reads from a
  transaction's `affectedIds` instead of the whole document, M4-03),
  `defaults` (`type` → `{ type, data }`, a built-in authored component's
  Rust `Default` through the document codec — what "Create with
  component" writes; a behaviour's defaults come from its schema; the
  test `defaults_cover_every_authored_component` keeps it in step with
  `AUTHORED_COMPONENTS`, M4-03), `revision` (`path` → the revision and
  identity of a document **file** on disk now, not of any open editor —
  the conflict dialog's third revision, M4-03), `inspect` (the `fynkel scene|prefab inspect --json` view; a
  material's shader, alpha mode, cutoff, double-sided and fields; plus the
  document's diagnostics now), `validate`, `validateProject` (the
  `fynkel validate --json` report), `assets` (`kind` document|source,
  `document` a kind name), `asset`, `dependents`, `schemas`, `schema`
  (with the `x-fynkel` display hints), `metadata`, `migrations` (what
  `fynkel migrate` would do to every document behind its kind's
  `formatVersion` — the steps this build knows and, per outdated document,
  the applied steps, the diff and any diagnostics; nothing is written —
  the editor's Preview migration, M4-02), `instance` (`handle`, `entity`
  → the expansion of the prefab instance `entity`, M4-07:
  `{ anchor, instance, asset: { asset, sub }, expanded, prefab: { id, name,
  path, kind: prefab|source|rig, open, handle, readOnly } | null,
  diagnostics, entities: [{ id, local, name, parent, children, added,
  components: { <id>: { id, local, type, version, data, added, overridden:
  [field] } } }], overrides: [{ index, op, entity, entityId, entityName,
  component?, componentId?, componentType?, field?, value?, pointer }] }` —
  `entities` in the expansion's document order under the **derived** ids
  — the first is the instance itself (its own id and parent, `local` the
  prefab's root; ADR-0022) — `local` the prefab's own ids,
  `overridden` the `setField`/`unsetField` keys of a component (the
  Inspector's bold), `added` an `addComponent`/`addObject`; the library is
  the preview's — an **open prefab editor first**, dirty edits included,
  then disk, imported hierarchies from the source's chunk or the rig — so
  `prefab.open`/`handle` say when the instance follows a tab; a prefab
  that is not available (`FYN-D0401`) or an override that does not apply
  (`D0403`/`D0404`/`D0407`) is `expanded: false` with the diagnostics and
  no entities — the instance keeps its data and nothing is rewritten; not an
  instance `FYN-E0008`, unknown entity `FYN-E0002`; `crates/fynkel_authoring/src/instance.rs`,
  `tests/prefab_mode.rs`), `commands`.
- **`rescan(paths)`** re-reads only the files a watcher (or an operation)
  names — `AssetDatabase::update` (M4-05): a source or its sidecar names the
  pair, a directory everything under it, a gone path drops its entry; the
  reference index is rebuilt for those documents only. A full scan of a
  2 000-asset project is ~50 ms; one change is well under a millisecond
  (`crates/fynkel_authoring/tests/ops.rs` asserts both on a generated
  2 000-asset tree). Files a duplicate id kept out are re-read too, so
  deleting the first owner of an id admits the second.
- **`operate(kind, args)`** (M4-05, `crates/fynkel_authoring/src/ops.rs`)
  is `query`'s mutating twin: the session's own changes to the **asset
  tree**, next to the per-document transactions. `newDocument { kind:
  scene|prefab|material, path, name?, shader? }` writes `Document::new`
  under the asset root (the kind's suffix added when missing; the PBR
  shader by default; refused when the file or another kind's file of that
  name exists). `renameAsset { asset, path }` moves a document, or a source
  **with its sidecar**, so the id survives — `path` is relative to the
  asset root, a path ending in `/` or naming a directory keeps the file
  name, parent directories are created, a document keeps its `.kind.json`
  suffix and a source its extension (refused otherwise), the asset root
  itself is refused (`the path names the asset root itself`), and an open
  editor of that document is `relocate`d (its next `save()` writes to the
  new path; revision and history untouched; the Dart side sees a
  `DocumentChangeKind.moved`). `duplicateAsset { asset, path? }`
  (Unity's Duplicate, 2026-09-24) writes a copy beside the asset —
  `coin 1`, `coin 2`, … (a trailing number counts on) or at `path` —
  under a fresh id: a document keeps its body, its `name` following the
  file when it was the file's stem; a source is copied byte for byte with
  a sidecar of its own (same settings, fresh sub-asset ids under the same
  keys); undo removes the copy (refused once it was edited).
  `installPrimitives {}` (ADR-0023, 2026-09-24) makes sure the project
  has the editor's built-in shapes: the asset of the fixed id
  `f7a1e5d0-0000-4000-8000-000000000023`, wherever it was moved, comes
  back unchanged (`installed: false`); otherwise `Fynkel/Primitives.glb`
  and its sidecar (embedded in the library, generated by
  `crates/fynkel_authoring/assets/primitives.py`) are written as one step
  on the asset history (`installed: true`). Its `mesh/Cube`, `mesh/Sphere`,
  `mesh/Capsule`, `mesh/Cylinder`, `mesh/Plane`, `mesh/Quad` and
  `material/Default` ids derive from the fixed id, so they are the same
  in every project.
  `deleteAsset { asset, force? }` is
  **refused while anything references the asset** — `FYN-T0003` with one
  `related` `FYN-A0004` per referencing field (`document` absolute path,
  `pointer`, `id`) — and with `force` moves the file and sidecar into
  `.fynkel/trash/<id>/` (never `rm`), reporting the now-dangling
  references as warnings. `setImportSettings { asset, settings }`
  replaces a sidecar's `settings` object; `reimport { asset }` runs the
  importer in memory with the sidecar's settings and answers `{ ok,
  summary, subAssets, sidecarWritten }` — `ok: false` with the importer's
  diagnostics and nothing written when it refuses — bringing `subAssets`
  up to date (existing ids kept). `setLayersAndTags { layers, tags,
  layersFrom?, tagsFrom? }` (ADR-0020, `src/layers.rs`) replaces the
  project's lists; `layersFrom[i]`/`tagsFrom[i]` name the old index new
  entry `i` comes from (null: a new entry; absent: the lists line up by
  index, i.e. renames and additions, which write the project file only).
  A reorder or a removal rewrites every scene and prefab so each object
  keeps its meaning — `fynkel.Layer` indices (a removed layer becomes
  `default`), `fynkel.Tags` bits (a removed tag's bit is dropped), every
  layer mask (the native `mask`/`culling_mask` fields and behaviour fields
  hinted `layerMask`; "Everything" stays everything and gains new
  entries) and the same values inside prefab overrides — and is refused,
  with nothing written, while such a document has unsaved edits; the
  result's `reload` lists the handles of the clean open documents it
  rewrote. `registerBehaviour { id, description? }` (`src/behaviours.rs`,
  the editor's Create → Behaviour, after `fynkel codegen` wrote the new
  type's metadata and schema) adds `id → { kind: behaviour, version: 1,
  description? }` to the project's component registry when the project
  has one and the entry is missing, as one asset-history step, and re-reads
  the project either way. Its answer is `{ id, registered, inMetadata }`.
  `setStartupScene { id }` (the M4 gate; Unity's first scene in Build
  Settings) writes the project file's `startupScene` — the scene
  `fynkel run` and `fynkel test --headless` start with — as one
  asset-history step; `FYN-T0003` for anything but a scene of the project
  or the scene that already is the startup scene.
  `undoAsset` / `redoAsset` replay the
  **asset history**, an undo stack of its own (a document's ⌘Z never moves
  a file): created documents are removed and re-written byte-for-byte,
  moves reversed, trashed files brought back, settings restored;
  `FYN-E0009` when empty. Every operation rescans only the paths it
  touched and answers `{ ok, op, label, entry, assetCount, canUndo,
  canRedo, undoLabel, redoLabel, … }`; a refused one touched nothing (a
  move whose second half failed is undone before it reports). A
  read-only project refuses every operation with `FYN-E0007`. Queries that
  go with it: `assetDetail { asset }` — the entry, the file's size and
  time, a source's sidecar (importer, version, settings, sub-assets), a
  material's shader/alpha/double-sided/fields, a scene's or prefab's
  entity count, and the `dependents` rows `{ document, path, pointer,
  asset, sub? }` — `assetHistory` (`canUndo`, `canRedo`, labels, the undo
  and redo lists) and `operations` (the kinds).
- Diagnostics of the session itself are `FYN-T####`: `T0001` no project,
  `T0002` unknown handle, `T0003` invalid request, `T0004` unknown query,
  `T0005` a caught panic, `T0006` I/O.

**The C ABI** (`crates/fynkel_native/include/fynkel_authoring.h`,
`fynkel_authoring_*`, `FYNKEL_AUTHORING_ABI_VERSION` **3** — version 2
added `fynkel_authoring_operate(session, kind, kind_len, args_json,
args_len)` for M4-05, version 3 the edit-preview world below for M4-06;
an editor built against an older version still loads) is the
session one call per method: text in as UTF-8 with a length, one JSON result out
(`fynkel_authoring_result`, `{ "ok": true, ... }` or `{ "ok": false,
"diagnostics": [...] }`, valid until the next call on the session) and a
`FynkelStatus` that says which without parsing — `OK`;
`COMMANDS_REJECTED` for an edit the editor's rules refused (`FYN-E####`);
`LOAD_FAILED` for a project or document that could not be read or written;
`INVALID_ARGUMENT` for NULL, bad UTF-8, non-JSON, no project, an unknown
handle or query (`FYN-T0001..T0004`); `PANIC` once (the session is
**poisoned** and answers `POISONED` from then on — destroy it and open a
new one). Every function is NULL-safe and every panic is caught
(ADR-0014); a session is driven from one thread. The entry points are
compiled into `libfynkel_native` / `libfynkel_platform_apple` **only with
the cargo feature `authoring`** (on by default in the workspace so the
editor and the tests get it; `package:fynkel`'s build hook passes
`--no-default-features` and adds `--features authoring` only when the
app's pubspec lists it under `hooks.user_defines.fynkel.features`) — a
game's runtime has none of these symbols, and CI checks that
`cargo build --no-default-features` exports no `fynkel_authoring_*`.

**The Dart side** is `packages/fynkel_authoring` (ffigen bindings over the
header onto the same code asset as `package:fynkel`; games never depend on
it): `AuthoringSession` (`openProject`, `openDocument`, the queries typed,
`rescan`, `operate` with typed wrappers — `newDocument`, `renameAsset`, `duplicateAsset`, `installPrimitives`,
`deleteAsset`, `setImportSettings`, `reimport`, `undoAsset`, `redoAsset`
→ `OperationResult` —, `assetDetail` → `AssetDetail`, `assetHistory` →
`AssetHistory`, `dependents`, a **synchronous** broadcast
`Stream<AuthoringEvent>` so an `AssetsChanged` reaches its listeners
before `operate` returns), `DocumentHandle`
(`state`, `apply(commands, baseRevision:)` → `Applied`, `undo`/`redo` →
`Replayed`, `save`, `reload`, `close`, `root()`, `inspect()`,
`validate()`, `instance(anchor)` → `PrefabExpansion` (M4-07: `prefab` an
`ExpansionPrefab` with `isImported`, `entities` as `ExpandedEntity`/
`ExpandedComponent`, `overrides` as `ExpansionOverride`, `entity(id)`,
`byLocal(local)`), `changes`), `Command` builders for every command above
(`setPrefabOverrideOp(entity, op)` takes any override op object,
`unpackPrefab`),
the ADR-0014 `Diagnostic` record and `AuthoringException` (`isConflict`,
`isReadOnly`, `isNothingToReplay`, `isPoisoned`). Its test drives a
complete session over the ABI on a copy of the fixture project.

#### The edit-preview world (M4-06)

What the editor's Scene view draws (blueprint §19.2e item 6; ADR-0016's
resolutions): a `fynkel_core::World` in **`WorldConfig::edit_mode`** —
`step()` applies commands and propagates transforms, no system ever runs,
the tick counter counts only those steps — holding the **live** document:
dirty edits included, prefab instances expanded from the editors that
have the prefab open (or from disk), imported hierarchies from the
importer. `crates/fynkel_authoring/src/preview.rs` (`Preview`, the
editor's own crate) maps authoring ids to entities and works on a
`&mut World` handed in; the world itself is owned by `fynkel_native`
(`PreviewSlot` in `src/authoring.rs`), so the `FynkelWorld` a viewport
renders is the one the session edits. The session never runs game code
and the editor never submits a command or a step to this world
(`fynkel_world_submit`/`fynkel_world_step` on it would be undone by the
next sync).

- **Load.** `AuthoringSession::describe_with_anchors(handle)` describes
  the document as `fynkel_document::describe_scene` would for the runtime
  and adds the **anchor map** — every derived entity of a prefab instance
  to the instance entity that owns it, so a pick inside an instance
  answers with the instance the document knows. `Preview::load` is a `World::load_scene`
  of that description with the skins registered and the closure retained.
  The first preview of a session **cooks the project into its cache in
  process** (`OpenProject::cook`, the same cooker as `fynkel cook`; the
  report is the open result's `cook`) and every later preview shares the
  asset store — a failed source is reported and the rest is drawn.
- **Sync — the incremental rule.** After `apply`, `undo`, `redo`,
  `reload`, `save`, `rescan` and `operate` the session brings every open
  preview up to date *before it returns*: `Preview::sync` diffs the world
  against the description the document has **now** and only the
  differences become commands — spawns, destroys, reparents, renames,
  component sets and removes, sibling order — applied through the world's
  own command path over as many steps as the new hierarchy is deep
  (`MAX_SYNC_ROUNDS` 64; no document is that deep). Undo, redo, prefab
  overrides, duplicates and an edit to a prefab open in another tab all go
  through the same diff; nothing has a special case. When a round makes no
  progress (the world rejected a command) the preview falls back to a
  fresh **reload** and says so (`SyncReport { spawned, destroyed, updated,
  reordered, steps, reloaded }`, reported as `"previews": [{ handle,
  … }]` on the call's result). This deviates from the blueprint's
  per-`Mutation` mapping on purpose: a diff needs no knowledge of the
  transaction kinds and is proven equal to a fresh load after every one of
  them (`tests/preview.rs` compares `Preview::dump` — the state keyed by
  authoring id in the shape of `fynkel state dump --json` — with a reload
  after each transaction kind).
- **Queries.** `pick(ray)` — a mesh through its bounding sphere then its
  triangles (two-sided), anything else through a 25 cm sphere
  (`ICON_RADIUS`) at its origin, nearest first, hits mapped to anchors;
  `pick_rect` — the entities whose projected bounds overlap a rectangle;
  `outline_lines` — twelve edges of a mesh's bounds, a cross for the rest,
  an anchor's whole instance; `icon_lines` — lights in their colour (range
  circles, cone, arrows), cameras (their frustum to `CAMERA_OUTLINE_DEPTH`
  4 m), colliders (green); `set_transforms`/`restore_transforms` — the
  gizmo's preview of local transforms without touching the document;
  `set_hidden` — the editor's hidden set (a filter in the world's dump and
  picks, never in the document); `world_matrix`; `stats`. A `View` is
  `{ world[16], aspect, projection?, fovY?, orthoHeight?, near?, far?,
  exposureMode?, exposure? }` with the runtime's own projection formulas.
- **The ABI (version 3).** `fynkel_authoring_preview_open(session, handle)`
  → `{ "ok", "opened", "stats", "cook"?, "diagnostics" }` (FYN-T0003 for a
  material — nothing to preview); `fynkel_authoring_preview_close`;
  `fynkel_authoring_preview_world` → `const FynkelWorld*` for the viewport
  ABI (`fynkel_apple_viewport_render`, `fynkel_world_stats`…), valid until
  the preview, document, project or session closes;
  `fynkel_authoring_preview_call(session, handle, kind, args_json)` with
  the kinds `camera` (the editor camera, so the world's exposure follows
  it), `hidden`, `pick` (`view` + `ndc`, or a `ray`), `pickRect`, `lines`
  (`outline` ids and colour, `iconsAspect`), `worldMatrix`,
  `setTransforms`, `restoreTransforms`, `resync`, `reload`, `recook` (cook
  again, point every preview at the new store, reload each; with
  `live: true` unsaved documents cook as held, their files untouched, and a
  recovery copy goes to `.fynkel/recovery/` — ADR-0021 P5), `dump`,
  `stats`. `close_document` reports `previewClosed`.
- **Dart.** `DocumentHandle.openPreview()` →
  `ScenePreview` (`worldAddress` for `FynkelViewportController.
  renderHandle`, `stats`, `diagnostics`, `cook`; `setCamera`,
  `setHidden`, `pick`/`pickRay`/`pickRect` → `PreviewPick`, `lines` →
  `PreviewLines`, `worldMatrices`, `setTransforms`/`restoreTransforms`,
  `resync` → `PreviewSyncReport`, `reload`/`refreshStats` →
  `PreviewStats`, `recook` → `CookReport`, `dump`, `close`), and a
  `PreviewSynced` event on the session's stream after every call that
  synced.

**Big documents (D7).** RFC 6901 lookups go through `json::Locator`, which
caches paths and builds a per-object key index lazily for objects of 16+
members; the parser's duplicate-key check switches to a set past the same
size. Scene, prefab, schema and asset-reference validation anchor their
diagnostics through a Locator, so a document with thousands of
diagnostics validates in linear time (release, 16 000 entities with 8 000
warnings: read 31 ms + validate 68 ms, was 726 ms at M2; apply+validate
on 10 000 entities 58 ms; the test asserts a 4× size costs under 6× the
time).

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

## Rig documents (M3-19b)

`<name>.rig.json` (`schemas/rig.schema.json`, hand-written; kind `rig`)
is the answer to "four downloads, one character": a character assembled
from imported models — one **skeleton** and any number of **clips**
named by the game, each from whichever file carries it:

```json
{ "$schema": "https://fynkel.dev/schemas/rig.schema.json", "formatVersion": 1,
  "id": "…", "name": "hero",
  "skeleton": { "asset": "<Idle.fbx>" },
  "clips": { "idle": { "asset": "<Idle.fbx>", "sub": "anim/mixamo.com" },
             "walk": { "asset": "<Walking.fbx>", "sub": "anim/mixamo.com" },
             "run":  { "asset": "<Fast Run.fbx>", "sub": "anim/mixamo.com" },
             "jump": { "asset": "<Jumping.fbx>", "sub": "anim/mixamo.com" } },
  "markers": { "walk": [ { "at": 0.12, "name": "footstep" }, { "at": 0.61, "name": "footstep" } ] } }
```

`skeleton.asset` names a source that imports as a model with a node
hierarchy (`fynkel.gltf`, `fynkel.fbx`; no `sub`), whose meshes, skins
and joints *are* the character; each clip is an `anim/<name>` of any such
source (the skeleton's own file included). Clip names are identifiers
(`[A-Za-z_][A-Za-z0-9_]{0,63}`). The rig is an asset like any document
(`fynkel_document::Rig`): the asset database lists its **sub-assets** —
`prefab` (the skeleton file's hierarchy) and one `anim/<clip name>` per
entry, uuids v5 of the rig id and the sub — so a scene names
`prefab: { asset: <rig>, sub: "prefab" }` and an `Animator`
`{ asset: <rig>, sub: "anim/walk" }`: the game reads as text, a
re-export never renumbers its clips, and swapping the character is
editing one document. Meshes and skins stay the skeleton source's own
sub-assets (the rig does not re-export them; the runtime's closure of a
rig is the rig plus its skeleton, never the clip files).

Validation: the schema, then against the database — a nil or missing
skeleton `FYN-D0500` (`/skeleton/asset`), a skeleton that is not a
model or carries a `sub` `D0500` (`/skeleton`), a clip that is not a
model's `anim/*` `D0501` (`/clips/<name>`, the hint lists the source's
clips; an unknown sub is also `A0005`), a malformed entry or a name
that is not an identifier `D0502`. Instancing `<rig>#prefab` goes
through the same `PrefabLibrary` path as `<source>#prefab` (§ Prefab
documents › Imported hierarchies): the skeleton's hierarchy is imported
and **adopted** under the rig's `prefab` sub uuid as its document id and
the rig's name (`Rig::adopt_prefab`); the entity and component ids stay
the skeleton file's (`node/<path>` of the *source* id), so overrides
written against the file keep working and a scene can move from
`<fbx>#prefab` to `<rig>#prefab` by changing the asset.

The cook writes the rig as one object: the document, a `prefab` JSON
chunk (the adopted hierarchy, expanded and described like a prefab
document's; `info.skeleton` names the source) and one `ANIM` chunk per
clip named `<name>` under `anim/<name>`, **matched to the skeleton by
bone name**: a channel whose target is a joint of the skeleton or an
entity of its hierarchy is kept; a target the skeleton lacks is
`FYN-A0106` (the first missing bone named, "(N more)"; one per clip)
when the clip's file has no skin or the target is one of its own joints
— the clip was made for another skeleton —, else dropped with an
`A0105` warning (a prop node the clip's file happened to animate);
extra bones in the skeleton are fine, a clip's own meshes are ignored,
the clip keeps its duration. The rig's `markers` (M3-19c: `<clip name>`
→ `[{ at, name }]`, an optional object; a name that is not one of
`clips`, a non-array, a negative or non-finite `at` or an empty `name`
is `D0502`) join the source clip's own markers in the cooked `ANIM`,
sorted by time; one past the clip's end is dropped with a `D0502`
warning at cook. `fynkel rig inspect` counts them per clip. The `ANIM`
info carries `channels`, `seconds`, `source` (`<path>#anim/<name>`),
`dropped` and `markers`. Runtime
dependencies are the skeleton only (the index and META
`dependencies`), while the cache key covers every clip source: editing
a clip file's sidecar recooks the file and the rig, editing the rig
recooks the rig alone.

`fynkel import <files>... --rig <name>` writes `<name>.rig.json` next
to the first file: the first file is the skeleton (`FYN-D0500` when it
has no hierarchy), every file's clips are entries named from the file
stem when the file carries one clip (`Fast Run.fbx` → `fast_run`) or
the clip's own name otherwise, lower-cased, runs of other characters to
`_`, a leading digit prefixed, later duplicates `_2`, `_3`; every clip is
matched against the skeleton as the cook will (`A0106` at
`/clips/<name>`, the document still written so it can be fixed; not
written when a file failed to import, `A0105`), an existing rig of that
name keeps its id and gets its `skeleton` and `clips` rewritten (the
report: `rig: { file, ok, written, created, id, clips: [{ name, sub,
source, seconds, channels, matched, dropped, missing }] }`; `--dry-run`
matches and writes nothing). `fynkel rig inspect <rig.json>` imports
the skeleton and every clip in memory and prints the skeleton (path,
joints, roots, entities) and each clip's source, seconds and
`matched/channels` with what was dropped or missing (`--json`: `{ ok,
id, name, path, skeleton: { asset, path, joints[], roots[], entities },
clips[], diagnostics }`); `fynkel rig validate <rig.json>...` runs the
same match and exits 1 on any error — the check `fynkel validate` does
not run (it resolves references, the bones are the cook's or `rig
validate`'s to compare).

## Blend spaces (M3-19c)

`<name>.blend.json` (`schemas/blend.schema.json`, hand-written; kind
`blend`, `fynkel_document::Blend`) is a 1D blend space: clips along a
named parameter, played by a `fynkel.Animator` like one clip
([animation.md](animation.md) § Blend spaces):

```json
{ "$schema": "https://fynkel.dev/schemas/blend.schema.json", "formatVersion": 1,
  "id": "…", "name": "Player locomotion",
  "parameter": "speed",
  "points": [ { "at": 0, "clip": { "asset": "<character>", "sub": "anim/idle" } },
              { "at": 4, "clip": { "asset": "<character>", "sub": "anim/walk" } },
              { "at": 7, "clip": { "asset": "<character>", "sub": "anim/run" } } ],
  "sync": "phase" }
```

`parameter` is an identifier; `points` holds 1 to 16 entries, each a
clip — an `anim/*` sub-asset of a rig document or of a model source —
at a finite `at`, **strictly increasing** (`FYN-D0600` at `/points`
otherwise, and for no points); a clip that is not one, or nil, is
`D0601` at `/points/<i>/clip` (an unknown sub is also `A0005`); a
malformed entry, a non-finite `at` or an unknown `sync` is `D0602`;
`sync` is `phase` (the only mode). A scene's animator names the
document with no sub (`clip: { asset: <blend id> }`); the asset
database lists the document as an asset of kind `document/blend` with
no sub-assets. The cook validates the document (errors fail the asset;
a scene naming a failed blend is `IMPORT_SKIPPED`) and writes it through
the generic document path as a JSON chunk (`documentKind: blend`, the
info carrying `parameter`, `points` and `sync`) whose dependencies are
the clips' carriers (the rig or model), so a re-import re-cooks the
space; the runtime store reads the chunk into a `BlendSpace`
(`Assets::blend`) and registers it with every point's clip
(`register_clip` on a blend id counts once). The lock's `documents`
gains `blend: 1`.

## Volume profiles (R3a)

`<name>.volume.json` (`schemas/volume.schema.json`, hand-written; kind
`volume`, `fynkel_document::VolumeDocument`) is the look a
`fynkel.Volume` component applies to the frame ([renderer.md](renderer.md)
§ Volumes):

```json
{ "$schema": "https://fynkel.dev/schemas/volume.schema.json", "formatVersion": 1,
  "id": "…", "name": "Showcase Look",
  "ambientOcclusion": { "radius": 0.9, "intensity": 1.6 },
  "bloom": { "intensity": 1.0, "threshold": 0.7, "scatter": 0.7 } }
```

Every block and every field is optional: a present field overrides what
is below it (the engine default or a lower-priority volume), an absent
one is inherited — so `{}` is a valid profile that changes nothing and
`{ "ambientOcclusion": { "intensity": 0 } }` turns the occlusion off and
nothing else. The schema holds the ranges (`ambientOcclusion`: `radius`
in `(0, 10]` m, `intensity` in `[0, 8]`; `bloom` (R3b): `intensity` in
`[0, 10]` — 0, the default, is off —, `threshold` in `[0, 20]` exposed
brightness, `scatter` in `[0, 1]`, `tint` three numbers in `[0, 1]`) and refuses unknown blocks and fields
(`FYN-D0100`); the read view reports what the schema cannot see as
`FYN-D0700` and leaves the field inherited. `fynkel document new volume
<name>` writes one with the engine defaults spelled out. The cook
validates it (errors fail the asset, and a scene whose volume names it
with it) and writes it through the generic document path as a JSON
chunk (`documentKind: volume`); a scene depends on the profiles its
volumes name. The runtime store reads the chunk into a
`fynkel_core::volume::VolumeProfile` (`Assets::volume_profile`, cached
per reference, evicted with its asset). The lock's `documents` gains
`volume: 1`. R3b's `bloom` block was the first addition, and R3c
(colour grading) adds another: blocks, not kinds.

## Migrations

Each kind has a current `formatVersion` (`DocumentKind::supported_format_version`:
material and project 2, scene and prefab 5, every other kind 1). `fynkel_document::migrate` brings older documents
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

- **scene 2 → 3** `scene/2-3/animator-layers` and **prefab 2 → 3**
  `prefab/2-3/animator-layers` (M3-19c;
  `fixtures/migrate/v2_dancer.scene.json` → `.migrated.json`): every
  `fynkel.Animator` gets the fields the record gained — `weight: 1.0`,
  `additive: false`, `root_motion: false` — where it lacks them (a
  document that already carries some keeps them); other components are
  untouched.
- **scene 3 → 4** `scene/3-4/object-layers` and **prefab 3 → 4**
  `prefab/3-4/object-layers` (ADR-0020;
  `fixtures/migrate/v3_arena.scene.json` → `.migrated.json`): a
  `fynkel.Collider`'s or `fynkel.CharacterController`'s `layer` bits
  leave the component and become its entity's `fynkel.Layer` — the index
  of the lowest set bit (when an entity's colliders disagree, or one has
  several bits, the lowest wins), and no component for bit 0 (the
  default layer); the component's id is UUID v5 of the entity's with the
  name `fynkel.Layer`, so a re-run and every copy agree. Cameras and point
  and spot lights gain `culling_mask: 4294967295` (every layer) unless
  they have one. The `data` of an `addComponent` override is rewritten the
  same way (a `fynkel.Layer` override follows it); a `setField` or
  `unsetField` of `layer` has no equivalent and fails the step (`D0018`,
  with the override's pointer).
- **project 1 → 2** `project/1-2/layers` (ADR-0020): `physics.layers`
  moves to the top-level `layers` unchanged, so every bit keeps its
  meaning; a project without it keeps the default.
- **scene 4 → 5** `scene/4-5/one-root` and **prefab 4 → 5**
  `prefab/4-5/one-root` (ADR-0022; `crates/fynkel_document/src/one_root.rs`,
  `crates/fynkel_import/tests/one_root_migration.rs`). An instance cannot
  move without its prefab, so the work is a **project** rewrite
  (`scene+prefab/4-5/one-root (project)`, run by `fynkel migrate` on a
  project directory when a document's chain reaches 4), and the
  per-document step only finishes what needs no other file (a prefab's
  single `roots` entry becomes `root`) and refuses the rest with the hint:
  - a prefab with several roots gets a new root named after the prefab
    (id `uuid_v5(prefab id, "root")`, Transform `uuid_v5(prefab id,
    "root/transform")`, identity), the old roots its children;
  - an anchor loses its components: its Transform composed with the
    prefab root's is written as `setField` overrides on the root's
    Transform (only the fields that differ), its other components become
    `addComponent` overrides on the root; its own children and top-level
    `addObject`s, placed relative to the anchor, are corrected by the
    root's inverse; a `setActive` on the root folds into the instance's
    flag, and a string equal to the old root's derived id becomes the
    instance's id;
  - refused, the document left as it was with the reason and a pointer:
    a composition that is not position/rotation/scale (non-uniform scale
    under a rotation), an anchor component whose type the root already
    has, a `removeObject` of the root, a prefab the project does not have.

  Nested anchors inside prefabs are rewritten the same way; a second run
  changes nothing.

New steps follow the same shape when a format changes: add the
`Migration`, bump the kind's version, keep the fixture pair. Until a
document is migrated every command refuses it with `FYN-D0016`.

## Asset sidecars and the asset database

A **source asset** (a `.glb`, `.png`, `.wav`, ... under `assets/`) has no
envelope of its own, so its identity lives in a sidecar next to it:
`<file>.meta.json` (`schemas/meta.schema.json`, hand-written; ADR-0006).
Besides the envelope (`name` defaults to the file name) it holds
`importer` (namespaced id chosen from the extension: `fynkel.gltf`,
`fynkel.fbx`, `fynkel.image`, `fynkel.audio`, `fynkel.file` for anything
else),
`importerVersion`, importer-defined `settings`, and `subAssets` — sub-asset
id (`mesh/Body`, `material/Painted`; the `sub` of an `assetRef`) → that
sub-asset's own stable uuid, written on first import and kept across
re-imports. Documents (`.scene.json`, `.prefab.json`, `.material.json`,
`.shader.json`, `.rig.json`) are their own identity and have no sidecar;
a rig document declares sub-assets of its own (`prefab`, `anim/<name>`;
§ Rig documents).
`fynkel asset init <file>` writes a fresh sidecar (new v4 id), and so
does the editor for a source it finds without one (its session's scan;
never in a read-only project); moving an
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
`A0005`, malformed sidecar entry `A0006`, a reference to an asset of the
wrong kind (a model's material remap naming something that is not a
material document, § Material remaps) `A0007`.

## Import

`fynkel_import` (blueprint §6 "Asset identity and import", ADR-0006) turns a
source file, its sidecar's `settings` and the importer's version into a
**normalized intermediate**: the one shape the cache (M2-12), the cooked
container and the renderer's loaders read, whatever the source format was.
It is development-only, never runs in a shipped game, and serializes to
bytes deterministically — the same source, settings and sidecar give the
same bytes on every machine, so it can be hashed, cached and diffed.

### Sub-asset keys

A model's sub-asset keys come from the **names** of its parts
(`docs/design/stable-sub-asset-ids.md`; sidecar `formatVersion` 2,
`fynkel.gltf` 6, `fynkel.fbx` 2), so reordering meshes, materials or clips
in the modelling tool keeps every reference on the same part:

- `mesh/<name>` (several primitives or material parts: `mesh/<name>/<p>`),
  `material/<name>`, `image/<name>` (else the image URI's or texture file's
  stem; `image/<name>/linear` for a dual-use copy), `skin/<name>`,
  `anim/<name>`; imported-prefab ids derive from `node/<name path>`
  (`node/Hero/mixamorig_Hips/…`), never a `sub`.
- A name is kept to letters, digits, `_ . space -`; anything else becomes
  `_` (`mixamorig:Hips` → `mixamorig_Hips`). An unnamed part keeps its
  position behind `#` (`anim/#0`); a name met again in the same kind (or
  among siblings, for nodes) gets `~2`, `~3` in file order
  (`mesh/Cube~2`); a name past 80 characters is cut and gets `~` and a
  hash. `#` and `~` never survive sanitising, so a marker cannot collide
  with a name.
- **Renaming** a part in the modelling tool breaks its references loudly:
  the old key is reported as no longer produced (`A0105`, naming the keys
  that appeared in the same import) and every reference to it is `A0005`.
  Nothing is remapped by guesswork.

A sidecar at `formatVersion` 1 keyed parts by position (`mesh/5`,
`anim/0`, `node/12`). It is behind its format (`D0016`) until `fynkel
migrate` moves the project: it imports each such model once, renames the
sidecar's keys **keeping their uuids**, renames every `{ asset: <model>,
sub }` in the project, and moves every override target derived from a node
position — on an instance of the model's hierarchy, of a rig built on it,
or of a prefab nesting either — to the id the name path derives
(`meta/1-2/name-keys`; `fynkel_import::keys_migration`). Given one
document of such a project, `fynkel migrate` widens to the whole project.
A model whose file is missing or does not import is reported and left
with its documents as they are. The editor's migration preview shows the
same plan.

### glTF/GLB (`fynkel.gltf`, version 7)

The importer reads glTF 2.0 `.gltf`/`.glb` through
the `gltf` crate (no `import` feature: buffers and images are resolved by
Fynkel, never by the crate) and produces, per file:

| Sub-asset id | What | Notes |
| --- | --- | --- |
| `mesh/<name>` (one primitive) or `mesh/<name>/<p>` | one mesh per **triangle** primitive | fixed 72-byte vertex: position, normal, tangent(4), uv0, uv1, colour(4); `u32` indices; bounds. Missing normals → unwelded with flat face normals (per spec); missing tangents → zero in the intermediate (the cook generates them, below); missing colour → white. Points/lines/strips/fans are skipped with `FYN-A0105`. |
| `material/<name>` | a `fynkel.pbr@1` material document (the current formatVersion) | `base_color`, `metallic`, `roughness`, `emissive`, `emissive_strength` (`KHR_materials_emissive_strength`), `normal_scale`, `occlusion_strength`, `alpha_mode`/`alpha_cutoff`, `double_sided`; the five `<slot>_texture` references bound to `{ asset: <this asset>, sub: "image/<name>" }` (or `image/<name>/linear`, below). A slot that samples `TEXCOORD_1` or a non-default sampler (nearest filtering, clamp/mirror wrap) is imported without that setting and warned about once (`FYN-A0105`): the PBR model has one UV set and one sampler. Validates like a hand-written one. |
| `image/<name>` | the encoded bytes (PNG/JPEG) with their MIME type and **colour space** | sRGB when a colour slot (`base_color`, `emissive`) uses it, linear when only data slots (`normal`, `metallic_roughness`, `occlusion`) do, sRGB when unused |
| `image/<name>/linear` | a linear copy of a **dual-use** image | when one image feeds both a colour and a data slot it is imported twice; the data slots point at the linear copy, so every texture reaches the cache in the colour space its slot needs |
| `prefab` | the default scene's node hierarchy as a prefab document (the current formatVersion; importer version 3 = prefab formatVersion 2) | one entity per node with a `fynkel.Transform` (decomposed TRS, matrices included); a node with a single-primitive mesh carries a `fynkel.MeshRenderer` (`mesh` and `material` are `assetRef`s into this asset; a primitive without a material gets the nil ref = engine default); a multi-primitive mesh gets one child entity per primitive; a node whose mesh has a skin carries a `fynkel.Skin { skin: skin/<name>, skeleton }` next to the renderer (M3-19). Entities are named after their nodes (`node<i>` when unnamed) — the names are what clips and skins target. Entity and component ids are UUID v5 of the asset id and `node/<path>`, `node/<path>/transform`, `node/<path>/renderer`, `node/<path>/skin`, `node/<path>/primitive/<p>[/transform|/renderer]`, where `<path>` is the node's name path from its root (§ Sub-asset keys). |
| `skin/<name>` (M3-19, [animation.md](animation.md)) | one per glTF skin | the joints in the skin's order, each by node name with its parent when that is a joint of the same skin, its inverse bind matrix and its rest pose; the skeleton root's name. Two joints sharing a name is `A0104`, more than `max_joints` (256) is `A0101`. A primitive with `JOINTS_0`/`WEIGHTS_0` is a **skinned mesh**: its `mesh/<name>` carries one normalized `SkinWeight` (4 × `u16` joint, 4 × `f32` weight) per vertex, a joint index past the skin is `A0104`. |
| `anim/<name>` (M3-19) | one per glTF animation | its translation/rotation/scale channels targeting nodes by name, step/linear/cubic-spline as authored, the duration; morph-weight channels skipped (`A0105`), a target name that is not unique in the file warned about (`A0105`). The animation's `extras` may carry `{ "markers": [ { "at": <seconds>, "name": "<name>" } ] }` (M3-19c, importer version 5): kept on the clip, sorted; an entry without a finite `at` in `[0, duration]` or a non-empty `name`, or a `markers` that is not an array, is `A0104`. |

Morph targets and cameras are skipped with `A0105` warnings; a skin on
a node without a skinned mesh, or a skinned mesh on a node without a
skin, is a warning and the mesh draws in its bind pose. Every sub-asset's
uuid comes from the sidecar's `subAssets`
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

#### Material remaps (`settings.materials`)

Both model importers (glTF from version 7, FBX from version 3) read a
remap table from the sidecar (`docs/design/material-remapping.md`):

```json
"settings": {
  "materials": {
    "material/Painted": { "asset": "<id of metal_red.material.json>" }
  }
}
```

- Keyed by the embedded material's key; the value is an `assetRef` to a
  **material document**. A nil reference means "the embedded one".
- Every `fynkel.MeshRenderer` in the model's `prefab` whose material is a
  remapped key points at the project material instead, so every instance
  of the model draws it; an instance's own override still wins. A
  reference that names `{ asset: <model>, sub: "material/Painted" }`
  itself is left alone, and the embedded material stays a sub-asset.
- The targets are the model's **runtime** dependencies (loaded with it),
  not key inputs: the cooked model holds only their ids, which the
  settings already cover, so editing a remapped material recooks that
  material alone.
- A target that is not a material document (another kind, or a sub-asset
  such as another embedded material) is `A0007`, one that does not
  resolve `A0004`, an entry that is not an `assetRef` `A0006` — all
  errors on the sidecar, from `fynkel validate` and the cook, and the
  model does not cook. A key the file no longer has (the material was
  renamed or deleted in the modelling tool) is an `A0105` warning naming
  the file's current material keys.
- **Extract Materials** (the editor's asset page, `fynkel asset
  extract-materials <model> [--to <dir>]`) writes each embedded material
  that is not remapped yet as an editable `<name>.material.json` —
  a fresh id, the imported values, its textures still the model's
  `image/…` sub-assets — into `Materials/` next to the model (a taken name
  gets ` 2`, ` 3`, …) and remaps it. `fynkel asset remap <model> <key>
  <material|none>` sets or clears one remap.

### FBX (`fynkel.fbx`, version 3)

M3-19b. The importer reads binary and ASCII FBX (`.fbx`, 6100–7700)
through `ufbx` (a single C file compiled into `fynkel_import`; nothing
of it reaches a shipped game) and produces the **same intermediate the
glTF importer does** — the same `MeshData`/`SkinData`/`AnimationData`,
material documents, images and `prefab`, through the same
`crates/fynkel_import/src/graph.rs` code — so everything downstream
(cache, cook, `fynkel.Skin`, `fynkel.Animator`, prefab instancing, the
renderer) is unchanged, and a rig exported to FBX and to glTF gives the
same joints, weights, bind matrices, keys, UVs and hierarchy
(`crates/fynkel_import/tests/fbx.rs` checks `rig.fbx`/`robot.fbx`
against `rig.glb`/`robot.glb`, the same content round-tripped through
Blender by `fixtures/blender_fbx.py`; the acceptance rig is a Mixamo
character, see [animation.md](animation.md)). This is **not** a general
FBX importer: what is listed here is imported, everything else is skipped
with a named `A0105` warning or refused with an error.

**Conversion.** `ufbx` converts the file's unit and axes to Fynkel's
(metres, right-handed Y-up, `space_conversion: modify geometry`: vertex
data is transformed, node transforms stay plain TRS — a Mixamo file in
centimetres comes out at metres, a Maya or Blender file keeps its
authored poses), geometry transforms and non-standard inherit modes become
helper nodes, missing normals are generated, skin weights are cleaned.
FBX UVs have their origin at the bottom-left; the importer flips `v` so a
texture reads the same as through the glTF path. Corners sharing a
vertex, normal, UVs, tangent and colour are welded back into one vertex
(a cube exported from 8 vertices is 24 again, as in glTF).

| Sub-asset id | What | Notes |
| --- | --- | --- |
| `mesh/<name>` (one material) or `mesh/<name>/<p>` (one per material part) | one mesh per polygon mesh, triangulated by `ufbx`, per material | `<name>` is the mesh element's name (§ Sub-asset keys). Same 72-byte vertex as glTF; the mesh's own `material/<name>`; point and line faces skipped (`A0105`), a mesh without polygons skipped (`A0105`). A mesh with a skin deformer is a **skinned mesh**: the four heaviest weights per vertex, normalized, joint = the cluster's index in the skin; a vertex no cluster touches is bound to joint 0 with weight 1. Blend-shape deformers are skipped (`A0105`), a second skin deformer on a mesh is ignored (`A0105`). |
| `material/<name>` | a `fynkel.pbr@1` material document | from `ufbx`'s PBR view of the material — a classic Phong/Lambert material maps diffuse → `base_color`, shininess → `roughness`, emissive; a real PBR material (Maya Standard Surface, 3ds Max Physical, Blender Principled) keeps its own values. Emission colour × factor; anything past 1 becomes `emissive_strength` (white at 3 → `[1, 1, 1]` at 3, like `KHR_materials_emissive_strength`). Opacity below 1 (or an opacity texture) → `alpha_mode: blend`; double-sided from the material's feature flag. Textures are bound for `base_color`, `normal`, `occlusion` and `emissive` slots only; a roughness, metalness or opacity texture is reported (`A0105`) and left unbound — the PBR model reads roughness and metallic from one packed `metallic_roughness_texture` and opacity from the base colour's alpha, both set on the material document afterwards. Clamped wrap or a UV transform on a texture is warned about once per texture (`A0105`, the sampler is the default); layered and procedural textures are skipped. Validates like a hand-written material. |
| `image/<name>` | the **embedded** bytes of a file texture bound by a slot, colour space by use as in glTF (`image/<name>/linear` for a dual-use image) | `<name>` is the texture file's stem, else the texture's name. PNG and JPEG only (sniffed, `A0103` otherwise; `max_image_bytes` → `A0101`). A texture that is not embedded is refused with `A0102` unless the sidecar says `"allowExternalFiles": true`, and then it is read from the plain relative path the file names (`\` → `/`; a drive, scheme, absolute path or `..` is `A0102`) next to the source — the same boundary as glTF. Textures no slot binds are not imported. |
| `skin/<name>` | one per skin deformer | the joints in **cluster order**, each named after its bone node with its parent when that bone is in the same skin, `inverse_bind` = the cluster's `geometry_to_bone` (checked against the bind pose on Mixamo and Blender files), rest pose = the bone's local TRS; the skeleton root = the one joint whose parent is not in the skin (`""` when there are several). A cluster bound to no bone or two joints sharing a name is `A0104`; more than `max_joints` is `A0101`. |
| `anim/<name>` | one per **animation stack**, in file order, named after the stack (`"mixamo.com"`, `"Take 001"`, Blender's action/NLA-track name) | baked by `ufbx` at the file's frame rate (`trim_start_time`: the clip starts at 0; key reduction on) into translation/rotation/scale channels targeting nodes by name, linear interpolation, duration = the last key; channels that do not move keep one key. A stack that animates nothing (Mixamo's empty `Take 001`) produces no clip and no warning. A target name that is not unique in the file is `A0105`. Clip names are what the file says — the rig document (§ Rig documents) is where a game names its clips (`walk`, `idle`, …) and matches them across files by bone name. |
| `prefab` | the node hierarchy under the scene root as a prefab document, the glTF layout | entity ids from `node/<name path>`; the file's root node is not an entity, its children are the prefab's roots; a mesh node carries `fynkel.MeshRenderer` (a multi-material mesh gets one child per part, `<name>[<p>]`) and, when skinned, `fynkel.Skin { skin, skeleton }`; the prefab is named after the file stem. Blender's FBX exporter writes its armature convention as a −90° X turn on the armature node and +90° X on the root bone: world poses are what was authored, the local numbers differ from the glTF export of the same rig. |

Cameras, lights, NURBS/procedural surfaces and curves are skipped
(`A0105`, counted). A file that is not FBX at all (or a version `ufbx`
does not read) is `A0100` with `ufbx`'s reason; the node graph limits
(`max_nodes`, `max_node_depth`), vertex and image limits are `A0101`. The
sidecar's `settings` are `allowExternalFiles` and `materials` (§ Material
remaps, above). Sub-asset ids and
their uuids follow the glTF rules above (`subAssets` in the sidecar,
UUID v5 otherwise, dropped ids reported).

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
anchored at `/settings/colorSpace` of the sidecar. Only PNG, JPEG and
Radiance HDR are compiled in (`image` with `default-features = false`);
anything else is `A0103`. Limits: `max_image_bytes` (64 MiB of file),
`max_texture_dimension` (16 384 per side, enforced before decoding) →
`A0101`. A standalone texture has no sub-assets: an `assetRef` to it is
`{ asset }` alone.

**HDR environments (E1, [renderer.md](renderer.md) § Image-based
lighting).** `.hdr` (Radiance RGBE, flat or run-length scanlines) → one
`TextureData` whose `hdr` is `width × height × 3` linear `f32` radiance
values (`rgba8` empty, `colorSpace` always `linear` — a sidecar saying
`srgb` is `A0104` at `/settings/colorSpace`, the file is what it is). The
cook writes it as `TEXR` format 3 (RGBA16F, the chain averaged in `f32`,
alpha 1; `hdr: true` in the chunk info) **and** — an HDR image being an
environment — an `ENVM` chunk next to it under the same name: the image
prefiltered for image-based lighting by `fynkel_import::environment`
(a 128² six-level GGX specular cube, roughness 0 at level 0 to 1 at the
last, importance-sampled with 256 samples per texel and the lookups
filtered by the solid angle each sample stands for; a 32² irradiance
cube of E/π; the mean irradiance for the exposure estimate; chunk info
`specularSize`, `specularMips`, `irradianceSize`, `meanIrradiance`). The
equirect mapping and the cube face order are the shaders' (`u = 0.5`
looks down −Z and grows toward +X, `v = 0` is up; faces +X −X +Y −Y +Z
−Z with the GL texel rule), so a mirror reflects the sky drawn behind
it. Deterministic: fixed sample sets, `f32` throughout. An 8-bit sky
gets no `ENVM` and lights nothing — the scene keeps its constant
ambient. Fixture: `fixtures/textures/sky.py` → `sky.hdr` (64×32, blue
above 45°, red toward −X, green toward +X below), pinned as
`sky.expected.fyni`; `tests/cook.rs` checks the two chunks and the
irradiance against the closed-form integrals.

### Audio (`fynkel.audio`, version 2)

`.wav` (PCM 8/16/24/32-bit and float) / `.ogg` (Vorbis) → one `AudioData`:
interleaved `f32` samples in `[-1, 1]` (lossy overshoot clamped), the
sample rate and channel count as the file had them, the frame count and
the source codec (`pcm`, `vorbis`). No resampling, no channel mixing, no normalization,
no tags — the runtime's mixer decides (M3-11, [audio.md](audio.md): the
cooked `CLIP` chunk is registered whole; mono and stereo only). Decoding is
`symphonia` with only `wav`/`pcm`/`ogg`/`vorbis` compiled in and no SIMD
features, so a Vorbis decode is bit-identical across machines; anything
else (FLAC, MP3, AAC, ...) is `A0103`. A stream that ends early or has a
corrupt packet is `A0104` — a game asset with a hole in it is fixed at
the source, not skipped over. Limit: `max_audio_frames` (10 minutes at
48 kHz) → `A0101`. One setting, `stream` (a boolean, default `false`;
anything else is `IMPORT_INVALID_DATA` at `/settings/stream`; version 2,
M3-11 part 2): a streamed sound is decoded and checked exactly the
same, then the samples are dropped and the `AudioData` keeps the
**source bytes** (`stream: Some(bytes)`, `frames` from the decode) for
the runtime to decode as it plays ([audio.md](audio.md) § Streamed
music — music, not effects). No sub-assets.

### The intermediate's bytes

`.fyni`: `FYNI`, version (6 since E1: a texture header may carry an `hdr`
blob of `f32` RGB; 5 since M3-11 part 2: a clip header carries
`frames` and, streamed, the source blob's `[offset, length]` with no
samples), a JSON header (source, importer, meshes / materials / images /
textures / clips / skins / animations with `[offset, length]` into the
blob area, the prefab document when there is one, the sub-asset ids) and
4-byte-aligned blobs (vertices, `u32` indices, skin weights, encoded image
bytes, RGBA8 texels or an HDR image's `f32` RGB, `f32` samples or a
streamed sound's source bytes, joint tables and key data). `Intermediate::to_bytes`/`from_bytes`
round-trip; the committed `fixtures/robot.expected.fyni`,
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
`.gltf`'s non-`data:` `buffers[].uri` / `images[].uri`, an `.fbx`'s
non-embedded file textures by relative path, hashed, sorted; a missing
file hashes as "missing"), the sidecar's `settings` as canonical
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
shadowResolution, ambientOcclusion, bloom, msaa, renderScale, anisotropy,
textureMaxSize }] } |
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

**Stamps.** `<cache>/stamps.json` remembers each source file's size,
modification time and hash, plus the extra files a `.gltf`/`.fbx`
names. A cook reads a source only when its size or time no longer
matches its stamp, or when the asset must cook. A key still hashes the
content, so a file that is touched but not changed stays cached. Only a
same-size edit that keeps the old time (a deliberate `touch -t`, a clock
set back) goes unseen; `cook --force` trusts no stamp. A file modified
within two seconds of the cook is hashed but not stamped. The stamps
are only an optimisation: deleting the file costs one full read of
every source.

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
normal; a skinned mesh's weights follow its indices in the same chunk; cache version 5 since M3-19c), skins → `SKIN` and animations → `ANIM` (M3-19, [animation.md](animation.md); the markers after the channels since M3-19c), materials → `JSON` (`documentKind: material`) **plus** a `MATL`
chunk of the same name/sub (the decoded `PbrMaterial`, what the renderer
reads; a material document cooks the same pair), embedded glTF images →
decoded `TEXR` in the colour space the importer assigned them (dual-use
images arrive as two sub-assets, one per colour space), the asset's own
texture → `TEXR` (an HDR image as RGBA16F plus its prefiltered `ENVM`,
E1; cache version 6), clips → `CLIP` (or `STRM` for a sidecar's `stream: true`: the source bytes, M3-11 part 2), the prefab → `JSON`. A `fynkel.file` source: its bytes in one
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
| `META` | JSON: `{ fynkel: "cooked", version, id, name, path, kind, importer, importerVersion, profile, key, dependencies: [uuid], subAssets: { sub: uuid }, chunks: [{ index, kind, name, sub?, id?, … }] }` — per-chunk extras: `documentKind`; `vertices`, `indices`, `material`; `width`, `height`, `colorSpace`, `sourceHasAlpha`, `hdr`, `mipLevels`; `specularSize`, `specularMips`, `irradianceSize`, `meanIrradiance`; `sampleRate`, `channels`, `frames`, `codec` (+ `streamed: true`, `bytes` for a `STRM`); `entities`; `extension` |
| `JSON` | A document's canonical text (scene, prefab, material, …) |
| `MATL` | `u32` payload version (1), `u32` record size (`PbrMaterial::SIZE`, 184), `u32` string-tail length, `u32` reserved, then the `fynkel.PbrMaterial` wire record (ADR-0012 layout: `Vec4` 16 bytes, optional asset refs as a nil id when unbound) and its string tail (the `sub` ids). `fynkel_wire::material::{encode_material, decode_material}`. META extras: `shader`, `alphaMode`, `textures` (the bound slot names). |
| `MESH` | `u32` vertex count, `u32` index count, `[f32; 3]` bounds min, `[f32; 3]` bounds max, vertices (72 bytes each, the intermediate's layout), `u32` indices |
| `TEXR` | `u32` width, `u32` height, `u32` format (1 = RGBA8 sRGB, 2 = RGBA8 linear, 3 = RGBA16F linear — an HDR image, E1), `u32` mip levels (the full chain, `full_mip_count(w, h)`), then every level's `w × h` texels (4 bytes each, 8 for format 3) concatenated, level 0 first, top row first (`TexturePayload::level(i)` / `base()` / `texel_bytes()`) |
| `ENVM` | `u32` specular face size, `u32` specular mip levels, `u32` irradiance face size, `[f32; 3]` mean irradiance, then the specular cube — per level, per face (+X −X +Y −Y +Z −Z), `size²` RGBA16F texels — and the irradiance cube (six faces of `size²` RGBA16F texels of E/π). An HDR image's prefiltered environment (E1); `EnvironmentPayload::specular_level(m)`; faces are bounded to `MAX_ENVIRONMENT_FACE` (2048) on decode. META extras: `specularSize`, `specularMips`, `irradianceSize`, `meanIrradiance`. |
| `CLIP` | `u32` sample rate, `u32` channels, `u64` frames, interleaved `f32` samples |
| `STRM` | `u32` sample rate, `u32` channels, `u64` frames, `u32` codec length, `u32` data length, the codec name (`pcm`, `vorbis`), the source file's bytes — a streamed sound (the sidecar's `stream: true`), decoded by the runtime as it plays |
| `BLOB` | The source file's bytes |

`fynkel_cooked::payload::{decode_mesh, decode_texture, decode_environment, decode_clip, decode_stream}` (and
the `encode_*` counterparts, `Vertex`, `MeshPayload`, `TexturePayload`,
`EnvironmentPayload`, `ClipPayload`, the `f32_to_f16`/`f16_to_f32` pair
the half-float payloads use) read the typed payloads back (length-checked, indices
bounds-checked; `None` on a malformed payload); `fynkel_import::cook`
re-exports them.

## Project and lock

`fynkel.project.json` (`fynkel_document::project`, `project.schema.json`,
hand-written like the material and shader schemas) is the one file that
makes a directory a project. It is a document (envelope, strict JSON,
`formatVersion` 2 since ADR-0020, `DocumentKind::Project`) and holds:

| Member | Meaning |
| --- | --- |
| `startupScene` | `{ asset }` — required; must resolve to a scene document in the asset database |
| `assets` | The asset root, relative to the project directory (default `assets`; no `..`, no absolute path) |
| `inputActions` | The game's actions (§5.1, [input.md](input.md)): `name → { type: button \| axis1d \| axis2d, description?, bindings: ["<device>:<control>", …] }` — `keyboard:WASD`, `keyboard:Space`, `mouse:delta*0.0025` (an axis control takes a `*<factor>` scale, M3-14), `gamepad:leftStick`, `touch:overlay/jump`, …; every binding is checked against the runtime's grammar (`FYN-P0009`). The cook copies the block into the cache index; the runtime compiles it into the world's binding table when the project loads (M3-07) |
| `layers` | The project's layers (ADR-0020): up to 32 unique, non-empty names, `"default"` first; index `i` is bit `i` of every layer mask (a collider's `mask`, a camera's or light's `culling_mask`, a raycast's) and the value of a `fynkel.Layer` (default `["default"]`) — `FYN-P0014` on more than 32, an empty or duplicate name, or a first entry that is not `default`. Scenes and prefabs naming an index past the list are `FYN-H0012` |
| `tags` | The project's tags (ADR-0020): up to 32 unique, non-empty names; bit `i` of a `fynkel.Tags` is `tags[i]` (default `[]`) — `FYN-P0014` like `layers`, `FYN-H0012` for a bit past the list |
| `physics` | `{ gravity: [x, y, z] }` ([physics.md](physics.md), M3-08): the world's gravity (default `[0, -9.81, 0]`) — `FYN-P0013` when not finite. The cook copies the block into the cache index; the world applies it when the project loads. (`physics.layers` of `formatVersion` 1 is the top-level `layers` now.) |
| `quality` | `{ default, profiles: { name → { shadows, shadowResolution, ambientOcclusion, bloom, msaa, renderScale, anisotropy, textureMaxSize } } }`; `default` must name a profile; unset fields take the schema defaults (shadows on, 2048, ambient occlusion on, bloom allowed, MSAA 4, scale 1, anisotropy 8, 4096). The cook copies the block into the cache index and the renderer reads it there (`docs/renderer.md` — MSAA runs at 1 or 4, the other values are clamped; `ambientOcclusion` is R2's screen-space occlusion, `bloom` whether R3b's bloom may run — a volume gives it its strength); `textureMaxSize` is for the cook |
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
malformed lock, `P0013` a `physics` block the runtime cannot apply (a
non-finite gravity), `P0014` the project's `layers` or `tags` (ADR-0020).

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
  lib/fynkel_game.dart         `buildGame(FynkelLaunch)`: the one entry point — main.dart runs it with `FynkelLaunch.fromEnvironment()`, the editor's Play mounts it (ADR-0021)
  lib/game/game.dart           `Game.load(dir)`: World, `registerBehaviours(world.registry)`, `loadProject`
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
fynkel scene apply <path> --patch <ops.json> [--dry-run] [--json] [--schemas <dir>] [--prefabs <dir>]
fynkel prefab inspect <path> [--json] [--schemas <dir>]
fynkel prefab validate <path>... [--json] [--schemas <dir>]
fynkel prefab apply <path> --patch <ops.json> [--dry-run] [--json] [--schemas <dir>] [--prefabs <dir>]
fynkel material inspect <path> [--json] [--schemas <dir>] [--shader <manifest>]...
fynkel material validate <path>... [--json] [--schemas <dir>] [--shader <manifest>]...
fynkel shader inspect <path> | --builtin <shaderId> [--json]
fynkel shader validate <path>... [--json] [--schemas <dir>]
fynkel asset list [--assets <dir>] [--json] [--schemas <dir>]
fynkel asset resolve <uuid>[#<sub>]... [--assets <dir>] [--json] [--schemas <dir>]
fynkel asset init <file>... [--json]
fynkel migrate <path|dir>... [--dry-run] [--json] [--schemas <dir>]
fynkel migrate --list [--json]
fynkel import <file>... [--out <dir>] [--prefab] [--rig <name>] [--dry-run] [--json] [--schemas <dir>]
fynkel rig inspect <path> [--json] [--schemas <dir>]
fynkel rig validate <path>... [--json] [--schemas <dir>]
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
roots[], expanded }` with `--json`) followed by the diagnostics; instances show their
prefab, instance id and override count, `--expand` shows the expanded scene
instead. Scenes with instances resolve prefabs from `--prefabs <dir>`
(default: the scene's own directory; imported models with their sidecars
under it serve `{ asset, sub: "prefab" }` instances, M3-19a) and report
override problems in `validate`. `prefab inspect` prints the tree (`root` in `--json`). `scene apply` and
`prefab apply` run one authoring transaction from a patch file (see
[Authoring commands and transactions](#authoring-commands-and-transactions)); a patch
that `unpackPrefab`s reads the prefabs the way `inspect --expand` does (`--prefabs`, else the
document's directory). `material inspect` prints the resolved material (every
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
kept); `--prefab` also writes the hierarchy as `<stem>.prefab.json` next
to the source (§ Prefab documents › Imported hierarchies; `prefab` in the
file's report); `--dry-run` prints the summary and writes nothing
(`--json`: `{ ok, dryRun, files: [{ file, ok, id, importer, sidecar,
sidecarCreated, sidecarWritten, summary, intermediate, bytes, prefab? }],
rig?, diagnostics }`; `--rig <name>` and `fynkel rig inspect|validate`:
§ Rig documents). `cook`
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
sceneLight, lights, lightsDropped, lightsMaxPerObject, lightsOverflow,
sky, ibl, ev100, shadowCasters, shadowMap, quality: { profile, shadows,
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
lands on it, with the project's `layers` `["default", "props"]` (Crate B
on `props` through its `fynkel.Layer`); since M3-09 a
second scene, Playground (`assets/scenes/playground.scene.json`, id
`…0031`: Floor, a 0.8 m Platform, a Player with a
`fynkel.CharacterController` driven by the `move`/`jump` actions, a
Camera with a `fynkel.ThirdPersonCamera` targeting it by name and turned
by the `look` action, Sun) with `test/platform.input.json` (120 frames:
W held, Space at frame 20, W released at 75) — the headless gate of item
9 — so the main scene's counts are unchanged; since M3-10 a third scene,
Catalogue (`…0032`); since M3-12 a fourth, Results
(`assets/scenes/results.scene.json`, id `…0033`: a Camera, a turning
Trophy, Sun — the scene the Dart gate transitions to) and a third prefab,
Coin (`assets/prefabs/coin.prefab.json`, id `…0042`: a half-size mesh, a
sensor sphere `fynkel.Collider` and `fynkel.TriggerZone { event:
"collect", once: true }`), instantiated at runtime by the tests; since
M3-19 a fifth scene, Rig (`assets/scenes/rig.scene.json`, id `…0034`: the
rig fixture's skinned column under a looping "wave" `fynkel.Animator`
with its joints `hips` → `spine` → `head` as entities and a `fynkel.Skin`
on the Body, a Prop under a one-shot "bob", a Camera, Sun) over
`assets/models/rig.glb` (a copy of `crates/fynkel_import/fixtures/rig.glb`,
written by `rig.py`) — the headless, render and Dart gates of item 19 —
and `fynkel.Animator`/`fynkel.Skin` in the registry; since M3-19b a
rig document, Column (`assets/rigs/column.rig.json`, id `…0050`: the
skeleton `rig.glb`, clips `wave` = `anim/wave` and `bob` = `anim/bob`) that
the Rig scene's `Imported` entity instances as `{ …0050, sub: "prefab" }`
under an `Animator` on `anim/wave`; since M3-19c a blend space, Column
energy (`assets/rigs/column.blend.json`, id `…0051`: `energy` 0 = the
rig's `bob`, 1 = its `wave`); since M3-11 a sound, `assets/audio/tone.wav`
(id `…0006`, a copy of `crates/fynkel_import/fixtures/audio/tone.wav`),
on the main scene's Robot as a `fynkel.AudioSource` (`sfx`, volume 0.8,
`playing: true` — one voice that starts at the load and ends at tick 59)
with a `fynkel.AudioListener` on its Camera, both in the registry — the
project validates as 11 documents, 6 sources, 31 references and cooks 17
assets).
`scene/physics_rules.scene.json` gained a `Solid zone` entity (a
`TriggerZone` with an empty event on a non-sensor collider: `H0008` and
the `H0009` warning). Every kind added later extends this corpus.

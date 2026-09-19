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
| `formatVersion` | integer ≥ 1. Newer than the tool supports → `FYN-D0013`, the document is **read-only**: it can be read and validated, never rewritten. |
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
codegen; append only.

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
| `FYN-D0205` | unknown component type (warning; `data` is kept opaque) |
| `FYN-D0206` | component `data` violates the type's schema (per field) |
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

## Scene → runtime

`fynkel_document::describe(&Scene)` turns a validated scene into the typed
`fynkel_core::SceneDescription` (entities in document order, each with its
`ComponentValue`s); unknown component types are skipped with `FYN-D0205`,
undecodable data is `FYN-D0206`. `World::load_scene(&description)` then
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

## CLI

```sh
fynkel document validate <path>... [--json] [--schemas <dir>] [--kind <kind>]
fynkel document format <path>... [--check] [--json]
fynkel document new <kind> <name> [--out <path>] [--json]
fynkel scene inspect <path> [--json] [--schemas <dir>] [--prefabs <dir>] [--expand]
fynkel scene validate <path>... [--json] [--schemas <dir>] [--prefabs <dir>]
fynkel prefab inspect <path> [--json] [--schemas <dir>]
fynkel prefab validate <path>... [--json] [--schemas <dir>]
fynkel material inspect <path> [--json] [--schemas <dir>] [--shader <manifest>]...
fynkel material validate <path>... [--json] [--schemas <dir>] [--shader <manifest>]...
fynkel shader inspect <path> | --builtin <shaderId> [--json]
fynkel shader validate <path>... [--json] [--schemas <dir>]
fynkel test --headless --scene <path> [--prefabs <dir>] [--steps N] [--json]
fynkel state dump --scene <path> [--prefabs <dir>] [--steps N] [--json]
```

`scene inspect` prints the entity tree (or `{ kind, id, name, formatVersion,
entityCount, instanceCount, componentCount, componentTypes, roots[],
expanded }` with `--json`) followed by the diagnostics; anchors show their
prefab, instance id and override count, `--expand` shows the expanded scene
instead. Scenes with instances resolve prefabs from `--prefabs <dir>`
(default: the scene's own directory) and report override problems in
`validate`. `prefab inspect` prints roots and the tree. `material inspect` prints the resolved material (every
manifest parameter with its value, `(default)` where the material did not set
it; every slot with its binding or fallback); `--shader` adds custom manifests
to the built-in library (a manifest with errors is reported and left out).
`shader inspect --builtin fynkel.pbr@1` shows the built-in interface.

Exit codes: 0 ok, 1 errors (or drift for `format --check`), 2 usage.
`--json` prints `{ "ok": bool, "diagnostics": [ ...records... ] }`
(`format` adds `"drift": [paths]`).

## Fixtures

`crates/fynkel_document/fixtures/`: `valid/` (parse, validate, canonical
fixed point), `unknown/` (unknown keys and lexemes byte-for-byte, hoisting
expectations in `*.expected.json`), `invalid/` (exact code/pointer/line/
column expectations in `*.expected.json`), `scene/` and `material/` (semantic
expectations per kind; `material/` also holds custom manifests the material
fixtures reference). Every kind added later extends this corpus.

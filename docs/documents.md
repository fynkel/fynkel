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

## CLI

```sh
fynkel document validate <path>... [--json] [--schemas <dir>] [--kind <kind>]
fynkel document format <path>... [--check] [--json]
fynkel document new <kind> <name> [--out <path>] [--json]
fynkel scene inspect <path> [--json] [--schemas <dir>]
fynkel scene validate <path>... [--json] [--schemas <dir>]
fynkel test --headless --scene <path> [--steps N] [--json]
fynkel state dump --scene <path> [--steps N] [--json]
```

`scene inspect` prints the entity tree (or `{ kind, id, name, formatVersion,
entityCount, componentCount, componentTypes, roots[] }` with `--json`) followed
by the diagnostics.

Exit codes: 0 ok, 1 errors (or drift for `format --check`), 2 usage.
`--json` prints `{ "ok": bool, "diagnostics": [ ...records... ] }`
(`format` adds `"drift": [paths]`).

## Fixtures

`crates/fynkel_document/fixtures/`: `valid/` (parse, validate, canonical
fixed point), `unknown/` (unknown keys and lexemes byte-for-byte, hoisting
expectations in `*.expected.json`), `invalid/` (exact code/pointer/line/
column expectations in `*.expected.json`). Every kind added later extends
this corpus.

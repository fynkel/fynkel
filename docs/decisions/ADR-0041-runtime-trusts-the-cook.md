# ADR-0041: The shipped runtime trusts the cook and does not link JSON Schema

**Status:** Accepted 2026-09-29 (the user agreed to both questions: trust the cooked cache, and a Cargo feature; docs/foundation.md 6.4)  
**Date:** 2026-09-29  
**Related:** ADR-0029 (package architecture, the stripped runtime), ADR-0038 (LoadSceneAsync), [documents.md](../documents.md) § The cook, [foundation.md](../foundation.md) 6.4

## Context

A shipped game reads only the cooked cache. The cook has already checked
every document against its JSON Schema and the semantic rules. The cache
is refused when its fingerprint does not match the runtime's, so the
runtime never reads a cache from a different set of schemas.

Even so, the runtime checks everything again. `Assets::scene` and
`Assets::prefab` (`fynkel_assets`) do this on every load:

1. parse the document JSON (`Document::parse`);
2. build `SchemaSet::embedded()`, which parses and compiles every
   embedded schema (the envelope, every document kind, every component);
3. run `scene.validate(&schemas)`, the schema pass plus the semantic
   rules (scenes only; a prefab load skips this step but still builds
   the set for step 4);
4. run `describe(&scene, &schemas)`. This decodes the components. It
   uses the schema set for one thing only: to ask whether a type it
   does not know is a declared behaviour. The cache already carries
   that list, in `scene_info.behaviourTypes`.

Measured on this Mac (release build, `playground.scene.json`, 163
entities, three runs):

| Step | Time |
| --- | --- |
| build `SchemaSet::embedded()` | 5.4 – 10.5 ms |
| parse the document | 1.0 – 1.1 ms |
| validate | 1.8 – 3.8 ms |
| describe | 0.07 – 0.14 ms |

So about 8 – 14 ms of each 9 – 15 ms scene load is checking work that
the cook has already done. The same happens for every prefab the first
time it is loaded. A phone is several times slower than this Mac.

The runtime also carries the code for it. `fynkel_native`, built with
only its runtime features, still links `fynkel_document`, and so links
`jsonschema`. The `jsonschema` dependency tree has 66 crates
(`fancy-regex`, `fraction`, `num-*`, `referencing`, `email_address`,
and more), some of them shared with other crates. The release
`libfynkel_native.dylib` is 13.0 MB today.

`fynkel_native` uses these from `fynkel_document` at run time:

- `Material::parse` (runtime shader materials);
- `diagnostic_json`;
- `generated::encode_component` and `asset::asset_refs` (the
  persistent-object preload, ADR-0039).

None of these needs JSON Schema.

## Decision

### 1. A cooked load does not validate

`Assets::scene` and `Assets::prefab` parse the document and describe
it. They do not build a `SchemaSet` or call `validate`. `describe`
takes the declared behaviour types, a plain list read from
`scene_info`, instead of a `SchemaSet`. The editor and the CLI keep
their `describe(&scene, &schemas)` through a small wrapper, so they
behave as before.

A damaged chunk is still caught. The parse fails, or `describe`'s
component decode fails, and the load returns `FYN-R0008` (recook) as
it does today. The warning for an unknown component type
(`FYN-D0205` "is not instantiated") is still given, by the cook and by
`describe`.

### 2. JSON Schema is an authoring feature of `fynkel_document`

`fynkel_document` gets a Cargo feature, `validate`, on by default. It
holds `SchemaSet`, every `validate` method, `migrate`, the text diffs,
and the `jsonschema` and `similar` dependencies. Without it, the crate
still parses documents, reads the model types, describes scenes and
prefabs, and encodes components.

`fynkel_assets` depends on `fynkel_document` with
`default-features = false`. The CLI, `fynkel_authoring` and the editor
turn `validate` on, so nothing changes for them. `fynkel_native`
without `authoring` (the shipped runtime) then links no JSON Schema
code.

### 3. What does not change

- The cook checks every document, as today.
- `fynkel validate`, the editor's Inspector and Save checks, and
  migration are unchanged.
- There is no new public API and no ABI change. The cache format and
  the fingerprint stay the same.

## Consequences

- A scene load in a shipped game drops from about 9 – 15 ms to about
  1 – 1.5 ms on this Mac. The same saving applies to each first load of
  a prefab. LoadSceneAsync (ADR-0038) does less work on its thread.
- The shipped library loses the `jsonschema` tree. I will measure the
  new size when step 2 is done; I do not want to guess the number.
- A cache that was changed by hand after the cook is no longer checked
  against the schemas. It is read as long as it decodes. This is how
  Unity treats its built player data. The fingerprint still refuses a
  cache from another build.
- `fynkel_document` gets `#[cfg(feature = "validate")]` on its
  `validate` items, in about 17 files. CI builds the crate without the
  feature, so a gate that is missing breaks the build straight away.

## Options not taken

- **Keep validating, but cache the `SchemaSet`** (build it once per
  process). This saves the 5 – 10 ms build after the first load, but
  keeps the 2 – 4 ms check on every load and keeps `jsonschema` in the
  shipped library.
- **A separate `fynkel_validate` crate.** This is cleaner in the long
  run, but it moves about 17 files' `validate` code and every caller's
  imports for the same result. The feature can become a crate later if
  the gates get in the way.
- **Cook scenes into a binary description**, so the runtime does not
  parse JSON at all. This saves the remaining ~1 ms, but it is a cache
  format change. It is a Phase 4 item (size and speed on devices), not
  a structure fix.

## Questions for the user (answered)

1. Do you agree that the shipped runtime can trust the cooked cache
   (checked by the cook and by the fingerprint) and skip the schema
   check? I recommend yes. **Yes.**
2. Should it be a Cargo feature (my recommendation) or a separate crate?
   **A Cargo feature.**

## Plan

Each step starts with a test that fails first.

1. **The load does not validate.** Failing test (`fynkel_assets`): a
   cooked scene whose component data breaks a schema rule, but still
   decodes, loads. Today the load refuses it. `describe` takes the
   behaviour list. Load time measured again.
2. **The `validate` feature.** Failing test: a CI check
   (`tools/check_runtime_deps.sh`) that fails when
   `cargo tree -p fynkel_native --no-default-features -e normal`
   lists `jsonschema` or `similar`. `fynkel_document` gets the feature
   and `fynkel_assets` turns it off. Library size measured again.
3. **Docs.** `docs/documents.md` § The cook, `docs/foundation.md` 6.4,
   and this ADR's Progress.

## Progress

- **Step 1 done.** The failing test was
  `fynkel_assets/tests/store.rs`
  `a_cooked_scene_loads_without_the_schema_check`: a cooked scene with
  an extra field in its `AudioListener` (refused by the schema,
  skipped by the decoder) failed with `FYN-D0100`. `describe_with` takes
  the behaviour types as a test; `describe` wraps it for the editor and
  the CLI. `Assets::scene` and `Assets::prefab` build no `SchemaSet`.
  `Assets::scene` on `playground.scene.json` (163 entities, release, a
  `git archive` copy of the reference game): 8.7 – 12.7 ms before,
  1.7 – 2.0 ms after.

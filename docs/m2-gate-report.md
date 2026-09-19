# M2 gate report and go/no-go

**Date:** 2026-09-19 · **Milestone:** M2 (blueprint §16 "M2 — Project documents, assets, prefabs, materials, and behaviours") · **Verdict: GO — proceed to M3.**

M2 asked whether a game's data can live in plain files that tools, agents and
people edit without an editor database in the way: strict documents with
exact diagnostics, stable identity for entities and assets, prefabs that
expand deterministically, an import and cook path that is content-addressed,
and behaviours that go from Dart source to schema to a running instance. The
seventeen backlog items (§19.2c) landed one commit each; this report is item
18. Everything below is reproducible from a clean clone with the steps in
`docs/status.md` §5, and the timings with `python3 benchmarks/m2/capture.py`
(profile `mac-m3max-2026`). The capture for this report is
`benchmarks/m2/results/2026-09-19-68e9a72.md`.

## 1. Gate criteria

| Gate | Result | Evidence |
| --- | --- | --- |
| Documents round-trip without losing unknown/valid data | **Pass.** The strict parser keeps every key with its span; the canonical writer reproduces a valid document byte for byte, and unknown keys and lexemes (extra top-level keys, unhoisted members, unknown component types) survive parse → write unchanged, in value and position. Every document the tools write (`document new`, `migrate`, `scene apply`, `fynkel create`, the importer's material and prefab documents) validates when read back. | `crates/fynkel_document/tests/corpus.rs` — `valid_documents_parse_validate_and_are_canonical` (4 documents: scene, prefab, 2 materials), `unknown_data_round_trips_byte_for_byte` (`fixtures/unknown/`, 2 documents), `new_documents_are_valid_and_canonical`, `newer_documents_are_read_only`; `authoring.rs::every_transaction_undoes_to_the_exact_original_text`; `gltf.rs::robot_material_documents_validate_against_the_pbr_manifest`, `robot_prefab_mirrors_the_node_hierarchy_and_validates` |
| Scene IDs remain stable through edits | **Pass.** Ids are authored once (UUID v4) and never regenerated: authoring commands edit by JSON pointer and undo to the original bytes; migration rewrites only what the step names; the canonical writer reorders keys by schema, never ids; a re-import keeps the sub-asset ids the sidecar already carries and reports the ones the file no longer produces instead of minting new ones. `fynkel state dump` reads the runtime back by authoring id after the world has ticked. | `authoring.rs` (15 tests: every §11.1 command applied and undone, `create_and_delete_maintain_the_hierarchy`, `reparent_refuses_cycles_and_unknown_entities`, `stale_base_revision_is_a_conflict_and_nothing_is_touched`); `migrate.rs::v0_scene_migrates_to_v1_exactly`; `gltf.rs::sidecar_ids_that_the_file_no_longer_produces_are_reported`; `apps/cli/tests/check.rs::scene_documents_load_headlessly_and_dump_by_id`, `import_writes_intermediate_and_maintains_the_sidecar` |
| Asset moves preserve identity | **Pass.** Identity is the `.meta.json` sidecar's id, not the path: moving a source with its sidecar keeps every reference resolving (`resolve` by id and sub-id), the database reports a sidecar without its file and a file without a sidecar exactly, and cache keys are content hashes (a moved, unchanged source is a cache hit; a renamed file with a changed sidecar re-cooks only its dependents). | `crates/fynkel_document/tests/asset.rs::moving_a_file_with_its_sidecar_preserves_identity`, `references_resolve_by_id_and_sub_id`, `scan_reports_exact_diagnostics_and_indexes_the_rest`; `crates/fynkel_import/tests/cook.rs::editing_import_settings_invalidates_the_same_set`, `a_clean_corpus_cooks_then_hits` |
| Prefab references remap correctly | **Pass.** Instance expansion derives every prefab-local entity and component id as UUID v5 in the instance's namespace (stable per instance, distinct across instances), remaps internal references (`parent`, `children`, override targets), shares external asset references, applies the ordered override list, and refuses nested instances and cycles. The expected expansion is a committed fixture compared byte for byte. The fixture project's `Crate A` instance (with a `setField` override) loads through `load_project`, the C ABI and `World.loadProject`. | `crates/fynkel_document/tests/prefab.rs::expansion_is_exact_and_deterministic` (`instances_ok.scene.expanded.json`), `derived_ids_are_stable_and_distinct_per_instance`, `scene_fixtures_validate_and_expand_with_exact_diagnostics`, `prefab_fixtures_report_exact_diagnostics` (nested, roots mismatch, unknown prefab, duplicate instance, bad overrides); `authoring.rs::instantiate_and_override_prefabs`; `crates/fynkel_native/tests/load.rs::project_and_scene_load_through_the_abi` |
| Invalid fields produce exact diagnostics | **Pass.** 30 `*.expected.json` fixtures pin code, severity, message, pointer, id, line and column for every diagnostic a malformed input produces (7 invalid documents, 11 scene semantics, 6 prefab, 5 material/shader, 1 unknown-data), and the tests fail on any extra or missing record. Importer and cache diagnostics are pinned by code in their tests. Codes are namespaced and documented (`FYN-D`, `-A`, `-P`, `-E`, `-B`, `-G`). | `corpus.rs::invalid_documents_report_exact_diagnostics`, `schema_violations_carry_pointer_and_position`; `scene.rs::scene_fixtures_report_exact_semantics`; `material.rs::material_fixtures_report_exact_diagnostics`, `shader_fixtures_report_exact_diagnostics`; `project.rs` (11 tests, codes `FYN-P0001`–`P0012` per field); `gltf.rs`, `media.rs`, `cook.rs` (`codes()` helpers); `docs/documents.md` code tables |
| A compiled Dart behaviour can be instantiated from authored data | **Pass.** `@FynkelBehaviour(id: 'game.Spinner')` with `@Expose` fields → `fynkel codegen --project` → `behaviours.g.dart` (factory, field get/set), `behaviours.g.json` and `schemas/behaviours/game.Spinner.schema.json`; the fixture scene's `game.Spinner { speed: 2.5 }` validates against that schema (`FYN-D0206` on bad data), travels in the load report and comes back from `World.loadProject` as a `Spinner` whose `speed == 2.5` and which rotates its entity every tick. The same holds for the `fynkel create` project's cube (`speed: 1.5`) under `flutter test`. Unregistered types and rejected data are warnings (`FYN-B0001`/`B0002`), not failed loads. | `packages/fynkel_generator/example/spinner_game/test/spinner_test.dart` (4 tests), `packages/fynkel_generator/test/emit_test.dart` (5), `packages/fynkel/test/behaviour_test.dart` (11: lifecycle, ordering, registry); `apps/cli/templates/create/test/game_test.dart.tmpl`; `codegen --check --project` in `apps/cli/tests/check.rs` and CI |
| Project can be validated/imported headlessly | **Pass.** `fynkel validate` walks the project file, lock, registry, asset database, every document and every reference (exit 0/1/2, `--json`); `fynkel import`, `fynkel cook` and `fynkel test --headless --project` need no window, GPU or Dart VM. The template project: `fynkel create` → `validate` (0 errors, 0 warnings) → `test --headless` (3 entities, 120 ticks, 0 diagnostics) → `state dump` → `codegen --check --project .` → `flutter analyze` → `flutter test`, in `tests/create.rs` and in CI on a clean runner. The fixture project is the same path plus `cook` and `render` to a PNG. | `apps/cli/tests/create.rs::a_fresh_project_validates_and_runs_headlessly`; `check.rs::validate_project_describe_lock_and_schema_commands_fix_their_exit_codes`, `cook_fills_the_cache_and_cooked_inspect_reads_it`; `project.rs::the_fixture_project_reads_and_validates_clean`, `broken_references_and_documents_under_the_asset_root_fail_validate`; `.github/workflows/ci.yml` (dart job) |

No hard-kill criterion was hit. The "do not" list held: no editor database
(every byte of project state is a file under version control), no nested
prefab inheritance (`FYN-D0402` refuses it), no shader graph (materials are
parameter sets over a shader manifest), and no editor UI — every mutation the
editor will make already exists as a `--json` command (ADR-0017).

## 2. Corpus results

Clean clone, commit `68e9a72` plus this report's changes, Rust 1.93.0,
Flutter 3.47.2 / Dart 3.13.2, M3 Max.

| Suite | Count | Result |
| --- | --- | --- |
| `cargo test --workspace` | 189 tests (fynkel_document 55 across 9 integration files + 4 unit; fynkel_import 37; fynkel_cli 16; the runtime crates and hosts the rest) | all pass; 2 doc-tests ignored by design (`fynkel_schema_derive` examples) |
| Document fixture corpus | `fixtures/valid` 4, `unknown` 2, `invalid` 7, `scene` 11, `prefab` 6 (+1 expanded), `material` 5 (+1 shader), `migrate` 4, `assets` 5, `project` 1 with its lock — 30 exact-diagnostic expectations | every expectation matches; the committed lock equals `expected_lock()` for this build |
| Import fixture corpus | glTF: `robot.glb` + `.bin`/data-URI/external variants, `cycle`, `draco`, `strip`; textures: RGBA/RGB/gray/palette/16-bit PNG, JPEG, GIF (refused); audio: 8/16/24-bit WAV, Vorbis mono/stereo, FLAC (refused), truncated; 3 committed `.expected.fyni` intermediates | byte-identical intermediates; every refusal is a pinned code |
| `fynkel codegen --check` (pipeline a) | 12 generated files, fingerprint `0xa3c3bd56d1793195` | up to date |
| `fynkel codegen --check --project` (pipeline b) | `spinner_game` (2 behaviour types), the starter project (1) | up to date |
| Dart | `package:fynkel` 49, `fynkel_generator` 5, `spinner_game` 4, starter project `flutter test` 1; `dart analyze --fatal-infos` / `flutter analyze` clean on all four | all pass |

Clean-clone wall clock on the reference machine: 26 s clippy, 109 s
`cargo test` (debug build included), 36 s `dart test` for `package:fynkel`
(builds the native-assets hook), 21 s for the starter project's first
`codegen --check` (build_runner AOT), 5 s `flutter test`; ≈5 min for the
whole gate.

## 3. Timings (`benchmarks/m2/capture.py`)

Release `fynkel` binary, wall clock from a shell (the number a person or an
agent waits for), min / p50 over 10 runs; process start-up (`fynkel schema
list`, 8 ms) is included in every row. Synthetic inputs are generated by the
script so the numbers reach beyond the hand-written fixtures.

| Command | Input | min / p50 |
| --- | --- | --- |
| `fynkel validate` | fixture project (3 documents, 3 sources incl. the sidecar of a 2 MiB mesh) | 11.7 / 12.5 ms |
| `fynkel validate` | `fynkel create` project (2 documents, 2 sources) | 11.6 / 12.2 ms |
| `fynkel scene validate` | synthetic scene, 1 000 entities (2 500 components, 1 MiB) | 26.2 / 26.9 ms |
| `fynkel scene validate` | synthetic scene, 10 000 entities (25 000 components, 10 MiB) | 238 / 242 ms |
| scene load (`test --headless --steps 0`: parse, validate, expand, describe, spawn) | fixture startup scene (4 entities incl. a prefab instance) | 10.8 / 11.7 ms |
| scene load | `fynkel create` startup scene (3 entities) | 10.7 / 11.3 ms |
| scene load | synthetic scene, 1 000 entities | 26.8 / 27.2 ms |
| scene load | synthetic scene, 10 000 entities | 254 / 255 ms |
| `fynkel import` | `robot.glb` (4 KB, 3 meshes, 2 materials, 1 image, a 9-node prefab) | 9.2 / 9.8 ms |
| `fynkel import` | generated grid (2.1 MiB, 40 000 vertices, 79 202 triangles, u32 indices) | 14.8 / 15.5 ms |
| `fynkel cook` miss | empty cache, 6 assets incl. the grid | 19.7 / 20.3 ms |
| `fynkel cook` hit | unchanged (6 cached) | 11.6 / 12.1 ms |
| `fynkel cook` partial | one texture's bytes changed (4 re-cooked: texture, material, prefab, scene; 2 cached) | 12.9 / 13.2 ms |
| `fynkel codegen --project` | cold: no `.dart_tool` (pub get, build_runner AOT compile) | 22 s |
| `fynkel codegen --project` | warm | 1.0 s |
| `fynkel codegen --check --project` | warm | 1.0 s |
| `fynkel create --no-flutter` | starter project (22 files, 2 imports, fresh ids) | 9.2 / 9.3 ms |

Read: validation and loading cost ≈20–25 µs per entity and scale close to
linearly to 10 000 entities (10× the entities → 9× validate, 9.5× load);
the whole fixture pipeline (validate + import + cook + load) is under 60 ms,
so an agent's validate→codegen→build→test loop is bounded by `build_runner`
(1 s warm) and `flutter test` (5 s), not by Fynkel's own tools. Two things
this capture found and item 18 fixed: `read_scene` (behind `--scene`,
`fynkel_world_load_scene`, `World.loadScene`) validated against the embedded
schemas only, so a scene inside a project warned `FYN-D0205` for every
behaviour component the project knows (5 000 warnings on the 10 000-entity
scene, which loaded in 407 ms) — it now picks up the enclosing project's
schemas like the scene commands do, and the load is the 254 ms above; and
`load_scene`'s diagnostic de-duplication was a linear scan per warning
(quadratic when a scene really has thousands of unknown components: 16 000
entities with 8 000 warnings loaded in 860 ms, now 726). What remains
super-linear there is `Node::pointer` — a linear member scan per lookup, so
a document with thousands of *diagnostics* pays O(n) per pointer. It does
not affect valid documents and is carried over (§5).

## 4. What M2 delivered

- **Documents (`fynkel_document`, ADR-0016).** One strict JSON reader with
  spans, the envelope (`$schema`, `formatVersion`, `id`, `name`), JSON
  Schema 2020-12 validation with `$ref`s resolved from the committed
  `schemas/`, the §14.2 diagnostic record everywhere, canonical writing with
  unknown data preserved, and the document kinds: scene, prefab, material,
  shader manifest, `.meta.json` sidecar, project, lock. Every native
  component's schema is generated from Rust (fingerprinted); behaviour
  schemas are generated from Dart; the five envelope/model schemas are
  hand-written and validated by the same code.
- **Identity.** Entities and components carry UUID v4; assets carry theirs
  in the sidecar with importer-derived sub-asset ids; prefab instances derive
  UUID v5; references are `{ asset, sub? }`, never paths. The asset database
  scans, resolves and cross-checks all of it.
- **Import and cook (`fynkel_import`, `fynkel_cooked`).** glTF/GLB (static
  meshes, `fynkel.pbr@1` materials, images, node hierarchy as a prefab),
  PNG/JPEG, WAV/Ogg Vorbis to deterministic intermediates; a content-addressed
  cache keyed by blake3 over source, settings, versions, dependencies and
  profile; the `FYNC` container with CRC-checked chunks and a
  dependency-free reader the renderer already uses (`MESH`, `TEXR`, `JSON`).
- **Project (`fynkel.project.json` + lock).** Startup scene, asset root,
  component registry, input actions, quality profiles and targets, validated
  as a whole by `fynkel validate`; the lock pins runtime/tool/format versions
  and the schema fingerprint per field.
- **Authoring commands (§11.1).** `Editor` over one document: revision =
  blake3 of the canonical text, thirteen commands plus `unsetField` compiled
  to reversible edits, atomic transactions with conflict detection,
  rollback to the byte on any new error, byte-exact undo/redo; driven by
  `fynkel scene|prefab apply --patch`.
- **Behaviours.** The `Behaviour` lifecycle (§3.2) driven by `World.frame` at
  the §5.1 points, `GameContext` services, the registry, and the
  `build_runner` generator that fills it from `@FynkelBehaviour`/`@Expose`
  (ADR-0012 pipeline b) — with schemas the document layer validates against.
- **Tooling for agents (ADR-0017).** Every command with `--json` and fixed
  exit codes, usable outside the checkout on the embedded schemas;
  `fynkel create` writes a Flutter app with a project, assets, one behaviour,
  a test and `AGENTS.md`; `fynkel run` validates and launches it.
- **Carried over from M1 and closed here:** C1, allocation-free transform
  writers (`TransformWriter`, `setPosition`/`setRotation`, item 1).
- **Pulled forward from M3:** the scene-on-screen spike (item 13a) — cooked
  meshes and materials drawn from the scene's own camera in the Flutter host
  and by `fynkel render`, so every later authoring item was checked against
  pixels.

## 5. Known gaps carried forward

Tracked as "M2 carry-over" D1–D8 in the blueprint (§19.2c), each with an
owning milestone. C2–C6 from M1 stand as listed there.

- **D1 — `package:fynkel_flutter` (M3, first).** A created project has no
  viewport: the Path A viewport lives in `hosts/flutter_apple` (Swift in the
  Runner + `viewport_ffi.dart`), not in a package a game can depend on. The
  template shows the simulation as text until the widget exists. Its hook
  lock (`packages/fynkel/fynkel.lock.json`, snake_case, artifacts) is a
  different shape from the project lock; reconcile when artifacts are
  published (ADR-0015, C6).
- **D2 — Scenes load from source, not from the cache (M3).** `load_project`
  reads `.scene.json`/`.prefab.json` from the asset root and expands prefabs
  at load; the cooked scene chunk is unexpanded canonical JSON nobody reads.
  A shipped game would still need its source documents. Cook the expanded
  scene and make the loader read only the cache.
- **D3 — Unread project fields (M3).** `inputActions`, `quality` and
  `targets` are validated and nothing consumes them; input waits for the
  §5.1 contract (C2), quality for renderer V1.
- **D4 — Cooked audio has no consumer (M3).** `CLIP` chunks are produced and
  verified, never played.
- **D5 — Authoring commands have no ABI (M4).** `Editor` exists in Rust and
  behind `fynkel scene apply`; no C ABI or Dart surface exposes it, and the
  generated Inspector metadata (`label`, `tooltip`, `min`/`max`/`step`) is
  read by nothing until the editor.
- **D6 — Hand-written model schemas (M3).** `material`, `shader`, `meta`,
  `project` and `lock` schemas are hand-written and their Rust models are
  validation-only; when renderer V1 needs a Rust `Material`, generate the
  schema from it like the components.
- **D7 — Pointer lookup is linear (M4, before the editor scales).**
  `Node::pointer` scans object members; fine for documents, quadratic for a
  document with thousands of diagnostics (§3). Index members by key when the
  Inspector starts resolving pointers per keystroke.
- **D8 — Cache hygiene (M6).** No garbage collection beyond `cook --prune`;
  textures are RGBA8 with one mip level for every profile — mipmaps,
  compression and per-profile transforms are renderer/pipeline work.
- Migrations have one synthetic step (`v0 → v1`); no real format change has
  been through them yet. The first real one should add its fixture pair.
- ADR-0005 (license) stays open; the repository stays private through M3's
  gate, where it is due.

## 6. Decision

**GO.** M3 (code-first reference game, renderer V1) starts on the M2 data
layer as is: `package:fynkel_flutter` first (D1), so the starter project
shows pixels, then materials and lights against the picture the spike
already produces, cooked-scene loading (D2), input (C2, D3) and audio (D4).
D5–D7 are owned by M4, D8 by M6.

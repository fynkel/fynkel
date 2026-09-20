# Schema codegen (ADR-0012)

## Native pipeline (a)

Rust structs are the source of truth for every record that crosses the bridge.

```text
#[derive(Schema)] structs                      fynkel_core::schema::ALL (registry)
  fynkel_scene::Transform, fynkel_core::Name,          │
  fynkel_core::components::{Camera, MeshRenderer, …}   │
  fynkel_core::schema::{*Command, *Event, *Record}     ▼
                                              `fynkel codegen`  (apps/cli)
                                                       │
        ┌──────────────────────────────┬───────────────┴───────────────┐
        ▼                              ▼                               ▼
 schemas/fynkel.schema.json    packages/fynkel/lib/generated/   crates/fynkel_wire/src/generated.rs
 schemas/components/*.json     schema.g.dart (Dart codecs,      (Rust codecs, SCHEMA_FINGERPRINT,
 schemas/scene.schema.json      copyWith, componentCodecs/Ids)   ComponentValue wire dispatch)
                                                                crates/fynkel_document/src/generated.rs
                                                                (JSON ↔ typed component codecs,
                                                                 embedded schemas, ComponentValue dispatch)
```

* `fynkel_contracts::schema` defines the metadata (`RecordSchema`, `FieldType`), the wire layout rule (little-endian, natural alignment, records padded to 8, strings as `(offset, len)` into a string tail) and the fingerprint (FNV-1a over the canonical text of the registry).
* `fynkel_schema_derive` provides `#[derive(Schema)]` with `#[schema(kind = "component" | "command" | "event" | "record", id = N, name = "fynkel.X")]`. Supported field types: `f32 u32 i32 u64 bool Vec3 Vec4 Quat Entity Option<Entity> String AssetRef Option<AssetRef>`, nested schema structs, and unit-variant enums that also `#[derive(Schema)]` (`#[schema(name = "fynkel.Projection")]`). An enum is a `u32` declaration index on the wire and the lowerCamel variant name in JSON (`"perspective"`) and Dart (`Projection.perspective`); the variant list is part of the fingerprint, so append only. `AssetRef` is 16 id bytes + `sub` as a wire string (24 bytes), `{ "asset": uuid, "sub"? }` in JSON (`envelope.schema.json#/$defs/assetRef`), and `AssetRef(asset, sub:)` in Dart; `Option<AssetRef>` is the same 24 bytes with a nil id for `None`, `assetRef | null` in JSON (never required) and `AssetRef?` in Dart; `Vec4` is 16 bytes / a 4-number array; an unknown enum index or malformed id fails decoding/encoding loudly on both sides. Doc comments flow into JSON Schema and Dart docs. `authored = false` marks runtime-only records (`fynkel.Name`): they get no `data` binding in `scene.schema.json` and scene validation rejects them (`FYN-D0207`). `document = "<kind>"` on a `record` marks a **document record** — the top level of a document kind (`fynkel.PbrMaterial` → `.material.json`, M3-02): every field is optional in the document and takes the Rust `Default` (the generated `from_node` reads through `or_default`; `generated::document_defaults()` exposes the defaults so the schema carries them as `default`), and codegen writes `schemas/<kind>.schema.json` from it (for material: the envelope, `shader`, the record's fields, plus the custom-shader `parameters`/`textures` groups and the `$defs` they need; `x-fynkel.document` names the kind). `document` is not part of the fingerprint; the record's wire layout is.
* Generated files are **committed**. `cargo test -p fynkel_cli` runs `fynkel codegen --check` and fails on drift; regenerate with `cargo run -p fynkel_cli -- codegen`.
* Protocol fixtures: `tests/fixtures/wire/<fingerprint>/<Record>.bin` are written by the Rust codecs (`FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_wire`) and decoded + re-encoded byte-for-byte by the Dart codecs (`dart test` in `packages/fynkel`). A schema change produces a new fingerprint directory; delete the old one in the same commit.

Adding a built-in component: give it a number in `fynkel_contracts::ids::ComponentId` (the registry test checks the two agree), add the Rust struct with `#[derive(Schema)]`, append it to `ALL`, add it to `fynkel_core::components::ComponentValue` (its `id`/`validate` arms and the world's `set_component`/`RemoveComponent` arms — the generated wire dispatch `fynkel_wire::{decode_component_value, encode_component_value, component_value_size}` and the Dart `componentCodecs`/`componentIds` tables follow from `ALL`, so `GameObject.get<T>()`/`set` and `FYNKEL_QUERY_COMPONENTS` need one more `ecs.get::<T>` arm in `bridge::query`, M3-07a), run codegen, add a sample to both wire fixture tests and regenerate (`FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_wire`; the old fingerprint directory goes), bump `schema_fingerprint` in `packages/fynkel/fynkel.lock.json`, commit all of it. Starter components (M2-04): `fynkel.MeshRenderer` (4), `fynkel.Camera` (5, `Projection` and `ExposureMode` enums), `fynkel.DirectionalLight` (6), `fynkel.PointLight` (7); M3-04 added `fynkel.SpotLight` (8) and `fynkel.Environment` (9, an `OptionalAssetRef` sky) in `fynkel_core::components` — the renderer reads all of them (`docs/renderer.md`). A field added to an existing component is a required schema field, so it needs a document migration (`scene/1-2/camera-exposure` is the model); bootstrapping codegen after adding one means hand-patching the generated Rust codecs so the CLI compiles, then regenerating.

## Behaviour generator (ADR-0012 pipeline (b), M2-16)

The game's Dart classes are the source of truth for behaviour types. Nothing
is discovered at runtime and nothing uses mirrors: a `build_runner` builder
in `packages/fynkel_generator` turns every `@FynkelBehaviour` class into
ordinary Dart the game registers explicitly, plus the JSON Schemas the
document layer validates scene data against.

```text
@FynkelBehaviour(id: 'game.Spinner') class Spinner extends Behaviour {
  @Expose(min: 0, step: 0.1) double speed = 1.0;              lib/**.dart of the game package
  @Expose() Vec3 axis = Vec3.up; …                                    │
}                                                                     ▼
                              `fynkel codegen --project <package>`  → `dart run build_runner build`
                                                                      │
              ┌───────────────────────────────────────────────────────┴──────────────┐
              ▼                                                                      ▼
 lib/generated/behaviours.g.dart                                    lib/generated/behaviours.g.json
   gameSpinnerType = BehaviourType(id, version, create,               per type: id, version, class, library,
     fields: [BehaviourField(name, type, defaultValue,                fields (type, default, min/max/step,
       min/max/step/label/tooltip, get, set)])                        label/tooltip, description) + `schema`
   behaviourTypes, registerBehaviours(registry)                                    │  (split by the CLI)
                                                                                   ▼
                                                                    schemas/behaviours/<id>.schema.json
```

* **Package:** `packages/fynkel_generator` (deps `build`, `source_gen`, `analyzer`, `dart_style`, `glob`). Its `build.yaml` auto-applies the `behaviours` builder to every package that depends on `fynkel_generator` (a dev dependency of the game), `build_to: source`. The builder is an aggregate over the placeholder input `$lib$`: it resolves every library under `lib/` (skipping `lib/generated/` and `*.g.dart`), reads the annotations through `source_gen`'s `TypeChecker`/`ConstantReader` and the analyzer's element model, reads field initializers from the AST for defaults, and writes the two outputs once per package. Output is sorted by type id and formatted with `dart_style`, so it is byte-stable.
* **Rules the builder enforces (build errors that name the file and member):** the class extends `Behaviour`, is public and concrete, has an unnamed constructor without required parameters; `id` is namespaced (`game.Spinner`; `fynkel.` is reserved) and unique in the package; `version >= 1`; an `@Expose` field is a public, assignable instance field (inherited fields up to `Behaviour` count too) of type `double`, `int`, `bool`, `String`, `Vec3`, `Quat` or `AssetRef`, optionally nullable.
* **Field wire form** (`BehaviourField.type` / schema): `double` ↔ `number`, `int` ↔ `integer` (a non-integral JSON number is rejected), `bool` ↔ `boolean`, `String` ↔ `string`, `Vec3` ↔ `vec3` = `[x, y, z]`, `Quat` ↔ `quat` = `[x, y, z, w]`, `AssetRef` ↔ `assetRef` = `envelope.schema.json#/$defs/assetRef`. `get` returns the JSON form, `set` converts from it and throws on a value that does not fit (the registry wraps that in `BehaviourDataError`). A literal initializer (`1.0`, `-2`, `true`, `'x'`, `Vec3(0, 1, 0)`, `Vec3.up`, `Quat.identity`, `AssetRef('…', sub: '…')`, `AssetRef.none`, `null`) becomes `defaultValue` and the schema's `default`; any other initializer still applies at runtime (the class sets it) but is not in the metadata.
* **Schema:** `schemas/behaviours/<id>.schema.json` is a JSON Schema 2020-12 for the component's `data` — every field optional (the class default applies), `additionalProperties: false`, `minimum`/`maximum` from `@Expose(min:, max:)` on numbers, nullable fields as `["number", "null"]` (or `anyOf` with `null` for `assetRef`), Inspector metadata under each property's `x-fynkel` (`type`, `nullable`, `label`, `tooltip`, `min`, `max`, `step`) and the type's identity under the document's `x-fynkel: { kind: "behaviour", id, version, dart: "package:…#Class" }`. `fynkel_document` loads a project's `schemas/` directory on top of the embedded schemas (`SchemaSet::embedded_with`, `Project::schemas`), so `fynkel validate`, the cooker and every project-aware command know the type, validate its data (`FYN-D0206`) and stop reporting it as unknown (`FYN-D0205`). A registry entry of kind `behaviour` in `fynkel.project.json` declares a type without a schema (`SchemaSet::declare_behaviour`): known, data accepted as any object.
* **CLI:** `fynkel codegen --project <dart package> [--check]` runs pipeline (a) as before and then (b): `dart pub get` when the package has no `.dart_tool/package_config.json`, `dart run build_runner build`, then the split into `schemas/behaviours/` (stale `<id>.schema.json` files removed). The Dart SDK is `$FYNKEL_DART`, else the `dart` next to `flutter` on `PATH`, else `dart` on `PATH`. `--check` runs the generator too, compares the three outputs and the schema directory, puts the committed generated files back and exits 2 on drift. The two pipelines have separate caches: (a) has none (a pure function of the Rust registry), (b) is build_runner's `.dart_tool/build`. Both outputs are **committed** in the game package.
* **At load:** the native load report lists every non-native component as `scene.behaviours[] = { entity (spawned handle), authoringId, component, type, version, known, data, pointer }`; `World.loadScene`/`loadProject` instantiate each through `world.registry` and attach it (`FYN-B0001` unregistered type, `FYN-B0002` rejected data; both warnings, the entity still loads). The game calls the generated `registerBehaviours(world.registry)` once before loading.
* **Gate (blueprint item 16):** `packages/fynkel_generator/example/spinner_game` — `Spinner`/`Blinker` classes, their committed generated files and schemas — and `crates/fynkel_document/fixtures/project`, whose scene gives the Robot a `game.Spinner` with `speed: 2.5` and whose `schemas/behaviours/game.Spinner.schema.json` is a byte-identical copy of the example's (a `fynkel_cli` test checks). `dart test` in the example loads the fixture project and asserts the attached `Spinner` has `speed == 2.5` and rotates; `cargo test -p fynkel_cli` runs `fynkel codegen --check --project <example>` when a Dart SDK is on `PATH`.

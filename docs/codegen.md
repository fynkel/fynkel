# Schema codegen (ADR-0012, native pipeline)

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
 schemas/components/*.json
 schemas/scene.schema.json     schema.g.dart (Dart codecs)      (Rust codecs, SCHEMA_FINGERPRINT)
```

* `fynkel_contracts::schema` defines the metadata (`RecordSchema`, `FieldType`), the wire layout rule (little-endian, natural alignment, records padded to 8, strings as `(offset, len)` into a string tail) and the fingerprint (FNV-1a over the canonical text of the registry).
* `fynkel_schema_derive` provides `#[derive(Schema)]` with `#[schema(kind = "component" | "command" | "event" | "record", id = N, name = "fynkel.X")]`. Supported field types: `f32 u32 i32 u64 bool Vec3 Quat Entity Option<Entity> String AssetRef`, nested schema structs, and unit-variant enums that also `#[derive(Schema)]` (`#[schema(name = "fynkel.Projection")]`). An enum is a `u32` declaration index on the wire and the lowerCamel variant name in JSON (`"perspective"`) and Dart (`Projection.perspective`); the variant list is part of the fingerprint, so append only. `AssetRef` is 16 id bytes + `sub` as a wire string (24 bytes), `{ "asset": uuid, "sub"? }` in JSON (`envelope.schema.json#/$defs/assetRef`), and `AssetRef(asset, sub:)` in Dart; an unknown enum index or malformed id fails decoding/encoding loudly on both sides. Doc comments flow into JSON Schema and Dart docs. `authored = false` marks runtime-only records (`fynkel.Name`): they get no `data` binding in `scene.schema.json` and scene validation rejects them (`FYN-D0207`).
* Generated files are **committed**. `cargo test -p fynkel_cli` runs `fynkel codegen --check` and fails on drift; regenerate with `cargo run -p fynkel_cli -- codegen`.
* Protocol fixtures: `tests/fixtures/wire/<fingerprint>/<Record>.bin` are written by the Rust codecs (`FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_wire`) and decoded + re-encoded byte-for-byte by the Dart codecs (`dart test` in `packages/fynkel`). A schema change produces a new fingerprint directory; delete the old one in the same commit.

Adding a built-in component: give it a number in `fynkel_contracts::ids::ComponentId` (the registry test checks the two agree), add the Rust struct with `#[derive(Schema)]`, append it to `ALL`, run codegen, add a sample to both wire fixture tests and regenerate (`FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_wire`; the old fingerprint directory goes), bump `schema_fingerprint` in `packages/fynkel/fynkel.lock.json`, commit all of it. Starter components (M2-04): `fynkel.MeshRenderer` (4), `fynkel.Camera` (5, `Projection` enum), `fynkel.DirectionalLight` (6), `fynkel.PointLight` (7) in `fynkel_core::components` — storage only until the M3 renderer reads them.

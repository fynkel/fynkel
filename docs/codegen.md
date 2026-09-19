# Schema codegen (ADR-0012, native pipeline)

Rust structs are the source of truth for every record that crosses the bridge.

```text
#[derive(Schema)] structs                      fynkel_core::schema::ALL (registry)
  fynkel_scene::Transform, fynkel_core::Name,          │
  fynkel_core::schema::{*Command, *Event, *Record}     ▼
                                              `fynkel codegen`  (apps/cli)
                                                       │
        ┌──────────────────────────────┬───────────────┴───────────────┐
        ▼                              ▼                               ▼
 schemas/fynkel.schema.json    packages/fynkel/lib/generated/   crates/fynkel_wire/src/generated.rs
 schemas/components/*.json     schema.g.dart (Dart codecs)      (Rust codecs, SCHEMA_FINGERPRINT)
```

* `fynkel_contracts::schema` defines the metadata (`RecordSchema`, `FieldType`), the wire layout rule (little-endian, natural alignment, records padded to 8, strings as `(offset, len)` into a string tail) and the fingerprint (FNV-1a over the canonical text of the registry).
* `fynkel_schema_derive` provides `#[derive(Schema)]` with `#[schema(kind = "component" | "command" | "event" | "record", id = N, name = "fynkel.X")]`. Supported field types: `f32 u32 i32 u64 bool Vec3 Quat Entity Option<Entity> String` and nested schema structs. Doc comments flow into JSON Schema and Dart docs.
* Generated files are **committed**. `cargo test -p fynkel_cli` runs `fynkel codegen --check` and fails on drift; regenerate with `cargo run -p fynkel_cli -- codegen`.
* Protocol fixtures: `tests/fixtures/wire/<fingerprint>/<Record>.bin` are written by the Rust codecs (`FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_wire`) and decoded + re-encoded byte-for-byte by the Dart codecs (`dart test` in `packages/fynkel`). A schema change produces a new fingerprint directory; delete the old one in the same commit.

Adding a built-in component: add the Rust struct with `#[derive(Schema)]`, append it to `ALL`, run codegen, update fixtures, commit all of it.

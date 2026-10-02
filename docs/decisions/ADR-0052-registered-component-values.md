# ADR-0052: Registered component values in the gameplay journal

**Date:** 2026-10-02

**Status:** Accepted historical first slice; extended by [ADR-0053](ADR-0053-registered-gameplay-and-extension-contract.md), which resolves the later F3 integration decisions. The remaining text describes the initial checkpoint, not the final F3 feature set.

**Related:** ADR-0044, ADR-0046, ADR-0051; foundation F3.1–F3.5.

## Contract

A component registration has a stable case-sensitive namespaced string ID, a positive schema version and ordered named fields with typed defaults. IDs must contain a namespace; the `fynkel` namespace is reserved. Field names must be unique. Runtime Rust `TypeId` is an in-process implementation detail, never a saved identity.

Registration belongs to the world and happens outside an owned gameplay edit. An exact repeat of the same ID, version, fields and native type is idempotent. Conflicting registration fails without replacing metadata or existing data. A native type cannot be registered under multiple stable IDs. Defaults must pass both structural schema validation and the type's codec before registration becomes visible. Schema changes require explicit migration; increasing a version is not permission to reinterpret live data.

In this first slice the supported field kinds are finite `f64`, `i64` and `bool`. Empty/marker layouts, strings, vectors, collections and object/asset references are not yet supported by this registry. In particular, an integer field is not an engine-managed object reference. Reference remapping must be implemented using explicit schema reference kinds before those layouts are enabled.

Native packages implement a value codec (`RegisteredComponent`) that encodes and reconstructs all component data. Codecs must be pure: shared mutable state, services, closures and external effects are outside the component-data contract. Schema shape/kinds/finite values are checked, followed by codec validation and a round-trip check. This is a semantic contract for trusted native extension code, not isolation from arbitrary native code or direct mutable ECS access.

## Storage and transactions

Committed native `T` is stored directly in the existing ECS's typed storage. The registry contains metadata and type-erased read/prepare/insert/remove functions, not a second entity/component store. Pending writes are owned prepared values in the existing gameplay journal, keyed by the registered identity. Decoding/validation runs before publication; commit moves the prepared T into storage without invoking a fallible codec.

Typed access and schema-value access use the same journal entry. Reads see pending values, replacement and removal. Unknown registrations, invalid values, stale/foreign entities and expired pending handles fail the edit stickily. A rejected registered write rolls back built-in writes, and a rejected built-in edit rolls back registered writes. Destruction drops registered data with the entity. Clone copies current logical registered scalar values through the same codec, so it sees pending replacements/removals and creates independent native values.

`Health` exists only in example/test code outside core; neither its identity nor its field names are added to `ComponentValue`, the built-in schema catalogue or the command decoder. The example runs through `World::register_component` and `GameplayEdit` and checks the committed native ECS value.

## Ordinary authoring and unfinished integration

The native methods are the foundation slice, not a second proposed ordinary Dart API. The intended user experience remains generated typed component access through GameObjects and bulk queries over the same authority. Game authors must not need to hand-write Rust to define ordinary gameplay data.

This slice proves **static native value registration**, not runtime registration of arbitrary Dart schemas. The next bridge/layout decision must explicitly resolve how Dart-defined layouts receive native storage without requiring hand-written native components or a conflicting Dart sidecar. Evaluate registration/build/reload cost and bulk access before claiming the static generic implementation settles that route. Do not silently make a native rebuild for every Dart schema edit a product requirement.

The end-to-end Health acceptance sequence remains:

1. Native metadata registration and atomic typed/scalar access, including clone and destruction — implemented in this batch.
2. Runtime-defined layout/storage and generated Dart registration/access over the owned bridge, with the ownership/build/reload decision recorded and measured.
3. Authored serialization and version handling, load/prefab/clone parity and actual Inspector editing. Unknown authored data must be preserved; runtime cannot silently pretend a required unknown component loaded successfully. This policy is a requirement for that integration, not implemented scene support today.
4. Frozen-membership bulk queries with explicit borrow/view lifetime and structural invalidation, measuring calls, bytes and allocations. Current per-record value copying is a correctness baseline, not the F3.2 performance result.
5. Registered DamageSystem and behaviour scheduling with ordering/access diagnostics. A native component in an integration test is not yet the supported packaged native-extension route.

No scene format, generated binding, ABI, scheduler or built-in component catalogue changes in this first batch. F3 stays active until the whole vertical slice and the roadmap's remaining acceptance criteria pass.

## Qualification

[Batch evidence](../../benchmarks/foundation/results/2026-10-02-0e79ac30-f3-native-registry.json): release workspace tests passed (1,090 passed, 8 ignored), stripped-feature registered-component/clone tests passed (18), and SDK tests passed (586). The Health example, workspace Clippy with warnings denied, formatting and generated-code freshness checks passed. Nine new registry regression tests cover storage/journal agreement, mixed-edit rollback, clone/destruction, schema and codec rejection, owned-session visibility and foreign/expired handles. These checks qualify this native slice only.

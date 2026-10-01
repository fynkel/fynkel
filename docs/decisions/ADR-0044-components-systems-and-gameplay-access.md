# ADR-0044: Components systems and gameplay access

**Status:** Proposed for general registration, supported custom layouts and extension delivery; shared ownership/access direction is accepted in ADR-0046.
**Date:** 2026-09-30  
**Related:** ADR-0042, ADR-0043, ADR-0003, ADR-0011, ADR-0012, ADR-0018, ADR-0028, ADR-0029, ADR-0036; roadmap F3, F4, F7.

## Context

`hecs` stores native components, but native component values and tick stages are a closed catalogue. Custom Dart behaviours own fields and execute individually. Making storage ECS-based does not itself provide efficient custom bulk gameplay or safe parallel execution.

## Proposed decision

[ADR-0046](ADR-0046-transactional-world-state-replacement.md) accepts the shared mutation/access boundary and one authoritative owner per type, with native-owned built-ins and registered native data. The mixed prototype proves numeric/bool custom records in the core ECS and ordinary Dart logic. It does not implement a general registry or settle every custom field/storage default; qualify those choices and generated bulk views under F3 before publishing them as supported.

Introduce component registration with stable namespaced type identity, versions, defaults, field/reference schemas, and storage ownership. Use the same metadata for loading, clone, inspectors, authored overrides, and generated access. Runtime service objects, coroutine execution, and arbitrary closures are not automatically serializable components.

Support typed access to one entity and queries over populations through one component model. `GameObject` is an entity facade; component handles are accessors. Do not introduce a second entity store solely to support convenient object syntax. A custom gameplay component has one declared authority whether its storage is native or Dart-owned.

Register systems with phases, dependency edges, read/write declarations, and structural commit rules. Validate cycles and dependencies on disabled modules; choose stable tie-breaking. Initially execute sequentially. Register behaviour phases within that execution model and preserve explicit lifecycle/event semantics.

Offer a native system/component extension contract so a game package can add heavy bulk logic without patching core. Keep ordinary Dart authoring simple. Package compatibility, generated codecs, and module stripping are part of the extension contract.

## Questions to settle in F0

- Prototype and compare custom Dart component storage, generated native storage for suitable data, and a hybrid with one owner per type. Measure realistic reads/writes and full-frame bridge traffic. Select a default and document supported field types and crossings.
- Choose immutable record replacement, generated mutable accessors, or scoped query views for each access boundary. Define lifetime, structural invalidation, error reporting, and whether views require copying.
- Specify query iteration order and permitted structural changes without promising deterministic simulation from iteration order alone.
- Choose registration/discovery and package delivery for native extensions; decide whether the first supported route is build-time linking. A dynamic binary plugin ABI is not implied.
- Specify authored custom-data schemas, references, type/version migration, and missing-type behavior in an editor that must preserve unknown content.

## Alternatives and consequences

Retaining only catalogue components avoids extension design but limits advanced games. Exposing raw `hecs` as the gameplay API couples consumers to storage and breaks the facade. Requiring all gameplay to become systems harms local scripting ergonomics. A registry plus shared access preserves both local and bulk use, but adds type/borrow/schema machinery that must remain behind ordinary APIs.

Acceptance implements ADR-0042's amendment to ADR-0011. ADR-0012's generated source-of-truth principle remains; its pipeline gains custom component support deliberately rather than maintaining duplicate hand-written schemas. Existing value-record and object-handle conveniences may remain as adapters to the same model.

## Acceptance evidence

The [September 30 prototype](../design/foundation-prototype.md) mixes local object access and a bulk poison system over the same custom records, validates scheduler dependencies and round-trips metadata-driven field edits. It compares generic Dart-owned immutable records, native ECS processing, and native-owned records edited through two batched FFI calls per round. The fixed-layout native candidates omit registration/scope work present in the Dart candidate; timings do not settle the default owner. Generated scoped views, AOT, allocations, structural changes, actual inspector integration and a general extension route remain required.

F3 proves a custom health/poison system, local door behaviour, inspector/serialization round trip, efficient population access, dependency diagnostics, and a native extension package outside core. F4 proves no bookkeeping bypass and safe view lifetimes. F7 proves fresh-project ergonomics and migration. A parallel scheduler is a later measured decision.

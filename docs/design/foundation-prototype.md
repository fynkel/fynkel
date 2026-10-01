# F0 gameplay and storage prototype

September 30, 2026. Experimental working-tree implementation against `87e3dec8`; [recorded commands, source hashes and results](../../benchmarks/foundation/results/2026-09-30-87e3dec8-prototype.json). This is evidence for [F0.3](../foundation-roadmap.md), **not the F0 gate or an adopted public API**. No shipping gameplay exports or runtime ABI have changed.

**Follow-up:** [the mixed-state comparison](foundation-state-comparison.md) extends built-in/custom atomicity and chooses the replacement boundary in ADR-0046. Sections below retain the earlier experiment's observations and limitations; they are not the current decision ledger.

## What runs

[The slice](../../packages/fynkel/tool/foundation_prototype/run.dart) creates a real native World with a player, door and 128 projectile entities. Registered Dart-owned Health records are processed by a bulk poison system. A local door interaction uses object syntax to read the same Health records after poison commits. A real engine Behaviour drives the experimental sequential scheduler in fixedUpdate.

The slice exercises metadata-based field editing, JSON serialization and restoration onto a replacement scene entity. Old wrapper references are rejected at the scene boundary. [Twelve tests](../../packages/fynkel/test/foundation_prototype_test.dart) cover shared reads, query changes, rollback, permissions, scoped lifetimes, foreign references, scene retirement, schema rejection and scheduling. Analysis with fatal infos passes. The local interaction is a prototype adapter; production behaviour lifecycle integration remains unimplemented. Field editing proves the codec/access path, not integration with the actual editor inspector.

[The access kernel](../../packages/fynkel/tool/foundation_prototype/world.dart) provides:

- Namespaced component identity/version, field metadata, defaults, validation and codecs; one registered descriptor per Dart type.
- Dense custom-data stores indexed by real GameObject references. They own only custom records; there is no duplicate built-in component state or second entity allocator.
- Immutable example records, typed object access and bulk access through the same scoped transaction.
- Read/write declarations, explicit dependency edges, cycle/missing-dependency errors, sequential execution and stable system-ID tie-breaking.
- Owner/scene checks around native references. This protects the prototype wrapper and does **not** repair the shipping native namespace allocator.

```dart
world.addSystem(poisonSystem());
world.addSystem(DoorBehaviour(player: player, door: door).system);
world.initialize();
world.transaction(writes: [healthType], run: (access) {
  access.object(player).set(healthType, const Health(poisonPerSecond: 2));
});
world.frame(1 / 60);
```

This setup syntax is experimental. Production authoring must eventually create data and entities together without making beginners manage a setup commit.

## Executable visibility contract

Each custom-data callback stages validated edits. Success commits them together before the next callback; an uncaught error discards that callback's journal. Outside a running frame, an explicit transaction uses the same rules and commits on return. Scope accessors expire on both success and failure. Previously committed callbacks are not rolled back when a later callback fails.

| Operation | Object read in callback | Fresh bulk query in callback | Next callback |
| --- | --- | --- | --- |
| Replace a record | New immutable value | New value | Committed new value |
| Add a component | Present immediately | Included when query opens | Present |
| Remove a component | Absent immediately | Excluded | Absent |
| Add during an open query | Present through direct read | Current candidates stay fixed; next query includes addition | Present |
| Remove during an open query | Absent through direct read | Removed candidates not yet visited are skipped; dense storage stays unchanged until commit | Absent |
| Invalid edit followed by uncaught error | Callback exits with error | No callback commit | Previously committed records remain |

Query candidates freeze at opening; values reflect current pending edits. Structural changes never modify the backing dense array during traversal. Sparse queries with pending additions allocate a small candidate list; ordinary dense queries avoid a population copy and per-row wrapper allocations. Immutable record replacement and journals still allocate. Callback-scoped helpers must not be retained.

Multiple writers execute in the validated schedule and later committed writes win. This prototype has one fixed phase; it does not yet prove update/after-physics/presentation ordering, parallel borrows, system disabling or runtime registration. Stable iteration/scheduling is not a rollback-determinism claim.

The atomic scope covers **custom Dart data only**. Native transform/hierarchy/physics edits still use the current engine route. Adding this sidecar to production would leave that important contract unresolved. Runtime spawn/destroy during a callback is deliberately unsupported; scene replacement is an explicit setup boundary.

## Storage and crossing experiments

Three candidates process the same fixed Health workload: hp100, poison2/s, dt1/60, 20 warm-up rounds, reset, then 120 measured rounds. Output checksums agree. Apple M3 Max, macOS26.5.1, Flutter3.47.2/Dart3.13.2 JIT; Rust1.93 release with debug symbols.

| Candidate | 1,000 entities p95 | 10,000 entities p95 | Included work |
| --- | --- | --- | --- |
| Generic Dart-owned custom-data kernel | .309ms | 3.073ms | Immutable replacement, validation, owner/scope checks, query, journal and commit; no per-entity FFI |
| Native-owned records with Dart bulk logic | .027ms | .190ms | Native snapshot copy, two FFI calls per round, Dart borrowed-view loop, whole-batch validation and native commit |
| Native ECS storage/system only | .003ms | .039ms | Typed native query/update and basic validation; no bridge or atomic journal |

[The native storage example](../../crates/fynkel_ecs/examples/foundation_storage.rs) uses the existing Fynkel ECS facade. [The candidate bridge](../../crates/fynkel_ecs/examples/foundation_bridge.rs) is an example cdylib, loaded explicitly by [the Dart crossing probe](../../packages/fynkel/tool/foundation_prototype/native_storage.dart). It owns an independent candidate ECS and returns a reusable snapshot of 24-byte rows. Dart edits the snapshot; all identity/value rows validate before authority changes. Rejected batches leave authority untouched. Consumed, replaced and disposed snapshots are rejected through guarded Dart access. Raw pointers remain a trusted, exclusive caller contract, not a complete safe public extension ABI.

At 10,000 entities, the bridged run performs 240 FFI calls over 120 rounds and copies 28,800,000 bytes into native snapshots. Authoritative writeback is additional work already included in timing. No per-entity FFI calls are required. This demonstrates that batched crossings are viable in this narrow workload; it does not establish acceptable full-game traffic.

The generic Dart path performs more checks and supports registration/journaling that the fixed-layout native candidate omits. It allocates records/journal entries while the native candidate reuses a flat buffer. These results measure **these implementations**, not an intrinsic Dart-versus-Rust ratio. Allocator/GC counters, AOT, sparse changes, multi-type joins, references, structural churn, physics, rendering, inspector integration and full-frame cost remain unmeasured. Clippy for the ECS examples with warnings denied passes. Bridge contract probes confirm invalid-batch rollback and view expiry/disposal.

## Architectural implications and unresolved choices

Object convenience and bulk access can share one authority: the door and poison paths need no competing Health copy. Keeping local behaviours is compatible with an ECS-centered model.

The generic journal/immutable-record approach has material cost at 10,000 dense updates in this capture. Generated typed views or packed data deserve a full contract prototype before choosing the default. Native-owned data can still support ordinary Dart logic through a batched scoped view, so native ownership need not mean writing every system in Rust.

The incremental candidate is easier to attach to the current SDK but cannot make mixed built-in/custom operations atomic or reconcile all existing read routes. The isolated native candidate demonstrates a narrower coherent data kernel, but lacks the current scene, physics and editor integrations. Its lower measured time is not proof that a complete rewrite would be cheaper. The next comparison must integrate native built-ins and custom data under one mutation/commit boundary. A focused state/bridge rebuild remains a hypothesis; existing render/import/audio/editor work should be retained where compatible.

ADR-0043 and ADR-0044 remain Proposed. Candidate choices to test further: callback commits visible to subsequent sequential systems; immutable reads versus generated scoped views; one owner per type; explicit structural boundaries and published snapshots. Do not accept these as whole-engine decisions until the remaining F0 contracts and slice capabilities are proven. Asset readiness is outside this prototype and ADR-0045 remains unresolved.

## Reproduce and remaining gate work

From the repository root:

```sh
python3 tools/foundation_baseline.py --only prototype,prototype-native,prototype-bridge
cargo clippy -p fynkel_ecs --examples --release --locked -- -D warnings
```

Raw capture: `target/foundation-baseline/20260930T115638854624Z`. The runner's dependency-resolving prototype setup runs `dart pub get`; the recorded initial capture used already resolved package dependencies. The default runner also includes these suites. For the bridge candidate, run its build suite before invoking the Dart script directly.

Outstanding F0.3 evidence: actual skeletal animation and render integration; runtime structural changes; mixed built-in/custom transactions and read parity; component-reference schemas and unknown-type preservation; actual editor/generated metadata integration; full-frame bridge/allocation/AOT captures; and measured implementation complexity for the integrated replacement strategies. F0.4 still needs clock, identity and asset-readiness decisions. These experiments do not complete F0.

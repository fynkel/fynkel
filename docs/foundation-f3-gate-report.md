# F3 gate report: registered gameplay data and systems

**Date:** 2026-10-02. **Status:** complete under [ADR-0053](decisions/ADR-0053-registered-gameplay-and-extension-contract.md). F0 performance qualification and F4–F8 remain open.

F3 connects game-defined component data, generated Dart access, native storage, bulk operations, authored scenes/prefabs, Inspector changes, behaviours and registered systems. GameObjects remain the approachable object interface. Health and Damage are game/package definitions; the core has no Health-specific catalogue case.

## Acceptance evidence

| Item | Delivered contract | Executable coverage |
| --- | --- | --- |
| F3.1 | Immutable namespaced schemas, versions/defaults, scalar/text/asset/object fields, native or runtime-defined ECS storage; common journal for typed access, clone, scene/prefab loading and Inspector writes | Core registered/dynamic component tests; generated gameplay example cooks/loads scenes and prefabs with forward/self references; authoring live/preview/undo tests; editor Play component edit/remove/version test |
| F3.2 | GameObject typed value access, owned read-only bulk snapshots and atomic bulk updates; edit/world lifetime and invalidation checks; no per-row bridge call required | SDK one-call query/two-call update assertions; mixed built-in/registered rollback; pending/foreign/destroyed references; malformed registration, expired tokens and truncated native requests; measurements below |
| F3.3 | Native built-in stages and package systems share a compiled sequential graph; host systems order around behaviour phases; names, access declarations, dependencies, module requirements, structural transaction boundaries and timings | Graph order/cycle/unknown/backwards dependency tests; stripped module tests; callback rejection/panic rollback; next-system visibility |
| F3.4 | Behaviour phases use scheduling and existing shared callbacks; lifecycle, contact, coroutine and exception machinery retained; generated effective callback participation | Full SDK/Flutter suites; mixin-based Door example; known-unused callbacks skipped; same-count metadata replacement and no constructor probing regressions; matching fixed ticks |
| F3.5 | Static Rust extension package/host route, API 1 / runtime ABI 5, atomic installation and module requirements; metadata-only editor | External native_combat crate and executable with core default features disabled; incompatible/partial installation, matching host schema adoption, editor non-execution; dynamic-library identity/export gate |

`examples/registered_gameplay` is the mixed-game exit test: Damage updates the Health population and Door reads a player's Health through its GameObject in the same fixed phase. The project writer, cooker, loader and generated schemas use that same data definition. `examples/native_combat` supplies a native component/system outside core and demonstrates installation by a host.

## Decisions and boundaries

See ADR-0053 for the complete contract. The deliberate constraints are:

- Supported components are flat scalar/text/asset records and nullable managed object references. Arrays, nested records, vectors, typed asset wrappers and marker layouts are rejected. Generated defaults come from literal metadata shared with the editor, not game constructors. Native typed codecs do not support managed object-reference fields; runtime-defined layouts do.
- Schemas are immutable in a live world. Exact repeats adopt the existing registration; incompatible versions/layouts require explicit authored migration and world recreation. Unknown authored JSON can be preserved, but missing runtime registrations reject activation.
- Query results are owned values. There is no zero-copy mutable borrow. A bulk update is bound to one transaction and expires after intervening registered/membership changes. It validates before publication and rolls back as a whole.
- Scheduling is hierarchical: host fixedUpdate, native tick, host afterPhysics, then presentation-rate update/lateUpdate. Native custom insertion is limited to Simulation/PostPhysics. Dependency edges do not arbitrarily interleave Dart and native callbacks.
- Read/write and structural declarations are audit metadata, not runtime permissions or proof of parallel safety. Execution stays sequential. Native extensions are trusted, statically linked source packages; this is not a dynamic binary plugin loader or automatic Dart-to-native package linker.

These are explicit API contracts, not fallback compatibility paths. There is no second authoritative Dart component store. F7 retains broader package/tooling adoption; F4 retains synchronization work; F8 retains production qualification.

## Review corrections (2026-10-02)

The follow-up source review found three defects in the original checkpoint despite its passing checks. They are now fixed, with permanent regressions demonstrated failing before the fixes:

- Exposed mixin fields were omitted. Shared field discovery now visits class declarations, rightmost mixins and then the superclass. Health's fixture covers multiple mixins, default/override precedence and an inherited object reference. Its existing scene/prefab and live-data tests exercise the generated codec; the CLI freshness gate now includes this fixture.
- Host systems used a different tick than behaviours, including afterPhysics. The scheduler now receives the captured phase tick explicitly from the behaviour phase driver. A replacement regression compares systems before/after behaviours across every host phase, including zero- and multi-tick frames. Native completion counters are unchanged; ADR-0053 specifies the host interval labels.
- `addData` decoded defaults outside the transaction guard. Registration validation and decoding now run inside the guard, so catching a rejected operation cannot publish earlier staged writes. The permanent regression verifies the prior name survives a caught invalid-default/declaration error.

[Review-fix evidence](../benchmarks/foundation/results/2026-10-02-407a45b7-f3-review-fixes.json): **12/12 affected qualification commands pass** — 601 SDK, 14 generator, 3 registered-gameplay, 8 existing generated-example, 26 CLI, 28 core, 39 Flutter and 71 editor tests. SDK/generator analysis, formatting and CLI Clippy with warnings denied pass. Runtime ABI and generated schemas are unchanged. The initial qualification and performance measurements below remain historical evidence; this batch did not rerun the full workspace or performance benchmarks.

## Qualification

[Archived evidence](../benchmarks/foundation/results/2026-10-02-2311b783-f3-completion.json) records commands, results, retry history, log hashes, source hashes and raw measurements.

All 20 qualification commands pass: **1,106 Rust workspace tests** (8 ignored), **27 stripped core tests**, **24 stripped native protocol tests**, **600 SDK tests**, **14 generator tests**, **2 generated gameplay integration tests**, **16 authoring tests**, **39 Flutter tests**, **71 focused editor tests**, and the separately enabled **dynamic-library identity/export test**. The native extension executable, Clippy with warnings denied, formatting, codegen freshness and affected Dart/Flutter analysis pass. The eight ordinary-workspace ignores remain explicit; enabling the dynamic-library test separately does not claim that all other ignored platform/manual tests ran.

The audit fixed duplicate authored records, unknown-field preview comparison, metadata registration on preview reload, upcoming fixed-tick context, mixin callback discovery, stale same-count behaviour metadata caches, generated defaults and new-project template drift. Regression failures from a stripped test's optional-physics dependency and a generated-file refresh race were corrected and rerun; previous attempts remain in the evidence. The final live Play test uses a double record and exercises schema version 3 through actual native inspection and editing.

## Measured cost

Apple M3 Max, macOS 26.5.1. The SDK workload creates Health `(double, bool)` records, runs one bulk Damage system in every fixed frame, warms up 40 frames and measures 60. Full-frame time includes World.frame, native tick, callback reconciliation and publication. It excludes a rendered game, GPU work, loaded asset workloads, physics bodies and real behaviour populations. Timings are local descriptive results, not production budgets.

| Entities | AOT bulk p50 / p95 μs | AOT full frame p50 / p95 μs | JIT full frame p50 / p95 μs | Bulk copied bytes |
| --- | --- | --- | --- | --- |
| 1 | 7 / 11 | 33 / 40 | 116 / 201 | 128 |
| 1,000 | 1,080 / 1,378 | 1,303 / 1,601 | 1,398 / 1,523 | 80,048 |
| 10,000 | 14,037 / 15,535 | 16,887 / 18,521 | 17,375 / 21,841 | 800,048 |

Every measured population uses **2 bulk FFI calls and 4 counted buffer copies**; the complete frame uses 22 calls. A read-only bulk query requires one population call. Those counts exclude additional operations deliberately performed by user callbacks. Schema registration took 354 μs for the first AOT schema and 7/8 μs in subsequent worlds; initial entity creation took 1,066 / 8,548 / 71,856 μs. This is runtime registration without a native rebuild, not a measurement of a full developer edit/reload cycle.

The VM's allocation-profile window reports approximately 34,860 / 37,725 / 92,861 counted objects per frame and 2.94 / 3.09 / 5.61 MB of counted allocation traffic at these populations. It includes service overhead and can omit optimized allocations; it is **not an exact game-only allocation count**. The raw counters are retained, including an earlier non-monotonic small-population result. AOT allocation counters are unavailable in this harness.

The isolated native counterpart counts allocator requests directly over complete native ticks:

| Entities | Native tick p50 / p95 μs | Allocation/reallocation calls per tick | Requested bytes per tick |
| --- | --- | --- | --- |
| 1 | 5 / 5 | 9 | 3,672 |
| 1,000 | 363 / 394 | 4,187 | 1,570,956 |
| 10,000 | 2,783 / 2,821 | 41,694 | 16,784,556 |

Requested bytes measure allocation traffic, not retained memory. The native test excludes Dart and bridge serialization, so its timings cannot be subtracted to claim an exact FFI overhead. The bridge byte counters now include reply copies previously omitted; do not compare them directly with old F2 counters. An earlier AOT run measured 30.679 ms p50 at 10,000 entities; that sample is retained alongside the final run rather than discarded as an engine-wide performance claim.

**The 10,000-entity workload does not provide a 60 Hz game budget.** Owned rows, wire decoding, journals and full callback-entry reconciliation still cost time and allocate. F3 proves a safe shared data path with constant bridge-call count, and measures its costs. F0/F4/F8 must establish targets, profile and reduce record/journal/bridge allocation traffic, and measure sparse and dense real workloads. No parallel or zero-allocation claim is made.

Reproduce with `dart run tool/f3_registry_bench.dart` from `packages/fynkel`, or `dart build cli -t tool/f3_registry_bench.dart -o ../../target/f3-registry-aot` and the bundled binary with `--aot`. Native: `cargo run --release -p fynkel_core --example registered_bulk_bench --locked`.

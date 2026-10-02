# F4 gate report: ownership, revisions and synchronization

**Date:** 2026-10-02. **Status:** complete under [ADR-0055](decisions/ADR-0055-incremental-synchronization-and-publication.md). This closes F4.1–F4.4; F0 performance targets and F5–F8 remain separate gates.

## Delivered and qualified

| Item | Result | Permanent coverage |
| --- | --- | --- |
| F4.1 | One gameplay owner; separate behaviour execution, scene lifecycle, resource services and publication owners. Public edits use the shared journal or tracked ECS; private scene metadata writes advance their revision. Core receives a resource capability rather than importing the loader/bridge/editor. | Existing transaction, extension, scene/persistence/resource and callback suites; ECS compile-fail tests; dependency audit below |
| F4.2 | Bounded per-entity/type journal, independent consumers, explicit reset on lost relevant history; sparse SDK identities and editor rows; unchanged native entity tables reuse their encoding. Dynamic type presence is tracked separately from values. | ECS change tests; core F4 synchronization tests; native cache allocation gate and state boundary tests; authoring delta tests; SDK publication tests |
| F4.3 | Retained dirty roots/activity, physics membership and active spatial render membership. Sleeping bodies avoid full writeback scans; new bodies still receive their first writeback. | Direct native pose mutation, hierarchy/activity changes, sleeping bodies, exact-generation churn, nested colliders, invalid solver pose and mesh invalidation regressions; static/sparse/churn benchmarks |
| F4.4 | Immutable Rust snapshot allocations and immutable owned Dart wire buffers. Clear C borrowed-view and sequential world-handoff contracts. No clone of the native bulk token's population; compact sorted registered journal maps. | Retention after repeated queries, growth, poisoning, disposal and scene replacement; Rust snapshot retention/thread handoff; F3 owned-row/token invalidation and rollback suites |

The new regressions are [core synchronization](../crates/fynkel_core/tests/f4_synchronization.rs), [ECS changes](../crates/fynkel_ecs/tests/changes.rs), [native table cache](../crates/fynkel_native/tests/f4_cache.rs), [authoring deltas](../crates/fynkel_authoring/tests/live.rs), [SDK publications](../packages/fynkel/test/f4_publication_test.dart) and [scene replacement](../packages/fynkel/test/data_asset_test.dart). The authoring Dart suite now verifies stable old publications and incremental ordering/removal through the actual native engine.

## Ownership and mutation audit

This is more than relocating fields. `SceneLifecycle` hides its vectors behind revision-advancing mutable leases, so persistence and authored identity changes cannot silently bypass table invalidation. `SceneSync` consumes tracked ECS changes without importing core or the bridge. `Publication` owns subscriptions and creates an immutable replacement before exposing it. `ResourceServices` owns preparation/residency/cancellation lifetime and store adoption. `GameplayState` is the only committed component authority; the existing edit scope and native `WorldSlot` retain exclusive transaction ownership. `_BehaviourSystem` remains the separate execution owner, with no authoritative Dart component mirror.

`World` remains the public composition/orchestration facade. Its public API routes edits to `GameplayEdit`, native data writes to tracked `Ecs`, resources through a service capability, and publication to owned or explicitly borrowed read surfaces. The API has not been replaced by multiple competing ways to commit gameplay. Full and conditional state reads remain compatible projections, not mutation paths.

The archived normal dependency trees verify: ECS depends on contracts/hecs; scene depends on ECS/contracts; physics consumes scene/ECS; core has no native ABI, assets, authoring or renderer dependency. The game runtime has no authoring/preview, document-validation (`jsonschema`) or text-diff (`similar`) dependency. `tools/check_runtime_deps.sh` passes. Compile-fail tests enforce read-only shared queries and exclusive bulk-write lifetimes. Component-owned interior mutation and arbitrary corrupt reciprocal hierarchy edits are not supported plain-data writes; ADR-0054/0055 make those boundaries explicit.

## Measurements

Apple M3 Max, macOS arm64. Native synchronization harness: 20 warmups, 100 measured samples, 1,000 and 10,000 spatial objects. These are local microbenchmarks, not whole-game budgets. Candidate counts establish the algorithm; sub-microsecond timings and differences between population runs are sensitive to CPU scheduling. Requested bytes count allocation/reallocation traffic, not retained memory.

| Workload, 10,000 objects | Candidates per sample | p50 / p95 μs | Allocations / requested bytes per sample |
| --- | --- | --- | --- |
| Unchanged scene + static physics | 0 | 0.166 / 0.167 | 0 / 0 |
| One changed local pose + physics | 2 (one in each consumer) | 0.875 / 1.000 | 8 / 944 |
| One destroy + replacement | 4 (two in each consumer) | 2.209 / 3.667 | 21.16 / 2,636 |
| Sleeping scene + physics synchronization | 0 | 0.166 / 0.167 | 0 / 0 |
| Unchanged table revision | 0 | 0.125 / 0.125 | 0 / 0 |
| Full render extraction | 0 membership candidates; 10,000 output records | 1,415 / 1,576 | 14 / 3,613,988 |

At 1,000 objects the static/sparse/churn candidate counts are also 0/2/4. The first smaller-population sparse run includes journal capacity growth (6,350.72 requested bytes/sample); the full raw data preserves that cost. The stateless reference scene scan visits all 10,000 entities (6.542 μs p50). It is a reference kernel, not a paired old-engine benchmark. A complete tick with 10,000 sleeping bodies takes 21.375 μs p50 and still allocates one 64-byte request; synchronization alone being allocation-free does not mean a frame is.

The isolated native cache regression performs 100 unchanged 2,048-row table queries with **zero allocation requests**, verifies each request ID and then checks direct native name/layer/tag invalidation. SDK callback-equivalent edits at 2,048 objects × 32 edits measure **0.769 ms p50 / 1.302 ms p95**, zero full identity snapshots and 384 identity-response bytes. The historical F2 result was 10.956 ms p50; this is a descriptive comparison, not a controlled production speedup claim. Sparse native rename/destruction regression requires fewer than 128 response bytes and no full snapshot.

F3's native 10,000-row workload now requests **9,424,556 bytes/tick**, down from 16,784,556 in the F3 report (43.85%). The compact journal accounts for 736 saved bytes per changed row; the unchanged-empty publication reuses its allocation. Allocation-call count remains 41,694. Native tick p50 is 3.145 ms, versus the earlier F3 2.783 ms: reduced allocation traffic is not proof of faster dense ticks. The bridge also no longer clones a frozen batch before applying it.

Dense SDK bulk updates still use 2 FFI calls, 4 counted bulk copies and 800,048 copied bytes at 10,000 rows. JIT full-frame p50/p95 is 15.938/20.580 ms; AOT is 17.878/32.435 ms, versus the historical F3 AOT 16.887/18.521 ms. The JIT VM profile reports approximately 10.18 MB counted allocation traffic per frame at 10,000 rows, higher than F3's 5.61 MB sample. Those service-inclusive counters are noisy and may omit optimized allocations; all raw readings are retained. Immutable output copies are intentional, and no reduction in total Dart allocations or guaranteed dense-frame speed is claimed. This workload still does not qualify a 60 Hz large game.

## Qualification and reproducibility

[Archived evidence](../benchmarks/foundation/results/2026-10-02-9b7caf3c-f4-completion.json) records logs/hashes, commands, final source hashes, retry history and raw measurements. Passing checks include **1,135 workspace Rust tests** (8 existing ignores), **1 additional native cache test**, **48 stripped-core tests**, **26 stripped-native unit tests**, **606 SDK tests**, **17 final publication/resource checks**, **16 authoring tests**, **39 Flutter tests**, **105 affected editor tests**, and the separately enabled **dynamic-library identity test**. Workspace Clippy with warnings denied, Rust formatting, generated FFI bindings, dependency checks and SDK/authoring/Flutter/editor analysis pass. The ignored manual/platform tests are not claimed run.

During development, a first-writeback regression caught newly inserted Rapier bodies missing from the active-body list; an explicit pending set fixes that. Qualification retries include a physics benchmark missing its required-feature declaration, an incorrect panic expectation, test invocation/configuration mistakes and a document-validation timing threshold failure. The document test passed alone and in the subsequent complete workspace run. The unrestricted stripped-native integration command exposed an existing optional-uuid test dependency; the stripped gate covers its unit protocol suite, not all integration fixtures. These attempts remain recorded rather than being represented as passing runs.

Reproduce native workloads with `cargo run --release -p fynkel_core --example f4_sync_bench --locked` and `--example registered_bulk_bench`. Run `dart run tool/foundation_callbacks.dart` and `dart run tool/f3_registry_bench.dart` from `packages/fynkel`; the F3 report records the AOT build command. Dart fixture checks set `FYNKEL_CLI` to this checkout's built release CLI.

## What remains outside F4

Full render-output construction and interpolation still scale with the output population; F6.1 explicitly owns extraction-buffer reuse and smaller render publications. Hierarchy/order changes can require full ordering output; history overflow, schema changes and scene metadata changes deliberately request a safe reset. Dense gameplay still allocates owned rows and journals. F5 covers readiness/failure propagation, bounded preparation/upload work, residency budgets and stable skeleton binding. F8 covers production/platform qualification. No parallel-world execution or Unity/Godot-scale readiness is inferred from F4 completion.

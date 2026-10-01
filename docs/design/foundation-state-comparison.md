# Mixed built-in/custom state comparison

September 30, 2026. Working-tree experiment against `87e3dec8`; [full capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-state.json). [ADR-0046](../decisions/ADR-0046-transactional-world-state-replacement.md) records the resulting scoped decision. Task status remains in [the roadmap](../foundation-roadmap.md).

## One authority and two strategies

[The native kernel](../../crates/fynkel_core/examples/state_kernel/mod.rs) uses the existing core World/ECS for Name, local Transform, Parent/Children, local/inherited activity, custom Health and DoorState. Its journal is temporary transaction state, not a second committed component store. Queries, name search, exact derived matrices and activity resolve through that same view. Commit publishes built-ins through the current core commands and custom records into that core's ECS. No shipping API/runtime behavior has been replaced yet.

The **shared view** validates changed records and hierarchy before applying a prepared change set. It reuses core command bookkeeping, including hierarchy maintenance and teleport/interpolation handling. Supported pure edits have no user callbacks during apply; an unexpected apply failure poisons the kernel. Allocation/panic failure is not a recoverable general world rollback mechanism.

The **preflight adapter** reconstructs the experiment's entire supported state in a temporary core World, applies the intended edits there, then applies them to the real World. The preview remaps handles internally and is never exposed. Reconstruction batches scene creation to avoid repeated propagation. It does not claim to clone physics, audio, animation or arbitrary extension state. Both candidates use the same view, validation and prepared apply code; the measured difference is this additional preflight. This is not an exhaustive search of possible adapters.

[The Dart facade](../../packages/fynkel/tool/foundation_prototype/mixed_world.dart) has scoped object helpers, queries and checked references. It retains no mutable component mirrors. Temporary references resolve after commit and retire after abort. Its diagnostic JSON calls copy/decode native output before reuse. The candidate library owns a small retired namespace allocator; production/cross-library identity remains F1 work.

## Executable contract evidence

[Dart contract runs](../../packages/fynkel/tool/foundation_prototype/mixed_contracts.dart) pass for both strategies:

- Spawn/find and rename/find agree before commit, including pending references and parent links.
- Exact world matrices include staged ancestor scale/rotation. A nested rotated nonuniform scale produces the same geometry through gameplay and native render extraction.
- Sequential poison and door callbacks share custom Health and native Transform edits.
- A rejected cycle rolls back earlier name, Transform and Health changes together.
- Metadata-driven Health editing follows the same route; even a caught validation error prevents that callback's commit.
- Deactivation affects descendant derived activity immediately; reparenting out of that hierarchy restores activity immediately.
- Query traversal skips a subtree destroyed earlier in that traversal, excludes newly spawned candidates from the current cursor, and includes them in the next query.
- Destroyed, aborted, foreign and expired references/accessors reject access. Undeclared writes reject the callback.
- A native tick and core render extraction agree with the supported gameplay matrices/activity.

Three native tests separately verify unknown-field rejection/rollback, stale-reference rejection/rollback, and pending query/destruction behavior. Read-only callbacks do not change the committed revision. Analysis with fatal infos and Clippy with warnings denied pass. The current shipping apply_now observation still shows an earlier name edit surviving a later self-parent rejection; it is recorded as behavior, not a regression requiring that disagreement to remain.

The callback roles in this mixed candidate are driven explicitly from Dart; full production Behaviour lifecycle and phase integration is not proven by these runs. The earlier isolated prototype separately tested registered sequential scheduling in the existing Behaviour driver.

## Native frames and allocations

[Native benchmark](../../crates/fynkel_core/examples/foundation_state_bench.rs): 1k/10k fixed population, one changed Health record or all records changed. Five warm-up frames, 30 measured frames. Each sample includes journal staging, validation/commit, an actual core fixed tick and core render extraction. Checksums and extracted entity counts validate results. Rust release, modules stripped, no active physics/audio/animation population, no Dart/JSON/GPU. The installed core CountingAllocator counts allocation/reallocation operations, not bytes allocated or Dart GC.

| Entities | Changed/frame | Shared p95 | Preflight p95 | Shared median allocations/frame | Preflight median allocations/frame |
| --- | --- | --- | --- | --- | --- |
| 1,000 | 1 | .544ms | 1.191ms | 236 | 14,611 |
| 1,000 | 1,000 | .686ms | 1.911ms | 5,397 | 19,772 |
| 10,000 | 1 | 1.658ms | 12.284ms | 1,881 | 143,843 |
| 10,000 | 10,000 | 7.478ms | 31.201ms | 53,541 | 195,503 |

The adapter copies 350k preflight entities across the 35-frame 10k run, even with a single changed record. The shared path avoids that clone but still builds mapping/index scratch and uses existing full propagation/extraction scans. Its sparse frame cost is not O(changes), and dense allocation remains material. Single-machine short captures are observations, not regression budgets or game-engine qualification.

## Dart crossing and full publication

[JIT bridge benchmark](../../packages/fynkel/tool/foundation_prototype/mixed_bench.dart): 128 entities, five warm-up frames and ten measured frames. Timing includes Dart staging, JSON RPC, native transaction, core tick/extraction, full matrix/activity publication and Dart decode. Health checksums match.

| Changed/frame | Shared p95 | Preflight p95 | FFI calls over ten frames | Response bytes over ten frames |
| --- | --- | --- | --- | --- |
| 1 | 1.484ms | 1.421ms | 40 | 482,360 |
| 128 | 2.194ms | 1.853ms | 1,310 | 496,330 |

The end-to-end small JSON experiment does **not** show a shared-path speed win. Diagnostic per-record calls and full publication dominate useful data changes; transport must change before meaningful large-population gameplay comparisons. The earlier [typed-view crossing](foundation-prototype.md) already processed 10k records with two FFI calls per round, but lacks this prototype's complete mutation/view contract. Combining those properties is required next. AOT and Dart allocation/GC have not been captured.

Machine/toolchain: Apple M3 Max, macOS26.5.1 arm64; Cargo1.93.0, Flutter3.47.2/Dart3.13.2. Runtime samples exclude build time. Exact commands, starting tree hashes, native test/analysis/lint results and complete measurements are preserved in the capture. Raw logs: `target/foundation-baseline/20260930T124200209636Z`.

## Complexity and decision

Both candidates share a transaction/read/validation kernel. The adapter adds reconstruction/remapping and the obligation to reproduce every supported subsystem's state. It cannot generically clone the current engine. Removing preflight alone is safe only with unified validation and exclusive commit ownership. That makes replacing the gameplay state boundary the more coherent implementation strategy while retaining the current core and subsystems.

This is a design/ownership comparison, not a claim that fewer source lines automatically mean a simpler engine. The prototype's fixed component switches, scans, JSON RPC and scoped-access ceremony are experiments. Production needs registered/generated component access, incremental publication and approachable helpers. New components must not require adding another central switch.

Remaining qualification: full callback phases/lifecycle and native effects; supported dynamic/custom schemas and references; general serialization/migration and unknown-type preservation; real inspector/generator integration; runtime module interactions, skeletal animation and asset readiness; safe binary view lifetimes; AOT/GC and larger integrated workloads. F0 is not complete. ADR-0046 deliberately accepts the pure-data/state replacement decision while keeping those questions assigned to their milestones.

## Reproduce

```sh
python3 tools/foundation_baseline.py --only prototype-state
```

The suite builds the explicit candidate cdylib, runs native tests/lint, resolves/analyzes Dart, runs both strategy contracts and bridge frame captures, then runs the native sparse/dense frame benchmark. serde_json is a dev dependency of core for these examples; it is not added to the shipped core's runtime dependencies. The lockfile change records that example dependency only. Existing user-generated reference-game assets were preserved.

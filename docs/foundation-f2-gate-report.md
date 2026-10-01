# F2: unified gameplay state qualification

**Date:** 2026-10-02
**Revision:** `5b66fb4f` plus the qualified working tree (source hashes in the capture).
**Status:** PASS — F2 complete under ADR-0051; the later foundation milestones remain open.
**Decision:** [ADR-0051](decisions/ADR-0051-f2-boundaries-and-presentation-policy.md), refining [ADR-0046](decisions/ADR-0046-transactional-world-state-replacement.md).

## Completion re-audit (2026-10-02)

The user's follow-up requested checking the completion claim against code rather
than treating passing tests as proof of completeness. The audit found a genuine
resource-lifetime gap: made materials/meshes/clips and loaded Dart data assets
could report alive after their owning world was disposed. The ownership table's
world-lifetime claim was therefore too strong at the initial capture.

This is fixed at ownership/lifetime boundaries. Runtime asset handles check their
world; loaded Dart data instances record their owner, while standalone instances
remain independent. Disposal releases SDK resource descriptors and shared-data
maps. Data lookup refuses a disposed owner. Automatic callback disposal remains
deferred until commit; abort leaves the owner and its resources alive.

Five new tests failed before the fix and now pass, covering ordinary/native-host
disposal, commit/abort of deferred callback disposal, shader descriptors and
nested data references. [Audit evidence](../benchmarks/foundation/results/2026-10-02-5b66fb4f-f2-completion-audit.json)
records the failing reproductions, final source hashes, and eight passing affected
analysis/test gates. SDK coverage is now **586 passing tests**. Authoring (16),
Flutter (39) and focused editor (91) remain passing. Native sources are unchanged;
the nine native/generated gates and separate identity/export check are carried
from the base qualification, not claimed to have been rerun in this audit.

No further unresolved F2 correctness issue was identified in this focused audit.
This is not proof that all code is defect-free. The conservative full callback
entry scan, independent arbitrary-Dart authoring operations and optional startup
contributions remain explicit limitations/policies, described below and in
ADR-0051. F3/F4 work is still necessary before large-game readiness.

## What F2 establishes

The native ECS owns committed gameplay data. A scoped native journal owns pending changes. Ordinary object helpers, queries, construction, behaviours, callbacks, native systems and editor entity batches use that validation/publication authority. Standalone ordinary calls open the same transaction. There is no SDK command overlay or competing raw compatibility transport. Pending identities are scoped tokens, not temporary entity IDs. An invalid operation fails its transaction even if its exception is caught.

Resource plans prepare before publication. Runtime clips, PBR/shader materials and meshes share commit/abort with entity data and effects. SDK metadata publishes after native success. Material representations coalesce together, and shader parameter descriptors cannot be silently changed by mutating a caller-owned list. Mesh collision replacements are prepared before commit and invalidate existing colliders on publication.

Scene activation, including replacement destruction, validates through the journal. Failed startup preserves the previous project's persistent objects. Optional persistent prefab entries and authored behaviours retain their explicit reported failure policy; the host does not promise rollback of arbitrary constructors. Persistence marking and listing now share the logical hierarchy, including pending roots. The duplicate SDK persistent-root list is removed.

## State-visibility matrix

| Operation / observation | Inside the owning edit | After successful commit | On abort / next phase |
| --- | --- | --- | --- |
| Components, name lookup, local/world transforms, parent/children, activity | Reads staged changes, including affected descendants | Next callback sees the committed result without a tick | Abort restores prior committed state; no replay |
| Spawn/build/clone/prefab | Scoped pending identity; ordinary operations work on it | Surviving spawns resolve once to live handles | Canceled spawns allocate no live slot; aborted handles expire |
| Destruction and scene persistence | Logical hierarchy determines descendants and current persistent roots | Commit removes final subtree / records surviving root marks | Destruction notifications are delivered at lifecycle boundary; aborted marks do not survive |
| Queries | Candidates freeze at query creation; values remain logical; removed candidates are skipped | New query sees new membership | New spawns/re-additions require a new query |
| Behaviour attachment/enabling/removal, coroutines | Ownership/scheduling intents join the edit | Metadata publishes with entity data | Lifecycle callbacks run at their normal boundary, not during publication; arbitrary script fields are not rolled back |
| Runtime materials/meshes/clips | Read-your-writes descriptors/liveness; prepared native intent | Final per-ID intent publishes once to resource owner | Abort changes neither store revisions nor SDK membership; no delayed replay |
| Body/character/audio/animation requests | Validate and stage typed effects; logical configuration reads its writes | Committed requests wait for the eligible subsystem boundary | Abort emits no effect; device/playback/physics execution retains its declared phase |
| Body, character and animation playback state | Completed subsystem result | Does not pretend a gameplay commit simulated time | Changes at the subsystem boundary |
| Spatial physics queries | Committed physics world; staged transforms/colliders excluded | Outside the edit, explicit query synchronization may refresh committed geometry | No implicit time advancement; this differs deliberately from staged component reads |
| Completed snapshots and interpolation | Last completed tick remains unchanged | Logical commit does not relabel the snapshot tick | Fixed-tick preparation preserves previous/current interpolation history |
| Presentation setters | Persistent gameplay writes through the same journal | Survive later frames until another writer changes them | No hidden one-frame override; explicit helpers reject dynamic-body presentation ownership |
| Scene-manager requests | Commit-controlled scheduling | Runs at the host's declared scene boundary | Rejection does not enqueue a request; synchronous host activation remains separate |
| Editor entity/material edits | Shared entity/resource journal where applicable | Same validators and final publication | Document history/revisions and arbitrary Dart data setters retain separate authoring ownership |

Host frame advancement, synchronous loading, disposal, native escape, device operations and arbitrary shared Dart data-asset setters remain deliberately guarded inside gameplay edits. Their exact contracts and alternatives are in ADR-0051. These guards do not conceal a second gameplay queue.

## Retained state: owners and invalidation

A lifetime/generation or an explicit refresh boundary is a revision rule; not every cache needs a numeric revision. No retained cache below is an alternate authoritative component store.

| Retained data | Owner / version | Invalidation or publication rule |
| --- | --- | --- |
| `_byEntity`, typed component handles | SDK identity keyed by world/slot/generation | Callback-entry native identity reconciliation; commit identity deltas remove dead handles/update names; disposal invalidates world access |
| `_projectPersistent`, authored entity IDs | SDK load-report identity metadata | Replaced from successful project/scene reports; liveness uses native logical membership; failed activation preserves prior state |
| Native persistent roots | Core scene lifecycle | Journal marks, logical root filtering, committed destruction and scene replacement; no SDK duplicate list |
| Pending handles, staged data/resource descriptors, identity deltas | Single native/SDK edit sequence | Abort discards; successful commit resolves/publishes; expired scopes cannot access another edit |
| `_snapshot`, `_snapshotIndex`, observed subscriptions | Completed native tick and SDK observation lifetime | Replaced at snapshot read; subscribe/unsubscribe follows observation and destruction; never used as ordinary logical component truth |
| Final diagnostic pose/name after disposal | SDK dead-handle fallback | Frozen for diagnostics; cannot be written into a live world |
| Behaviour lists, attachment order, dirty lifecycle state, coroutine/tween queues | Dart execution owner and committed attachment sequence | Transaction publication marks lifecycle dirty; settle/phase processing reconciles activity and destruction; disposal ends execution |
| Input action names, sampled states and undeclared-name warnings | Input binding table and host/tick phases | Binding/load commit invalidates name table; capture/refresh updates phase samples; aborted bindings do not publish |
| Runtime material/mesh descriptors and clip liveness | SDK resource API ownership; per-edit staged intents | Replace/remove only after native success; shader/PBR representation invalidation is symmetric; world lifetime across scene loads; disposal clears descriptors and invalidates handles |
| Shader property overrides for Inspector reapplication | SDK code-written property ownership | Copied on write, applied over saved fields; material removal/representation change clears; failed edit preserves prior descriptors |
| Assets runtime/cooked material and mesh caches | Shared `Assets` store; `live_edits`, `material_edits`, `mesh_edits` and per-asset change maps | Final publication increments existing revisions; mesh changes evict decoded cooked mesh entry; renderer consumes changes through existing store APIs |
| Clip/controller/skin/sound/physics registration | Respective core subsystem and prepared resource plan | Prepare/preflight before commit; publish on success; controller definitions may be visible only to the owning edit; clip removal preserves already-playing copy by policy |
| Physics mesh triangles / built collider geometry | Physics registry and collider stale flag | Prepared mesh replacement replaces registration and marks built colliders stale; synchronization/next tick rebuilds; completed results are separate from logical data |
| Shared Dart `DataAsset` instances | One instance per asset ID, world lifetime | Newly loaded instances publish only on successful edit; abort invalidates new instances; existing arbitrary Dart fields remain outside rollback; owning-world disposal invalidates loaded instances and clears the shared map; standalone instances retain independent lifetime |
| Native prefab descriptions, asset residency and async tickets | Assets/host scene services and request identity | Store replacement invalidates prefab cache; successful activation replaces scene residency; cancellation/failure releases owned preparation; F5 owns finer eviction/budgets |
| Authoring preview ID maps and described components | Preview scene/document revision | Reload/batch updates only after successful shared transaction; failed reload preserves prior mappings/description; event reconciliation does not drain unrelated lifecycle events |
| Transform history, render extraction/projection scratch, skin palettes | Native simulation/render phase and entity generation | Existing propagation/tick/extraction rebuild or memoization boundaries; persistent gameplay commits snap/update history explicitly; legacy overrides are removed |

F2 does not assert complete incremental invalidation for raw ECS references. Callback entry intentionally still enumerates the current identity/name state. F4 introduces comprehensive structural/component/hierarchy revisions and removes this conservative scan. F5/F6 own resource eviction and renderer scaling; the table defines ownership, not a claim that current memory/CPU cost is optimal.

## Qualification

The final capture records all 17 command results, retry reasons, tool/environment details, test counts and hashes of the actual sources tested. The commands cover release workspace checking/testing, stripped core/native/resource configurations, formatting, Clippy with warnings denied, generated-contract checks, SDK/authoring/Flutter analysis and tests, and focused editor Play/document/scene-view/hierarchy tests.

The new regressions cover resource commit/rollback and coalescing, actual native store revisions, sticky malformed resource requests, material representation changes, caller-owned shader lists, pending mesh/collider creation at 30/60/120/240 Hz including a zero-tick callback, mesh replacement collision geometry, transactional persistence, identity deltas, and failed project replacement preserving old persistent roots. Existing F2 tests cover zero/multi-tick callback ordering, lifecycle, query membership, effects, runtime resources, scene activation and native-system integration.

Callback workload: `cd packages/fynkel && dart run tool/foundation_callbacks.dart`. It runs the shipping SDK/native bridge at 128 and 2048 entities with 1 and 32 callback-equivalent edits per sample, five warmups and ten measurements, and checks the final state. These are Dart JIT CPU measurements excluding GPU, assets, physics and scheduler dispatch; they are not a complete-game frame budget. The retained full entry scan and per-operation bridge work remain measured F3/F4 targets.

[Final evidence](../benchmarks/foundation/results/2026-10-02-5b66fb4f-f2-completion.json): **17/17 checks pass**, plus the separate dynamic-library identity/export test. Counts: 1081 Rust passed / 8 ignored, 131 stripped core, 22 stripped native protocol, 581 SDK, 16 authoring, 39 Flutter, 91 focused editor, 21 stripped resource and 2 stripped mesh tests. The capture records 564 source hashes. Initial failures from the new integration test's imports/buffer construction and the SDK's missing `jsonDecode` import were fixed; affected checks were rerun, retaining failed attempts and log hashes.

Measured on Apple M3 Max, macOS 26.5.1, Dart JIT:

| Live entities | Edits per sample | p50 μs | p95 μs (10 samples) | FFI calls per sample |
| --- | --- | --- | --- | --- |
| 128 | 1 | 320 | 524 | 8 |
| 128 | 32 | 2780 | 3411 | 256 |
| 2048 | 1 | 382 | 657 | 8 |
| 2048 | 32 | 10956 | 11023 | 256 |

Final-state checks pass. Commit identity deltas keep the measured FFI call count independent of population, but total time still grows with full entry reconciliation. **10.956 ms for 32 small edits at 2048 entities is a concrete remaining F4 performance problem**, not a result to describe as production-ready. Five warmups/ten samples give local descriptive evidence, with JIT and sampling limitations. Full frame/allocation/GPU budgets remain F0/F3–F8 qualification. Runtime ABI 4 and schema fingerprint `0xbd96f0308508f61f` are unchanged by the closing batch.

## What completion does not claim

F3 custom component registration, bulk views, registered systems and native extensions remain open. F4 incremental synchronization and sparse-world costs remain open. F5 readiness/residency/budgeting, F6 rendering scale, F7 the complete authoring/example migration and F8 production qualification remain open. The larger F0 performance qualification also remains active. F2 completion means one specified, tested gameplay state contract; it does not mean the engine is ready to compete with mature engines at production scale.

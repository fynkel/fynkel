# ADR-0051: F2 transaction boundaries and presentation policy

**Status:** Accepted for the implemented F2 contract.
**Date:** 2026-10-02
**Context:** Closing the unified-state milestone under the user's instruction to finish F2. This clarifies ADR-0043/0046; it does not expand F2 into F3–F8.

## Decision

Ordinary World/GameObject operations and sequential gameplay callbacks use the native gameplay journal. The callback owns its transaction; an explicit edit inside an automatic callback joins it. Standalone ordinary operations open the same transaction. Validation failures are sticky, even if caught. Later successful callbacks observe earlier commits. A failed callback cannot roll back earlier callbacks or arbitrary Dart script state.

Runtime PBR materials, shader materials, meshes and clips prepare owned values before commit. Publication follows successful entity/resource validation. Repeated writes coalesce by asset ID; PBR and shader material representations share one native intent slot. Their SDK descriptors/liveness are staged metadata, published only after native success. SDK descriptors do not own renderer or physics state. Resource removal restores cooked lookup where applicable; removing a clip preserves the existing already-playing core copy. Renderable meshes above the collision triangle budget retain the established next-physics-boundary unavailable diagnostic. Resource eviction/budget policy belongs to F5.

Scene persistence is gameplay data. Marking a pending root and reading persistent roots work in the same edit. Abort discards marking. Non-root marking keeps the existing refusal/warning behavior. Reparenting and destruction affect the logical root list immediately. The native persistent-root list is the only authority; the duplicate SDK list and after-commit marking callback are removed.

Required startup scene replacement and retirement of the previous project's persistent roots commit together. Failed startup validation preserves the old world. Projects without startup scenes preserve ordinary existing entities and retire previous project-owned roots at the successful host boundary. Configured persistent prefab entries remain **independent optional contributions**: each has its own atomic prefab transaction; failure yields a warning/null entry and does not fail project loading. Authored behaviour construction similarly retains its reported warning policy. F2 does not promise an all-or-nothing rollback of arbitrary user constructors across a project load.

## Presentation

All supported transform/property setters change persistent gameplay data. The explicitly named presentation helpers are forwarding conveniences with their documented dynamic-body restriction. They do not create a hidden temporary transform layer or replay an old write on a later tick. A visual placement survives until another committed writer changes it. Later writers in the sequential frame schedule win.

There is **no transient render-only transform API in F2**. This is a deliberate policy, not an unfinished alternative state store. A separate visual child can carry camera bob, recoil or other graphical placement while the collision/character root keeps its own transform. A future renderer-only adjustment feature requires a separate decision specifying composition, lifetime, picking, bounds and interpolation; it must not change ordinary setter semantics.

Completed simulation snapshots retain their tick. Body/character/animation playback state retains its subsystem boundary. Spatial queries inside a gameplay edit use committed physics state and do not incorporate staged collider/transform writes. Outside an edit, the existing explicit query synchronization can refresh committed collider geometry without advancing time. Therefore a post-commit raycast can observe a new mesh before a simulation tick; that does not mean simulation advanced. These exceptions are named in the F2 visibility matrix and covered by tests.

## Intentionally separate operations

The following guarded operations are host/service boundaries, not missing gameplay mutation transports:

| Boundary | Reason / supported route |
| --- | --- |
| `beginFrame`, `captureInput`, `step`, `drainEvents`, `frame`, `stepFrame` | The host owns frame/input/event advancement; callbacks cannot recursively advance or drain their owner. |
| `loadProject`, synchronous `loadScene`/`unloadScene` | Host activation owns scene reports, residency and authored-script attachment. Gameplay uses the commit-controlled `world.scenes` requests. |
| `dispose`, raw `native`, snapshot budget | Raw access/output-budget configuration and explicit-edit disposal are rejected during ownership. Disposal requested by an automatic callback is deferred until successful commit; abort cancels it. |
| Audio device open/close/suspend and `play(presentation: true)` | Explicit external-device operations cannot be undone. Ordinary sound/control APIs stage typed effects; immediate presentation playback stays an independent device boundary. |
| `useSettings` | Replaces the external settings-store association; supported audio settings application already stages gameplay bus configuration. File/store edits are external state. |
| `applyDataEdit`, data-asset destruction | Independent Dart authoring/lifetime operations. Existing shared Dart objects can have arbitrary setters and side effects, so they are not represented as rollback-capable ECS data. They reject inside an edit before invoking setters. New data-asset instances/identity resolution remain transaction-scoped. F3 defines schema-owned custom components; F7 owns richer authoring integration. |
| Removing Transform | Transform is required by the object model; deleting it is not a supported gameplay mutation. |

An independent editor operation is not advertised as atomic with gameplay. If a custom Dart field setter fails, earlier arbitrary Dart side effects cannot be rolled back. Moving custom gameplay data into the F3 component registry is the planned foundation for stronger guarantees, rather than introducing a second mutable rollback store in F2.

## Performance and scope

Commit metadata now consumes native identity deltas (renames, surviving spawns and removals), including subtree destruction and canceled pending objects. It no longer enumerates all entity names through one FFI call per object at every commit. Callback entry still reconciles the full live identity/name table. No revision cache is asserted for native/raw ECS mutation routes that cannot yet guarantee complete invalidation.

F2 qualifies the state contract and records callback-scale costs. F4.2/4.3 retain incremental bridge publication, comprehensive revisions and sparse-world performance as explicit work. F3 retains bulk gameplay access and registered systems. Passing F2 is not evidence of Unity/Godot scale, zero allocations, a 50k-entity frame budget, or production readiness.

## Evidence

See the [F2 qualification report](../foundation-f2-gate-report.md), the shared transaction tests, runtime-resource and scene regressions, and the callback workload in `packages/fynkel/tool/foundation_callbacks.dart`. Historical ADR-0046 batch notes describe earlier states; this decision and the final gate report define the closing contract.

### Disposal ownership correction from the completion audit

Loaded resources end with their owning world. Made material/mesh/clip handles
check that world even when a native host has disposed it directly. SDK disposal
clears runtime descriptors and shared-data caches. Loaded Dart data assets record
their world owner; standalone Dart data assets remain independent. The initial
completion implementation missed this invalidation, reproduced by five tests and
fixed in the [completion audit](../foundation-f2-gate-report.md#completion-re-audit-2026-10-02).
Post-commit notifications and deferred host actions execute after data publication;
an error in that later phase does not undo an already committed transaction.

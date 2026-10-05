# F8 synchronization breakdown and hierarchy reduction — 2026-10-04

The retained change skips descendant-walk setup for leaf candidates. Leaves
remain in the same ordered synchronization candidate set and still receive
validation, updates and removal handling. Objects with children still walk their
ancestors and expand affected descendants. This avoids unnecessary work without
adding a cache, changing solver settings or simplifying the scene.

## Live iPhone measurements

Four bounded eight-second dense-5000 captures, in baseline/candidate/control/
repeat order, use identical opt-in phase and all-tick diagnostics. All captures
finished at their deadlines; the runner terminated each exact app PID. Thermal
samples were nominal. Each native sample matches its SDK tick (native completed
ID = SDK pre-step ID + 1). First ticks are retained; p95 uses nearest rank.

Medians, milliseconds per simulation tick:

| Phase | Baseline | Candidate | Control | Candidate repeat |
| --- | ---: | ---: | ---: | ---: |
| Collect changes | 0.168 | 0.172 | 0.169 | 0.172 |
| Expand hierarchy | 0.287 | 0.232 | 0.294 | 0.239 |
| Validate/classify inputs | 0.684 | 0.685 | 0.689 | 0.690 |
| Apply removals/updates/nested ownership | 0.696 | 0.695 | 0.711 | 0.718 |
| Entire synchronization | 1.852 | 1.800 | 1.885 | 1.830 |
| Entire physics adapter | 3.158 | 3.100 | 3.179 | 3.154 |
| Entire core physics system | 4.292 | 4.248 | 4.369 | 4.342 |
| Entire native step | 7.014 | 7.023 | 7.105 | 7.110 |
| Matched ticks | 474 | 480 | 478 | 479 |

Hierarchy work falls about 19% in both comparisons, saving roughly 55 µs per
tick. Whole-step timing does not improve. Retain this small local reduction;
do not call it an FPS gain. Diagnostics add overhead and short nominal thermal
samples do not establish sustained performance. F8 remains open.

On the Mac, the native-only candidate reduced hierarchy median from 297 to
241 µs, but total synchronization was 1.521 vs 1.532 ms and the native step
was slower. The Mac runs therefore did not establish a total-speed benefit;
phone repetitions were necessary. Profiled and unprofiled simulation outputs
match in the native diagnostic (this is not a cross-build state comparison).

## Measurement changes and correctness

The existing optional physics profile now includes four nested synchronization
phases. Their timers run only with profiling enabled. The trace adds `collect_ns`,
`hierarchy_ns`, `classify_ns` and `apply_ns`; these are contained within `sync_ns`
and must not be added again to outer totals. The parser checks both containment
relationships and rejects stale skipped-step values. Earlier raw archives retain
their original parser/source; this diagnostic CSV is not a gameplay ABI.

A new regression covers a collider losing its last child: both resulting leaves
must still move with stable collider handles, and component removal/despawn must
remove their physics entries. Existing regressions cover affected ancestors,
already-affected intermediate nodes, nested body ownership, inactive objects,
invalid transforms and entity reuse.

Validation:

- 38 normal physics tests pass; both normally ignored moving/nested synchronization
  qualification tests were also run explicitly and pass.
- 15 core physics tests and 95 native tests pass (two unrelated native tests remain ignored).
- 18 Python diagnostic tests pass, including nested phase accounting.
- Native runtime checks with optional features stripped; Clippy passes with warnings denied.
- iOS release build succeeds; the retained build is installed and stopped.

## Next target

Validation/classification and applying updates each cost about 0.69–0.72 ms on
the phone. Inspect those paths for repeated transform decomposition, component
comparison and entry lookup before changing data structures. Preserve transform
validation and nested-collider ownership. The additional core propagation/event
cost and previously measured GPU costs remain independent targets; this small
hierarchy saving does not solve either.

[Results](../benchmarks/foundation/results/2026-10-04-f8-sync-hierarchy.json)
and [raw captures, source and validation](../benchmarks/foundation/results/2026-10-04-f8-sync-hierarchy-raw.tar.gz).

Follow-up: [reuse of the physics entry lookup](foundation-f8-entry-lookup.md)
reduces update-application costs in repeated phone comparisons. Both builds in
that comparison retain the hierarchy change described here.

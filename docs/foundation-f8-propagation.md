# F8 shared transform-propagation reads — 2026-10-04

Retained: reuse shared ECS query views for dirty-node input gathering, ancestor
inspection and local-transform/children reads during traversal. Previously these
paths repeatedly resolved the same entity and acquired separate component reads.
WorldTransform writes still use precise tracked per-entity leases. No bulk write
lease, global invalidation, new cache, hierarchy-order change or math change was
introduced. Earlier physics optimizations remain in both compared builds.

## Repeated device evidence

Dense-5000 live iPhone, medium quality, same assets and workload. Four eight-second
phase captures match every native timing sample to its SDK tick. Medians in ms:

| Work | Control | Candidate | Control repeat | Candidate repeat |
| --- | ---: | ---: | ---: | ---: |
| Propagation before physics | 1.148 | 0.833 | 1.135 | 0.864 |
| Entire native step | 7.073 | 6.383 | 6.889 | 6.602 |
| Propagation p95 | 1.551 | 0.972 | 1.361 | 1.032 |
| Native step p95 | 9.106 | 7.236 | 7.896 | 7.597 |

Propagation drops 24–27%, saving 0.27–0.31 ms per tick. The native step improves
in both pairs, but the first control also has higher adapter costs; do not assign
its entire whole-step reduction to this change. The first control has an unknown
thermal/startup window with no acquisition measurement. Its aggregate acquisition
rate is left null, not manufactured by discarding that window. Remaining recorded
thermal states across these runs were nominal.

A separate pair without phase tracing or all-tick readback reports **43.428 vs
43.444 acquired frames/s**: essentially unchanged. The median of its eight
one-second native host-work medians is 6.919 vs 6.788 ms. This statistic is neither
a pooled per-tick median nor an all-tick measurement, and includes host work and
catch-up ticks. Native host-work window medians were lower in all eight candidate
windows. Frame acquisition is not display scanout, and no FPS improvement is
claimed. The repeated phase pair also does not show an acquisition-rate increase.

All six runs reached the app deadline and their exact PIDs were terminated. The
retained build is installed and stopped. No sustained smoothness/thermal pass or
F8 completion is claimed. GPU/rendering costs remain separate constraints.

Sequential Mac native-only control/candidate runs measured propagation
1.290→0.958 and 1.288→0.958 ms. Profiled and unprofiled transforms and motion state
match within each build. That check does not establish cross-build equality.

## Correctness and qualification

The new permanent regression alternates sparse and dense raw local-transform
writes, including a parent and already-dirty child. It checks world matrices
against independently computed ancestor products; exact changed-entity sets
without a global change-history reset; and no writes on a clean follow-up tick.

25 scene tests and 375 core tests pass. Existing core coverage compares incremental
scene state with reference behavior through mixed edits/history overflow and
checks sparse scene/physics/resource ownership after dense native writes. Existing
scene coverage includes non-spatial ancestors, hierarchy changes and dirty
subtrees. One manual qualification test in each suite remains normally ignored.
Clippy with warnings denied and the stripped native-runtime build check pass.
iOS release build succeeds. The new regression checks sparse correctness, not
a separate sparse timing benchmark.

[Results](../benchmarks/foundation/results/2026-10-04-f8-propagation.json)
and [raw captures, source, timings and checks](../benchmarks/foundation/results/2026-10-04-f8-propagation-raw.tar.gz).

Follow-up: [skipping redundant dirty-flag writes](foundation-f8-dirty-writes.md)
was tested and removed for lack of consistent phone-wide benefit. The shared
propagation reads described here remain retained.

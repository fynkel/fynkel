# F8 iPhone physics phase attribution — 2026-10-04

The bounded live dense-5000 run collected 479 physics samples, each matched to
its SDK system-timing sample. Synchronization is the largest adapter phase.
This supplies the phone attribution missing from the previous CPU follow-up;
it is a diagnostic change, not a measured performance improvement.

| Work | Median per tick | p95 per tick |
| --- | ---: | ---: |
| Synchronize scene changes into physics | 1.873 ms | 2.212 ms |
| Solver phase (including force reset) | 0.996 ms | 1.218 ms |
| Write body poses back | 0.157 ms | 0.186 ms |
| Adapter events and statistics | 0.129 ms | 0.156 ms |
| Entire physics adapter | 3.185 ms | 3.590 ms |
| Entire core physics system | 4.344 ms | 4.867 ms |
| Entire native step | 7.017 ms | 7.967 ms |

Commands and character-controller movement each have a median below 1 µs in
this workload. The 200 animated rigs are not 200 physics character controllers.
The fixture includes 5,000 moving colliders, 500 dynamic bodies, 300 Dart
behaviours and 200 animated rigs. It must not be described as 5,000 stationary
scenery objects or silently simplified to obtain a better number.

The per-tick difference between the core physics system and the adapter has a
1.149 ms median. The core system also propagates scene transforms and handles
rejections/events. This residual is not proven to be entirely transform work.
Medians are computed independently, not added/subtracted to manufacture totals.
The first tick is retained; p95 uses the nearest-rank definition.

## Measurement contract

`capture.py --trace-physics --cpu-ticks --trace-frames` opts into a unique
`FYNKEL_TRACE_PHYSICS_FILE` for that process. Native worlds share a locked file,
with world IDs and completed tick IDs, capped at 8,192 rows. The trace resets
physics profiling before each step so a skipped physics stage produces a zero
row, never an old sample. Profiling stops at the cap or after a write failure.
The default path enables no physics phase timers or trace writes. No gameplay
ABI, workload, solver configuration or rendering quality was changed.

The parser rejects missing, truncated, malformed, repeated, stale and saturated
traces. Native completed tick IDs are SDK pre-step IDs plus one; all 479 pairs
were checked against this convention in the source and the capture. The archive
contains the summarization script, source and logs for reproduction.

The app stopped at its eight-second deadline; the runner terminated exact PID
4879 before copying results. All thermal samples were nominal. Timers, per-tick
readbacks and file writes add overhead: these numbers establish phase costs,
not an FPS improvement or sustained thermal/smoothness qualification.

## Next optimization

Concentrate on scene-to-physics synchronization first. Break down change-list
collection, hierarchy expansion and per-collider update work before choosing a
rewrite. Preserve hierarchy ownership, removals, inactive objects, collider
validation and deterministic ordering. The previously rejected visual-leaf
filter remains removed. Separately attribute the core-system residual before
calling it a transform bottleneck. Existing GPU screen-space costs also remain;
reducing physics alone does not establish 120 FPS.

Validation: the full native suite passes (95 tests), as do 18 Python diagnostic tests; the native
runtime checks with all optional modules stripped, and the iOS release build
succeeds. F8 remains open.

[Results](../benchmarks/foundation/results/2026-10-04-f8-physics-phases.json)
and [raw evidence](../benchmarks/foundation/results/2026-10-04-f8-physics-phases-raw.tar.gz).

Follow-up: [synchronization subphase measurements and a retained hierarchy
reduction](foundation-f8-sync-hierarchy.md) now separate collection, hierarchy,
validation and application. Larger validation/application costs remain.

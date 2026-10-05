# F8 visibility CPU attribution — 2026-10-04

Added nested visibility timers and measured one bounded iPhone run. No runtime
optimization or FPS gain is claimed. The rejected dense record lookup remains
reverted; earlier extraction and preparation diagnostics remain in place.

## Implementation and validation

`FrameStats.visibility_phases_us` records three disjoint elapsed CPU intervals:
bounds construction, spatial-index update, and camera/shadow queries. The last
also includes clearing shadow visibility when no shadow frustum exists. A monotonic
start and three elapsed boundary reads are added. These timings are nested inside
the existing visibility preparation phase and must not be added to frame totals.
No native/Dart ABI changes are needed. Opt-in bounded frame tracing writes one
additional `visibility` CSV record per prepared frame.

The analyzer requires complete visibility records in every preparation group once
they appear, associates them with the enclosing host render call, and rejects
negative durations or sums exceeding the parent phase. Older traces return null
visibility detail. Regression cases cover missing/duplicate records, invalid
nested totals and older traces; the real Apple viewport test checks the nested
sum. 29 renderer unit tests, five Apple unit tests, 23 Python checks, Clippy with
warnings denied and the iOS release build pass. The previous preparation capture
still analyzes successfully without visibility detail.

## Observation

Live dense-5000, medium, 1320×2868, eight seconds with frame tracing and all-tick
CPU readback. 330 prepared frames match complete phase groups. All thermal
samples are nominal. Exact PID 7059 was stopped automatically after capture.

| Visibility work | Median ms | Mean ms | p95 ms | Share of detailed total |
| --- | ---: | ---: | ---: | ---: |
| Bounds construction | 0.419 | 0.424 | 0.502 | 54.9% |
| Spatial-index update | 0.160 | 0.312 | 0.784 | 40.5% |
| Camera/shadow queries | 0.035 | 0.035 | 0.043 | 4.6% |

Shares use accumulated elapsed time (139.824, 103.014 and 11.667 ms), not a sum
of medians. Parent visibility has median 0.636 ms, mean 0.772 ms and p95 1.285 ms.
The small parent/detail difference includes timer and call overhead. Hierarchy
updates have a much wider distribution than bounds or queries; this recording
does not yet distinguish rebuild from refit samples.

New acquisitions average 37.92/s. This is one instrumented diagnostic, not a
before/after performance comparison or physical presentation rate. Short nominal
thermal samples do not prove sustained performance or constant clock speed.

## Next target

Prioritize bounds construction and hierarchy maintenance rather than query
traversal. Source inspection finds that bounds construction transforms fallback
cube bounds for every record, then replaces mesh records with their authored
bounds; main-world paths also multiply by an identity placement. Measure a
semantics-preserving reduction in that redundant work, keeping missing resources
and custom vertex deformation conservative and retaining skin/extra-world bounds.
This capture does not isolate the cost of those individual operations.

For hierarchy updates, attribute rebuilds and refits before altering the policy.
The implementation rebuilds when membership size changes, and can rebuild after
large movement increases its cost metric. Those are hypotheses for the long tail,
not causes established by these three timers. Do not skip necessary updates or
weaken culling correctness to improve the benchmark.

F8, sustained thermal/60/120 FPS qualification and the extraction reverse-order
comparison remain open. The diagnostic app is installed and stopped.

[Results](../benchmarks/foundation/results/2026-10-04-f8-visibility-phases.json)
and [raw capture and checks](../benchmarks/foundation/results/2026-10-04-f8-visibility-phases-raw.tar.gz).

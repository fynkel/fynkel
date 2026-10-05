# F8 reuse of physics update entry lookup — 2026-10-04

The retained change reuses the mutable entry lookup in `Physics::update_entry`.
Previously, the existing entry was looked up to decide whether to rebuild, then
looked up again for an in-place update. The same borrow now serves both checks
and updates. The rebuild branch releases it and performs the same removal and
creation as before. No cache, ordering, validation, workload or solver change is
introduced. The previous hierarchy improvement remains in both builds.

## Repeated live iPhone results

Four bounded eight-second dense-5000 runs used identical physics phase traces
and all-tick readback. Each native sample was matched to its SDK tick; all runs
reached the deadline, and their exact app PIDs were terminated before copying
results. All recorded thermal states were nominal.

Medians in milliseconds per simulation tick:

| Work | Control | Candidate | Control repeat | Candidate repeat |
| --- | ---: | ---: | ---: | ---: |
| Validate/classify inputs | 0.669 | 0.695 | 0.708 | 0.706 |
| Apply physics updates | 0.688 | 0.576 | 0.733 | 0.604 |
| Entire synchronization | 1.768 | 1.696 | 1.870 | 1.739 |
| Entire physics adapter | 3.036 | 3.006 | 3.197 | 3.091 |
| Entire core physics system | 4.156 | 4.181 | 4.377 | 4.293 |
| Entire native step | 6.800 | 6.892 | 7.227 | 7.144 |
| Matched ticks | 480 | 480 | 479 | 480 |

Update application falls 16.2% and 17.5%, saving 112 and 128 µs per tick.
Synchronization falls in both comparisons. Whole-step timings are mixed, and
candidate whole-step p95 is slightly higher in both pairs. This supports a local
cost reduction, not an FPS or overall latency improvement. File writes and
per-tick readbacks add diagnostic overhead. Short nominal thermal captures do
not qualify sustained performance; F8 remains open.

The initial Mac samples varied widely and overlapped build activity. They are
archived but are not used as improvement evidence. Subsequent sequential runs
of separately retained control and candidate executables measured application
medians 546→469 and 556→414 µs. Other phases varied too. The native diagnostic
also verifies that profiling on/off produces equal transforms and motion state;
it does not prove cross-build state equality.

## Validation

- 38 physics tests pass, covering in-place motion, component membership changes,
  nested body ownership, collider refitting, character behavior and invalid transforms.
- Both normally ignored moving/nested synchronization qualification tests were
  explicitly run and pass.
- 15 core physics tests pass, including cross-world determinism.
- Clippy passes with warnings denied; stripped native runtime check passes.
- iOS release build and all four bounded captures succeed.

Existing regressions exercise the changed path; no test was added merely to
assert how many map lookups the implementation performs. The code change is
limited to reusing the lookup, with the same branches and operations afterward.
The retained candidate build is installed and stopped.

## Remaining work

Validation/classification still costs about 0.7 ms per tick. Inspect its matrix
conversion, hierarchy validation and component comparison costs before choosing
a further change. The core propagation/event residual and GPU screen-space work
remain separate bottlenecks. These CPU savings alone cannot establish 120 FPS.

[Results](../benchmarks/foundation/results/2026-10-04-f8-entry-lookup.json)
and [raw evidence and source](../benchmarks/foundation/results/2026-10-04-f8-entry-lookup-raw.tar.gz).

Follow-up: [moving the classification comparison into the update path](foundation-f8-validation-followup.md)
did not show a consistent total benefit and was removed. The entry-lookup reuse
described here remains unchanged.

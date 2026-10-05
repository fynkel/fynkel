# F8 redundant dirty-write experiment — 2026-10-04

The candidate was removed. Earlier retained hierarchy, physics entry lookup and
shared propagation-read improvements remain. The expanded regression coverage
for normal transform setters versus direct local writes is retained.

## Experiment

Scene propagation currently marks local/hierarchy candidates dirty before
finding propagation roots. Normal scene setters have often already set this
flag. Acquiring another mutable lease publishes an additional change-history
entry even when the flag remains true. The candidate gathered only unflagged
candidates through a shared query, then used normal tracked writes for those
objects. It preserved downstream per-entity notifications and changed no scene,
solver or renderer settings.

Two sequential Mac native-only comparisons reduced propagation 0.944→0.681 and
0.952→0.680 ms and native step 6.386→5.948 and 6.302→5.923 ms. The native probe
also checks equal profiling-on/off transforms and motion state within each build;
it is not cross-build state comparison.

## iPhone outcome

Bounded dense-5000 live phase captures, medians in milliseconds per tick:

| Work | Control | Candidate | Control repeat | Candidate repeat |
| --- | ---: | ---: | ---: | ---: |
| Propagation | 0.845 | 0.679 | 0.854 | 0.656 |
| Entire native step | 6.574 | 6.783 | 6.552 | 6.466 |
| Native step p95 | 7.456 | 7.952 | 7.750 | 7.586 |

The targeted phase improves, but whole-step results are mixed. Separate captures
without phase tracing or all-tick readback measured:

| Order | Control acquired frames/s | Candidate acquired frames/s |
| --- | ---: | ---: |
| Control then candidate | 40.540 | 36.653 |
| Candidate then control | 44.243 | 44.284 |

The first delivery regression did not repeat in reverse order; that pair is
essentially tied. The cause of this run-to-run variation was not isolated. These
results establish neither a consistent end-to-end benefit nor a consistent frame
rate regression. Given the added filtering/allocation path and lack of repeatable
phone-wide improvement, restore the simpler retained implementation. No FPS gain
is claimed. Acquisition measures completed textures, not display scanout.

All eight captures reached their eight-second deadlines; each exact launched PID
was terminated. Raw thermal observations remain in the results. Short captures
do not establish sustained thermal or smoothness qualification. F8 remains open.

## Restored state and checks

`sync.rs` was restored byte-for-byte to the prior retained version. The generated
iOS app was rebuilt from that source. The last phone run installed the previous
retained control, which is stopped. The rejected source and all results are
archived, not shipped as an alternate runtime path.

The permanent propagation test now exercises sparse/dense updates through both
normal setters (preflagged dirty) and raw local writes, including an independently
edited child. It checks hierarchy products, exact downstream changed-entity sets
and no writes on a clean follow-up. After restoration, 25 scene tests and 375 core
tests pass; each suite retains one normally ignored manual qualification. Candidate
Clippy and stripped runtime checks passed. The restored iOS release build passes.

Next work should return to the previously measured rendering/GPU costs. More
local CPU reductions have not yet raised frame delivery in this workload.

[Results](../benchmarks/foundation/results/2026-10-04-f8-dirty.json)
and [raw evidence, candidate/control source and checks](../benchmarks/foundation/results/2026-10-04-f8-dirty-raw.tar.gz).

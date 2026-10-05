# F8 validation follow-up — 2026-10-04

No new runtime optimization was retained in this experiment. The previous
hierarchy and entry-lookup improvements remain intact.

A temporary Mac-only probe measured checked matrix conversion at a median
272 µs across 5,532 objects per tick, and moving-body ancestor validation at
9 µs across 531 objects. These measurements include per-object clock overhead
and perturb the classification loop; they are not exact phone attribution.
The probe has been removed from the implementation.

The tested candidate moved unchanged-entry comparisons from classification
into the update path, reusing its existing lookup. Transform and ancestor
validation remained before application. This removed a classification lookup,
but gathered and copied inputs for unchanged objects as well, and moved their
comparison into application.

Sequential runs of separately compiled control/candidate executables produced
these Mac native-only medians (µs per tick, 40 warmup / 300 measured ticks):

| Work | Control | Candidate | Control repeat | Candidate repeat |
| --- | ---: | ---: | ---: | ---: |
| Classification | 604 | 509 | 619 | 501 |
| Application | 410 | 547 | 410 | 520 |
| Entire synchronization | 1,352 | 1,412 | 1,375 | 1,355 |
| Entire native step | 6,373 | 6,729 | 6,513 | 6,434 |

There is no consistent useful total synchronization benefit. The candidate was
removed rather than promoted to another phone comparison. The source was
restored byte-for-byte to the previously retained implementation. The native
diagnostic checks equal profiling-on/off gameplay output within each build;
it is not a cross-build comparison.

After restoration, 38 physics tests and 15 core physics tests pass; two manual
physics qualification tests remain ignored in this normal suite. Those manual
checks passed in the preceding retained entry-lookup work. No new phone build
was installed or launched. Its previous retained build remains stopped.

The next useful measurement is the roughly 1.15 ms core physics-system residual
outside the adapter: scene propagation and core event/rejection work. Separate
those costs before another rewrite. Matrix conversion remains a possible later
target, but this probe does not justify bypassing validation or introducing a
transform cache. No FPS improvement or F8 completion is claimed.

[Results](../benchmarks/foundation/results/2026-10-04-f8-validation-followup.json)
and [probe, rejected source, native timings and checks](../benchmarks/foundation/results/2026-10-04-f8-validation-followup-raw.tar.gz).

Follow-up: [core phase measurements](foundation-f8-core-physics-phases.md) now
identify transform propagation as the previously unassigned phone cost.

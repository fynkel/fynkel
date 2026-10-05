# F8 CPU follow-up — 2026-10-04

No CPU implementation is retained from this experiment. The previous lighting
optimization remains in place. A fresh iPhone all-tick diagnostic confirms that
physics is the largest native system; a proposed synchronization relevance filter
was tested and removed because it did not produce a useful synchronization gain.

## Fresh device evidence

The bounded dense-5000 run recorded 479 fixed ticks using the existing optional
per-tick readback. Median native step time was 6.839 ms. System medians were:

| System | Median time per tick |
| --- | ---: |
| Physics | 4.207 ms |
| Look-at-camera | 0.620 ms |
| Animation | 0.591 ms |
| Oscillation | 0.481 ms |
| Rotation | 0.393 ms |
| Transform propagation | 0.366 ms |

System medians are not additive, and this instrumented capture is not a new FPS
baseline. The core physics stage includes preceding scene propagation and event
publication around the adapter; its total does not isolate Rapier's solver.

The generated workload's `static=5000` population is **not stationary scenery**.
`benchmarks/stress/generate.py` gives every object a collider and, by default,
rotation, oscillation or look-at-camera motion. It also authors 500 rigid bodies,
300 Dart behaviours and 200 animated rigs. The test therefore stresses thousands
of moving collider updates as well as rendering. Those updates cannot simply be
skipped without changing the workload. The benchmark remains unchanged.

## Rejected synchronization filter

The candidate filtered changed visual leaves without physics components before
building the ordered physics work set. It preserved existing entries, rejected
transforms and parents with children, so component removal and nested ownership
could still be processed. Regression coverage exercised visual motion, adding and
removing colliders, and cleanup of rejected transforms; existing ancestor and
nested-collider tests also passed.

The first implementation checked the existing-entry maps before component
presence. The second checked component presence first to avoid map lookups for
the numerous real colliders. Mac cooked-scene profiling (40 warmup ticks and 300
measured ticks, no Dart callbacks or rendering) did not establish useful savings:

| Implementation | Median synchronization | Median profiled native step |
| --- | ---: | ---: |
| Original | 1.503 ms | 6.425 ms |
| First filter | 1.836 ms | 6.833 ms |
| Reordered filter | 1.516 ms | 6.440 ms |

The first filter was slower; the reordered filter was close to the original but
added a query and bookkeeping without a measured synchronization benefit. It was
removed before device benchmarking. These Mac results are not phone phase
measurements. Each benchmark verified that enabling its phase timers did not
change its final simulation state; that check alone is not a comparison of
candidate versus original state. Functional regressions passed separately.

## CPU sampler limitation and next work

Two bounded exact-PID Time Profiler attachment attempts reported `Cannot find
process for provided pid`. Both runners stopped their exact launched app PIDs.
No CPU sampling attribution was obtained, and the unverified runner option was
removed. The failed attempt's source and logs are archived, not shipped as a
working capability. The precise attachment failure cause was not isolated.

The existing native physics phase timer can separate synchronization, commands,
characters, solver, writeback and event collection on the Mac. The next useful
step is to expose those same bounded phase measurements on the phone through a
controlled diagnostic path, or establish a working CPU recording startup protocol.
Do that before another physics rewrite; current phone evidence identifies the
system but does not establish its internal phase costs.

After restoration, 31 physics unit tests, 6 physics integration tests, 15 core
physics tests and 17 Python diagnostic tests pass (2 physics tests remain ignored
manual benchmarks). The retained lighting build is the generated and installed
phone app, and it is stopped. No FPS gain or F8 completion is claimed.

[Results](../benchmarks/foundation/results/2026-10-04-f8-cpu-followup.json)
and [raw capture, phase timings, rejected source and logs](../benchmarks/foundation/results/2026-10-04-f8-cpu-followup-raw.tar.gz).

Follow-up: the [phone physics phase trace](foundation-f8-physics-phases.md) now
provides the internal phase attribution requested above. Synchronization is the
largest adapter phase; the previously rejected filter remains removed.
